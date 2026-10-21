//! Queries for the games of HLSW's list that do not speak Valve A2S: Quake 1/2/3 (Call of
//! Duty, Medal of Honor, Warsow …), GameSpy 1 (Unreal Tournament, Battlefield 1942 …), the
//! native Unreal 2 protocol (UT2003/2004, Killing Floor …), Doom 3 (Quake 4, Prey, ETQW),
//! All-Seeing Eye (Far Cry, MTA), Frostbite (Battlefield Bad Company 2 / 3), TeamSpeak 2 / 3,
//! Ventrilo, Mumble, Tribes 2, Savage, Frontlines, SA-MP, Teeworlds, OpenTTD, Minecraft
//! (Java and Bedrock), FiveM / RedM, Counter-Strike 2D, Tibia, StarMade, the Mafia
//! multiplayer mods, the HTTP APIs of Terraria (TShock), Eco and Stationeers, and a
//! reachability check for Shockvoice.
//!
//! Every protocol returns the whole [`Status`] (info, players, rules) at once. The packet
//! formats follow the descriptions in QStat, node-GameDig and GameQ (written anew here).

pub mod ase;
pub mod cfx;
pub mod cs2d;
pub mod doom3;
pub mod ffow;
pub mod frostbite;
pub mod gamespy1;
pub mod http;
pub mod mafia;
pub mod minecraft;
pub mod mumble;
pub mod openttd;
pub mod quake;
pub mod raknet;
pub mod samp;
pub mod savage;
pub mod starmade;
pub mod teamspeak;
pub mod teeworlds;
pub mod tibia;
pub mod tribes2;
pub mod unreal2;
pub mod ventrilo;
mod ventrilo_tables;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::a2s::{Player, QueryError, QueryMeta, Rule, ServerInfo};

/// How a server is queried (the protocols of HLSW's game list).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Protocol {
    /// Valve A2S (Source, Half-Life 1, CS2), detected automatically.
    #[default]
    A2s,
    /// GameSpy 2 / 3 on a separate query port (Battlefield 2, ArmA …).
    GameSpy2,
    /// GameSpy 1 (`\status\`): Unreal Tournament, Battlefield 1942, Halo, Vietcong …
    GameSpy1,
    /// Unreal Engine 2's own query (port + 1), with GameSpy 1 as fallback: UT2003/2004,
    /// Killing Floor, Red Orchestra, Devastation …
    Unreal2,
    /// Quake 3 `getstatus`: Call of Duty 1–5, Quake 3, Enemy Territory, Warsow …
    Quake3,
    /// Medal of Honor (Quake 3 with a `0x02` prefix, or GameSpy 1 on port + 97).
    MoHAA,
    /// Quake 1 (QuakeWorld) and Quake 2 `status`.
    Quake,
    /// Doom 3 `getInfo`: Doom 3, Quake 4, Prey, Wolfenstein, ET: Quake Wars.
    Doom3,
    /// All-Seeing Eye (port + 123): Far Cry, Multi Theft Auto.
    Ase,
    /// Frostbite RCON port (TCP): Battlefield Bad Company 2 / Battlefield 3.
    Frostbite,
    /// TeamSpeak 2 TCP query (port 51234).
    TeamSpeak2,
    /// TeamSpeak 3 ServerQuery (TCP port 10011).
    TeamSpeak3,
    /// Ventrilo (encrypted UDP status).
    Ventrilo,
    /// Tribes 2 info query.
    Tribes2,
    /// Savage: The Battle for Newerth.
    Savage,
    /// Frontlines: Fuel of War (A2S variant on port + 2).
    Ffow,
    /// Shockvoice: no documented query, only reachability (TCP connect).
    Shockvoice,
    /// San Andreas Multiplayer / open.mp.
    Samp,
    /// Teeworlds / DDNet.
    Teeworlds,
    /// Mumble (UDP ping, channel viewer on TCP 27800).
    Mumble,
    /// OpenTTD.
    OpenTtd,
    /// RakNet ping: Minecraft: Bedrock Edition.
    Raknet,
    /// Minecraft: Java Edition server list ping (TCP).
    Minecraft,
    /// CitizenFX: FiveM, RedM.
    Cfx,
    /// Counter-Strike 2D.
    Cs2d,
    /// Open Tibia servers (TCP).
    Tibia,
    /// StarMade (TCP).
    StarMade,
    /// Lost Heaven Multiplayer (Mafia).
    Lhmp,
    /// Mafia 2 Multiplayer.
    M2mp,
    /// Terraria with TShock (REST API, HTTP).
    TShock,
    /// Eco (web server, HTTP).
    Eco,
    /// Stationeers (public server list, HTTP).
    Stationeers,
}

impl Protocol {
    /// Name shown in the settings (My Games).
    pub fn name(self) -> &'static str {
        match self {
            Protocol::A2s => "Valve",
            Protocol::GameSpy2 => "GameSpy 2/3",
            Protocol::GameSpy1 => "GameSpy 1",
            Protocol::Unreal2 => "Unreal 2",
            Protocol::Quake3 => "Quake 3",
            Protocol::MoHAA => "Medal of Honor",
            Protocol::Quake => "Quake 1/2",
            Protocol::Doom3 => "Doom 3",
            Protocol::Ase => "All-Seeing Eye",
            Protocol::Frostbite => "Frostbite",
            Protocol::TeamSpeak2 => "TeamSpeak 2",
            Protocol::TeamSpeak3 => "TeamSpeak 3",
            Protocol::Ventrilo => "Ventrilo",
            Protocol::Tribes2 => "Tribes 2",
            Protocol::Savage => "Savage",
            Protocol::Ffow => "Frontlines",
            Protocol::Shockvoice => "TCP",
            Protocol::Samp => "SA-MP",
            Protocol::Teeworlds => "Teeworlds",
            Protocol::Mumble => "Mumble",
            Protocol::OpenTtd => "OpenTTD",
            Protocol::Raknet => "RakNet",
            Protocol::Minecraft => "Minecraft",
            Protocol::Cfx => "CitizenFX",
            Protocol::Cs2d => "CS2D",
            Protocol::Tibia => "Tibia",
            Protocol::StarMade => "StarMade",
            Protocol::Lhmp => "LHMP",
            Protocol::M2mp => "M2MP",
            Protocol::TShock => "TShock (HTTP)",
            Protocol::Eco => "Eco (HTTP)",
            Protocol::Stationeers => "Stationeers (HTTP)",
        }
    }

    /// Whether the protocol is answered over TCP (slower, and some servers limit how often
    /// they may be asked, so answers are kept a little longer).
    fn is_tcp(self) -> bool {
        matches!(
            self,
            Protocol::Frostbite
                | Protocol::TeamSpeak2
                | Protocol::TeamSpeak3
                | Protocol::Shockvoice
                | Protocol::Minecraft
                | Protocol::Cfx
                | Protocol::Tibia
                | Protocol::StarMade
                | Protocol::TShock
                | Protocol::Eco
                | Protocol::Stationeers
        )
    }
}

/// Everything a query found out.
#[derive(Debug, Clone, Default)]
pub struct Status {
    pub info: ServerInfo,
    pub players: Vec<Player>,
    pub rules: Vec<Rule>,
}

/// The server to ask.
#[derive(Debug, Clone)]
pub struct Target {
    pub host: String,
    /// Game (or voice) port.
    pub port: u16,
    /// Query port given in the address (`ip:port:queryport`).
    pub query_port: Option<u16>,
    /// HLSW key of the game (picks the usual query port), e.g. "UT2004".
    pub game: String,
}

/// Queries `target` and returns info, players and rules. Tries the usual query ports of
/// the game (the given query port first) and remembers the one that answered.
pub fn query(
    protocol: Protocol,
    target: &Target,
    timeout: Duration,
) -> Result<(Status, QueryMeta), QueryError> {
    let key = (
        target.host.clone(),
        target.port,
        target.query_port,
        protocol,
    );
    let ttl = if protocol.is_tcp() {
        Duration::from_secs(4)
    } else {
        Duration::from_millis(900)
    };
    if let Some((status, meta, at)) = lock(&CACHE).get(&key)
        && at.elapsed() < ttl
    {
        return Ok((status.clone(), *meta));
    }
    let ip = resolve(&target.host)?;
    let mut tries = candidates(protocol, target);
    // The port (and protocol) that answered last time goes first.
    if let Some(&remembered) = lock(&PORTS).get(&key) {
        tries.retain(|t| *t != remembered);
        tries.insert(0, remembered);
    }
    // Several candidates share the time, but each gets at least 400 ms.
    let each = (timeout / tries.len().max(1) as u32).max(Duration::from_millis(400));
    let mut last = QueryError::Timeout;
    for (proto, port) in tries {
        let addr = SocketAddr::new(ip, port);
        let result = match proto {
            Protocol::GameSpy1 => gamespy1::query(addr, each),
            Protocol::Unreal2 => unreal2::query(addr, each),
            Protocol::Quake3 => quake::query_quake3(addr, each, false),
            Protocol::MoHAA => quake::query_quake3(addr, each, true),
            Protocol::Quake => quake::query_quake(addr, each),
            Protocol::Doom3 => doom3::query(addr, each),
            Protocol::Ase => ase::query(addr, each),
            Protocol::Frostbite => frostbite::query(addr, each),
            Protocol::TeamSpeak2 => teamspeak::query_ts2(addr, target.port, each),
            Protocol::TeamSpeak3 => teamspeak::query_ts3(addr, target.port, each),
            Protocol::Ventrilo => ventrilo::query(addr, each),
            Protocol::Tribes2 => tribes2::query(addr, each),
            Protocol::Savage => savage::query(addr, each),
            Protocol::Ffow => ffow::query(addr, each),
            Protocol::Shockvoice => reachable(addr, each),
            Protocol::Samp => samp::query(addr, each),
            Protocol::Teeworlds => teeworlds::query(addr, each),
            Protocol::Mumble => mumble_full(addr, each),
            Protocol::OpenTtd => openttd::query(addr, each),
            Protocol::Raknet => raknet::query(addr, each),
            Protocol::Minecraft => minecraft::query(addr, &target.host, each),
            Protocol::Cfx => cfx::query(addr, &target.host, each),
            Protocol::Cs2d => cs2d::query(addr, each),
            Protocol::Tibia => tibia::query(addr, each),
            Protocol::StarMade => starmade::query(addr, each),
            Protocol::Lhmp => mafia::query_lhmp(addr, each),
            Protocol::M2mp => mafia::query_m2mp(addr, each),
            Protocol::TShock => http::query_tshock(addr, &target.host, each),
            Protocol::Eco => http::query_eco(addr, &target.host, each),
            Protocol::Stationeers => http::query_stationeers(ip, target.port, each),
            Protocol::A2s | Protocol::GameSpy2 => {
                return Err(QueryError::Protocol("not handled here".into()));
            }
        };
        match result {
            Ok((mut status, meta)) => {
                if status.info.game_port.is_none() {
                    status.info.game_port = Some(target.port);
                }
                status.info.gamespy = true;
                lock(&PORTS).insert(key.clone(), (proto, port));
                lock(&CACHE).insert(key, (status.clone(), meta, Instant::now()));
                return Ok((status, meta));
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

type Key = (String, u16, Option<u16>, Protocol);
type Cache = HashMap<Key, (Status, QueryMeta, Instant)>;
static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(Default::default);
static PORTS: LazyLock<Mutex<HashMap<Key, (Protocol, u16)>>> = LazyLock::new(Default::default);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn resolve(host: &str) -> Result<std::net::IpAddr, QueryError> {
    (host, 0)
        .to_socket_addrs()
        .map_err(QueryError::Io)?
        .next()
        .map(|a| a.ip())
        .ok_or_else(|| QueryError::Protocol(format!("DNS: {host} not found")))
}

/// Query ports to try, in order: the one in the address, then the game's usual ones.
fn candidates(protocol: Protocol, t: &Target) -> Vec<(Protocol, u16)> {
    let p = t.port;
    let off = |o: i32| u16::try_from(i32::from(p) + o).ok();
    let mut out: Vec<(Protocol, u16)> = Vec::new();
    let mut add = |proto: Protocol, port: Option<u16>| {
        if let Some(port) = port
            && port > 0
            && !out.contains(&(proto, port))
        {
            out.push((proto, port));
        }
    };
    if let Some(q) = t.query_port {
        add(protocol, Some(q));
        if protocol == Protocol::Unreal2 {
            add(Protocol::GameSpy1, Some(q));
        }
        if protocol == Protocol::MoHAA {
            add(Protocol::GameSpy1, Some(q));
        }
        // An explicit query port is meant: only that one (TeamSpeak / Frostbite query
        // ports belong to the voice / game port, so they are only the query port).
        return out;
    }
    let g = t.game.as_str();
    match protocol {
        Protocol::GameSpy1 => {
            match g {
                "BF1942" => add(protocol, Some(23000)),
                "CCR" => add(protocol, Some(25300)),
                "_Custom:Vietcong" => add(protocol, off(10000)),
                "ROO" | "RO" => add(protocol, off(10)),
                "AvP2" => add(protocol, Some(p)),
                _ => {}
            }
            add(protocol, off(1));
            add(protocol, off(10));
            add(protocol, Some(p));
        }
        Protocol::Unreal2 => {
            add(Protocol::Unreal2, off(1));
            add(Protocol::GameSpy1, off(10));
            add(Protocol::GameSpy1, off(1));
            add(Protocol::Unreal2, Some(p));
        }
        Protocol::MoHAA => {
            add(Protocol::MoHAA, Some(p));
            add(Protocol::GameSpy1, off(97));
            add(Protocol::Quake3, Some(p));
        }
        Protocol::Doom3 => {
            add(protocol, Some(p));
            if g == "ETQW" {
                add(protocol, Some(27733));
            }
        }
        Protocol::Ase => {
            add(protocol, off(123));
            add(protocol, Some(p));
        }
        Protocol::Frostbite => {
            add(protocol, Some(48888));
            add(protocol, off(22000));
            add(protocol, Some(47200));
            add(protocol, Some(p));
        }
        Protocol::TeamSpeak2 => add(protocol, Some(51234)),
        Protocol::TeamSpeak3 => add(protocol, Some(10011)),
        Protocol::Ffow => {
            add(protocol, off(2));
            add(protocol, Some(p));
        }
        Protocol::Lhmp | Protocol::M2mp | Protocol::Eco => {
            add(protocol, off(1));
            add(protocol, Some(p));
        }
        Protocol::TShock => {
            add(protocol, off(101));
            add(protocol, Some(7878));
            add(protocol, Some(p));
        }
        _ => add(protocol, Some(p)),
    }
    out
}

// --------------------------------------------------------------------------
// Helpers shared by the protocols
// --------------------------------------------------------------------------

/// What a protocol makes of a received packet.
pub(crate) enum Packet {
    /// Not an answer to us.
    Ignore,
    /// Part of the answer; more may come.
    More,
    /// The answer is complete.
    Done,
}

/// Sends `request` and feeds every answer to `on_packet` until it says [`Packet::Done`].
/// With `settle`, the answer is also complete that long after the last accepted packet
/// (for protocols without an end marker). Returns the round trip time.
pub(crate) fn udp_exchange(
    addr: SocketAddr,
    request: &[u8],
    timeout: Duration,
    settle: Option<Duration>,
    mut on_packet: impl FnMut(&[u8]) -> Result<Packet, QueryError>,
) -> Result<Duration, QueryError> {
    let socket = UdpSocket::bind(if addr.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    })?;
    socket.connect(addr)?;
    let start = Instant::now();
    socket.send(request)?;
    crate::stats::add_sent(request.len() + 28);
    let mut deadline = start + timeout;
    let mut rtt = None;
    let mut buf = vec![0u8; 65536];
    loop {
        let now = Instant::now();
        if now >= deadline {
            return match (rtt, settle) {
                (Some(rtt), Some(_)) => Ok(rtt),
                _ => Err(QueryError::Timeout),
            };
        }
        socket.set_read_timeout(Some(deadline - now))?;
        let n = match socket.recv(&mut buf) {
            Ok(n) => n,
            Err(e) => {
                let e = QueryError::from(e);
                if matches!(e, QueryError::Timeout) {
                    continue;
                }
                return Err(e);
            }
        };
        crate::stats::add_received(n + 28);
        match on_packet(&buf[..n])? {
            Packet::Ignore => {}
            Packet::More => {
                rtt.get_or_insert_with(|| start.elapsed());
                if let Some(settle) = settle {
                    deadline = deadline.min(Instant::now() + settle);
                }
            }
            Packet::Done => return Ok(*rtt.get_or_insert_with(|| start.elapsed())),
        }
    }
}

/// A TCP connection with read / write timeouts.
pub(crate) fn tcp_connect(addr: SocketAddr, timeout: Duration) -> Result<TcpStream, QueryError> {
    let stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let _ = stream.set_nodelay(true);
    Ok(stream)
}

pub(crate) fn tcp_send(stream: &mut TcpStream, data: &[u8]) -> Result<(), QueryError> {
    stream.write_all(data)?;
    crate::stats::add_sent(data.len() + 40);
    Ok(())
}

/// Reads until `complete` finds the answer complete (returns its length).
pub(crate) fn tcp_read_until(
    stream: &mut TcpStream,
    buf: &mut Vec<u8>,
    mut complete: impl FnMut(&[u8]) -> Option<usize>,
) -> Result<Vec<u8>, QueryError> {
    let mut chunk = [0u8; 8192];
    loop {
        if let Some(len) = complete(buf) {
            let answer = buf[..len].to_vec();
            buf.drain(..len);
            return Ok(answer);
        }
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Err(QueryError::Protocol("Connection closed".into()));
        }
        crate::stats::add_received(n + 40);
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// Mumble: the UDP ping on the voice port; names and users from the channel viewer
/// (TCP port 27800) when the server has one.
fn mumble_full(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let (mut status, meta) = mumble::query(addr, timeout)?;
    let viewer = SocketAddr::new(addr.ip(), 27800);
    if let Ok((json, _)) = mumble::query_json(viewer, timeout.min(Duration::from_millis(700))) {
        status.info.name = json.info.name;
        status.players = json.players;
    }
    Ok((status, meta))
}

/// Shockvoice: no documented status query, so only whether the server accepts a
/// connection (and how fast).
fn reachable(addr: SocketAddr, timeout: Duration) -> Result<(Status, QueryMeta), QueryError> {
    let start = Instant::now();
    TcpStream::connect_timeout(&addr, timeout)?;
    let info = ServerInfo {
        name: addr.to_string(),
        game: "Shockvoice".into(),
        server_type: b'd',
        ..Default::default()
    };
    Ok((
        Status {
            info,
            ..Default::default()
        },
        meta(start.elapsed()),
    ))
}

pub(crate) fn meta(rtt: Duration) -> QueryMeta {
    QueryMeta {
        rtt,
        packets: 1,
        challenge: None,
    }
}

/// Removes Quake 3 colour codes (`^1`, `^7`, `^X…` with six hex digits).
pub(crate) fn strip_q3_colors(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '^' {
            match chars.peek() {
                Some('X') | Some('x') => {
                    chars.next();
                    for _ in 0..6 {
                        chars.next();
                    }
                    continue;
                }
                Some(n) if n.is_ascii_alphanumeric() || n.is_ascii_punctuation() => {
                    chars.next();
                    continue;
                }
                _ => {}
            }
        }
        // Call of Duty sends control bytes before the name.
        if !c.is_control() {
            out.push(c);
        }
    }
    out
}

/// Latin-1 bytes to a string (old games send Latin-1, not UTF-8).
pub(crate) fn latin1(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// Counts at most 255 (the size of the player fields of [`ServerInfo`]).
pub(crate) fn count(n: impl TryInto<u32>) -> u8 {
    n.try_into().unwrap_or(0).min(255) as u8
}

pub(crate) fn rules_from(pairs: impl IntoIterator<Item = (String, String)>) -> Vec<Rule> {
    pairs
        .into_iter()
        .filter(|(k, _)| !k.is_empty())
        .map(|(key, value)| Rule { key, value })
        .collect()
}

/// Value of `key` (any case) in `rules`.
pub(crate) fn rule<'a>(rules: &'a [Rule], key: &str) -> Option<&'a str> {
    rules
        .iter()
        .find(|r| r.key.eq_ignore_ascii_case(key))
        .map(|r| r.value.as_str())
}

pub(crate) fn truthy(v: Option<&str>) -> bool {
    matches!(
        v.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_colors() {
        assert_eq!(strip_q3_colors("^1Red^7 Server"), "Red Server");
        assert_eq!(strip_q3_colors("^XFF0000Hex"), "Hex");
        assert_eq!(strip_q3_colors("a^"), "a^");
    }

    #[test]
    fn query_port_candidates() {
        let t = |game: &str, q: Option<u16>| Target {
            host: "localhost".into(),
            port: 7777,
            query_port: q,
            game: game.into(),
        };
        assert_eq!(
            candidates(Protocol::Unreal2, &t("UT2004", None))[..3],
            [
                (Protocol::Unreal2, 7778),
                (Protocol::GameSpy1, 7787),
                (Protocol::GameSpy1, 7778)
            ]
        );
        assert_eq!(
            candidates(Protocol::GameSpy1, &t("BF1942", None))[0],
            (Protocol::GameSpy1, 23000)
        );
        assert_eq!(
            candidates(Protocol::TeamSpeak3, &t("TS3", Some(10022))),
            [(Protocol::TeamSpeak3, 10022)]
        );
        assert_eq!(candidates(Protocol::Ase, &t("FarCry", None))[0].1, 7900);
    }
}
