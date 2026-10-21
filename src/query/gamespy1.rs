//! GameSpy 1 (`\status\`): Unreal Tournament, Battlefield 1942, Halo, Vietcong, Operation
//! Flashpoint, Serious Sam, Medal of Honor (port + 97) …
//!
//! The answer is `\key\value\…`, possibly split over several packets that carry
//! `\queryid\<id>.<part>` and end with `\final\` in the last one. Players come as
//! `player_0`, `frags_0`, `ping_0`, `team_0` …

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;

use super::{Packet, Status, count, latin1, meta, rule, rules_from, truthy};
use crate::a2s::{Player, QueryError, QueryMeta, ServerInfo};

pub fn query(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let mut parts: BTreeMap<u32, Vec<(String, String)>> = BTreeMap::new();
    let mut last_part = None;
    // Answers without part numbers (e.g. Halo) end after a short pause.
    let rtt = super::udp_exchange(
        addr,
        b"\\status\\",
        timeout,
        Some(Duration::from_millis(250)),
        |p| {
            let Some(fields) = split(&latin1(p)) else {
                return Ok(Packet::Ignore);
            };
            let mut part = parts.len() as u32 + 1;
            let mut fin = false;
            let mut data = Vec::new();
            for (k, v) in fields {
                match k.as_str() {
                    "queryid" => {
                        if let Some(n) = v.split('.').nth(1).and_then(|n| n.parse().ok()) {
                            part = n;
                        }
                    }
                    "final" => fin = true,
                    _ => data.push((k, v)),
                }
            }
            parts.insert(part, data);
            if fin {
                last_part = Some(part);
            }
            Ok(match last_part {
                Some(last) if (1..=last).all(|n| parts.contains_key(&n)) => Packet::Done,
                _ => Packet::More,
            })
        },
    )?;
    if parts.is_empty() {
        return Err(QueryError::Timeout);
    }
    let fields: Vec<(String, String)> = parts.into_values().flatten().collect();
    Ok((from_fields(fields), meta(rtt)))
}

/// `\key\value\…` into pairs; `None` if it is not that format.
fn split(text: &str) -> Option<Vec<(String, String)>> {
    let text = text.trim_end_matches('\0');
    let body = text.strip_prefix('\\')?;
    let mut it = body.split('\\');
    let mut out = Vec::new();
    while let Some(k) = it.next() {
        if k.is_empty() {
            continue;
        }
        out.push((k.to_string(), it.next().unwrap_or_default().to_string()));
    }
    Some(out)
}

/// Builds the status from all `key, value` pairs of the answer.
pub fn from_fields(fields: Vec<(String, String)>) -> Status {
    let mut players: BTreeMap<u32, Player> = BTreeMap::new();
    let mut team_names: BTreeMap<u32, String> = BTreeMap::new();
    let mut plain = Vec::new();
    for (k, v) in fields {
        let lower = k.to_ascii_lowercase();
        let indexed = lower
            .rsplit_once('_')
            .and_then(|(name, n)| Some((name.to_string(), n.parse::<u32>().ok()?)));
        let Some((name, n)) = indexed else {
            plain.push((k, v));
            continue;
        };
        if name == "teamname" {
            team_names.insert(n, v);
            continue;
        }
        if name.starts_with("team") && name != "team" {
            continue; // team scores etc.
        }
        let p = players.entry(n).or_default();
        let int = || v.trim().parse::<f32>().unwrap_or(0.0) as i32;
        match name.as_str() {
            "player" | "playername" => p.name = v.clone(),
            "frags" | "score" | "kills" => p.score = int(),
            "deaths" => p.deaths = Some(int()),
            "ping" => p.ping = Some(int().max(0) as u32),
            "team" => p.team = v.clone(),
            "skill" => p.skill = Some(int()),
            "time" => p.duration = int() as f32,
            _ => {}
        }
    }
    let mut players: Vec<Player> = players
        .into_values()
        .filter(|p| !p.name.is_empty())
        .collect();
    for (i, p) in players.iter_mut().enumerate() {
        p.index = count(i);
        if let Ok(t) = p.team.parse::<u32>()
            && let Some(name) = team_names.get(&t)
        {
            p.team = name.clone();
        }
        p.bot = p.ping == Some(0);
    }
    let rules = rules_from(plain);
    let get = |k: &str| rule(&rules, k).unwrap_or_default().to_string();
    let num = |k: &str| get(k).trim().parse::<u32>().unwrap_or(0);
    let info = ServerInfo {
        name: get("hostname"),
        map: {
            let m = get("mapname");
            if m.is_empty() { get("maptitle") } else { m }
        },
        folder: get("gamename"),
        game: {
            let t = get("gametype");
            if t.is_empty() { get("gamename") } else { t }
        },
        players: count(num("numplayers").max(players.len() as u32)),
        max_players: count(num("maxplayers")),
        bots: count(players.iter().filter(|p| p.bot).count()),
        passworded: truthy(rule(&rules, "password")) || truthy(rule(&rules, "passworden")),
        version: get("gamever"),
        game_port: get("hostport").parse().ok(),
        server_type: if get("dedicated") == "0" { b'l' } else { b'd' },
        ..Default::default()
    };
    Status {
        info,
        players,
        rules,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_parts_and_players() {
        let a = split("\\hostname\\UT Server\\gamename\\ut\\mapname\\DM-Deck16\\numplayers\\2\\maxplayers\\16\\queryid\\12.1").unwrap();
        let b = split("\\player_0\\Alice\\frags_0\\7\\ping_0\\40\\team_0\\1\\player_1\\Bot\\ping_1\\0\\teamname_1\\Blue\\password\\0\\queryid\\12.2\\final\\").unwrap();
        let fields: Vec<_> = a
            .into_iter()
            .chain(b)
            .filter(|(k, _)| k != "queryid" && k != "final")
            .collect();
        let s = from_fields(fields);
        assert_eq!(s.info.name, "UT Server");
        assert_eq!(s.info.map, "DM-Deck16");
        assert_eq!(
            (s.info.players, s.info.max_players, s.info.bots),
            (2, 16, 1)
        );
        assert_eq!(s.players[0].name, "Alice");
        assert_eq!((s.players[0].score, s.players[0].ping), (7, Some(40)));
        assert_eq!(s.players[0].team, "Blue");
        assert!(!s.info.passworded);
    }
}
