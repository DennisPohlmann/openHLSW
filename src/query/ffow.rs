//! Frontlines: Fuel of War (query port = game port + 2): Valve A2S with an own info
//! request (`FF FF FF FF "FLSQ"` → `FF FF FF FF 'I'` …); players and rules are the normal
//! A2S requests.

use std::net::SocketAddr;
use std::time::Duration;

use super::{Packet, Status, count, latin1, meta};
use crate::a2s::{self, QueryError, QueryMeta, ServerInfo};

pub fn query(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let mut info = None;
    let rtt = super::udp_exchange(addr, b"\xFF\xFF\xFF\xFFFLSQ", timeout, None, |p| {
        match p.strip_prefix(b"\xFF\xFF\xFF\xFFI").and_then(parse_info) {
            Some(i) => {
                info = Some(i);
                Ok(Packet::Done)
            }
            None => Ok(Packet::Ignore),
        }
    })?;
    let info = info.ok_or(QueryError::Timeout)?;
    let players = a2s::query_players_with(addr, timeout, None)
        .map(|(p, _)| p)
        .unwrap_or_default();
    let rules = a2s::query_rules_with(addr, timeout, None)
        .map(|(r, _)| r)
        .unwrap_or_default();
    Ok((
        Status {
            info,
            players,
            rules,
        },
        meta(rtt),
    ))
}

pub fn parse_info(d: &[u8]) -> Option<ServerInfo> {
    let mut i = 1; // protocol
    let mut cstr = || -> Option<String> {
        let start = i;
        let end = start + d.get(start..)?.iter().position(|&b| b == 0)?;
        i = end + 1;
        Some(latin1(&d[start..end]))
    };
    let name = cstr()?;
    let map = cstr()?;
    let folder = cstr()?;
    let mode = cstr()?;
    let _description = cstr()?;
    let version = cstr()?;
    let rest = d.get(i..)?;
    let game_port = u16::from_be_bytes([*rest.first()?, *rest.get(1)?]);
    Some(ServerInfo {
        name,
        map,
        folder,
        game: mode,
        version,
        game_port: Some(game_port).filter(|&p| p > 0),
        players: count(*rest.get(2)?),
        max_players: *rest.get(3)?,
        server_type: *rest.get(4)?,
        environment: *rest.get(5)?,
        passworded: *rest.get(6)? != 0,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ffow_info() {
        let mut d = vec![7];
        for s in ["FFOW Srv", "Harbor", "ffow", "CTF", "Frontlines", "1.0"] {
            d.extend(s.as_bytes());
            d.push(0);
        }
        d.extend(5476u16.to_be_bytes());
        d.extend([3, 32, b'd', b'w', 1]);
        let i = parse_info(&d).unwrap();
        assert_eq!(
            (i.name.as_str(), i.map.as_str(), i.game_port),
            ("FFOW Srv", "Harbor", Some(5476))
        );
        assert_eq!((i.players, i.max_players, i.passworded), (3, 32, true));
    }
}
