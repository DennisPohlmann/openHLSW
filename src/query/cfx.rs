//! CitizenFX: FiveM and RedM (port 30120). UDP `getinfo` is answered like Quake 3
//! (`infoResponse` and `\key\value`); the player list comes from `/players.json` of the
//! server's HTTP endpoint on the same port (TCP).

use std::net::SocketAddr;
use std::time::Duration;

use super::{Packet, Status, count, meta, quake};
use crate::a2s::{Player, QueryError, QueryMeta};

pub fn query(
    addr: SocketAddr,
    host: &str,
    timeout: Duration,
) -> Result<(Status, QueryMeta), QueryError> {
    let mut status = None;
    let rtt = super::udp_exchange(addr, b"\xFF\xFF\xFF\xFFgetinfo oHLSW", timeout, None, |p| {
        match quake::parse(p, &["infoResponse"]) {
            Some(s) => {
                status = Some(s);
                Ok(Packet::Done)
            }
            None => Ok(Packet::Ignore),
        }
    })?;
    let mut status = status.ok_or(QueryError::Timeout)?;
    let get = |k: &str| {
        super::rule(&status.rules, k)
            .unwrap_or_default()
            .to_string()
    };
    status.info.players = count(get("clients").parse::<u32>().unwrap_or(0));
    status.info.max_players = count(get("sv_maxclients").parse::<u32>().unwrap_or(0));
    status.info.game = get("gametype");
    status.info.folder = get("gamename");
    status.rules.retain(|r| r.key != "challenge");
    if let Ok(body) = super::http::get(addr, host, "/players.json", timeout)
        && let Ok(v) = serde_json::from_slice::<serde_json::Value>(&body)
    {
        status.players = parse_players(&v);
    }
    Ok((status, meta(rtt)))
}

/// Only name, id and ping (the list also holds account identifiers, which are not kept).
pub fn parse_players(v: &serde_json::Value) -> Vec<Player> {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|p| Player {
            index: count(p["id"].as_u64().unwrap_or(0)),
            name: p["name"].as_str().unwrap_or_default().to_string(),
            ping: p["ping"].as_u64().map(|p| p as u32),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_players() {
        let v = serde_json::json!([{"id": 3, "name": "Alice", "ping": 40, "identifiers": ["x"]}]);
        let p = parse_players(&v);
        assert_eq!(
            (p[0].index, p[0].name.as_str(), p[0].ping),
            (3, "Alice", Some(40))
        );
    }
}
