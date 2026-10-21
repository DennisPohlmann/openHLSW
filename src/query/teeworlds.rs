//! Teeworlds 0.6 / DDNet server info (UDP, game port 8303).
//!
//! Request: ten `FF` bytes, `gie3` and a token byte. Answer: ten `FF` bytes, `inf3`, then
//! zero-terminated strings: token, version, name, map, game type, flags, players, max
//! players, clients, max clients, and per client name, clan, country, score, is-player.

use std::net::SocketAddr;
use std::time::Duration;

use super::{Packet, Status, count, latin1, meta, rules_from};
use crate::a2s::{Player, QueryError, QueryMeta, ServerInfo};

const PREFIX: [u8; 10] = [0xff; 10];
const TOKEN: u8 = 0x2a;

pub fn query(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let mut req = PREFIX.to_vec();
    req.extend(b"gie3");
    req.push(TOKEN);
    let mut status = None;
    let rtt = super::udp_exchange(addr, &req, timeout, None, |p| {
        match p
            .strip_prefix(&PREFIX[..])
            .and_then(|p| p.strip_prefix(b"inf3"))
            .and_then(parse)
        {
            Some(s) => {
                status = Some(s);
                Ok(Packet::Done)
            }
            None => Ok(Packet::Ignore),
        }
    })?;
    Ok((status.ok_or(QueryError::Timeout)?, meta(rtt)))
}

pub fn parse(body: &[u8]) -> Option<Status> {
    let mut f = body.split(|&b| b == 0).map(latin1);
    let _token = f.next()?;
    let version = f.next()?;
    let name = f.next()?;
    let map = f.next()?;
    let game_type = f.next()?;
    let flags: u32 = f.next()?.parse().unwrap_or(0);
    let _players = f.next()?;
    let _max_players = f.next()?;
    let clients: u32 = f.next()?.parse().unwrap_or(0);
    let max_clients: u32 = f.next()?.parse().unwrap_or(0);
    let mut players = Vec::new();
    let rest: Vec<String> = f.collect();
    for c in rest.chunks(5) {
        let [name, clan, _country, score, is_player] = c else {
            break;
        };
        players.push(Player {
            index: count(players.len()),
            name: name.clone(),
            score: score.parse().unwrap_or(0),
            team: if is_player == "0" {
                "Spectator".into()
            } else {
                clan.clone()
            },
            ..Default::default()
        });
    }
    Some(Status {
        info: ServerInfo {
            name,
            map,
            game: game_type.clone(),
            folder: "teeworlds".into(),
            version: version.clone(),
            players: count(clients),
            max_players: count(max_clients),
            passworded: flags & 1 != 0,
            server_type: b'd',
            ..Default::default()
        },
        players,
        rules: rules_from([
            ("gametype".to_string(), game_type),
            ("version".to_string(), version),
        ]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_teeworlds() {
        let fields = [
            "42", "0.6.4", "Tee Srv", "dm1", "DM", "1", "1", "16", "2", "16", "Alice", "Clan",
            "-1", "7", "1", "Bob", "", "-1", "0", "0",
        ];
        let mut body = Vec::new();
        for f in fields {
            body.extend(f.as_bytes());
            body.push(0);
        }
        let s = parse(&body).unwrap();
        assert_eq!(
            (s.info.name.as_str(), s.info.map.as_str(), s.info.players),
            ("Tee Srv", "dm1", 2)
        );
        assert!(s.info.passworded);
        assert_eq!(
            (s.players[0].score, s.players[1].team.as_str()),
            (7, "Spectator")
        );
    }
}
