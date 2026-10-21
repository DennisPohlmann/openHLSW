//! Mumble (voice port 64738). The server answers a UDP ping with its version, the number
//! of users, the user limit and the bandwidth: request `00 00 00 00` + 8 byte ident,
//! answer version (4 bytes), ident, users (u32 BE), max users (u32 BE), bandwidth (u32 BE).
//! Servers with the channel viewer (TCP port 27800, `json`) also give the user list.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use super::{Packet, Status, count, meta, rules_from};
use crate::a2s::{Player, QueryError, QueryMeta, ServerInfo};

const IDENT: [u8; 8] = *b"oHLSWmbl";

pub fn query(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let mut req = vec![0u8; 4];
    req.extend(IDENT);
    let mut status = None;
    let rtt = super::udp_exchange(addr, &req, timeout, None, |p| {
        if p.len() < 24 || p[4..12] != IDENT {
            return Ok(Packet::Ignore);
        }
        status = Some(parse_ping(p));
        Ok(Packet::Done)
    })?;
    Ok((status.ok_or(QueryError::Timeout)?, meta(rtt)))
}

pub fn parse_ping(p: &[u8]) -> Status {
    let be = |i: usize| u32::from_be_bytes([p[i], p[i + 1], p[i + 2], p[i + 3]]);
    let version = format!("{}.{}.{}", p[1], p[2], p[3]);
    Status {
        info: ServerInfo {
            name: "Mumble".into(),
            game: "Mumble".into(),
            version: version.clone(),
            players: count(be(12)),
            max_players: count(be(16)),
            server_type: b'd',
            ..Default::default()
        },
        rules: rules_from([
            ("version".to_string(), version),
            ("bandwidth".to_string(), be(20).to_string()),
        ]),
        ..Default::default()
    }
}

/// Channel viewer (`json` over TCP): server name and users.
pub fn query_json(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let start = Instant::now();
    let mut s = super::tcp_connect(addr, timeout)?;
    super::tcp_send(&mut s, b"json")?;
    let mut buf = Vec::new();
    let raw = super::tcp_read_until(&mut s, &mut buf, |b| {
        serde_json::from_slice::<serde_json::Value>(b)
            .ok()
            .map(|_| b.len())
    })?;
    let v: serde_json::Value =
        serde_json::from_slice(&raw).map_err(|e| QueryError::Protocol(format!("Mumble: {e}")))?;
    parse_json(&v)
        .map(|s| (s, meta(start.elapsed())))
        .ok_or_else(|| QueryError::Protocol("Mumble: bad answer".into()))
}

pub fn parse_json(v: &serde_json::Value) -> Option<Status> {
    let mut players = Vec::new();
    fn walk(c: &serde_json::Value, out: &mut Vec<Player>) {
        let channel = c["name"].as_str().unwrap_or_default().to_string();
        for u in c["users"].as_array().into_iter().flatten() {
            out.push(Player {
                index: count(out.len()),
                name: u["name"].as_str().unwrap_or_default().to_string(),
                team: channel.clone(),
                ping: u["tcpPing"].as_f64().map(|p| p as u32),
                duration: u["onlinesecs"].as_f64().unwrap_or(0.0) as f32,
                ..Default::default()
            });
        }
        for sub in c["channels"].as_array().into_iter().flatten() {
            walk(sub, out);
        }
    }
    walk(v.get("root")?, &mut players);
    let name = v["name"].as_str().unwrap_or("Mumble").to_string();
    let max = v["x_gtmurmur_max_users"].as_u64().unwrap_or(0);
    Some(Status {
        info: ServerInfo {
            name,
            game: "Mumble".into(),
            players: count(players.len()),
            max_players: count(max),
            server_type: b'd',
            ..Default::default()
        },
        players,
        rules: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mumble() {
        let mut p = vec![0, 1, 4, 0];
        p.extend(IDENT);
        p.extend(3u32.to_be_bytes());
        p.extend(50u32.to_be_bytes());
        p.extend(72000u32.to_be_bytes());
        let s = parse_ping(&p);
        assert_eq!(
            (s.info.version.as_str(), s.info.players, s.info.max_players),
            ("1.4.0", 3, 50)
        );

        let v: serde_json::Value = serde_json::from_str(
            r#"{"name":"Murmur","x_gtmurmur_max_users":20,"root":{"name":"Root","users":[{"name":"Alice","tcpPing":12.5,"onlinesecs":60}],"channels":[{"name":"Games","users":[{"name":"Bob"}],"channels":[]}]}}"#,
        )
        .unwrap();
        let s = parse_json(&v).unwrap();
        assert_eq!((s.info.name.as_str(), s.info.players), ("Murmur", 2));
        assert_eq!(
            (s.players[1].name.as_str(), s.players[1].team.as_str()),
            ("Bob", "Games")
        );
    }
}
