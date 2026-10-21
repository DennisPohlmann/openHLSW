//! Frostbite admin port (TCP): Battlefield: Bad Company 2 (port 48888) and Battlefield 3
//! (game port + 22000). `serverInfo`, `version` and `listPlayers all` need no login.
//!
//! Packet: sequence (u32, bit 30 = response), total size (u32), word count (u32), then per
//! word its length (u32), the bytes and a zero byte. All little-endian.

use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use super::{Status, count, meta, rules_from};
use crate::a2s::{Player, QueryError, QueryMeta, ServerInfo};

pub fn query(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let start = Instant::now();
    let mut stream = super::tcp_connect(addr, timeout)?;
    let mut buf = Vec::new();
    let info = command(&mut stream, &mut buf, &["serverInfo"])?;
    let rtt = start.elapsed();
    let version = command(&mut stream, &mut buf, &["version"]).unwrap_or_default();
    let players = command(&mut stream, &mut buf, &["listPlayers", "all"]).unwrap_or_default();
    let mut status = parse_server_info(&info).ok_or_else(|| bad("serverInfo"))?;
    if let [game, ver, ..] = version.as_slice() {
        status.info.version = format!("{game} {ver}");
        status.info.game = match game.as_str() {
            "BC2" => "Battlefield: Bad Company 2".into(),
            "BF3" => "Battlefield 3".into(),
            other => other.to_string(),
        };
    }
    status.players = parse_players(&players);
    Ok((status, meta(rtt)))
}

fn bad(what: &str) -> QueryError {
    QueryError::Protocol(format!("Frostbite: bad {what} answer"))
}

pub fn packet(words: &[&str]) -> Vec<u8> {
    let size = 12 + words.iter().map(|w| w.len() + 5).sum::<usize>();
    let mut out = Vec::with_capacity(size);
    out.extend(0u32.to_le_bytes());
    out.extend((size as u32).to_le_bytes());
    out.extend((words.len() as u32).to_le_bytes());
    for w in words {
        out.extend((w.len() as u32).to_le_bytes());
        out.extend(w.as_bytes());
        out.push(0);
    }
    out
}

/// Sends a command and returns the words of the answer after "OK".
fn command(
    stream: &mut TcpStream,
    buf: &mut Vec<u8>,
    words: &[&str],
) -> Result<Vec<String>, QueryError> {
    super::tcp_send(stream, &packet(words))?;
    loop {
        let raw = super::tcp_read_until(stream, buf, |b| {
            let size = u32::from_le_bytes(b.get(4..8)?.try_into().ok()?) as usize;
            (size >= 12 && b.len() >= size).then_some(size)
        })?;
        let header = u32::from_le_bytes(raw[..4].try_into().unwrap_or_default());
        // Events the server sends on its own are no answer.
        if header & 0x4000_0000 == 0 {
            continue;
        }
        let mut words = decode(&raw).ok_or_else(|| bad("packet"))?;
        if words.first().map(String::as_str) != Some("OK") {
            return Err(QueryError::Protocol(format!(
                "Frostbite: {}",
                words.first().cloned().unwrap_or_default()
            )));
        }
        words.remove(0);
        return Ok(words);
    }
}

pub fn decode(raw: &[u8]) -> Option<Vec<String>> {
    let n = u32::from_le_bytes(raw.get(8..12)?.try_into().ok()?) as usize;
    let mut i = 12;
    let mut out = Vec::with_capacity(n.min(4096));
    for _ in 0..n {
        let len = u32::from_le_bytes(raw.get(i..i + 4)?.try_into().ok()?) as usize;
        i += 4;
        out.push(String::from_utf8_lossy(raw.get(i..i + len)?).into_owned());
        i += len + 1;
    }
    Some(out)
}

/// `serverInfo`: name, players, max players, game mode, map, rounds played / total,
/// team count, the teams' scores, target score, state, ranked, PunkBuster, password …
pub fn parse_server_info(w: &[String]) -> Option<Status> {
    let mut it = w.iter();
    let name = it.next()?.clone();
    let players = it.next()?.parse::<u32>().unwrap_or(0);
    let max = it.next()?.parse::<u32>().unwrap_or(0);
    let mode = it.next()?.clone();
    let map = it.next()?.clone();
    let rounds_played = it.next().cloned().unwrap_or_default();
    let rounds_total = it.next().cloned().unwrap_or_default();
    let teams = it.next().and_then(|t| t.parse::<usize>().ok()).unwrap_or(0);
    let scores: Vec<String> = it.by_ref().take(teams).cloned().collect();
    let target = it.next().cloned().unwrap_or_default();
    let state = it.next().cloned().unwrap_or_default();
    let ranked = it.next().cloned().unwrap_or_default();
    let punkbuster = it.next().cloned().unwrap_or_default();
    let password = it.next().cloned().unwrap_or_default();
    let mut pairs = vec![
        ("gamemode".to_string(), mode.clone()),
        (
            "rounds".to_string(),
            format!("{rounds_played} / {rounds_total}"),
        ),
        ("teamscores".to_string(), scores.join(", ")),
        ("targetscore".to_string(), target),
        ("state".to_string(), state),
        ("ranked".to_string(), ranked),
        ("punkbuster".to_string(), punkbuster),
    ];
    if let Some(uptime) = it.next() {
        pairs.push(("uptime".into(), uptime.clone()));
    }
    let info = ServerInfo {
        name,
        map,
        game: mode,
        players: count(players),
        max_players: count(max),
        passworded: password == "true",
        server_type: b'd',
        ..Default::default()
    };
    Some(Status {
        info,
        rules: rules_from(pairs),
        ..Default::default()
    })
}

/// `listPlayers all`: field count, field names, player count, the values.
pub fn parse_players(w: &[String]) -> Vec<Player> {
    let mut it = w.iter();
    let Some(n_fields) = it.next().and_then(|n| n.parse::<usize>().ok()) else {
        return Vec::new();
    };
    let fields: Vec<String> = it.by_ref().take(n_fields).cloned().collect();
    let n = it.next().and_then(|n| n.parse::<usize>().ok()).unwrap_or(0);
    let mut out = Vec::new();
    for i in 0..n {
        let mut p = Player {
            index: count(i),
            ..Default::default()
        };
        for f in &fields {
            let Some(v) = it.next() else { break };
            let int = || v.parse::<i32>().unwrap_or(0);
            match f.as_str() {
                "name" => p.name = v.clone(),
                "score" => p.score = int(),
                "kills" if p.score == 0 => p.score = int(),
                "deaths" => p.deaths = Some(int()),
                "ping" => p.ping = Some(int().max(0) as u32),
                "teamId" => p.team = v.clone(),
                _ => {}
            }
        }
        out.push(p);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn packets_and_answers() {
        let p = packet(&["serverInfo"]);
        assert_eq!(p.len(), 12 + 4 + 10 + 1);
        assert_eq!(decode(&p).unwrap(), ["serverInfo"]);

        let s = parse_server_info(&w(&[
            "BC2 Server",
            "12",
            "32",
            "CONQUEST",
            "Levels/MP_001",
            "0",
            "2",
            "2",
            "150",
            "120",
            "0",
            "",
            "true",
            "true",
            "false",
            "3600",
        ]))
        .unwrap();
        assert_eq!(
            (s.info.name.as_str(), s.info.players, s.info.max_players),
            ("BC2 Server", 12, 32)
        );
        assert_eq!(s.info.map, "Levels/MP_001");
        assert!(!s.info.passworded);

        let p = parse_players(&w(&[
            "4", "name", "kills", "deaths", "ping", "1", "Alice", "9", "2", "70",
        ]));
        assert_eq!(
            (p[0].name.as_str(), p[0].score, p[0].deaths, p[0].ping),
            ("Alice", 9, Some(2), Some(70))
        );
    }
}
