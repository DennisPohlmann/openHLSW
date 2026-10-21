//! TeamSpeak 2 (TCP query port 51234: `sel <port>`, `si`, `pl`) and TeamSpeak 3
//! (ServerQuery, TCP port 10011: `use port=<port>`, `serverinfo`, `clientlist`).
//! The address holds the voice port; the query port is the usual one unless the address
//! names it (`ip:voiceport:queryport`).

use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use super::{Status, count, meta, rules_from};
use crate::a2s::{Player, QueryError, QueryMeta, ServerInfo};

// --------------------------------------------------------------------------
// TeamSpeak 2
// --------------------------------------------------------------------------

pub fn query_ts2(
    addr: SocketAddr,
    voice_port: u16,
    timeout: Duration,
) -> Result<(Status, QueryMeta), QueryError> {
    let start = Instant::now();
    let mut s = super::tcp_connect(addr, timeout)?;
    let mut buf = Vec::new();
    let sel = ts2_command(&mut s, &mut buf, &format!("sel {voice_port}"))?;
    if !sel.contains("[TS]") && !sel.trim().is_empty() {
        return Err(QueryError::Protocol("TeamSpeak 2: no [TS] greeting".into()));
    }
    let rtt = start.elapsed();
    let si = ts2_command(&mut s, &mut buf, "si")?;
    let pl = ts2_command(&mut s, &mut buf, "pl").unwrap_or_default();
    let _ = super::tcp_send(&mut s, b"quit\n");
    Ok((parse_ts2(&si, &pl), meta(rtt)))
}

/// Sends a command; the answer ends with "OK" (or "ERROR…").
fn ts2_command(s: &mut TcpStream, buf: &mut Vec<u8>, cmd: &str) -> Result<String, QueryError> {
    super::tcp_send(s, format!("{cmd}\n").as_bytes())?;
    let raw = super::tcp_read_until(s, buf, |b| {
        let text = String::from_utf8_lossy(b);
        if let Some(p) = text.find("\r\nOK\r\n") {
            return Some(p + 6);
        }
        if text.starts_with("OK\r\n") {
            return Some(4);
        }
        text.find("ERROR")
            .and_then(|p| text[p..].find("\r\n").map(|e| p + e + 2))
    })?;
    let text = super::latin1(&raw);
    if let Some(e) = text.find("ERROR") {
        return Err(QueryError::Protocol(format!(
            "TeamSpeak 2: {}",
            text[e..].trim()
        )));
    }
    Ok(text
        .trim_end_matches("\r\nOK\r\n")
        .trim_end_matches("OK\r\n")
        .to_string())
}

pub fn parse_ts2(si: &str, pl: &str) -> Status {
    let pairs: Vec<(String, String)> = si
        .lines()
        .filter_map(|l| {
            let (k, v) = l.trim().split_once('=')?;
            Some((k.to_string(), v.to_string()))
        })
        .collect();
    let get = |k: &str| {
        pairs
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let mut lines = pl.lines().filter(|l| !l.trim().is_empty());
    let fields: Vec<&str> = lines.next().unwrap_or_default().split('\t').collect();
    let col = |row: &[&str], name: &str| {
        fields
            .iter()
            .position(|f| *f == name)
            .and_then(|i| row.get(i))
            .map(|v| v.trim_matches('"').to_string())
            .unwrap_or_default()
    };
    let players: Vec<Player> = lines
        .enumerate()
        .map(|(i, l)| {
            let row: Vec<&str> = l.split('\t').collect();
            Player {
                index: count(i),
                name: col(&row, "nick"),
                ping: col(&row, "ping").parse().ok(),
                duration: col(&row, "logintime").parse().unwrap_or(0.0),
                ..Default::default()
            }
        })
        .collect();
    let info = ServerInfo {
        name: get("server_name"),
        game: "TeamSpeak 2".into(),
        players: count(
            get("server_currentusers")
                .parse::<u32>()
                .unwrap_or(players.len() as u32),
        ),
        max_players: count(get("server_maxusers").parse::<u32>().unwrap_or(0)),
        passworded: get("server_password") == "1",
        version: get("server_version"),
        environment: if get("server_platform").to_ascii_lowercase().contains("win") {
            b'w'
        } else if get("server_platform")
            .to_ascii_lowercase()
            .contains("linux")
        {
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

// --------------------------------------------------------------------------
// TeamSpeak 3
// --------------------------------------------------------------------------

pub fn query_ts3(
    addr: SocketAddr,
    voice_port: u16,
    timeout: Duration,
) -> Result<(Status, QueryMeta), QueryError> {
    let start = Instant::now();
    let mut s = super::tcp_connect(addr, timeout)?;
    let mut buf = Vec::new();
    // Greeting: "TS3\n\r" plus a welcome line.
    super::tcp_read_until(&mut s, &mut buf, |b| {
        let text = String::from_utf8_lossy(b);
        let first = text.find("\n\r")?;
        if !text.starts_with("TS3") {
            return Some(first + 2);
        }
        let second = text[first + 2..].find("\n\r")?;
        Some(first + 2 + second + 2)
    })?;
    ts3_command(&mut s, &mut buf, &format!("use port={voice_port}"))?;
    let rtt = start.elapsed();
    let info = ts3_command(&mut s, &mut buf, "serverinfo")?;
    let clients = ts3_command(&mut s, &mut buf, "clientlist").unwrap_or_default();
    let _ = super::tcp_send(&mut s, b"quit\n");
    Ok((parse_ts3(&info, &clients), meta(rtt)))
}

/// Sends a command and returns the data before the `error id=0 msg=ok` line.
fn ts3_command(s: &mut TcpStream, buf: &mut Vec<u8>, cmd: &str) -> Result<String, QueryError> {
    super::tcp_send(s, format!("{cmd}\n").as_bytes())?;
    let raw = super::tcp_read_until(s, buf, |b| {
        let text = String::from_utf8_lossy(b);
        let e = text.find("error id=")?;
        text[e..].find("\n\r").map(|end| e + end + 2)
    })?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let e = text.find("error id=").unwrap_or(text.len());
    let error = &text[e..];
    if !error.starts_with("error id=0 ") {
        let msg = ts3_fields(error.trim())
            .into_iter()
            .find(|(k, _)| k == "msg")
            .map(|(_, v)| v)
            .unwrap_or_else(|| error.trim().to_string());
        return Err(QueryError::Protocol(format!("TeamSpeak 3: {msg}")));
    }
    Ok(text[..e].trim().to_string())
}

/// `key=value key2=value2` with ServerQuery escapes.
fn ts3_fields(text: &str) -> Vec<(String, String)> {
    text.split(' ')
        .filter(|f| !f.is_empty())
        .map(|f| {
            let (k, v) = f.split_once('=').unwrap_or((f, ""));
            (k.to_string(), ts3_unescape(v))
        })
        .collect()
}

fn ts3_unescape(v: &str) -> String {
    let mut out = String::new();
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('p') => out.push('|'),
            Some('/') => out.push('/'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => {}
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

pub fn parse_ts3(serverinfo: &str, clientlist: &str) -> Status {
    let pairs = ts3_fields(serverinfo);
    let get = |k: &str| {
        pairs
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    // Query clients (client_type=1) are no players.
    let players: Vec<Player> = clientlist
        .split('|')
        .map(ts3_fields)
        .filter(|f| f.iter().any(|(k, v)| k == "client_type" && v == "0"))
        .enumerate()
        .map(|(i, f)| Player {
            index: count(i),
            name: f
                .iter()
                .find(|(k, _)| k == "client_nickname")
                .map(|(_, v)| v.clone())
                .unwrap_or_default(),
            ..Default::default()
        })
        .collect();
    let num = |k: &str| get(k).parse::<u32>().unwrap_or(0);
    let platform = get("virtualserver_platform").to_ascii_lowercase();
    let info = ServerInfo {
        name: get("virtualserver_name"),
        game: "TeamSpeak 3".into(),
        players: count(
            num("virtualserver_clientsonline")
                .saturating_sub(num("virtualserver_queryclientsonline")),
        ),
        max_players: count(num("virtualserver_maxclients")),
        passworded: get("virtualserver_flag_password") == "1",
        version: get("virtualserver_version"),
        environment: if platform.contains("win") {
            b'w'
        } else if platform.contains("linux") {
            b'l'
        } else if platform.contains("os x") || platform.contains("mac") {
            b'm'
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
    fn parses_ts2() {
        let si = "server_name=My TS2\r\nserver_platform=Linux\r\nserver_currentusers=2\r\nserver_maxusers=50\r\nserver_password=0";
        let pl = "p_id\tc_id\tping\tlogintime\tnick\r\n1\t1\t40\t300\t\"Alice\"\r\n2\t1\t55\t20\t\"Bob\"";
        let s = parse_ts2(si, pl);
        assert_eq!(
            (s.info.name.as_str(), s.info.players, s.info.max_players),
            ("My TS2", 2, 50)
        );
        assert_eq!(
            (s.players[1].name.as_str(), s.players[1].ping),
            ("Bob", Some(55))
        );
    }

    #[test]
    fn parses_ts3() {
        let info = "virtualserver_name=My\\sTS3 virtualserver_maxclients=32 virtualserver_clientsonline=3 virtualserver_queryclientsonline=1 virtualserver_platform=Linux virtualserver_flag_password=1";
        let cl = "clid=1 cid=1 client_database_id=1 client_nickname=Alice client_type=0|clid=2 cid=1 client_database_id=2 client_nickname=serveradmin client_type=1|clid=3 cid=2 client_database_id=3 client_nickname=Bob\\pX client_type=0";
        let s = parse_ts3(info, cl);
        assert_eq!(
            (s.info.name.as_str(), s.info.players, s.info.max_players),
            ("My TS3", 2, 32)
        );
        assert!(s.info.passworded);
        assert_eq!(s.players.len(), 2);
        assert_eq!(s.players[1].name, "Bob|X");
    }
}
