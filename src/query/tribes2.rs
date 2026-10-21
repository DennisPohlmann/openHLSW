//! Tribes 2 (UDP, port 28000), as described by QStat.
//!
//! "Ping" request `0E 02 <key>` → `10 …` with the query version (`VER3` / `VER5`),
//! protocol and build numbers and the server name. "Info" request `12 02 <key>` →
//! `14 …` with game type, mission type, map, status flags, players, max players, bots,
//! CPU speed, an info text and a text block with the teams and players.

use std::net::SocketAddr;
use std::time::Duration;

use super::{Packet, Status, count, latin1, meta, rules_from};
use crate::a2s::{Player, QueryError, QueryMeta, ServerInfo};

const PING: [u8; 6] = [14, 2, 1, 2, 3, 4];
const INFO: [u8; 6] = [18, 2, 1, 2, 3, 4];

pub fn query(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let mut name = None;
    let _ = super::udp_exchange(addr, &PING, timeout, None, |p| {
        if p.first() != Some(&16) {
            return Ok(Packet::Ignore);
        }
        name = parse_ping(p);
        Ok(Packet::Done)
    });
    let mut status = None;
    let rtt = super::udp_exchange(addr, &INFO, timeout, None, |p| {
        if p.first() != Some(&20) {
            return Ok(Packet::Ignore);
        }
        status = parse_info(p);
        Ok(Packet::Done)
    })?;
    let mut status = status.ok_or_else(|| QueryError::Protocol("Tribes 2: bad answer".into()))?;
    if let Some(name) = name
        && !name.is_empty()
        && !name.starts_with("VER")
    {
        status.info.name = name;
    }
    Ok((status, meta(rtt)))
}

fn pascal(d: &[u8], i: &mut usize) -> Option<String> {
    let len = usize::from(*d.get(*i)?);
    let s = latin1(d.get(*i + 1..*i + 1 + len)?);
    *i += 1 + len;
    Some(s)
}

/// Server name from the ping answer.
pub fn parse_ping(p: &[u8]) -> Option<String> {
    let vlen = usize::from(*p.get(6)?);
    let version = latin1(p.get(7..7 + vlen)?);
    let mut i = 7 + vlen;
    if version.ends_with('5') {
        i += 4; // net protocol
    }
    i += 8; // minimum net protocol, build version
    pascal(p, &mut i)
}

pub fn parse_info(p: &[u8]) -> Option<Status> {
    let mut i = 6;
    let game_type = pascal(p, &mut i)?;
    let mission = pascal(p, &mut i)?;
    let map = pascal(p, &mut i)?;
    let flags = *p.get(i)?;
    let players = *p.get(i + 1)?;
    let max = *p.get(i + 2)?;
    let bots = *p.get(i + 3)?;
    i += 6; // + CPU speed
    let info_text = pascal(p, &mut i).unwrap_or_default();
    let len = usize::from(u16::from_le_bytes([*p.get(i)?, *p.get(i + 1)?]));
    i += 2;
    let text = latin1(p.get(i..(i + len).min(p.len()))?);
    let list = parse_lists(&text);
    let info = ServerInfo {
        name: info_text.clone(),
        map,
        game: game_type.clone(),
        folder: game_type.clone(),
        players: count(u32::from(players).max(list.len() as u32)),
        max_players: max,
        bots,
        passworded: flags & 2 != 0,
        server_type: if flags & 1 != 0 { b'd' } else { b'l' },
        environment: if flags & 4 != 0 { b'l' } else { b'w' },
        ..Default::default()
    };
    let rules = rules_from([
        ("gametype".to_string(), game_type),
        ("missiontype".to_string(), mission),
        ("info".to_string(), info_text),
        (
            "teamdamage".to_string(),
            u8::from(flags & 32 != 0).to_string(),
        ),
    ]);
    Some(Status {
        info,
        players: list,
        rules,
    })
}

/// "<teams>\n" + "name\tscore\n" per team, "<players>\n" + per player
/// `0x10 name 0x11 0x09 team 0x09 score 0x0a`.
fn parse_lists(text: &str) -> Vec<Player> {
    let mut lines = text.split('\n');
    let teams: usize = lines
        .next()
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0);
    for _ in 0..teams {
        lines.next();
    }
    let n: usize = lines
        .next()
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0);
    lines
        .take(n)
        .enumerate()
        .filter_map(|(i, l)| {
            let l = l.trim_start_matches('\x10');
            let (name, rest) = l.split_once('\x11')?;
            let mut f = rest.trim_start_matches('\t').split('\t');
            let team = f.next().unwrap_or_default().to_string();
            let score = f.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
            Some(Player {
                index: count(i),
                name: name.to_string(),
                team,
                score,
                ..Default::default()
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ps(t: &str) -> Vec<u8> {
        let mut v = vec![t.len() as u8];
        v.extend(t.as_bytes());
        v
    }

    #[test]
    fn parses_ping_and_info() {
        let mut ping = vec![16, 0, 1, 2, 3, 4, 4];
        ping.extend(b"VER3");
        ping.extend([0; 8]);
        ping.extend(ps("Tribes Srv"));
        assert_eq!(parse_ping(&ping).unwrap(), "Tribes Srv");

        let mut info = vec![20, 0, 1, 2, 3, 4];
        info.extend(ps("Classic"));
        info.extend(ps("CTF"));
        info.extend(ps("Katabatic"));
        info.extend([1 | 2, 1, 32, 0, 0, 0]);
        info.extend(ps("Welcome"));
        let text = "2\nStorm\t3\nInferno\t1\n1\n\x10Alice\x11\tStorm\t15\n";
        info.extend((text.len() as u16).to_le_bytes());
        info.extend(text.as_bytes());
        let s = parse_info(&info).unwrap();
        assert_eq!(
            (s.info.map.as_str(), s.info.players, s.info.max_players),
            ("Katabatic", 1, 32)
        );
        assert!(s.info.passworded);
        assert_eq!(
            (
                s.players[0].name.as_str(),
                s.players[0].team.as_str(),
                s.players[0].score
            ),
            ("Alice", "Storm", 15)
        );
    }
}
