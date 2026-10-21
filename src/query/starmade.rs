//! StarMade (TCP, port 4242): a fixed info request; the big-endian answer holds a typed
//! parameter list (info version, version, name, description, start time, players, max).

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use super::{Status, count, latin1, meta, rules_from};
use crate::a2s::{QueryError, QueryMeta, ServerInfo};

const REQUEST: [u8; 13] = [0, 0, 0, 9, 0x2a, 0xff, 0xff, 1, 0x6f, 0, 0, 0, 0];

pub fn query(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let start = Instant::now();
    let mut s = super::tcp_connect(addr, timeout)?;
    super::tcp_send(&mut s, &REQUEST)?;
    let mut buf = Vec::new();
    let raw = super::tcp_read_until(&mut s, &mut buf, |b| {
        let n = usize::try_from(i32::from_be_bytes(b.get(..4)?.try_into().ok()?)).ok()?;
        (b.len() >= 4 + n).then_some(4 + n)
    })?;
    parse(&raw)
        .map(|st| (st, meta(start.elapsed())))
        .ok_or_else(|| QueryError::Protocol("StarMade: bad answer".into()))
}

#[derive(Debug, PartialEq)]
enum Value {
    Int(i64),
    Float(f32),
    Text(String),
}

impl Value {
    fn text(&self) -> String {
        match self {
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Text(t) => t.clone(),
        }
    }
}

pub fn parse(d: &[u8]) -> Option<Status> {
    // size (4), timestamp (8), check (1), packet id (2), command (1), type (1)
    let mut i = 17;
    let be = |i: usize, n: usize| d.get(i..i + n);
    let n = i32::from_be_bytes(be(i, 4)?.try_into().ok()?);
    i += 4;
    let mut v = Vec::new();
    for _ in 0..n.clamp(0, 64) {
        let t = *d.get(i)?;
        i += 1;
        let value = match t {
            1 => {
                i += 4;
                Value::Int(i64::from(i32::from_be_bytes(
                    be(i - 4, 4)?.try_into().ok()?,
                )))
            }
            2 => {
                i += 8;
                Value::Int(i64::from_be_bytes(be(i - 8, 8)?.try_into().ok()?))
            }
            3 => {
                i += 4;
                Value::Float(f32::from_be_bytes(be(i - 4, 4)?.try_into().ok()?))
            }
            4 => {
                let len = usize::from(u16::from_be_bytes(be(i, 2)?.try_into().ok()?));
                i += 2 + len;
                Value::Text(latin1(be(i - len, len)?))
            }
            5 | 6 => {
                i += 1;
                Value::Int(i64::from(*d.get(i - 1)? as i8))
            }
            7 => {
                i += 2;
                Value::Int(i64::from(i16::from_be_bytes(
                    be(i - 2, 2)?.try_into().ok()?,
                )))
            }
            _ => return None,
        };
        v.push(value);
    }
    let get = |k: usize| v.get(k).map(Value::text).unwrap_or_default();
    Some(Status {
        info: ServerInfo {
            name: get(2),
            game: "StarMade".into(),
            folder: "starmade".into(),
            version: get(1),
            players: count(get(5).parse::<u32>().unwrap_or(0)),
            max_players: count(get(6).parse::<u32>().unwrap_or(0)),
            server_type: b'd',
            ..Default::default()
        },
        rules: rules_from([("description".to_string(), get(3))]),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_params() {
        let mut d = vec![0; 17];
        d.extend(7i32.to_be_bytes());
        d.push(1);
        d.extend(2i32.to_be_bytes());
        d.push(3);
        d.extend(0.2f32.to_be_bytes());
        for s in ["Star Srv", "Desc"] {
            d.push(4);
            d.extend((s.len() as u16).to_be_bytes());
            d.extend(s.as_bytes());
        }
        d.push(2);
        d.extend(5i64.to_be_bytes());
        d.push(1);
        d.extend(3i32.to_be_bytes());
        d.push(1);
        d.extend(32i32.to_be_bytes());
        let s = parse(&d).unwrap();
        assert_eq!(
            (s.info.name.as_str(), s.info.players, s.info.max_players),
            ("Star Srv", 3, 32)
        );
    }
}
