//! Ventrilo status (UDP, port 3784). The packets are scrambled with fixed key tables
//! ([`super::ventrilo_tables`]): a 20 byte header and the body are each shifted byte by
//! byte with a table entry picked by a start value and a step chosen by the sender.
//! The answer is text: `NAME: …`, `MAXCLIENTS: …`, one `CLIENT: NAME=…,PING=…,SEC=…` line
//! per user.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::ventrilo_tables::{BODY, CRC, HEAD};
use super::{Packet, Status, count, meta, rules_from};
use crate::a2s::{Player, QueryError, QueryMeta, ServerInfo};

/// Command 2 = general status with the client list.
const STATUS: u16 = 2;

pub fn query(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let request = encrypt(STATUS, &[0u8; 16], seed());
    let mut parts: BTreeMap<u16, Vec<u8>> = BTreeMap::new();
    let rtt = super::udp_exchange(addr, &request, timeout, None, |p| {
        let Some((total, num, body)) = decrypt(p) else {
            return Ok(Packet::Ignore);
        };
        parts.insert(num, body);
        Ok(if parts.len() >= usize::from(total) {
            Packet::Done
        } else {
            Packet::More
        })
    })?;
    let body: Vec<u8> = parts.into_values().flatten().collect();
    Ok((parse(&String::from_utf8_lossy(&body)), meta(rtt)))
}

/// Four key bytes (start / step for header and body; steps must not be 0).
fn seed() -> [u8; 4] {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() ^ (d.as_secs() as u32).wrapping_mul(2_654_435_761))
        .unwrap_or(12345);
    let b = n.to_le_bytes();
    [b[0], b[1].max(1), b[2], b[3].max(1)]
}

fn crc(body: &[u8]) -> u16 {
    body.iter().fold(0u16, |crc, &b| {
        CRC[usize::from(crc >> 8)] ^ u16::from(b) ^ (crc << 8)
    })
}

pub fn encrypt(cmd: u16, body: &[u8], key: [u8; 4]) -> Vec<u8> {
    let [head_start, head_add, body_start, body_add] = key;
    let len = body.len() as u16;
    let mut h = [0u8; 20];
    h[0] = head_start;
    h[1] = head_add;
    h[4..6].copy_from_slice(&cmd.to_be_bytes());
    h[8..10].copy_from_slice(&len.to_be_bytes());
    h[10..12].copy_from_slice(&len.to_be_bytes());
    h[12..14].copy_from_slice(&1u16.to_be_bytes());
    h[16] = body_start;
    h[17] = body_add;
    h[18..20].copy_from_slice(&crc(body).to_be_bytes());
    let mut offset = usize::from(head_start);
    for (i, b) in h.iter_mut().enumerate().skip(2) {
        *b = b
            .wrapping_add(HEAD[offset])
            .wrapping_add(((i - 2) % 5) as u8);
        offset = (offset + usize::from(head_add)) & 0xff;
    }
    let mut out = h.to_vec();
    let mut offset = usize::from(body_start);
    for (i, &b) in body.iter().enumerate() {
        out.push(b.wrapping_add(BODY[offset]).wrapping_add((i % 72) as u8));
        offset = (offset + usize::from(body_add)) & 0xff;
    }
    out
}

/// Unscrambles one answer packet: (packet count, packet number, body).
pub fn decrypt(p: &[u8]) -> Option<(u16, u16, Vec<u8>)> {
    if p.len() < 20 {
        return None;
    }
    let mut h = [0u8; 20];
    h.copy_from_slice(&p[..20]);
    let mut offset = usize::from(h[0]);
    for (i, b) in h.iter_mut().enumerate().skip(2) {
        *b = b
            .wrapping_sub(HEAD[offset])
            .wrapping_sub(((i - 2) % 5) as u8);
        offset = (offset + usize::from(p[1])) & 0xff;
    }
    let word = |i: usize| u16::from_be_bytes([h[i], h[i + 1]]);
    if word(2) != 0 {
        return None;
    }
    let mut offset = usize::from(h[16]);
    let body = p[20..]
        .iter()
        .enumerate()
        .map(|(i, &b)| {
            let v = b.wrapping_sub(BODY[offset]).wrapping_sub((i % 72) as u8);
            offset = (offset + usize::from(h[17])) & 0xff;
            v
        })
        .collect();
    Some((word(12).max(1), word(14), body))
}

/// `%20` style escapes in names.
fn unescape(v: &str) -> String {
    let b = v.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(hex) = v.get(i + 1..i + 3)
            && let Ok(n) = u8::from_str_radix(hex, 16)
        {
            out.push(n);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn parse(text: &str) -> Status {
    let mut pairs = Vec::new();
    let mut players = Vec::new();
    for line in text.split('\n') {
        let line = line.trim_end_matches(['\r', '\0']);
        let Some((key, value)) = line.split_once(": ") else {
            continue;
        };
        if key == "CLIENT" {
            let f: Vec<(&str, &str)> = value
                .split(',')
                .filter_map(|kv| kv.split_once('='))
                .collect();
            let get = |k: &str| f.iter().find(|(key, _)| *key == k).map(|(_, v)| *v);
            players.push(Player {
                index: count(players.len()),
                name: unescape(get("NAME").unwrap_or_default()),
                ping: get("PING").and_then(|v| v.parse().ok()),
                duration: get("SEC").and_then(|v| v.parse().ok()).unwrap_or(0.0),
                ..Default::default()
            });
        } else if key != "CHANNEL" {
            pairs.push((key.to_string(), unescape(value)));
        }
    }
    let get = |k: &str| {
        pairs
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let platform = get("PLATFORM").to_ascii_lowercase();
    let info = ServerInfo {
        name: get("NAME"),
        game: "Ventrilo".into(),
        players: count(
            get("CLIENTCOUNT")
                .parse::<u32>()
                .unwrap_or(players.len() as u32),
        ),
        max_players: count(get("MAXCLIENTS").parse::<u32>().unwrap_or(0)),
        passworded: get("AUTH") == "1",
        version: get("VERSION"),
        environment: if platform.contains("win") {
            b'w'
        } else if platform.contains("linux") {
            b'l'
        } else {
            0
        },
        server_type: b'd',
        ..Default::default()
    };
    Status {
        info,
        players,
        rules: rules_from(pairs),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scramble_round_trip_and_parse() {
        let body = b"NAME: Vent%20Srv\nMAXCLIENTS: 20\nCLIENTCOUNT: 1\nAUTH: 0\nCLIENT: ADMIN=0,CID=0,PING=42,SEC=90,NAME=Alice\n";
        // An answer looks like a request with zero in the "zero" field.
        let packet = encrypt(STATUS, body, [7, 3, 200, 11]);
        let (total, num, plain) = decrypt(&packet).unwrap();
        assert_eq!((total, num), (1, 0));
        assert_eq!(plain, body);
        let s = parse(&String::from_utf8_lossy(&plain));
        assert_eq!(
            (s.info.name.as_str(), s.info.players, s.info.max_players),
            ("Vent Srv", 1, 20)
        );
        assert_eq!(
            (s.players[0].name.as_str(), s.players[0].ping),
            ("Alice", Some(42))
        );
    }
}
