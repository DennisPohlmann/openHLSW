//! Minecraft: Java Edition server list ping (TCP, port 25565). Handshake with "next
//! state = status", then a status request; the answer is JSON with the description,
//! version and players (`online`, `max`, a sample of names).
//!
//! Packets: VarInt length, VarInt packet id, data. Strings: VarInt length + UTF-8.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{Status, count, meta, rules_from};
use crate::a2s::{Player, QueryError, QueryMeta, ServerInfo};

fn varint(mut v: u32, out: &mut Vec<u8>) {
    loop {
        if v & !0x7f == 0 {
            out.push(v as u8);
            return;
        }
        out.push((v & 0x7f) as u8 | 0x80);
        v >>= 7;
    }
}

/// VarInt at the start of `d`: (value, bytes used).
fn read_varint(d: &[u8]) -> Option<(u32, usize)> {
    let mut v = 0u32;
    for (i, &b) in d.iter().take(5).enumerate() {
        v |= u32::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 {
            return Some((v, i + 1));
        }
    }
    None
}

fn packet(id: u8, data: &[u8]) -> Vec<u8> {
    let mut body = vec![id];
    body.extend_from_slice(data);
    let mut out = Vec::new();
    varint(body.len() as u32, &mut out);
    out.extend(body);
    out
}

pub fn handshake(host: &str, port: u16) -> Vec<u8> {
    let mut d = Vec::new();
    varint(767, &mut d); // protocol version (any works for the status)
    varint(host.len() as u32, &mut d);
    d.extend(host.as_bytes());
    d.extend(port.to_be_bytes());
    varint(1, &mut d); // next state: status
    let mut out = packet(0, &d);
    out.extend(packet(0, &[]));
    out
}

pub fn query(
    addr: SocketAddr,
    host: &str,
    timeout: Duration,
) -> Result<(Status, QueryMeta), QueryError> {
    let start = Instant::now();
    let mut s = super::tcp_connect(addr, timeout)?;
    super::tcp_send(&mut s, &handshake(host, addr.port()))?;
    let mut buf = Vec::new();
    let raw = super::tcp_read_until(&mut s, &mut buf, |b| {
        let (len, used) = read_varint(b)?;
        let total = used + len as usize;
        (b.len() >= total).then_some(total)
    })?;
    let (len, used) = read_varint(&raw).ok_or_else(bad)?;
    let body = &raw[used..used + len as usize];
    let (id, used) = read_varint(body).ok_or_else(bad)?;
    if id != 0 {
        return Err(bad());
    }
    let (n, used2) = read_varint(&body[used..]).ok_or_else(bad)?;
    let start_json = used + used2;
    let json = body
        .get(start_json..start_json + n as usize)
        .ok_or_else(bad)?;
    let v: Value = serde_json::from_slice(json).map_err(|_| bad())?;
    Ok((parse(&v), meta(start.elapsed())))
}

fn bad() -> QueryError {
    QueryError::Protocol("Minecraft: bad answer".into())
}

/// Text of a chat component (string, `{"text", "extra"}` or a list).
fn chat(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(chat).collect(),
        Value::Object(o) => {
            let mut s = o.get("text").map(chat).unwrap_or_default();
            if let Some(extra) = o.get("extra") {
                s += &chat(extra);
            }
            s
        }
        _ => String::new(),
    }
}

/// Removes `§x` formatting codes.
pub(crate) fn strip_codes(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

pub fn parse(v: &Value) -> Status {
    let name = strip_codes(&chat(&v["description"]))
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ");
    let players: Vec<Player> = v["players"]["sample"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, p)| Player {
            index: count(i),
            name: strip_codes(p["name"].as_str().unwrap_or_default()),
            ..Default::default()
        })
        .collect();
    let version = strip_codes(v["version"]["name"].as_str().unwrap_or_default());
    Status {
        info: ServerInfo {
            name,
            game: "Minecraft".into(),
            folder: "minecraft".into(),
            version: version.clone(),
            players: count(v["players"]["online"].as_u64().unwrap_or(0)),
            max_players: count(v["players"]["max"].as_u64().unwrap_or(0)),
            server_type: b'd',
            ..Default::default()
        },
        players,
        rules: rules_from([
            ("version".to_string(), version),
            (
                "protocol".to_string(),
                v["version"]["protocol"].as_u64().unwrap_or(0).to_string(),
            ),
        ]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varints_and_status() {
        let mut v = Vec::new();
        varint(300, &mut v);
        assert_eq!(v, [0xac, 0x02]);
        assert_eq!(read_varint(&v), Some((300, 2)));
        let h = handshake("mc.example.org", 25565);
        assert_eq!(h[1], 0);
        let s = parse(&serde_json::json!({
            "version": {"name": "1.21", "protocol": 767},
            "players": {"max": 20, "online": 2, "sample": [{"name": "Steve", "id": "x"}]},
            "description": {"text": "§aHello ", "extra": [{"text": "World"}]}
        }));
        assert_eq!(
            (s.info.name.as_str(), s.info.players, s.info.max_players),
            ("Hello World", 2, 20)
        );
        assert_eq!(s.players[0].name, "Steve");
    }
}
