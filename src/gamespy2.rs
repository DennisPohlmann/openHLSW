//! GameSpy 2 query protocol (UDP), used by Deer Hunter 2005, Battlefield Vietnam,
//! Halo, America's Army and others. The query port differs from the game port, so these
//! servers are entered as `ip:gameport:queryport`.
//!
//! Request:  FE FD 00 <id:4> <info> <players> <teams>   (each 0xFF = wanted, 0x00 = not)
//! Response: 00 <id:4> then the requested sections:
//!   info:    key\0value\0 ... \0
//!   players: [00] [count] field_\0 ... \0 value\0 ... (one value per field and player)

use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use crate::a2s::{Player, QueryError, QueryMeta, Rule, ServerInfo, decode_text};

const REQUEST_ID: [u8; 4] = *b"oHLS";

fn request(
    addr: SocketAddr,
    sections: [u8; 3],
    timeout: Duration,
) -> Result<(Vec<u8>, QueryMeta), QueryError> {
    let socket = UdpSocket::bind(if addr.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    })?;
    socket.connect(addr)?;
    let mut packet = vec![0xFE, 0xFD, 0x00];
    packet.extend_from_slice(&REQUEST_ID);
    packet.extend_from_slice(&sections);
    let mut buf = vec![0u8; 65536];
    for attempt in 1..=2u32 {
        socket.send(&packet)?;
        crate::stats::add_sent(packet.len() + 28);
        let sent = Instant::now();
        let deadline = sent + timeout;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            socket.set_read_timeout(Some(left.max(Duration::from_millis(1))))?;
            let n = match socket.recv(&mut buf) {
                Ok(n) => n,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    break;
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::ConnectionReset
                    ) =>
                {
                    return Err(QueryError::Protocol("Port unreachable".into()));
                }
                Err(e) => return Err(e.into()),
            };
            crate::stats::add_received(n + 28);
            if n >= 5 && buf[0] == 0 && buf[1..5] == REQUEST_ID {
                let meta = QueryMeta {
                    rtt: sent.elapsed(),
                    packets: attempt,
                    challenge: None,
                };
                return Ok((buf[5..n].to_vec(), meta));
            }
        }
    }
    Err(QueryError::Timeout)
}

/// Splits `data` at NUL bytes, decoding each piece.
struct Strings<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Strings<'_> {
    fn next(&mut self) -> Option<String> {
        if self.pos >= self.data.len() {
            return None;
        }
        let rest = &self.data[self.pos..];
        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        self.pos += end + 1;
        Some(decode_text(&rest[..end]))
    }
}

pub fn parse_rules(data: &[u8]) -> Vec<Rule> {
    let mut s = Strings { data, pos: 0 };
    let mut rules = Vec::new();
    while let Some(key) = s.next() {
        if key.is_empty() {
            break;
        }
        let value = s.next().unwrap_or_default();
        rules.push(Rule { key, value });
    }
    rules
}

pub fn info_from_rules(rules: &[Rule]) -> ServerInfo {
    let get = |k: &str| {
        rules
            .iter()
            .find(|r| r.key.eq_ignore_ascii_case(k))
            .map(|r| r.value.clone())
            .unwrap_or_default()
    };
    let num = |k: &str| get(k).trim().parse::<u32>().unwrap_or(0).min(255) as u8;
    let gamename = get("gamename");
    let game = match gamename.to_ascii_lowercase().as_str() {
        "dh2005" | "deerhunter2005" | "deerhunt2005" => "Deer Hunter 2005".to_string(),
        "" => "GameSpy 2".to_string(),
        _ => {
            let v = get("gamevariant");
            if v.is_empty() { gamename.clone() } else { v }
        }
    };
    ServerInfo {
        name: get("hostname"),
        map: get("mapname"),
        folder: gamename,
        game,
        players: num("numplayers"),
        max_players: num("maxplayers"),
        server_type: if get("dedicated") == "0" { b'l' } else { b'd' },
        environment: match get("os").to_ascii_lowercase().as_str() {
            o if o.contains("win") => b'w',
            o if o.contains("linux") => b'l',
            _ => 0,
        },
        passworded: matches!(get("password").as_str(), "1" | "true" | "True"),
        version: get("gamever"),
        game_port: get("hostport").parse().ok(),
        gamespy: true,
        ..Default::default()
    }
}

/// Player table. Field names end with '_' ("player_", "score_", "ping_", ...).
pub fn parse_players(data: &[u8]) -> Vec<Player> {
    let mut pos = 0;
    // Optional leading 0x00 and player count (some servers omit the count).
    if data.first() == Some(&0) {
        pos += 1;
    }
    let mut count = None;
    if let Some(&b) = data.get(pos)
        && b < b'A'
    {
        count = Some(b as usize);
        pos += 1;
    }
    let mut s = Strings { data, pos };
    let mut fields = Vec::new();
    while let Some(f) = s.next() {
        if f.is_empty() {
            break;
        }
        fields.push(f.trim_end_matches('_').to_ascii_lowercase());
    }
    let mut players = Vec::new();
    if fields.is_empty() {
        return players;
    }
    'rows: while count.is_none_or(|c| players.len() < c) {
        let mut p = Player {
            index: players.len() as u8,
            ..Default::default()
        };
        for (i, field) in fields.iter().enumerate() {
            let Some(v) = s.next() else {
                break 'rows;
            };
            if i == 0 && v.is_empty() && s.pos >= data.len() {
                break 'rows;
            }
            match field.as_str() {
                "player" | "name" | "playername" => p.name = v,
                "score" | "frags" | "kills" => p.score = v.trim().parse().unwrap_or(0),
                "time" => p.duration = v.trim().parse().unwrap_or(0.0),
                _ => {}
            }
        }
        players.push(p);
    }
    players
}

pub fn query_info(
    addr: SocketAddr,
    timeout: Duration,
) -> Result<(ServerInfo, QueryMeta), QueryError> {
    let (data, meta) = request(addr, [0xFF, 0, 0], timeout)?;
    Ok((info_from_rules(&parse_rules(&data)), meta))
}

pub fn query_rules(
    addr: SocketAddr,
    timeout: Duration,
) -> Result<(Vec<Rule>, QueryMeta), QueryError> {
    let (data, meta) = request(addr, [0xFF, 0, 0], timeout)?;
    Ok((parse_rules(&data), meta))
}

pub fn query_players(
    addr: SocketAddr,
    timeout: Duration,
) -> Result<(Vec<Player>, QueryMeta), QueryError> {
    let (data, meta) = request(addr, [0, 0xFF, 0], timeout)?;
    Ok((parse_players(&data), meta))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_info_and_players() {
        let info = b"hostname\0Deer Camp\0gamename\0dh2005\0mapname\0Valley\0numplayers\x002\0maxplayers\x0016\0password\x000\0\0";
        let rules = parse_rules(info);
        let i = info_from_rules(&rules);
        assert_eq!(i.name, "Deer Camp");
        assert_eq!(i.game, "Deer Hunter 2005");
        assert_eq!((i.players, i.max_players), (2, 16));
        assert!(i.gamespy);

        let players = b"\x00\x02player_\0score_\0ping_\0\0Bob\x0012\x0040\0Ann\x003\x0055\0";
        let p = parse_players(players);
        assert_eq!(p.len(), 2);
        assert_eq!((p[1].name.as_str(), p[1].score), ("Ann", 3));
        // without zero and count byte
        let p = parse_players(b"player_\0score_\0\0Bob\x007\0");
        assert_eq!((p.len(), p[0].score), (1, 7));
    }
}
