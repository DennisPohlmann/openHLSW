//! GameSpy 2 query protocol (UDP), used by Deer Hunter 2005, Battlefield Vietnam,
//! Halo, America's Army and others, and its GameSpy 3 form (Battlefield 2 / 2142, Crysis,
//! ArmA …) with answers split over several packets. The query port differs from the game port, so these
//! servers are entered as `ip:gameport:queryport`.
//!
//! Request:  FE FD 00 <id:4> <info> <players> <teams>   (each 0xFF = wanted, 0x00 = not)
//! Response: 00 <id:4> then the requested sections:
//!   info:    key\0value\0 ... \0
//!   players: [00] [count] field_\0 ... \0 value\0 ... (one value per field and player)

use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::a2s::{Player, QueryError, QueryMeta, Rule, ServerInfo, decode_text};

const REQUEST_ID: [u8; 4] = *b"oHLS";
/// Marks a GameSpy 3 answer split over several packets (Battlefield 2, 2142, Crysis, …).
const SPLITNUM: &[u8] = b"splitnum\0";

/// Query variant a server answers to. Battlefield 2 and newer GameSpy games use the
/// GameSpy 3 form of the request (and some of them a challenge first); the answer may come
/// in several packets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// `FE FD 00 id <info> <players> <teams>`, one answer packet.
    Gs2,
    /// `FE FD 00 id FF FF FF 01`, answer split into packets.
    Gs3,
    /// Like Gs3, but after `FE FD 09 id` (challenge), e.g. Battlefield 2142.
    Gs3Challenge,
}

/// Variant that worked last per server.
fn modes() -> &'static Mutex<HashMap<SocketAddr, Mode>> {
    static MODES: OnceLock<Mutex<HashMap<SocketAddr, Mode>>> = OnceLock::new();
    MODES.get_or_init(Default::default)
}

/// Sends `packet` (retried once) and returns the payloads after the 5 byte header of the
/// answer(s) starting with `kind`. GameSpy 3 answers split over several packets are
/// collected until the last one; the payloads are then sorted, without "splitnum".
fn exchange(
    socket: &UdpSocket,
    packet: &[u8],
    kind: u8,
    timeout: Duration,
    attempts: u32,
) -> Result<(Vec<Vec<u8>>, bool, QueryMeta), QueryError> {
    let mut buf = vec![0u8; 65536];
    for attempt in 1..=attempts {
        socket.send(packet)?;
        crate::stats::add_sent(packet.len() + 28);
        let sent = Instant::now();
        let deadline = sent + timeout;
        // Split answer: packet number -> payload, plus the number of the last packet.
        let mut parts: HashMap<u8, Vec<u8>> = HashMap::new();
        let mut last: Option<u8> = None;
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
            if n < 5 || buf[0] != kind || buf[1..5] != REQUEST_ID {
                continue;
            }
            let meta = || QueryMeta {
                rtt: sent.elapsed(),
                packets: attempt,
                challenge: None,
            };
            let payload = &buf[5..n];
            let Some(rest) = payload.strip_prefix(SPLITNUM) else {
                return Ok((vec![payload.to_vec()], false, meta()));
            };
            let Some((&num, data)) = rest.split_first() else {
                continue;
            };
            let index = num & 0x7F;
            if num & 0x80 != 0 {
                last = Some(index);
            }
            parts.insert(index, data.to_vec());
            if let Some(l) = last
                && (0..=l).all(|i| parts.contains_key(&i))
            {
                let payloads = (0..=l).filter_map(|i| parts.remove(&i)).collect();
                return Ok((payloads, true, meta()));
            }
        }
    }
    Err(QueryError::Timeout)
}

/// Info (`rules`) and players of one answer.
#[derive(Default)]
struct Reply {
    rules: Vec<Rule>,
    players: Vec<Player>,
}

/// GameSpy 3 sections: 00 = server keys, 01 = player fields, 02 = team fields.
/// Each packet starts with a section number; a field list may continue in the next packet,
/// starting again with the field name and the index of its first value.
fn parse_gs3(payloads: &[Vec<u8>]) -> Reply {
    let mut rules: Vec<Rule> = Vec::new();
    let mut fields: Vec<(String, Vec<String>)> = Vec::new();
    let mut team_fields: Vec<(String, Vec<String>)> = Vec::new();
    for data in payloads {
        let mut pos = 0;
        while pos < data.len() {
            let section = data[pos];
            pos += 1;
            let mut s = Strings { data, pos };
            if section == 0 {
                while let Some(key) = s.next() {
                    if key.is_empty() {
                        break;
                    }
                    let value = s.next().unwrap_or_default();
                    if !rules.iter().any(|r| r.key == key) {
                        rules.push(Rule { key, value });
                    }
                }
                pos = s.pos;
                continue;
            }
            // Player (1) or team (2) fields: name\0 <offset> value\0 ... \0, ..., \0
            while let Some(name) = s.next() {
                if name.is_empty() {
                    break;
                }
                let Some(&offset) = data.get(s.pos) else {
                    break;
                };
                s.pos += 1;
                let mut values = Vec::new();
                while let Some(v) = s.next() {
                    if v.is_empty() {
                        break;
                    }
                    values.push(v);
                }
                if section == 1 || section == 2 {
                    let table = if section == 1 {
                        &mut fields
                    } else {
                        &mut team_fields
                    };
                    let key = name.trim_end_matches('_').to_ascii_lowercase();
                    let i = match table.iter().position(|(k, _)| *k == key) {
                        Some(i) => i,
                        None => {
                            table.push((key, Vec::new()));
                            table.len() - 1
                        }
                    };
                    let column = &mut table[i].1;
                    let start = offset as usize;
                    if column.len() < start {
                        column.resize(start, String::new());
                    }
                    column.truncate(start);
                    column.extend(values);
                }
            }
            pos = s.pos;
        }
    }
    // Team names (team_t: "MECSF", "US" …) for the team numbers of the players.
    let team_names: Vec<String> = team_fields
        .iter()
        .find(|(k, _)| k == "team_t" || k == "team")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    let players = players_from_columns(&fields, &team_names);
    Reply { rules, players }
}

/// Players from GameSpy field columns (`player_`, `score_`, `deaths_`, `ping_`, `team_`,
/// `skill_`, `AIBot_`, …). Team numbers are replaced by the team names when known.
fn players_from_columns(fields: &[(String, Vec<String>)], team_names: &[String]) -> Vec<Player> {
    let count = fields
        .iter()
        .find(|(k, _)| k == "player" || k == "name" || k == "playername")
        .map_or(0, |(_, v)| v.len());
    let get = |keys: &[&str], i: usize| {
        fields
            .iter()
            .find(|(k, _)| keys.contains(&k.as_str()))
            .and_then(|(_, v)| v.get(i))
            .map(|v| v.trim().to_string())
    };
    (0..count)
        .map(|i| {
            let team = get(&["team"], i).unwrap_or_default();
            let team = team
                .parse::<usize>()
                .ok()
                .and_then(|n| team_names.get(n.checked_sub(1)?))
                .cloned()
                .unwrap_or(team);
            Player {
                index: i as u8,
                name: get(&["player", "name", "playername"], i).unwrap_or_default(),
                score: get(&["score", "frags", "kills"], i)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
                duration: get(&["time"], i)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.0),
                deaths: get(&["deaths"], i).and_then(|v| v.parse().ok()),
                ping: get(&["ping"], i).and_then(|v| v.parse().ok()),
                skill: get(&["skill"], i).and_then(|v| v.parse().ok()),
                team,
                bot: get(&["aibot", "bot"], i).is_some_and(|v| v == "1"),
            }
        })
        .collect()
}

/// Queries info / players with the variant the server understands.
fn query(
    addr: SocketAddr,
    timeout: Duration,
    players: bool,
) -> Result<(Reply, QueryMeta), QueryError> {
    let socket = UdpSocket::bind(if addr.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    })?;
    socket.connect(addr)?;
    let known = modes().lock().ok().and_then(|m| m.get(&addr).copied());
    let result = match known {
        // Known variant: as usual with one retry.
        Some(mode) => query_with(&socket, mode, timeout, players, 2).map(|r| (r, mode)),
        // Unknown server: try each variant once.
        None => {
            let mut result = Err(QueryError::Timeout);
            for mode in [Mode::Gs2, Mode::Gs3, Mode::Gs3Challenge] {
                result = query_with(&socket, mode, timeout, players, 1).map(|r| (r, mode));
                if !matches!(result, Err(QueryError::Timeout)) {
                    break;
                }
            }
            result
        }
    };
    match result {
        Ok((reply, mode)) => {
            if let Ok(mut m) = modes().lock() {
                m.insert(addr, mode);
            }
            Ok(reply)
        }
        Err(e) => {
            // Forget the variant after a timeout, so it is detected again.
            if matches!(e, QueryError::Timeout)
                && let Ok(mut m) = modes().lock()
            {
                m.remove(&addr);
            }
            Err(e)
        }
    }
}

fn query_with(
    socket: &UdpSocket,
    mode: Mode,
    timeout: Duration,
    players: bool,
    attempts: u32,
) -> Result<(Reply, QueryMeta), QueryError> {
    let mut packet = vec![0xFE, 0xFD, 0x00];
    packet.extend_from_slice(&REQUEST_ID);
    if mode == Mode::Gs2 {
        packet.extend_from_slice(if players {
            &[0, 0xFF, 0]
        } else {
            &[0xFF, 0, 0]
        });
        let (payloads, split, meta) = exchange(socket, &packet, 0, timeout, attempts)?;
        // A GameSpy 3 server may answer the old request in the new, split format.
        if split {
            return Ok((parse_gs3(&payloads), meta));
        }
        let data = payloads.into_iter().next().unwrap_or_default();
        let reply = if players {
            Reply {
                players: parse_players(&data),
                ..Default::default()
            }
        } else {
            Reply {
                rules: parse_rules(&data),
                ..Default::default()
            }
        };
        return Ok((reply, meta));
    }
    let mut packets = 0;
    if mode == Mode::Gs3Challenge {
        let mut req = vec![0xFE, 0xFD, 0x09];
        req.extend_from_slice(&REQUEST_ID);
        let (answer, _, meta) = exchange(socket, &req, 0x09, timeout, attempts)?;
        packets = meta.packets;
        let text = answer.first().map(|a| decode_text(a)).unwrap_or_default();
        let number: i64 = text
            .trim_matches(char::from(0))
            .trim()
            .parse()
            .map_err(|_| QueryError::Protocol("Bad challenge".into()))?;
        packet.extend_from_slice(&(number as i32).to_be_bytes());
    }
    packet.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0x01]);
    let (payloads, _, mut meta) = exchange(socket, &packet, 0, timeout, attempts)?;
    meta.packets += packets;
    Ok((parse_gs3(&payloads), meta))
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
    let variant = get("gamevariant");
    let game = match gamename.to_ascii_lowercase().as_str() {
        "dh2005" | "deerhunter2005" | "deerhunt2005" => "Deer Hunter 2005".to_string(),
        "battlefield2" => "Battlefield 2".to_string(),
        "stella" => "Battlefield 2142".to_string(),
        "bfvietnam" => "Battlefield Vietnam".to_string(),
        "armedass" => "Armed Assault".to_string(),
        "arma2" | "arma2pc" => "ARMA 2".to_string(),
        "crysis" => "Crysis".to_string(),
        "crysiswars" => "Crysis Wars".to_string(),
        "" => "GameSpy 2".to_string(),
        _ => {
            if variant.is_empty() {
                gamename.clone()
            } else {
                variant.clone()
            }
        }
    };
    // Battlefield 2 / 2142 run mods (Special Forces = "xpack"): keep the mod in the folder,
    // like HLSW's "mods/<GAMEDIR>".
    let folder = match gamename.to_ascii_lowercase().as_str() {
        "battlefield2" | "stella"
            if !variant.is_empty()
                && !variant.eq_ignore_ascii_case("bf2")
                && !variant.eq_ignore_ascii_case("bf2142") =>
        {
            format!("{gamename}/{variant}")
        }
        _ => gamename.clone(),
    };
    ServerInfo {
        name: get("hostname"),
        map: get("mapname"),
        folder,
        game,
        players: num("numplayers"),
        max_players: num("maxplayers"),
        server_type: if get("dedicated") == "0" || get("bf2_dedicated") == "0" {
            b'l'
        } else {
            b'd'
        },
        // Battlefield 2 reports the system as bf2_os ("linux-64", "win32").
        environment: match format!("{}{}", get("os"), get("bf2_os"))
            .to_ascii_lowercase()
            .as_str()
        {
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
                "deaths" => p.deaths = v.trim().parse().ok(),
                "ping" => p.ping = v.trim().parse().ok(),
                "skill" => p.skill = v.trim().parse().ok(),
                "team" => p.team = v.trim().to_string(),
                "aibot" | "bot" => p.bot = v.trim() == "1",
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
    let (reply, meta) = query(addr, timeout, false)?;
    let mut info = info_from_rules(&reply.rules);
    // Battlefield 2 counts only humans in numplayers; bots are flagged per player.
    let bots = reply.players.iter().filter(|p| p.bot).count();
    if bots > 0 {
        info.bots = bots.min(255) as u8;
        info.players = info
            .players
            .saturating_add(info.bots)
            .max(reply.players.len().min(255) as u8);
    } else if !reply.players.is_empty() && info.players == 0 {
        info.players = reply.players.len().min(255) as u8;
    }
    Ok((info, meta))
}

pub fn query_rules(
    addr: SocketAddr,
    timeout: Duration,
) -> Result<(Vec<Rule>, QueryMeta), QueryError> {
    let (reply, meta) = query(addr, timeout, false)?;
    Ok((reply.rules, meta))
}

/// Name of a Battlefield 2 / 2142 mod (`gamevariant`), as HLSW shows it.
pub fn bf_mod_name(dir: &str) -> String {
    match dir.to_ascii_lowercase().as_str() {
        "" | "bf2" | "bf2142" => String::new(),
        "xpack" => "Special Forces Expansion".into(),
        "airmaps" => "Airmaps".into(),
        "aix2" => "Allied Intent Extended".into(),
        "battleracer2" => "Battleracer 2".into(),
        "bf2pro" => "Pro-Mod".into(),
        "fh2" => "Forgotten Hope 2".into(),
        "hardjustice" => "Hard Justice".into(),
        "opk2" => "Operation Peacekeeper".into(),
        "poe2" => "Point of Existence 2".into(),
        "pr" => "Project Reality".into(),
        "sandbox" => "Sandbox".into(),
        "northernstrike" => "Northern Strike".into(),
        _ => dir.to_string(),
    }
}

pub fn query_players(
    addr: SocketAddr,
    timeout: Duration,
) -> Result<(Vec<Player>, QueryMeta), QueryError> {
    let (reply, meta) = query(addr, timeout, true)?;
    Ok((reply.players, meta))
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

    #[test]
    fn parses_gamespy3_split_answer() {
        // Battlefield 2: keys in packet 0, players split over packets 0 and 1.
        let p0 = b"\x00hostname\0Lost Soldiers\0gamename\0battlefield2\0mapname\0Kubra Dam\0numplayers\x002\0maxplayers\x0064\0\0\x01player_\0\x00Bob\0\0score_\0\x0010\0\0\0".to_vec();
        let p1 = b"\x01player_\0\x01Ann\0\0score_\0\x015\0\0\0".to_vec();
        let r = parse_gs3(&[p0, p1]);
        let i = info_from_rules(&r.rules);
        assert_eq!(i.name, "Lost Soldiers");
        assert_eq!(i.game, "Battlefield 2");
        assert_eq!(i.folder, "battlefield2");
        let mut sf = r.rules.clone();
        sf.push(Rule {
            key: "gamevariant".into(),
            value: "xpack".into(),
        });
        assert_eq!(info_from_rules(&sf).folder, "battlefield2/xpack");
        assert_eq!(bf_mod_name("xpack"), "Special Forces Expansion");

        // Team section and per-player extra columns.
        let p0 = b"\x00hostname\0X\0bf2_os\0linux-64\0\0\x01player_\0\x00Bot1\0\0deaths_\0\x003\0\0ping_\0\x0045\0\0team_\0\x002\0\0skill_\0\x007\0\0AIBot_\0\x001\0\0\0\x02team_t\0\x00US\0MECSF\0\0\0".to_vec();
        let r2 = parse_gs3(&[p0]);
        let b = &r2.players[0];
        assert_eq!(
            (b.deaths, b.ping, b.skill, b.team.as_str(), b.bot),
            (Some(3), Some(45), Some(7), "MECSF", true)
        );
        assert_eq!(info_from_rules(&r2.rules).environment, b'l');
        assert_eq!((i.players, i.max_players), (2, 64));
        assert_eq!(r.players.len(), 2);
        assert_eq!((r.players[1].name.as_str(), r.players[1].score), ("Ann", 5));
    }
}
