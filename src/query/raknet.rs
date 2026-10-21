//! RakNet unconnected ping: Minecraft: Bedrock Edition (UDP, port 19132).
//!
//! Request: `01`, time (u64 BE), the offline magic and a client GUID. Answer: `1C`, time,
//! server GUID, magic, a u16 BE length and the text
//! `edition;motd;protocol;version;players;max players;server id;level;game mode;…`.

use std::net::SocketAddr;
use std::time::Duration;

use super::{Packet, Status, count, meta, rules_from};
use crate::a2s::{QueryError, QueryMeta, ServerInfo};

const MAGIC: [u8; 16] = [
    0x00, 0xff, 0xff, 0x00, 0xfe, 0xfe, 0xfe, 0xfe, 0xfd, 0xfd, 0xfd, 0xfd, 0x12, 0x34, 0x56, 0x78,
];

pub fn query(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let mut req = vec![0x01];
    req.extend(1u64.to_be_bytes());
    req.extend(MAGIC);
    req.extend(2u64.to_be_bytes());
    let mut status = None;
    let rtt = super::udp_exchange(addr, &req, timeout, None, |p| match parse(p) {
        Some(s) => {
            status = Some(s);
            Ok(Packet::Done)
        }
        None => Ok(Packet::Ignore),
    })?;
    Ok((status.ok_or(QueryError::Timeout)?, meta(rtt)))
}

pub fn parse(p: &[u8]) -> Option<Status> {
    if p.first() != Some(&0x1c) || p.get(17..33)? != MAGIC {
        return None;
    }
    let len = usize::from(u16::from_be_bytes([*p.get(33)?, *p.get(34)?]));
    let text = String::from_utf8_lossy(p.get(35..(35 + len).min(p.len()))?).into_owned();
    let f: Vec<&str> = text.split(';').collect();
    let get = |i: usize| {
        super::minecraft::strip_codes(f.get(i).copied().unwrap_or_default())
            .trim()
            .to_string()
    };
    let game_port = get(10).parse().ok();
    Some(Status {
        info: ServerInfo {
            name: get(1),
            map: get(7),
            game: get(8),
            folder: get(0),
            version: get(3),
            players: count(get(4).parse::<u32>().unwrap_or(0)),
            max_players: count(get(5).parse::<u32>().unwrap_or(0)),
            game_port,
            server_type: b'd',
            ..Default::default()
        },
        rules: rules_from([
            ("edition".to_string(), get(0)),
            ("protocol".to_string(), get(2)),
            ("version".to_string(), get(3)),
            ("level".to_string(), get(7)),
            ("gamemode".to_string(), get(8)),
        ]),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pong() {
        let text = b"MCPE;Bedrock Srv;622;1.20.40;3;10;123;Level;Survival;1;19132;19133;";
        let mut p = vec![0x1c];
        p.extend(1u64.to_be_bytes());
        p.extend(9u64.to_be_bytes());
        p.extend(MAGIC);
        p.extend((text.len() as u16).to_be_bytes());
        p.extend(text);
        let s = parse(&p).unwrap();
        assert_eq!(
            (s.info.name.as_str(), s.info.players, s.info.max_players),
            ("Bedrock Srv", 3, 10)
        );
        assert_eq!(
            (s.info.map.as_str(), s.info.game_port),
            ("Level", Some(19132))
        );
    }
}
