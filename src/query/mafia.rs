//! Mafia multiplayer mods (UDP, game port + 1).
//!
//! Lost Heaven (LHMP): `LHMPo` (info) and `LHMPp` (players). Info: protocol and password as
//! zero-terminated strings, players and max players (u16 LE), name, game mode, website and
//! map (u8 length + text). Players: count (u16 LE), then id (u16 LE) and name.
//!
//! Mafia 2 Multiplayer (M2MP): `M2MP`; the answer holds name, players, max players and game
//! mode (u8 length including the length byte + text), a password byte and the player names.

use std::net::SocketAddr;
use std::time::Duration;

use super::{Packet, Status, count, latin1, meta};
use crate::a2s::{Player, QueryError, QueryMeta, ServerInfo};

fn ask(addr: SocketAddr, req: &[u8], timeout: Duration) -> Result<(Vec<u8>, Duration), QueryError> {
    let mut body = None;
    let rtt = super::udp_exchange(addr, req, timeout, None, |p| match p.strip_prefix(req) {
        Some(b) => {
            body = Some(b.to_vec());
            Ok(Packet::Done)
        }
        None => Ok(Packet::Ignore),
    })?;
    Ok((body.unwrap_or_default(), rtt))
}

pub fn query_lhmp(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let (info, rtt) = ask(addr, b"LHMPo", timeout)?;
    let mut status =
        parse_lhmp_info(&info).ok_or_else(|| QueryError::Protocol("LHMP: bad info".into()))?;
    if let Ok((players, _)) = ask(addr, b"LHMPp", timeout) {
        status.players = parse_lhmp_players(&players);
    }
    Ok((status, meta(rtt)))
}

fn cstr(d: &[u8], i: &mut usize) -> Option<String> {
    let end = *i + d.get(*i..)?.iter().position(|&b| b == 0)?;
    let s = latin1(&d[*i..end]);
    *i = end + 1;
    Some(s)
}

/// u8 length + text; `including` = the length counts its own byte (M2MP).
fn pstr(d: &[u8], i: &mut usize, including: bool) -> Option<String> {
    let n = usize::from(*d.get(*i)?);
    let n = if including { n.saturating_sub(1) } else { n };
    let s = latin1(d.get(*i + 1..*i + 1 + n)?);
    *i += 1 + n;
    Some(s)
}

fn u16le(d: &[u8], i: &mut usize) -> Option<u16> {
    let v = u16::from_le_bytes([*d.get(*i)?, *d.get(*i + 1)?]);
    *i += 2;
    Some(v)
}

pub fn parse_lhmp_info(d: &[u8]) -> Option<Status> {
    let mut i = 0;
    let _protocol = cstr(d, &mut i)?;
    let password = cstr(d, &mut i)?;
    let players = u16le(d, &mut i)?;
    let max = u16le(d, &mut i)?;
    let name = pstr(d, &mut i, false)?;
    let mode = pstr(d, &mut i, false).unwrap_or_default();
    let _website = pstr(d, &mut i, false);
    let map = pstr(d, &mut i, false).unwrap_or_default();
    Some(Status {
        info: ServerInfo {
            name,
            map,
            game: mode,
            folder: "lhmp".into(),
            players: count(players),
            max_players: count(max),
            passworded: !matches!(password.as_str(), "" | "0"),
            server_type: b'd',
            ..Default::default()
        },
        ..Default::default()
    })
}

pub fn parse_lhmp_players(d: &[u8]) -> Vec<Player> {
    let mut i = 0;
    let n = u16le(d, &mut i).unwrap_or(0);
    let mut out = Vec::new();
    for _ in 0..n {
        let Some(id) = u16le(d, &mut i) else { break };
        let Some(name) = pstr(d, &mut i, false) else {
            break;
        };
        out.push(Player {
            index: count(id),
            name,
            ..Default::default()
        });
    }
    out
}

pub fn query_m2mp(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let (body, rtt) = ask(addr, b"M2MP", timeout)?;
    parse_m2mp(&body)
        .map(|s| (s, meta(rtt)))
        .ok_or_else(|| QueryError::Protocol("M2MP: bad answer".into()))
}

pub fn parse_m2mp(d: &[u8]) -> Option<Status> {
    let mut i = 0;
    let name = pstr(d, &mut i, true)?;
    let players = pstr(d, &mut i, true)?;
    let max = pstr(d, &mut i, true)?;
    let mode = pstr(d, &mut i, true)?;
    let password = d.get(i).copied().unwrap_or(0) != 0;
    i += 1;
    let mut list = Vec::new();
    while d.len() > i + 1 {
        let Some(n) = pstr(d, &mut i, true) else {
            break;
        };
        list.push(Player {
            index: count(list.len()),
            name: n.trim().to_string(),
            ..Default::default()
        });
    }
    Some(Status {
        info: ServerInfo {
            name,
            game: mode,
            folder: "m2mp".into(),
            players: count(players.trim().parse::<u32>().unwrap_or(0)),
            max_players: count(max.trim().parse::<u32>().unwrap_or(0)),
            passworded: password,
            server_type: b'd',
            ..Default::default()
        },
        players: list,
        rules: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lhmp() {
        let mut d = b"1\x000\x00".to_vec();
        d.extend(2u16.to_le_bytes());
        d.extend(32u16.to_le_bytes());
        for s in ["LH Srv", "Freeride", "example.org", "tutorial"] {
            d.push(s.len() as u8);
            d.extend(s.as_bytes());
        }
        let s = parse_lhmp_info(&d).unwrap();
        assert_eq!(
            (
                s.info.name.as_str(),
                s.info.map.as_str(),
                s.info.max_players
            ),
            ("LH Srv", "tutorial", 32)
        );
        let mut p = 1u16.to_le_bytes().to_vec();
        p.extend(4u16.to_le_bytes());
        p.extend(b"\x05Tommy");
        assert_eq!(parse_lhmp_players(&p)[0].name, "Tommy");
    }

    #[test]
    fn parses_m2mp() {
        let mut d = Vec::new();
        for s in ["M2 Srv", "3", "16", "Roleplay"] {
            d.push(s.len() as u8 + 1);
            d.extend(s.as_bytes());
        }
        d.push(1);
        d.extend(b"\x05Vito");
        let s = parse_m2mp(&d).unwrap();
        assert_eq!(
            (s.info.name.as_str(), s.info.players, s.info.max_players),
            ("M2 Srv", 3, 16)
        );
        assert_eq!(s.players[0].name, "Vito");
    }
}
