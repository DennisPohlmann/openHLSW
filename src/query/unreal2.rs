//! Unreal Engine 2's own query on port + 1: Unreal Tournament 2003 / 2004, Killing Floor,
//! Red Orchestra, Devastation …
//!
//! Requests `79 00 00 00 <type>` (0 = info, 1 = rules, 2 = players). Answers repeat the
//! header and type; strings are length-prefixed (UCS-2 when the length has bit 7 set).

use std::net::SocketAddr;
use std::time::Duration;

use super::{Packet, Status, count, meta, rule, rules_from, truthy};
use crate::a2s::{Player, QueryError, QueryMeta, ServerInfo};

pub fn query(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let (info_data, rtt) = request(addr, 0, timeout)?;
    let mut info = parse_info(&info_data).ok_or_else(|| bad("info"))?;
    // Rules and players are optional (some servers do not answer them).
    let rules = request(addr, 1, timeout)
        .map(|(d, _)| parse_rules(&d))
        .unwrap_or_default();
    let players = request(addr, 2, timeout)
        .map(|(d, _)| parse_players(&d))
        .unwrap_or_default();
    info.passworded = truthy(rule(&rules, "GamePassword"));
    if let Some(v) = rule(&rules, "ServerVersion").or(rule(&rules, "Server Version")) {
        info.version = v.to_string();
    }
    info.bots = count(players.iter().filter(|p| p.bot).count());
    Ok((
        Status {
            info,
            players,
            rules,
        },
        meta(rtt),
    ))
}

fn bad(what: &str) -> QueryError {
    QueryError::Protocol(format!("Unreal 2: bad {what} answer"))
}

/// Sends one request and joins the data of all answers of that type.
fn request(
    addr: SocketAddr,
    kind: u8,
    timeout: Duration,
) -> Result<(Vec<u8>, Duration), QueryError> {
    let mut data = Vec::new();
    let rtt = super::udp_exchange(
        addr,
        &[0x79, 0, 0, 0, kind],
        timeout,
        Some(Duration::from_millis(150)),
        |p| {
            if p.len() < 5 || p[4] != kind {
                return Ok(Packet::Ignore);
            }
            data.extend_from_slice(&p[5..]);
            Ok(Packet::More)
        },
    )?;
    Ok((data, rtt))
}

struct Reader<'a> {
    d: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn new(d: &'a [u8]) -> Self {
        Self { d, i: 0 }
    }

    fn done(&self) -> bool {
        self.i >= self.d.len()
    }

    fn u8(&mut self) -> Option<u8> {
        let v = *self.d.get(self.i)?;
        self.i += 1;
        Some(v)
    }

    fn u32(&mut self) -> Option<u32> {
        let b = self.d.get(self.i..self.i + 4)?;
        self.i += 4;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Length-prefixed string: Latin-1 up to its zero byte (the length is not reliable),
    /// or UCS-2 when bit 7 of the length is set.
    fn string(&mut self) -> Option<String> {
        let len = self.u8()?;
        if len >= 0x80 {
            let bytes = (usize::from(len) & 0x7f) * 2;
            // Some games put an extra 0x01 here that the length does not count.
            if self.d.get(self.i) == Some(&1) {
                self.i += 1;
            }
            let raw = self.d.get(self.i..self.i + bytes)?;
            self.i += bytes;
            let units: Vec<u16> = raw
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .take_while(|&u| u != 0)
                .collect();
            return Some(clean(&String::from_utf16_lossy(&units)));
        }
        let start = self.i;
        let end = self.d[start..]
            .iter()
            .position(|&b| b == 0)
            .map_or(self.d.len(), |p| start + p);
        self.i = (end + 1).min(self.d.len());
        Some(clean(&super::latin1(&self.d[start..end])))
    }
}

/// Removes Unreal colour codes (ESC + three bytes) and control characters.
fn clean(s: &str) -> String {
    let mut out = String::new();
    let mut skip = 0;
    for c in s.chars() {
        if skip > 0 {
            skip -= 1;
            continue;
        }
        if c == '\x1b' {
            skip = 3;
            continue;
        }
        if !c.is_control() {
            out.push(c);
        }
    }
    out
}

fn parse_info(d: &[u8]) -> Option<ServerInfo> {
    let mut r = Reader::new(d);
    let _server_id = r.u32()?;
    let _ip = r.string()?;
    let game_port = r.u32()?;
    let _query_port = r.u32()?;
    let name = r.string()?;
    let map = r.string()?;
    let game = r.string()?;
    let players = r.u32()?;
    let max_players = r.u32()?;
    Some(ServerInfo {
        name,
        map,
        game: game.clone(),
        folder: game,
        players: count(players),
        max_players: count(max_players),
        game_port: u16::try_from(game_port).ok().filter(|&p| p > 0),
        server_type: b'd',
        ..Default::default()
    })
}

fn parse_rules(d: &[u8]) -> Vec<crate::a2s::Rule> {
    let mut r = Reader::new(d);
    let mut pairs = Vec::new();
    while !r.done() {
        let (Some(k), Some(v)) = (r.string(), r.string()) else {
            break;
        };
        pairs.push((k, v));
    }
    rules_from(pairs)
}

fn parse_players(d: &[u8]) -> Vec<Player> {
    let mut r = Reader::new(d);
    let mut out = Vec::new();
    while !r.done() {
        let (Some(id), Some(name), Some(ping), Some(score), Some(_stats)) =
            (r.u32(), r.string(), r.u32(), r.u32(), r.u32())
        else {
            break;
        };
        // id 0 = team scores, not a player.
        if id == 0 {
            continue;
        }
        out.push(Player {
            index: count(out.len()),
            name,
            score: score as i32,
            ping: Some(ping),
            bot: ping == 0,
            ..Default::default()
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> Vec<u8> {
        let mut v = vec![(text.len() + 1) as u8];
        v.extend_from_slice(text.as_bytes());
        v.push(0);
        v
    }

    #[test]
    fn parses_info_rules_players() {
        let mut info = 1u32.to_le_bytes().to_vec();
        info.extend(s("1.2.3.4"));
        info.extend(7777u32.to_le_bytes());
        info.extend(7778u32.to_le_bytes());
        info.extend(s("\x1b\x10\x20\x30UT2004 Server"));
        info.extend(s("DM-Rankin"));
        info.extend(s("xDeathMatch"));
        info.extend(5u32.to_le_bytes());
        info.extend(16u32.to_le_bytes());
        let i = parse_info(&info).unwrap();
        assert_eq!(
            (i.name.as_str(), i.map.as_str()),
            ("UT2004 Server", "DM-Rankin")
        );
        assert_eq!((i.players, i.max_players, i.game_port), (5, 16, Some(7777)));

        let mut rules = s("GamePassword");
        rules.extend(s("True"));
        assert!(truthy(rule(&parse_rules(&rules), "GamePassword")));

        let mut players = 3u32.to_le_bytes().to_vec();
        players.extend(s("Alice"));
        players.extend(50u32.to_le_bytes());
        players.extend(12u32.to_le_bytes());
        players.extend(0u32.to_le_bytes());
        let p = parse_players(&players);
        assert_eq!(
            (p[0].name.as_str(), p[0].score, p[0].ping),
            ("Alice", 12, Some(50))
        );

        // UCS-2 name
        let mut u = vec![0x80 | 3];
        u.extend_from_slice(&[b'U', 0, b'T', 0, 0, 0]);
        assert_eq!(Reader::new(&u).string().unwrap(), "UT");
    }
}
