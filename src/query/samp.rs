//! San Andreas Multiplayer / open.mp (UDP, game port 7777).
//!
//! Request: `"SAMP"`, the server's IPv4 address (4 bytes), its port (u16 LE) and an
//! opcode: `i` info, `r` rules, `d` players with id, score and ping. The answer repeats
//! the 11 byte header. Info: password (u8), players (u16), max players (u16), then the
//! host name, game mode and language, each as u32 length + text. Rules: u16 count, then
//! pairs of u8 length + text. Players: u16 count, then id (u8), name (u8 length + text),
//! score (i32), ping (i32). All little-endian.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use super::{Packet, Status, count, latin1, meta, rules_from};
use crate::a2s::{Player, QueryError, QueryMeta, ServerInfo};

pub fn query(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let header = header(addr).ok_or_else(|| QueryError::Protocol("SA-MP: IPv4 only".into()))?;
    let ask = |op: u8| -> Result<(Vec<u8>, Duration), QueryError> {
        let mut req = header.to_vec();
        req.push(op);
        let mut body = None;
        let rtt = super::udp_exchange(addr, &req, timeout, None, |p| {
            if p.len() < 11 || p[..10] != header[..] || p[10] != op {
                return Ok(Packet::Ignore);
            }
            body = Some(p[11..].to_vec());
            Ok(Packet::Done)
        })?;
        Ok((body.unwrap_or_default(), rtt))
    };
    let (info, rtt) = ask(b'i')?;
    let mut status =
        parse_info(&info).ok_or_else(|| QueryError::Protocol("SA-MP: bad info".into()))?;
    if let Ok((rules, _)) = ask(b'r') {
        status.rules = parse_rules(&rules);
        if let Some(r) = status.rules.iter().find(|r| r.key == "mapname") {
            status.info.map = r.value.clone();
        }
        if let Some(r) = status.rules.iter().find(|r| r.key == "version") {
            status.info.version = r.value.clone();
        }
    }
    // Servers with more than 100 players do not answer the player list.
    if let Ok((players, _)) = ask(b'd') {
        status.players = parse_players(&players);
    }
    Ok((status, meta(rtt)))
}

fn header(addr: SocketAddr) -> Option<[u8; 10]> {
    let IpAddr::V4(ip) = addr.ip() else {
        return None;
    };
    let mut h = [0u8; 10];
    h[..4].copy_from_slice(b"SAMP");
    h[4..8].copy_from_slice(&ip.octets());
    h[8..10].copy_from_slice(&addr.port().to_le_bytes());
    Some(h)
}

struct Reader<'a> {
    d: &'a [u8],
    i: usize,
}

impl Reader<'_> {
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
    fn i32(&mut self) -> Option<i32> {
        self.take(4)
            .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn str32(&mut self) -> Option<String> {
        let n = usize::try_from(self.i32()?).ok()?;
        self.take(n).map(latin1)
    }
    fn str8(&mut self) -> Option<String> {
        let n = usize::from(self.u8()?);
        self.take(n).map(latin1)
    }
}

pub fn parse_info(d: &[u8]) -> Option<Status> {
    let mut r = Reader { d, i: 0 };
    let password = r.u8()? != 0;
    let players = r.u16()?;
    let max = r.u16()?;
    let name = r.str32()?;
    let mode = r.str32().unwrap_or_default();
    let language = r.str32().unwrap_or_default();
    Some(Status {
        info: ServerInfo {
            name,
            game: mode.clone(),
            folder: "samp".into(),
            players: count(players),
            max_players: count(max),
            passworded: password,
            server_type: b'd',
            ..Default::default()
        },
        rules: rules_from([
            ("gamemode".to_string(), mode),
            ("language".to_string(), language),
        ]),
        ..Default::default()
    })
}

pub fn parse_rules(d: &[u8]) -> Vec<crate::a2s::Rule> {
    let mut r = Reader { d, i: 0 };
    let n = r.u16().unwrap_or(0);
    let mut pairs = Vec::new();
    for _ in 0..n {
        let (Some(k), Some(v)) = (r.str8(), r.str8()) else {
            break;
        };
        pairs.push((k, v));
    }
    rules_from(pairs)
}

pub fn parse_players(d: &[u8]) -> Vec<Player> {
    let mut r = Reader { d, i: 0 };
    let n = r.u16().unwrap_or(0);
    let mut out = Vec::new();
    for _ in 0..n {
        let (Some(id), Some(name), Some(score), Some(ping)) = (r.u8(), r.str8(), r.i32(), r.i32())
        else {
            break;
        };
        out.push(Player {
            index: id,
            name,
            score,
            ping: u32::try_from(ping).ok(),
            ..Default::default()
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_samp() {
        let mut info = vec![1];
        info.extend(5u16.to_le_bytes());
        info.extend(100u16.to_le_bytes());
        for s in ["SA-MP Srv", "Freeroam", "English"] {
            info.extend((s.len() as i32).to_le_bytes());
            info.extend(s.as_bytes());
        }
        let s = parse_info(&info).unwrap();
        assert_eq!(
            (s.info.name.as_str(), s.info.players, s.info.max_players),
            ("SA-MP Srv", 5, 100)
        );
        assert!(s.info.passworded);

        let mut rules = 1u16.to_le_bytes().to_vec();
        rules.extend([7]);
        rules.extend(b"mapname");
        rules.extend([2]);
        rules.extend(b"LS");
        assert_eq!(parse_rules(&rules)[0].value, "LS");

        let mut pl = 1u16.to_le_bytes().to_vec();
        pl.extend([3, 5]);
        pl.extend(b"Alice");
        pl.extend(12i32.to_le_bytes());
        pl.extend(40i32.to_le_bytes());
        let p = parse_players(&pl);
        assert_eq!(
            (p[0].index, p[0].name.as_str(), p[0].score, p[0].ping),
            (3, "Alice", 12, Some(40))
        );
        assert_eq!(
            header("192.0.2.1:7777".parse().unwrap()).unwrap()[4..],
            [192, 0, 2, 1, 0x61, 0x1e]
        );
    }
}
