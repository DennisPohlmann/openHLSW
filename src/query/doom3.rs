//! Doom 3 engine `getInfo`: Doom 3, Quake 4, Prey, Wolfenstein (2009), ET: Quake Wars.
//!
//! Request `FF FF "getInfo" 00 <challenge> 00`; answer `FF FF "infoResponse" 00`,
//! the challenge, the protocol version, `key 00 value 00 …` and the players. The player
//! records differ between the games (Quake 4 / Prey add a clan tag, ET: Quake Wars has no
//! rate and a type flag), so the known layouts are tried in turn.

use std::net::SocketAddr;
use std::time::Duration;

use super::{Packet, Status, count, latin1, meta, rule, rules_from, strip_q3_colors, truthy};
use crate::a2s::{Player, QueryError, QueryMeta, ServerInfo};

pub fn query(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let mut status = None;
    let rtt = super::udp_exchange(
        addr,
        b"\xFF\xFFgetInfo\x00PiNGPoNg\x00",
        timeout,
        None,
        |p| match parse(p) {
            Some(s) => {
                status = Some(s);
                Ok(Packet::Done)
            }
            None => Ok(Packet::Ignore),
        },
    )?;
    Ok((status.ok_or(QueryError::Timeout)?, meta(rtt)))
}

pub fn parse(p: &[u8]) -> Option<Status> {
    let rest = p.strip_prefix(b"\xFF\xFFinfoResponse\x00")?;
    let mut rest = rest.strip_prefix(b"PiNG")?;
    // Some servers return only half of the challenge.
    rest = rest.strip_prefix(b"PoNg").unwrap_or(rest);
    let version = u32::from_le_bytes(rest.get(..4)?.try_into().ok()?);
    let mut i = 4;
    // ET: Quake Wars adds the packet size (its upper bytes are zero).
    if rest.get(i + 2..i + 4) == Some(&[0, 0]) {
        i += 4;
    }
    let mut pairs = Vec::new();
    loop {
        let key = cstr(rest, &mut i)?;
        if key.is_empty() {
            // Some games end the list with an empty key and value.
            if rest.get(i) == Some(&0) {
                i += 1;
            }
            break;
        }
        let mut value = strip_q3_colors(&cstr(rest, &mut i)?);
        if key == "si_map" {
            value = value.replace("maps/", "").replace(".entities", "");
        }
        pairs.push((key, value));
    }
    let rules = rules_from(pairs);
    let etqw = rule(&rules, "gamename").is_some_and(|g| g.to_ascii_lowercase().contains("etqw"));
    let tail = &rest[i..];
    let players = [
        (false, false, false),
        (true, false, false),
        (true, true, true),
    ]
    .into_iter()
    .find_map(|(clan, clan_pos, type_flag)| players(tail, etqw, clan, clan_pos, type_flag))
    .unwrap_or_default();
    let get = |k: &str| rule(&rules, k).unwrap_or_default().to_string();
    let max = get("si_maxPlayers");
    let max = if max.is_empty() {
        get("si_maxplayers")
    } else {
        max
    };
    let info = ServerInfo {
        name: get("si_name"),
        map: get("si_map"),
        game: {
            let g = get("gamename");
            if g.is_empty() { get("si_gameType") } else { g }
        },
        folder: get("fs_game"),
        players: count(players.len()),
        bots: count(players.iter().filter(|p| p.bot).count()),
        max_players: count(max.trim().parse::<u32>().unwrap_or(0)),
        passworded: truthy(rule(&rules, "si_usePass")) || truthy(rule(&rules, "si_needPass")),
        version: format!("{}.{}", version >> 16, version & 0xffff),
        server_type: b'd',
        ..Default::default()
    };
    Some(Status {
        info,
        players,
        rules,
    })
}

fn cstr(d: &[u8], i: &mut usize) -> Option<String> {
    let start = *i;
    let end = start + d.get(start..)?.iter().position(|&b| b == 0)?;
    *i = end + 1;
    Some(latin1(&d[start..end]))
}

/// One layout of the player list; `None` if the data does not fit it.
fn players(
    d: &[u8],
    etqw: bool,
    clan: bool,
    clan_pos: bool,
    type_flag: bool,
) -> Option<Vec<Player>> {
    let mut i = 0;
    let mut last: i32 = -1;
    let mut out = Vec::new();
    loop {
        let id = *d.get(i)?;
        i += 1;
        if i32::from(id) <= last || id > 0x20 {
            return None;
        }
        last = i32::from(id);
        if id == 0x20 {
            return Some(out);
        }
        let ping = u16::from_le_bytes([*d.get(i)?, *d.get(i + 1)?]);
        i += 2;
        if !etqw {
            d.get(i..i + 4)?; // rate
            i += 4;
        }
        let name = strip_q3_colors(&cstr(d, &mut i)?);
        if clan {
            if clan_pos {
                i += 1;
            }
            cstr(d, &mut i)?;
        }
        let mut bot = ping == 0;
        if type_flag {
            bot |= *d.get(i)? != 0;
            i += 1;
        }
        out.push(Player {
            index: count(out.len()),
            name,
            ping: Some(u32::from(ping)),
            bot,
            ..Default::default()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_doom3() {
        let mut p = b"\xFF\xFFinfoResponse\x00PiNGPoNg".to_vec();
        p.extend(((1u32 << 16) | 41).to_le_bytes());
        for (k, v) in [
            ("si_name", "^1Doom Srv"),
            ("si_map", "maps/game/mp/d3dm1"),
            ("si_maxPlayers", "8"),
            ("si_usePass", "0"),
        ] {
            p.extend(k.as_bytes());
            p.push(0);
            p.extend(v.as_bytes());
            p.push(0);
        }
        p.extend([0, 0]);
        // player 0: ping 40, rate, "Marine"
        p.push(0);
        p.extend(40u16.to_le_bytes());
        p.extend(25000u32.to_le_bytes());
        p.extend(b"Marine\x00");
        p.push(0x20);
        p.extend(0u32.to_le_bytes());
        let s = parse(&p).unwrap();
        assert_eq!(s.info.name, "Doom Srv");
        assert_eq!(s.info.map, "game/mp/d3dm1");
        assert_eq!((s.info.players, s.info.max_players), (1, 8));
        assert_eq!(s.info.version, "1.41");
        assert_eq!(
            (s.players[0].name.as_str(), s.players[0].ping),
            ("Marine", Some(40))
        );
    }
}
