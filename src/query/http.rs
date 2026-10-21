//! Servers that answer over HTTP with JSON: TShock (Terraria REST API, port 7878), Eco
//! (web server, game port + 1) and Stationeers (public server list of the game).

use std::net::{SocketAddr, ToSocketAddrs};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{Status, count, meta, rules_from};
use crate::a2s::{Player, QueryError, QueryMeta, ServerInfo};

/// Plain HTTP/1.0 GET; returns the body of a 200 answer.
pub(crate) fn get(
    addr: SocketAddr,
    host: &str,
    path: &str,
    timeout: Duration,
) -> Result<Vec<u8>, QueryError> {
    let mut s = super::tcp_connect(addr, timeout)?;
    let req = format!(
        "GET {path} HTTP/1.0\r\nHost: {host}\r\nAccept: application/json, */*\r\nUser-Agent: openHLSW\r\nConnection: close\r\n\r\n"
    );
    super::tcp_send(&mut s, req.as_bytes())?;
    let deadline = Instant::now() + timeout;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 16384];
    loop {
        use std::io::Read;
        if Instant::now() > deadline || buf.len() > 8 << 20 {
            break;
        }
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                crate::stats::add_received(n + 40);
                buf.extend_from_slice(&chunk[..n]);
            }
            Err(e) if buf.is_empty() => return Err(e.into()),
            Err(_) => break,
        }
    }
    let end = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| QueryError::Protocol("HTTP: no answer".into()))?;
    let head = String::from_utf8_lossy(&buf[..end]);
    let code = head.split_whitespace().nth(1).unwrap_or_default();
    if code != "200" {
        return Err(QueryError::Protocol(format!("HTTP {code}")));
    }
    Ok(buf[end + 4..].to_vec())
}

fn json(body: &[u8]) -> Result<Value, QueryError> {
    serde_json::from_slice(body).map_err(|e| QueryError::Protocol(format!("JSON: {e}")))
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn num(v: &Value) -> u32 {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        .unwrap_or(0) as u32
}

/// Removes `<color=…>` style markup (Eco descriptions).
fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

// --- TShock -----------------------------------------------------------------

pub fn query_tshock(
    addr: SocketAddr,
    host: &str,
    timeout: Duration,
) -> Result<(Status, QueryMeta), QueryError> {
    let start = Instant::now();
    let body = get(
        addr,
        host,
        "/v2/server/status?players=true&rules=true",
        timeout,
    )?;
    parse_tshock(&json(&body)?).map(|s| (s, meta(start.elapsed())))
}

pub fn parse_tshock(v: &Value) -> Result<Status, QueryError> {
    if text(&v["status"]) != "200" {
        return Err(QueryError::Protocol(format!(
            "TShock: status {}",
            text(&v["status"])
        )));
    }
    let players = v["players"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, p)| Player {
            index: count(i),
            name: text(&p["nickname"]),
            team: text(&p["team"]),
            ..Default::default()
        })
        .collect::<Vec<_>>();
    let rules = v["rules"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, v)| (k.clone(), text(v)))
        .collect::<Vec<_>>();
    Ok(Status {
        info: ServerInfo {
            name: text(&v["name"]),
            map: text(&v["world"]),
            game: "Terraria".into(),
            folder: "terraria".into(),
            version: text(&v["serverversion"]),
            players: count(num(&v["playercount"])),
            max_players: count(num(&v["maxplayers"])),
            passworded: v["serverpassword"].as_bool().unwrap_or(false),
            game_port: v["port"].as_u64().and_then(|p| u16::try_from(p).ok()),
            server_type: b'd',
            ..Default::default()
        },
        players,
        rules: rules_from(rules),
    })
}

// --- Eco ----------------------------------------------------------------------

pub fn query_eco(
    addr: SocketAddr,
    host: &str,
    timeout: Duration,
) -> Result<(Status, QueryMeta), QueryError> {
    let start = Instant::now();
    let body = match get(addr, host, "/info", timeout) {
        Ok(b) => b,
        Err(_) => get(addr, host, "/frontpage", timeout)?,
    };
    let v = json(&body)?;
    parse_eco(&v).map(|s| (s, meta(start.elapsed())))
}

pub fn parse_eco(v: &Value) -> Result<Status, QueryError> {
    // `/frontpage` wraps the info in "Info".
    let i = if v.get("Info").is_some() {
        &v["Info"]
    } else {
        v
    };
    if !i.is_object() {
        return Err(QueryError::Protocol("Eco: bad answer".into()));
    }
    let name = strip_tags(&text(&i["Description"]));
    let rules = i
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, v)| !v.is_array() && !v.is_object())
        .map(|(k, v)| (k.to_ascii_lowercase(), text(v)))
        .collect::<Vec<_>>();
    Ok(Status {
        info: ServerInfo {
            name,
            game: "Eco".into(),
            folder: "eco".into(),
            version: text(&i["Version"]),
            players: count(num(&i["OnlinePlayers"])),
            max_players: count(num(&i["TotalPlayers"])),
            passworded: i["HasPassword"].as_bool().unwrap_or(false),
            game_port: i["GamePort"].as_u64().and_then(|p| u16::try_from(p).ok()),
            server_type: b'd',
            ..Default::default()
        },
        players: Vec::new(),
        rules: rules_from(rules),
    })
}

// --- Stationeers ----------------------------------------------------------------

/// Server list of the game ("meta server"), as used by GameQ.
const STATIONEERS_LIST: (&str, u16) = ("40.82.200.175", 8081);

pub fn query_stationeers(
    ip: std::net::IpAddr,
    port: u16,
    timeout: Duration,
) -> Result<(Status, QueryMeta), QueryError> {
    let start = Instant::now();
    let list = STATIONEERS_LIST
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| QueryError::Protocol("Stationeers: no server list".into()))?;
    let body = get(list, STATIONEERS_LIST.0, "/list", timeout)?;
    parse_stationeers(&json(&body)?, &ip.to_string(), port).map(|s| (s, meta(start.elapsed())))
}

pub fn parse_stationeers(v: &Value, ip: &str, port: u16) -> Result<Status, QueryError> {
    let entry = v["GameSessions"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| text(&s["Address"]) == ip && num(&s["Port"]) == u32::from(port))
        .ok_or_else(|| {
            QueryError::Protocol("Stationeers: server is not in the server list".into())
        })?;
    Ok(Status {
        info: ServerInfo {
            name: text(&entry["Name"]),
            map: text(&entry["MapName"]),
            game: "Stationeers".into(),
            folder: "stationeers".into(),
            version: text(&entry["Version"]),
            players: count(num(&entry["Players"])),
            max_players: count(num(&entry["MaxPlayers"])),
            passworded: entry["Password"]
                .as_bool()
                .unwrap_or(num(&entry["Password"]) != 0),
            server_type: b'd',
            ..Default::default()
        },
        players: Vec::new(),
        rules: rules_from([
            ("uptime".to_string(), text(&entry["UpTime"])),
            ("type".to_string(), text(&entry["Type"])),
        ]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_answers() {
        let v: Value = serde_json::from_str(r#"{"status":"200","name":"Terra","port":7777,"playercount":1,"maxplayers":8,"world":"Isle","serverversion":"v1.4","serverpassword":false,"players":[{"nickname":"Alice","team":0}],"rules":{"AutoSave":true}}"#).unwrap();
        let s = parse_tshock(&v).unwrap();
        assert_eq!(
            (s.info.name.as_str(), s.info.map.as_str(), s.info.players),
            ("Terra", "Isle", 1)
        );
        assert_eq!(
            (s.players[0].name.as_str(), s.rules[0].value.as_str()),
            ("Alice", "true")
        );

        let v: Value = serde_json::from_str(r#"{"Info":{"Description":"<color=green>Eco</color> Srv","OnlinePlayers":2,"TotalPlayers":30,"HasPassword":true,"Version":"0.9"}}"#).unwrap();
        let s = parse_eco(&v).unwrap();
        assert_eq!(
            (s.info.name.as_str(), s.info.players, s.info.passworded),
            ("Eco Srv", 2, true)
        );

        let v: Value = serde_json::from_str(r#"{"GameSessions":[{"Name":"Moon","Address":"192.0.2.5","Port":27500,"Players":2,"MaxPlayers":10,"MapName":"Moon","Password":false}]}"#).unwrap();
        let s = parse_stationeers(&v, "192.0.2.5", 27500).unwrap();
        assert_eq!((s.info.name.as_str(), s.info.max_players), ("Moon", 10));
        assert_eq!(strip_tags("<b>x</b> y"), "x y");
    }
}
