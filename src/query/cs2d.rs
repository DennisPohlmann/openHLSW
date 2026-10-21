//! Counter-Strike 2D (UDP, port 36963).
//!
//! Requests `01 00 FB 01` (info) and `01 00 FB 05` (players); the answer starts with the
//! same four bytes. Info: flags (bit 0 password, bit 5 bots), name and map (u8 length +
//! text), players, max players, game mode (u8) and, with bots, the bot count. Players:
//! count (u8), then per player id (u8), name (u8 length + text), team (u8), score and
//! deaths (i32 LE).

use std::net::SocketAddr;
use std::time::Duration;

use super::{Packet, Status, latin1, meta, rules_from};
use crate::a2s::{Player, QueryError, QueryMeta, ServerInfo};

const INFO: [u8; 4] = [0x01, 0x00, 0xfb, 0x01];
const PLAYERS: [u8; 4] = [0x01, 0x00, 0xfb, 0x05];

pub fn query(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let ask = |req: [u8; 4]| -> Result<(Vec<u8>, Duration), QueryError> {
        let mut body = None;
        let rtt = super::udp_exchange(addr, &req, timeout, None, |p| {
            match p.strip_prefix(&req[..]) {
                Some(b) => {
                    body = Some(b.to_vec());
                    Ok(Packet::Done)
                }
                None => Ok(Packet::Ignore),
            }
        })?;
        Ok((body.unwrap_or_default(), rtt))
    };
    let (info, rtt) = ask(INFO)?;
    let mut status =
        parse_info(&info).ok_or_else(|| QueryError::Protocol("CS2D: bad info".into()))?;
    if let Ok((players, _)) = ask(PLAYERS) {
        status.players = parse_players(&players);
    }
    Ok((status, meta(rtt)))
}

fn pstr(d: &[u8], i: &mut usize) -> Option<String> {
    let n = usize::from(*d.get(*i)?);
    let s = latin1(d.get(*i + 1..*i + 1 + n)?);
    *i += 1 + n;
    Some(s)
}

pub fn parse_info(d: &[u8]) -> Option<Status> {
    let flags = *d.first()?;
    let mut i = 1;
    let name = pstr(d, &mut i)?;
    let map = pstr(d, &mut i)?;
    let players = *d.get(i)?;
    let max = *d.get(i + 1)?;
    let mode = d.get(i + 2).copied().unwrap_or(0);
    let bots = if flags & 32 != 0 {
        d.get(i + 3).copied().unwrap_or(0)
    } else {
        0
    };
    let mode_name = match mode {
        0 => "Standard",
        1 => "Deathmatch",
        2 => "Team Deathmatch",
        3 => "Construction",
        4 => "Zombies!",
        _ => "",
    };
    Some(Status {
        info: ServerInfo {
            name,
            map,
            game: mode_name.into(),
            folder: "cs2d".into(),
            players,
            max_players: max,
            bots,
            passworded: flags & 1 != 0,
            server_type: b'd',
            ..Default::default()
        },
        rules: rules_from([
            (
                "friendlyfire".to_string(),
                u8::from(flags & 8 != 0).to_string(),
            ),
            ("fogofwar".to_string(), u8::from(flags & 4 != 0).to_string()),
            ("lua".to_string(), u8::from(flags & 64 != 0).to_string()),
        ]),
        ..Default::default()
    })
}

pub fn parse_players(d: &[u8]) -> Vec<Player> {
    let mut out = Vec::new();
    let n = d.first().copied().unwrap_or(0);
    let mut i = 1;
    for _ in 0..n {
        let Some(&id) = d.get(i) else { break };
        i += 1;
        let Some(name) = pstr(d, &mut i) else { break };
        let (Some(&team), Some(score), Some(deaths)) =
            (d.get(i), d.get(i + 1..i + 5), d.get(i + 5..i + 9))
        else {
            break;
        };
        i += 9;
        out.push(Player {
            index: id,
            name,
            team: match team {
                1 => "T".into(),
                2 => "CT".into(),
                _ => "Spectator".into(),
            },
            score: i32::from_le_bytes(score.try_into().unwrap_or_default()),
            deaths: Some(i32::from_le_bytes(deaths.try_into().unwrap_or_default())),
            ..Default::default()
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cs2d() {
        let mut d = vec![1 | 32, 7];
        d.extend(b"CS2D Sv");
        d.push(7);
        d.extend(b"de_dust");
        d.extend([4, 16, 2, 1]);
        let s = parse_info(&d).unwrap();
        assert_eq!(
            (s.info.name.as_str(), s.info.map.as_str(), s.info.bots),
            ("CS2D Sv", "de_dust", 1)
        );
        assert!(s.info.passworded);
        let mut p = vec![1, 3, 3];
        p.extend(b"Bob");
        p.push(2);
        p.extend(5i32.to_le_bytes());
        p.extend(2i32.to_le_bytes());
        let pl = parse_players(&p);
        assert_eq!(
            (pl[0].name.as_str(), pl[0].team.as_str(), pl[0].score),
            ("Bob", "CT", 5)
        );
    }
}
