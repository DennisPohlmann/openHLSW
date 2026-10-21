//! OpenTTD server info (UDP, port 3979): `CLIENT_FIND_SERVER` (`03 00 00`: size u16 LE and
//! type 0) is answered with `SERVER_RESPONSE` (type 1) and the game info. Its layout grows
//! with the info version (1–7); newer fields come first.

use std::net::SocketAddr;
use std::time::Duration;

use super::{Packet, Status, count, latin1, meta, rules_from};
use crate::a2s::{QueryError, QueryMeta, ServerInfo};

pub fn query(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let mut status = None;
    let rtt = super::udp_exchange(addr, &[3, 0, 0], timeout, None, |p| match parse(p) {
        Some(s) => {
            status = Some(s);
            Ok(Packet::Done)
        }
        None => Ok(Packet::Ignore),
    })?;
    Ok((status.ok_or(QueryError::Timeout)?, meta(rtt)))
}

struct R<'a> {
    d: &'a [u8],
    i: usize,
}

impl R<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.d.get(self.i..self.i + n)?;
        self.i += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn cstr(&mut self) -> Option<String> {
        let end = self.i + self.d.get(self.i..)?.iter().position(|&b| b == 0)?;
        let s = latin1(&self.d[self.i..end]);
        self.i = end + 1;
        Some(s)
    }
}

pub fn parse(p: &[u8]) -> Option<Status> {
    let size = usize::from(u16::from_le_bytes([*p.first()?, *p.get(1)?]));
    if p.get(2) != Some(&1) || size > p.len() {
        return None;
    }
    let mut r = R {
        d: &p[..size],
        i: 3,
    };
    let version = r.u8()?;
    let mut serialisation = 0;
    let mut rules = Vec::new();
    if version >= 7 {
        r.take(8)?; // ticks playing
    }
    if version >= 6 {
        serialisation = r.u8()?;
    }
    if version >= 5 {
        let gs_version = r.u32()?;
        let gs_name = r.cstr()?;
        if !gs_name.is_empty() {
            rules.push(("gamescript".to_string(), format!("{gs_name} {gs_version}")));
        }
    }
    if version >= 4 {
        let grfs = r.u8()?;
        for _ in 0..grfs {
            match serialisation {
                2 => {
                    r.u32()?;
                }
                1 => {
                    r.take(20)?;
                    r.cstr()?;
                }
                _ => {
                    r.take(20)?;
                }
            }
        }
        rules.push(("newgrfs".to_string(), grfs.to_string()));
    }
    if version >= 3 {
        r.u32()?; // game date
        r.u32()?; // start date
    }
    if version >= 2 {
        let max = r.u8()?;
        let on = r.u8()?;
        r.u8()?;
        rules.push(("companies".to_string(), format!("{on}/{max}")));
    }
    let name = r.cstr()?;
    let revision = r.cstr()?;
    if version < 6 {
        r.u8()?; // language
    }
    let password = r.u8()? != 0;
    let max_clients = r.u8()?;
    let clients = r.u8()?;
    let spectators = r.u8()?;
    if version < 3 {
        r.u16()?;
        r.u16()?;
    }
    let map = if version < 6 {
        r.cstr()?
    } else {
        String::new()
    };
    let width = r.u16()?;
    let height = r.u16()?;
    let landscape = r.u8()?;
    let dedicated = r.u8()? != 0;
    let climate = match landscape {
        0 => "Temperate",
        1 => "Sub-arctic",
        2 => "Sub-tropical",
        3 => "Toyland",
        _ => "",
    };
    rules.push(("map_size".to_string(), format!("{width}x{height}")));
    rules.push(("climate".to_string(), climate.to_string()));
    rules.push(("spectators".to_string(), spectators.to_string()));
    Some(Status {
        info: ServerInfo {
            name,
            map: if map.is_empty() {
                format!("{climate} {width}x{height}")
            } else {
                map
            },
            game: "OpenTTD".into(),
            folder: "openttd".into(),
            version: revision,
            players: count(clients),
            max_players: max_clients,
            passworded: password,
            server_type: if dedicated { b'd' } else { b'l' },
            ..Default::default()
        },
        rules: rules_from(rules),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_v4() {
        let mut b = vec![0, 0, 1, 4, 0]; // size later, type 1, version 4, no grfs
        b.extend(1000u32.to_le_bytes());
        b.extend(900u32.to_le_bytes());
        b.extend([15, 2, 0]);
        b.extend(b"TTD Srv\x001.10.3\x00");
        b.extend([0, 0, 25, 3, 0]);
        b.extend(b"Random Map\x00");
        b.extend(256u16.to_le_bytes());
        b.extend(512u16.to_le_bytes());
        b.extend([1, 1]);
        let n = b.len() as u16;
        b[..2].copy_from_slice(&n.to_le_bytes());
        let s = parse(&b).unwrap();
        assert_eq!(
            (
                s.info.name.as_str(),
                s.info.map.as_str(),
                s.info.players,
                s.info.max_players
            ),
            ("TTD Srv", "Random Map", 3, 25)
        );
    }
}
