//! All-Seeing Eye (query port = game port + 123): Far Cry, Multi Theft Auto.
//!
//! Request `s`; answer `EYE1` and strings whose length byte counts itself: game, port,
//! name, game type, map, version, password, players, max players, `key value …` up to an
//! empty key, then one record per player (flags say which fields follow).

use std::net::SocketAddr;
use std::time::Duration;

use super::{Packet, Status, count, latin1, meta, rules_from};
use crate::a2s::{Player, QueryError, QueryMeta, ServerInfo};

pub fn query(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let mut status = None;
    let rtt = super::udp_exchange(addr, b"s", timeout, None, |p| match parse(p) {
        Some(s) => {
            status = Some(s);
            Ok(Packet::Done)
        }
        None => Ok(Packet::Ignore),
    })?;
    Ok((status.ok_or(QueryError::Timeout)?, meta(rtt)))
}

struct Reader<'a> {
    d: &'a [u8],
    i: usize,
}

impl Reader<'_> {
    fn string(&mut self) -> Option<String> {
        let len = usize::from(*self.d.get(self.i)?);
        let start = self.i + 1;
        let end = start + len.saturating_sub(1);
        let s = latin1(self.d.get(start..end)?);
        self.i = end;
        Some(s)
    }

    fn done(&self) -> bool {
        self.i >= self.d.len()
    }
}

pub fn parse(p: &[u8]) -> Option<Status> {
    let d = p.strip_prefix(b"EYE1")?;
    let mut r = Reader { d, i: 0 };
    let game = r.string()?;
    let port = r.string()?;
    let name = r.string()?;
    let game_type = r.string()?;
    let map = r.string()?;
    let version = r.string()?;
    let password = r.string()?;
    let players = r.string()?;
    let max = r.string()?;
    let mut pairs = vec![("gametype".to_string(), game_type)];
    while !r.done() {
        let key = r.string()?;
        if key.is_empty() {
            break;
        }
        let value = r.string()?;
        pairs.push((key, value));
    }
    let mut list = Vec::new();
    while !r.done() {
        let flags = *r.d.get(r.i)?;
        r.i += 1;
        let mut p = Player {
            index: count(list.len()),
            ..Default::default()
        };
        if flags & 1 != 0 {
            p.name = r.string()?;
        }
        if flags & 2 != 0 {
            p.team = r.string()?;
        }
        if flags & 4 != 0 {
            r.string()?; // skin
        }
        if flags & 8 != 0 {
            p.score = r.string()?.trim().parse().unwrap_or(0);
        }
        if flags & 16 != 0 {
            p.ping = r.string()?.trim().parse().ok();
        }
        if flags & 32 != 0 {
            p.duration = r.string()?.trim().parse().unwrap_or(0.0);
        }
        list.push(p);
    }
    let info = ServerInfo {
        name,
        map,
        game: game.clone(),
        folder: game,
        players: count(players.trim().parse::<u32>().unwrap_or(list.len() as u32)),
        max_players: count(max.trim().parse::<u32>().unwrap_or(0)),
        passworded: password.trim() == "1",
        version,
        game_port: port.trim().parse().ok(),
        server_type: b'd',
        ..Default::default()
    };
    Some(Status {
        info,
        players: list,
        rules: rules_from(pairs),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> Vec<u8> {
        let mut v = vec![(t.len() + 1) as u8];
        v.extend_from_slice(t.as_bytes());
        v
    }

    #[test]
    fn parses_ase() {
        let mut p = b"EYE1".to_vec();
        for f in [
            "mta",
            "22003",
            "MTA Server",
            "Race",
            "map1",
            "1.6",
            "0",
            "1",
            "32",
        ] {
            p.extend(s(f));
        }
        p.extend(s("mod"));
        p.extend(s("race"));
        p.extend(s(""));
        p.push(1 | 8 | 16);
        p.extend(s("Alice"));
        p.extend(s("5"));
        p.extend(s("60"));
        let st = parse(&p).unwrap();
        assert_eq!(
            (st.info.name.as_str(), st.info.map.as_str()),
            ("MTA Server", "map1")
        );
        assert_eq!(
            (st.info.players, st.info.max_players, st.info.game_port),
            (1, 32, Some(22003))
        );
        assert_eq!(
            (
                st.players[0].name.as_str(),
                st.players[0].score,
                st.players[0].ping
            ),
            ("Alice", 5, Some(60))
        );
    }
}
