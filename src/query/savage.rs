//! Savage: The Battle for Newerth (UDP, port 11235), as described by QStat.
//!
//! Two fixed requests (server info and players). The answers hold `FF key FE value`
//! pairs; `players` is a text block: a team name line, the team's players, the next
//! line starting with "Team " and so on.

use std::net::SocketAddr;
use std::time::Duration;

use super::{Packet, Status, count, latin1, meta, rules_from};
use crate::a2s::{Player, QueryError, QueryMeta, ServerInfo};

const SERVER_QUERY: [u8; 9] = [0x9e, 0x4c, 0x23, 0x00, 0x00, 0xc8, 0x01, 0x21, 0x00];
const PLAYER_QUERY: [u8; 9] = [0x9e, 0x4c, 0x23, 0x00, 0x00, 0xce, 0x76, 0x46, 0x00];

pub fn query(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let mut pairs = Vec::new();
    let rtt = super::udp_exchange(addr, &SERVER_QUERY, timeout, None, |p| {
        let found = fields(p);
        if found.is_empty() {
            return Ok(Packet::Ignore);
        }
        pairs.extend(found);
        Ok(Packet::Done)
    })?;
    let _ = super::udp_exchange(addr, &PLAYER_QUERY, timeout, None, |p| {
        let found = fields(p);
        if found.is_empty() {
            return Ok(Packet::Ignore);
        }
        pairs.extend(found);
        Ok(Packet::Done)
    });
    Ok((from_fields(pairs), meta(rtt)))
}

/// `FF key FE value` pairs of a packet.
pub fn fields(p: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for chunk in p.split(|&b| b == 0xff).skip(1) {
        let Some(sep) = chunk.iter().position(|&b| b == 0xfe) else {
            continue;
        };
        let value = chunk[sep + 1..]
            .split(|&b| b == 0)
            .next()
            .unwrap_or_default();
        out.push((latin1(&chunk[..sep]), latin1(value)));
    }
    out
}

pub fn from_fields(pairs: Vec<(String, String)>) -> Status {
    let get = |k: &str| {
        pairs
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let mut players = Vec::new();
    let block = get("players");
    let mut lines = block.split('\n');
    let mut team = lines.next().unwrap_or_default().to_string();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if line.starts_with("Team ") {
            team = line.to_string();
            continue;
        }
        players.push(Player {
            index: count(players.len()),
            name: line.to_string(),
            team: team.clone(),
            ..Default::default()
        });
    }
    let info = ServerInfo {
        name: get("name"),
        map: get("world"),
        game: {
            let g = get("gametype");
            if g.is_empty() { "Savage".into() } else { g }
        },
        players: count(get("cnum").parse::<u32>().unwrap_or(players.len() as u32)),
        max_players: count(get("cmax").parse::<u32>().unwrap_or(0)),
        server_type: b'd',
        ..Default::default()
    };
    Status {
        info,
        players,
        rules: rules_from(pairs.into_iter().filter(|(k, _)| k != "players")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_savage() {
        let mut p = vec![0x9e, 0x4c, 0x23];
        for (k, v) in [
            ("name", "Savage Srv"),
            ("world", "eden2"),
            ("cnum", "2"),
            ("cmax", "32"),
            ("players", "Team 1\nAlice\nTeam 2\nBob\n"),
        ] {
            p.push(0xff);
            p.extend(k.as_bytes());
            p.push(0xfe);
            p.extend(v.as_bytes());
        }
        let s = from_fields(fields(&p));
        assert_eq!(
            (s.info.name.as_str(), s.info.map.as_str()),
            ("Savage Srv", "eden2")
        );
        assert_eq!((s.info.players, s.info.max_players), (2, 32));
        assert_eq!(
            (s.players[1].name.as_str(), s.players[1].team.as_str()),
            ("Bob", "Team 2")
        );
    }
}
