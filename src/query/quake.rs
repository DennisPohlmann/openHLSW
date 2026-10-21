//! Quake family: Quake 3 `getstatus` (Call of Duty 1–5, Quake 3, Enemy Territory, Urban
//! Terror, Jedi Knight, Warsow …), Medal of Honor (`getstatus` with a `0x02` byte) and the
//! `status` of Quake 2 (`print`) and QuakeWorld (`n`).
//!
//! Answer: a header line, then `\key\value\…` and one line per player
//! (`score ping "name"`; QuakeWorld: `id frags time ping "name" "skin" top bottom`).

use std::net::SocketAddr;
use std::time::Duration;

use super::{Packet, Status, count, latin1, meta, rule, rules_from, strip_q3_colors, truthy};
use crate::a2s::{Player, QueryError, QueryMeta, ServerInfo};

/// Quake 3 `getstatus`; `mohaa` sends Medal of Honor's variant.
pub fn query_quake3(
    addr: SocketAddr,
    timeout: Duration,
    mohaa: bool,
) -> Result<(Status, QueryMeta), QueryError> {
    let request: &[u8] = if mohaa {
        b"\xFF\xFF\xFF\xFF\x02getstatus\n"
    } else {
        b"\xFF\xFF\xFF\xFFgetstatus\n"
    };
    exchange(addr, request, timeout, &["statusResponse"])
}

/// Quake 2 / QuakeWorld `status`.
pub fn query_quake(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    exchange(addr, b"\xFF\xFF\xFF\xFFstatus\n", timeout, &["print", "n"])
}

fn exchange(
    addr: SocketAddr,
    request: &[u8],
    timeout: Duration,
    headers: &[&str],
) -> Result<(Status, QueryMeta), QueryError> {
    let mut status = None;
    let rtt = super::udp_exchange(addr, request, timeout, None, |p| match parse(p, headers) {
        Some(s) => {
            status = Some(s);
            Ok(Packet::Done)
        }
        None => Ok(Packet::Ignore),
    })?;
    let status = status.ok_or(QueryError::Timeout)?;
    Ok((status, meta(rtt)))
}

/// Parses an answer whose header is one of `headers`.
pub fn parse(packet: &[u8], headers: &[&str]) -> Option<Status> {
    let rest = packet.strip_prefix(b"\xFF\xFF\xFF\xFF")?;
    // Medal of Honor puts a 0x01 before the header.
    let rest = rest.strip_prefix(b"\x01").unwrap_or(rest);
    let text = latin1(rest);
    // QuakeWorld: "n\key\value…" without a line break after the header.
    let (header, body) = if let Some(body) = text.strip_prefix('n')
        && headers.contains(&"n")
        && body.starts_with('\\')
    {
        ("n", body.to_string())
    } else {
        let (h, b) = text.split_once('\n')?;
        (h.trim_end(), b.to_string())
    };
    if !headers.contains(&header) {
        return None;
    }
    let quakeworld = header == "n";
    let mut lines = body.split('\n');
    let pairs = lines.next().unwrap_or_default();
    let mut parts = pairs.split('\\');
    if pairs.starts_with('\\') {
        parts.next();
    }
    let mut kv = Vec::new();
    while let (Some(k), Some(v)) = (parts.next(), parts.next()) {
        kv.push((k.to_string(), strip_q3_colors(v)));
    }
    let rules = rules_from(kv);
    let mut players = Vec::new();
    for line in lines {
        let line = line.trim_end_matches(['\r', '\0']);
        if line.trim().is_empty() {
            continue;
        }
        if let Some(p) = parse_player(line, quakeworld, players.len()) {
            players.push(p);
        }
    }
    let get = |k: &str| rule(&rules, k).unwrap_or_default().to_string();
    let first = |keys: &[&str]| {
        keys.iter()
            .map(|k| get(k))
            .find(|v| !v.is_empty())
            .unwrap_or_default()
    };
    let bots = players.iter().filter(|p| p.bot).count();
    let info = ServerInfo {
        name: first(&["sv_hostname", "hostname"]),
        map: first(&["mapname", "map"]),
        folder: first(&["fs_game", "gamedir", "*gamedir", "gamename"]),
        game: first(&["gamename", "game", "*gamedir"]),
        players: count(players.len()),
        bots: count(bots),
        max_players: count(
            first(&["sv_maxclients", "maxclients", "maxspectators"])
                .parse::<u32>()
                .unwrap_or(0),
        ),
        passworded: truthy(rule(&rules, "g_needpass"))
            || truthy(rule(&rules, "pswrd"))
            || truthy(rule(&rules, "needpass")),
        version: first(&["shortversion", "version", "*version", "iv"]),
        server_type: if get("dedicated") == "0" { b'l' } else { b'd' },
        environment: {
            let v = first(&["version", "*version"]).to_ascii_lowercase();
            if v.contains("win") {
                b'w'
            } else if v.contains("linux") {
                b'l'
            } else {
                0
            }
        },
        ..Default::default()
    };
    Some(Status {
        info,
        players,
        rules,
    })
}

/// Words of a player line; quoted parts stay together.
fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, part) in line.split('"').enumerate() {
        if i % 2 == 1 {
            out.push(part.to_string());
        } else {
            out.extend(part.split_whitespace().map(str::to_string));
        }
    }
    out
}

fn parse_player(line: &str, quakeworld: bool, index: usize) -> Option<Player> {
    let w = words(line);
    let num = |i: usize| w.get(i).and_then(|v| v.trim().parse::<i64>().ok());
    if quakeworld {
        // id frags time ping "name" "skin" top bottom
        let ping = num(3).unwrap_or(0);
        return Some(Player {
            index: count(index),
            name: strip_q3_colors(w.get(4)?),
            score: num(1).unwrap_or(0) as i32,
            duration: num(2).unwrap_or(0) as f32 * 60.0,
            ping: Some(ping.max(0) as u32),
            ..Default::default()
        });
    }
    // score ping "name" [team / address]
    let score = num(0)? as i32;
    let ping = num(1).unwrap_or(0);
    let name = strip_q3_colors(w.get(2).map(String::as_str).unwrap_or_default());
    Some(Player {
        index: count(index),
        name,
        score,
        ping: Some(ping.max(0) as u32),
        // Bots answer with ping 0.
        bot: ping == 0,
        team: w.get(3).cloned().unwrap_or_default(),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quake3() {
        let p = b"\xFF\xFF\xFF\xFFstatusResponse\n\\sv_hostname\\^1My ^7CoD4\\mapname\\mp_crash\\sv_maxclients\\24\\g_needpass\\1\\shortversion\\1.7\\fs_game\\mods/pml\n10 45 \"^2Alice\"\n3 0 \"Bot\"\n";
        let s = parse(p, &["statusResponse"]).unwrap();
        assert_eq!(s.info.name, "My CoD4");
        assert_eq!(s.info.map, "mp_crash");
        assert_eq!(
            (s.info.players, s.info.max_players, s.info.bots),
            (2, 24, 1)
        );
        assert!(s.info.passworded);
        assert_eq!(s.info.version, "1.7");
        assert_eq!(s.players[0].name, "Alice");
        assert_eq!((s.players[0].score, s.players[0].ping), (10, Some(45)));
        assert!(s.players[1].bot);
    }

    #[test]
    fn parses_mohaa_and_quake2_and_quakeworld() {
        let m = b"\xFF\xFF\xFF\xFF\x01statusResponse\n\\hostname\\MoH\\mapname\\obj/obj_team1\n";
        assert_eq!(parse(m, &["statusResponse"]).unwrap().info.name, "MoH");

        let q2 = b"\xFF\xFF\xFF\xFFprint\n\\hostname\\Q2 Server\\maxclients\\8\\mapname\\q2dm1\n5 30 \"player\"\n";
        let s = parse(q2, &["print", "n"]).unwrap();
        assert_eq!((s.info.name.as_str(), s.info.max_players), ("Q2 Server", 8));
        assert_eq!(s.players.len(), 1);

        let qw = b"\xFF\xFF\xFF\xFFn\\hostname\\QW\\maxclients\\16\\map\\dm4\n1 12 5 40 \"bob\" \"base\" 4 4\n";
        let s = parse(qw, &["print", "n"]).unwrap();
        assert_eq!((s.info.name.as_str(), s.info.map.as_str()), ("QW", "dm4"));
        assert_eq!(
            (s.players[0].name.as_str(), s.players[0].score),
            ("bob", 12)
        );
        assert!(parse(qw, &["statusResponse"]).is_none());
    }
}
