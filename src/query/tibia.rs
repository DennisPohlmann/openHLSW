//! Open Tibia servers (TCP, port 7171): `06 00 FF FF "info"` is answered with an XML
//! document (`<tsqp>` with `serverinfo`, `players`, `map` … attributes).

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use super::{Status, count, meta, rules_from};
use crate::a2s::{QueryError, QueryMeta, ServerInfo};

pub fn query(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let start = Instant::now();
    let mut s = super::tcp_connect(addr, timeout)?;
    super::tcp_send(&mut s, b"\x06\x00\xFF\xFFinfo")?;
    let mut buf = Vec::new();
    let raw = super::tcp_read_until(&mut s, &mut buf, |b| {
        let t = String::from_utf8_lossy(b);
        t.find("</tsqp>").map(|e| e + 7)
    })?;
    parse(&String::from_utf8_lossy(&raw))
        .map(|st| (st, meta(start.elapsed())))
        .ok_or_else(|| QueryError::Protocol("Tibia: bad answer".into()))
}

/// Attributes of the first `<tag …>` element.
fn attrs(xml: &str, tag: &str) -> Vec<(String, String)> {
    let Some(start) = xml.find(&format!("<{tag} ")) else {
        return Vec::new();
    };
    let rest = &xml[start + tag.len() + 2..];
    let end = rest.find('>').unwrap_or(rest.len());
    let mut out = Vec::new();
    let mut s = &rest[..end];
    while let Some(eq) = s.find("=\"") {
        let key = s[..eq].trim().to_string();
        let after = &s[eq + 2..];
        let Some(close) = after.find('"') else { break };
        out.push((key, unescape(&after[..close])));
        s = &after[close + 1..];
    }
    out
}

fn unescape(v: &str) -> String {
    v.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

pub fn parse(xml: &str) -> Option<Status> {
    if !xml.contains("<tsqp") {
        return None;
    }
    let info = attrs(xml, "serverinfo");
    let players = attrs(xml, "players");
    let map = attrs(xml, "map");
    let get = |v: &[(String, String)], k: &str| {
        v.iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let motd = xml
        .find("<motd>")
        .and_then(|s| {
            xml[s + 6..]
                .find("</motd>")
                .map(|e| unescape(&xml[s + 6..s + 6 + e]))
        })
        .unwrap_or_default();
    let mut rules: Vec<(String, String)> = info
        .iter()
        .filter(|(k, _)| k != "ip" && k != "port")
        .cloned()
        .collect();
    rules.push(("players_peak".to_string(), get(&players, "peak")));
    rules.push(("motd".to_string(), motd));
    Some(Status {
        info: ServerInfo {
            name: get(&info, "servername"),
            map: get(&map, "name"),
            game: "Tibia".into(),
            folder: "tibia".into(),
            version: get(&info, "version"),
            players: count(get(&players, "online").parse::<u32>().unwrap_or(0)),
            max_players: count(get(&players, "max").parse::<u32>().unwrap_or(0)),
            game_port: get(&info, "port").parse().ok(),
            server_type: b'd',
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
    fn parses_tsqp() {
        let xml = r#"<?xml version="1.0"?><tsqp version="1.0"><serverinfo uptime="100" servername="OT &amp; Co" port="7172" version="1.5"/><players online="4" max="900" peak="20"/><map name="world" width="1000"/><motd>Welcome</motd></tsqp>"#;
        let s = parse(xml).unwrap();
        assert_eq!(
            (
                s.info.name.as_str(),
                s.info.map.as_str(),
                s.info.players,
                s.info.max_players
            ),
            ("OT & Co", "world", 4, 255)
        );
        assert_eq!(s.info.game_port, Some(7172));
    }
}
