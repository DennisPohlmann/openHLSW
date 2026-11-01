//! Valve Server Queries (A2S_INFO, A2S_PLAYER, A2S_RULES) for Source and GoldSrc (HL1) servers.
//! <https://developer.valvesoftware.com/wiki/Server_queries>

use std::collections::HashMap;
use std::fmt;
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

const HEADER_SINGLE: i32 = -1;
const HEADER_SPLIT: i32 = -2;
const MAX_CHALLENGE_ROUNDS: u32 = 3;
const MAX_SPLIT_PARTS: u8 = 64;

/// Also stored in the config, so offline servers still show their last known name and map.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ServerInfo {
    pub protocol: u8,
    pub name: String,
    pub map: String,
    /// Game directory, e.g. "cstrike".
    pub folder: String,
    /// Game description, e.g. "Counter-Strike: Source".
    pub game: String,
    pub app_id: u16,
    pub players: u8,
    pub max_players: u8,
    pub bots: u8,
    /// 'd' dedicated, 'l' listen, 'p' SourceTV relay
    pub server_type: u8,
    /// 'l' Linux, 'w' Windows, 'm'/'o' macOS
    pub environment: u8,
    pub passworded: bool,
    pub vac: bool,
    pub version: String,
    pub game_port: Option<u16>,
    pub steam_id: Option<u64>,
    pub tv_port: Option<u16>,
    pub tv_name: Option<String>,
    pub keywords: Option<String>,
    pub game_id: Option<u64>,
    /// GoldSrc (Half-Life 1 engine) server: answered with the old 'm' info format or
    /// reports a Half-Life 1 AppID (CS 1.6 = 10, CZ = 80, HL/mods = 70, ...).
    pub goldsrc: bool,
    /// Answered via the GameSpy 2 protocol (`ip:port:queryport` servers).
    pub gamespy: bool,
}

impl ServerInfo {
    pub fn os_name(&self) -> &'static str {
        match self.environment {
            b'l' => "Linux",
            b'w' => "Windows",
            b'm' | b'o' => "macOS",
            _ => "?",
        }
    }

    /// Full Steam AppID. The 16-bit `app_id` field is truncated for newer games;
    /// the 64-bit GameID (if present) carries the real one in its low 24 bits.
    pub fn real_app_id(&self) -> u32 {
        match self.game_id {
            Some(id) if id & 0xFF_FFFF != 0 => (id & 0xFF_FFFF) as u32,
            _ => self.app_id as u32,
        }
    }

    pub fn server_type_name(&self) -> &'static str {
        match self.server_type {
            b'd' => "Dedicated",
            b'l' => "Listen",
            b'p' => "SourceTV",
            _ => "?",
        }
    }

    /// HLSW's grey rows after the rules (cvars): the info query's fields under HLSW's
    /// keys. Fields the protocol does not report stay empty, like in HLSW.
    pub fn info_rules(&self) -> Vec<Rule> {
        let opt = |v: Option<u64>| v.map(|v| v.to_string()).unwrap_or_default();
        let app_id = self.real_app_id();
        [
            ("gamedir", self.folder.clone()),
            ("gamename", self.game.clone()),
            ("steamid", opt((app_id != 0).then_some(app_id.into()))),
            ("clientcount", self.players.to_string()),
            ("clientmax", self.max_players.to_string()),
            ("botcount", self.bots.to_string()),
            ("secure", u8::from(self.vac).to_string()),
            ("server_steam_id", opt(self.steam_id)),
            ("game_tag", self.keywords.clone().unwrap_or_default()),
            ("game_id", opt(self.game_id)),
        ]
        .into_iter()
        .map(|(key, value)| Rule {
            key: key.to_owned(),
            value,
        })
        .collect()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Player {
    pub index: u8,
    pub name: String,
    pub score: i32,
    /// Seconds connected.
    pub duration: f32,
    /// Extra columns some protocols report per player (GameSpy: Battlefield 2 …).
    pub deaths: Option<i32>,
    pub ping: Option<u32>,
    pub skill: Option<i32>,
    pub team: String,
    pub bot: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Rule {
    pub key: String,
    pub value: String,
}

#[derive(Debug)]
pub enum QueryError {
    Io(std::io::Error),
    Timeout,
    Protocol(String),
    /// An ICMP error came back instead of an answer: "port unreachable" from a closed
    /// port, or "administratively prohibited" / "host unreachable" from a firewall in
    /// front of the server. Firewalls of game server hosters answer like this when one
    /// address sends them too many queries, so it is a reason to slow down.
    Unreachable(std::io::Error),
}

/// True for the errors an ICMP "destination unreachable" leaves on a connected UDP
/// socket: port unreachable (`ECONNREFUSED`, Windows `WSAECONNRESET`), host or network
/// unreachable and "communication administratively prohibited" (`EHOSTUNREACH`,
/// `ENETUNREACH`), and a local firewall rule that rejects the packet (`EPERM`/`EACCES`).
pub fn is_unreachable(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::ConnectionRefused
            | ErrorKind::ConnectionReset
            | ErrorKind::HostUnreachable
            | ErrorKind::NetworkUnreachable
            | ErrorKind::PermissionDenied
    )
}

/// Text for an ICMP error: "Port unreachable", or the system's text for the others.
pub fn unreachable_text(e: &std::io::Error) -> String {
    match e.kind() {
        ErrorKind::ConnectionRefused | ErrorKind::ConnectionReset => "Port unreachable".into(),
        _ => format!("Unreachable: {e}"),
    }
}

/// An I/O error of a UDP query socket: ICMP errors become [`QueryError::Unreachable`].
pub(crate) fn udp_error(e: std::io::Error) -> QueryError {
    if is_unreachable(e.kind()) {
        QueryError::Unreachable(e)
    } else {
        e.into()
    }
}

/// How long a UDP query waits after an ICMP error before it sends again: a rate limit in
/// front of the server usually lets the next packet through after a short pause.
pub const UNREACHABLE_PAUSE: Duration = Duration::from_millis(300);

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryError::Io(e) => write!(f, "{e}"),
            QueryError::Timeout => write!(f, "Timeout"),
            QueryError::Protocol(m) => write!(f, "{m}"),
            QueryError::Unreachable(e) => write!(f, "{}", unreachable_text(e)),
        }
    }
}

impl std::error::Error for QueryError {}

impl From<std::io::Error> for QueryError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            ErrorKind::WouldBlock | ErrorKind::TimedOut => QueryError::Timeout,
            _ => QueryError::Io(e),
        }
    }
}

/// Details of a finished query, used for rate limiting.
#[derive(Debug, Clone, Copy)]
pub struct QueryMeta {
    /// Round trip time of the request that got the answer.
    pub rtt: Duration,
    /// Number of request packets sent (challenge round trips and retries included).
    pub packets: u32,
    /// The challenge the server accepted; pass it to the next query to save a round trip.
    pub challenge: Option<[u8; 4]>,
}

/// Queries A2S_INFO. Returns the info and the round trip time of the final request.
pub fn query_info(
    addr: SocketAddr,
    timeout: Duration,
) -> Result<(ServerInfo, Duration), QueryError> {
    query_info_with(addr, timeout, None).map(|(info, meta)| (info, meta.rtt))
}

pub fn query_players(addr: SocketAddr, timeout: Duration) -> Result<Vec<Player>, QueryError> {
    query_players_with(addr, timeout, None).map(|(p, _)| p)
}

pub fn query_rules(addr: SocketAddr, timeout: Duration) -> Result<Vec<Rule>, QueryError> {
    query_rules_with(addr, timeout, None).map(|(r, _)| r)
}

/// Like [`query_info`], starting with a known challenge (saves one packet per query).
pub fn query_info_with(
    addr: SocketAddr,
    timeout: Duration,
    challenge: Option<[u8; 4]>,
) -> Result<(ServerInfo, QueryMeta), QueryError> {
    let (payload, meta) = request(addr, Kind::Info, timeout, challenge)?;
    let info = parse_info(&payload)
        .ok_or_else(|| QueryError::Protocol("Malformed A2S_INFO response".into()))?;
    Ok((info, meta))
}

pub fn query_players_with(
    addr: SocketAddr,
    timeout: Duration,
    challenge: Option<[u8; 4]>,
) -> Result<(Vec<Player>, QueryMeta), QueryError> {
    let (payload, meta) = request(addr, Kind::Players, timeout, challenge)?;
    Ok((parse_players(&payload), meta))
}

pub fn query_rules_with(
    addr: SocketAddr,
    timeout: Duration,
    challenge: Option<[u8; 4]>,
) -> Result<(Vec<Rule>, QueryMeta), QueryError> {
    let (payload, meta) = request(addr, Kind::Rules, timeout, challenge)?;
    let rules = parse_rules(&payload)
        .ok_or_else(|| QueryError::Protocol("Malformed A2S_RULES response".into()))?;
    Ok((rules, meta))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Info,
    Players,
    Rules,
}

impl Kind {
    fn response_type(self) -> u8 {
        match self {
            Kind::Info => b'I',
            Kind::Players => b'D',
            Kind::Rules => b'E',
        }
    }
}

fn build_request(kind: Kind, challenge: Option<[u8; 4]>) -> Vec<u8> {
    let mut p = vec![0xFF, 0xFF, 0xFF, 0xFF];
    match kind {
        Kind::Info => {
            p.push(b'T');
            p.extend_from_slice(b"Source Engine Query\0");
            if let Some(c) = challenge {
                p.extend_from_slice(&c);
            }
        }
        Kind::Players | Kind::Rules => {
            p.push(if kind == Kind::Players { b'U' } else { b'V' });
            p.extend_from_slice(&challenge.unwrap_or([0xFF; 4]));
        }
    }
    p
}

/// Raw parts of a split response. Source and GoldSrc use different split headers, so
/// the parts are kept until the first part tells which one it is.
struct Split {
    raw: Vec<Vec<u8>>,
}

/// Sends one request, follows S2C_CHALLENGE answers, reassembles split packets
/// and retries once on timeout. Returns the payload starting at the type byte.
fn request(
    addr: SocketAddr,
    kind: Kind,
    timeout: Duration,
    known_challenge: Option<[u8; 4]>,
) -> Result<(Vec<u8>, QueryMeta), QueryError> {
    let socket = UdpSocket::bind(if addr.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    })?;
    socket.connect(addr)?;

    let mut challenge = known_challenge;
    let mut packets = 0;
    let mut challenge_rounds = 0;
    let mut timeouts = 0;
    let mut buf = vec![0u8; 65536];

    'send: loop {
        let packet = build_request(kind, challenge);
        // A pending ICMP error of the last packet may also show up here.
        match socket.send(&packet) {
            Ok(_) => {}
            Err(e) if is_unreachable(e.kind()) && timeouts == 0 => {
                timeouts += 1;
                std::thread::sleep(UNREACHABLE_PAUSE);
                socket.send(&packet).map_err(udp_error)?;
            }
            Err(e) => return Err(udp_error(e)),
        }
        packets += 1;
        crate::stats::add_sent(packet.len() + 28); // + IP/UDP headers
        let sent = Instant::now();
        let deadline = sent + timeout;
        let mut splits: HashMap<u32, Split> = HashMap::new();

        loop {
            let now = Instant::now();
            if now >= deadline {
                timeouts += 1;
                if timeouts > 1 {
                    return Err(QueryError::Timeout);
                }
                continue 'send;
            }
            socket.set_read_timeout(Some(deadline - now))?;
            let n = match socket.recv(&mut buf) {
                Ok(n) => {
                    crate::stats::add_received(n + 28);
                    n
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    continue;
                }
                // ICMP error (closed port, or a firewall that rejects us for now): like a
                // lost packet, wait a moment and send once more before giving up.
                Err(e) if is_unreachable(e.kind()) => {
                    timeouts += 1;
                    if timeouts > 1 {
                        return Err(QueryError::Unreachable(e));
                    }
                    std::thread::sleep(UNREACHABLE_PAUSE);
                    continue 'send;
                }
                Err(e) => return Err(e.into()),
            };

            let Some(payload) = handle_packet(&buf[..n], &mut splits)? else {
                continue;
            };
            match payload.first().copied() {
                Some(b'A') if payload.len() >= 5 => {
                    challenge_rounds += 1;
                    if challenge_rounds > MAX_CHALLENGE_ROUNDS {
                        return Err(QueryError::Protocol(
                            "Server keeps sending challenges".into(),
                        ));
                    }
                    challenge = Some([payload[1], payload[2], payload[3], payload[4]]);
                    continue 'send;
                }
                Some(t) if t == kind.response_type() => {
                    let meta = QueryMeta {
                        rtt: sent.elapsed(),
                        packets,
                        challenge,
                    };
                    return Ok((payload, meta));
                }
                // Old GoldSrc servers answer A2S_INFO in their own format.
                Some(b'm') if kind == Kind::Info => {
                    let meta = QueryMeta {
                        rtt: sent.elapsed(),
                        packets,
                        challenge,
                    };
                    return Ok((payload, meta));
                }
                _ => continue,
            }
        }
    }
}

/// Returns the complete payload once a single packet or all parts of a split
/// response have arrived.
fn handle_packet(
    data: &[u8],
    splits: &mut HashMap<u32, Split>,
) -> Result<Option<Vec<u8>>, QueryError> {
    let mut r = Reader::new(data);
    let Some(header) = r.i32() else {
        return Ok(None);
    };

    if header == HEADER_SINGLE {
        return Ok(Some(data[4..].to_vec()));
    }
    if header != HEADER_SPLIT {
        return Ok(None);
    }

    // Source:  id(4) total(1) number(1) max_size(2) payload
    // GoldSrc: id(4) number<<4|total(1) payload
    let Some(id) = r.u32() else {
        return Ok(None);
    };
    if data.len() < 10 || data.len() > 65535 {
        return Ok(None);
    }
    let split = splits
        .entry(id)
        .or_insert_with(|| Split { raw: Vec::new() });
    if split.raw.len() >= MAX_SPLIT_PARTS as usize {
        splits.remove(&id);
        return Ok(None);
    }
    split.raw.push(data.to_vec());

    const FF4: [u8; 4] = [0xFF; 4];
    let goldsrc_first = |p: &Vec<u8>| p.len() >= 13 && p[8] >> 4 == 0 && p[9..13] == FF4;
    let source_first = |p: &Vec<u8>| p.len() >= 16 && p[9] == 0 && p[12..16] == FF4;
    let goldsrc = if split.raw.iter().any(goldsrc_first) {
        true
    } else if split.raw.iter().any(source_first) {
        false
    } else {
        return Ok(None); // wait for the first part
    };
    if !goldsrc && id & 0x8000_0000 != 0 {
        return Err(QueryError::Protocol(
            "Compressed responses are not supported".into(),
        ));
    }

    let mut parts: HashMap<u8, &[u8]> = HashMap::new();
    let mut total = 0u8;
    for p in &split.raw {
        let (t, n, payload) = if goldsrc {
            (p[8] & 0x0F, p[8] >> 4, &p[9..])
        } else {
            if p.len() < 12 {
                continue;
            }
            (p[8], p[9], &p[12..])
        };
        if t == 0 || t > MAX_SPLIT_PARTS || n >= t {
            continue;
        }
        total = t;
        parts.insert(n, payload);
    }
    if total == 0 || parts.len() < total as usize {
        return Ok(None);
    }
    let mut full = Vec::new();
    for i in 0..total {
        full.extend_from_slice(parts.get(&i).copied().unwrap_or_default());
    }
    splits.remove(&id);
    if full.len() >= 5 && full[..4] == FF4 {
        Ok(Some(full[4..].to_vec()))
    } else {
        Ok(None)
    }
}

pub fn parse_info(payload: &[u8]) -> Option<ServerInfo> {
    let mut r = Reader::new(payload);
    match r.u8()? {
        b'I' => {}
        b'm' => return parse_goldsrc_info(&mut r),
        _ => return None,
    }
    let mut info = ServerInfo {
        protocol: r.u8()?,
        name: r.string()?,
        map: r.string()?,
        folder: r.string()?,
        game: r.string()?,
        app_id: r.u16()?,
        players: r.u8()?,
        max_players: r.u8()?,
        bots: r.u8()?,
        server_type: r.u8()?,
        environment: r.u8()?,
        passworded: r.u8()? != 0,
        vac: r.u8()? != 0,
        ..Default::default()
    };
    if info.app_id == 2400 {
        // The Ship: mode, witnesses, duration
        r.u8()?;
        r.u8()?;
        r.u8()?;
    }
    info.version = r.string()?;

    if let Some(edf) = r.u8() {
        if edf & 0x80 != 0 {
            info.game_port = r.u16();
        }
        if edf & 0x10 != 0 {
            info.steam_id = r.u64();
        }
        if edf & 0x40 != 0 {
            info.tv_port = r.u16();
            info.tv_name = r.string();
        }
        if edf & 0x20 != 0 {
            info.keywords = r.string();
        }
        if edf & 0x01 != 0 {
            info.game_id = r.u64();
        }
    }
    info.goldsrc = is_goldsrc_app(info.real_app_id());
    Some(info)
}

/// Half-Life 1 engine games and mods have AppIDs below 200 (CS 1.6 = 10, CZ = 80).
pub fn is_goldsrc_app(app_id: u32) -> bool {
    app_id > 0 && app_id < 200
}

/// Obsolete GoldSrc A2S_INFO answer ('m').
fn parse_goldsrc_info(r: &mut Reader<'_>) -> Option<ServerInfo> {
    let _address = r.string()?;
    let mut info = ServerInfo {
        name: r.string()?,
        map: r.string()?,
        folder: r.string()?,
        game: r.string()?,
        players: r.u8()?,
        max_players: r.u8()?,
        protocol: r.u8()?,
        server_type: r.u8()?.to_ascii_lowercase(),
        environment: r.u8()?.to_ascii_lowercase(),
        passworded: r.u8()? != 0,
        app_id: 70,
        goldsrc: true,
        ..Default::default()
    };
    if r.u8()? == 1 {
        // mod info: link, download link, null, version, size, type, dll
        r.string()?;
        r.string()?;
        r.u8()?;
        info.version = r.i32()?.to_string();
        r.i32()?;
        r.u8()?;
        r.u8()?;
    }
    info.vac = r.u8().is_some_and(|v| v != 0);
    info.bots = r.u8().unwrap_or(0);
    info.app_id = match info.folder.to_ascii_lowercase().as_str() {
        "cstrike" => 10,
        "tfc" => 20,
        "dod" => 30,
        "dmc" => 40,
        "gearbox" => 50,
        "ricochet" => 60,
        "czero" => 80,
        "bshift" => 130,
        _ => 70,
    };
    Some(info)
}

pub fn parse_players(payload: &[u8]) -> Vec<Player> {
    let mut r = Reader::new(payload);
    let mut players = Vec::new();
    // type byte + count (count is unreliable for >255 players, read until the end)
    if r.u8() != Some(b'D') || r.u8().is_none() {
        return players;
    }
    while !r.at_end() {
        let parsed = (|| {
            Some(Player {
                index: r.u8()?,
                name: r.string()?,
                score: r.i32()?,
                duration: r.f32()?,
                ..Default::default()
            })
        })();
        match parsed {
            Some(p) => players.push(p),
            None => break,
        }
    }
    players
}

pub fn parse_rules(payload: &[u8]) -> Option<Vec<Rule>> {
    let mut r = Reader::new(payload);
    if r.u8()? != b'E' {
        return None;
    }
    let count = r.u16()?;
    let mut rules = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let (Some(key), Some(value)) = (r.string(), r.string()) else {
            break;
        };
        rules.push(Rule { key, value });
    }
    Some(rules)
}

/// Little-endian reader; every getter returns `None` once data runs out.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    fn at_end(&self) -> bool {
        self.pos >= self.data.len()
    }
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let bytes = self.data.get(self.pos..self.pos + N)?;
        self.pos += N;
        bytes.try_into().ok()
    }
    fn u8(&mut self) -> Option<u8> {
        self.take::<1>().map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take().map(u16::from_le_bytes)
    }
    fn i32(&mut self) -> Option<i32> {
        self.take().map(i32::from_le_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.take().map(u32::from_le_bytes)
    }
    fn u64(&mut self) -> Option<u64> {
        self.take().map(u64::from_le_bytes)
    }
    fn f32(&mut self) -> Option<f32> {
        self.take().map(f32::from_le_bytes)
    }
    fn string(&mut self) -> Option<String> {
        let rest = self.data.get(self.pos..)?;
        let end = rest.iter().position(|&b| b == 0)?;
        let s = decode_text(&rest[..end]);
        self.pos += end + 1;
        Some(s)
    }
}

/// Names are UTF-8 on current servers; old ones send Windows-1252/Latin-1.
/// Decode as UTF-8 when valid, otherwise byte-by-byte instead of showing "�".
pub fn decode_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_info_with_edf() {
        let mut p = vec![b'I', 17];
        for s in ["My Server", "de_dust2", "cstrike", "Counter-Strike: Source"] {
            p.extend_from_slice(s.as_bytes());
            p.push(0);
        }
        p.extend_from_slice(&240u16.to_le_bytes());
        p.extend_from_slice(&[12, 32, 2, b'd', b'l', 0, 1]);
        p.extend_from_slice(b"9540945\0");
        p.push(0x80 | 0x20);
        p.extend_from_slice(&27015u16.to_le_bytes());
        p.extend_from_slice(b"alltalk,increased_maxplayers\0");

        let info = parse_info(&p).unwrap();
        assert_eq!(info.name, "My Server");
        assert_eq!(info.app_id, 240);
        assert_eq!((info.players, info.max_players, info.bots), (12, 32, 2));
        assert!(info.vac && !info.passworded);
        assert_eq!(info.os_name(), "Linux");
        assert_eq!(info.game_port, Some(27015));
        assert_eq!(
            info.keywords.as_deref(),
            Some("alltalk,increased_maxplayers")
        );
    }

    #[test]
    fn reassembles_goldsrc_split_packets() {
        let mut body = vec![0xFF, 0xFF, 0xFF, 0xFF, b'E', 1, 0];
        body.extend_from_slice(b"mp_timelimit\x0030\0");
        let (a, b) = body.split_at(10);
        let mk = |n: u8, chunk: &[u8]| {
            let mut v = (-2i32).to_le_bytes().to_vec();
            v.extend_from_slice(&9u32.to_le_bytes());
            v.push(n << 4 | 2);
            v.extend_from_slice(chunk);
            v
        };
        let mut splits = HashMap::new();
        assert!(handle_packet(&mk(1, b), &mut splits).unwrap().is_none());
        let payload = handle_packet(&mk(0, a), &mut splits).unwrap().unwrap();
        let rules = parse_rules(&payload).unwrap();
        assert_eq!(rules[0].key, "mp_timelimit");
        assert_eq!(rules[0].value, "30");
    }

    #[test]
    fn parses_goldsrc_info() {
        let mut p = vec![b'm'];
        for s in [
            "1.2.3.4:27015",
            "My CZ",
            "de_dust2",
            "czero",
            "Condition Zero",
        ] {
            p.extend_from_slice(s.as_bytes());
            p.push(0);
        }
        p.extend_from_slice(&[5, 20, 48, b'D', b'L', 0, 0, 1, 2]);
        let info = parse_info(&p).unwrap();
        assert!(info.goldsrc);
        assert_eq!(info.app_id, 80);
        assert_eq!((info.players, info.max_players, info.bots), (5, 20, 2));
        assert_eq!(info.server_type, b'd');
    }

    #[test]
    fn info_rules_use_hlsw_keys() {
        let info = ServerInfo {
            folder: "cstrike".into(),
            game: "Counter-Strike".into(),
            app_id: 10,
            max_players: 32,
            vac: true,
            game_id: Some(10),
            ..ServerInfo::default()
        };
        let rows: Vec<(String, String)> = info
            .info_rules()
            .into_iter()
            .map(|r| (r.key, r.value))
            .collect();
        let expected = [
            ("gamedir", "cstrike"),
            ("gamename", "Counter-Strike"),
            ("steamid", "10"),
            ("clientcount", "0"),
            ("clientmax", "32"),
            ("botcount", "0"),
            ("secure", "1"),
            ("server_steam_id", ""),
            ("game_tag", ""),
            ("game_id", "10"),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned()));
        assert_eq!(rows, expected);
        // Protocols without a Steam AppID (GameSpy …): empty, not "0".
        let other = ServerInfo::default().info_rules();
        assert_eq!(other[2].value, "");
        assert_eq!(other[6].value, "0");
    }

    #[test]
    fn reassembles_split_packets() {
        let body = {
            let mut b = vec![0xFF, 0xFF, 0xFF, 0xFF, b'E', 1, 0];
            b.extend_from_slice(b"sv_gravity\0800\0");
            b
        };
        let (a, b) = body.split_at(9);
        let mk = |n: u8, chunk: &[u8]| {
            let mut v = (-2i32).to_le_bytes().to_vec();
            v.extend_from_slice(&7u32.to_le_bytes());
            v.extend_from_slice(&[2, n]);
            v.extend_from_slice(&1248u16.to_le_bytes());
            v.extend_from_slice(chunk);
            v
        };
        let mut splits = HashMap::new();
        assert!(handle_packet(&mk(1, b), &mut splits).unwrap().is_none());
        let payload = handle_packet(&mk(0, a), &mut splits).unwrap().unwrap();
        let rules = parse_rules(&payload).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(
            (rules[0].key.as_str(), rules[0].value.as_str()),
            ("sv_gravity", "800")
        );
    }

    /// A local UDP port that is closed right now (the OS answers with ICMP).
    fn closed_port() -> SocketAddr {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.local_addr().unwrap()
    }

    fn info_packet() -> Vec<u8> {
        let mut p = vec![0xFF, 0xFF, 0xFF, 0xFF, b'I', 17];
        for s in ["My Server", "de_dust2", "cstrike", "Counter-Strike: Source"] {
            p.extend_from_slice(s.as_bytes());
            p.push(0);
        }
        p.extend_from_slice(&240u16.to_le_bytes());
        p.extend_from_slice(&[1, 32, 0, b'd', b'l', 0, 1]);
        p.extend_from_slice(b"1.0\0");
        p.push(0);
        p
    }

    #[test]
    fn closed_port_is_unreachable_after_a_retry() {
        let addr = closed_port();
        let started = Instant::now();
        let e = query_info(addr, Duration::from_millis(1500)).unwrap_err();
        assert!(matches!(e, QueryError::Unreachable(_)), "{e:?}");
        assert_eq!(e.to_string(), "Port unreachable");
        assert!(started.elapsed() >= UNREACHABLE_PAUSE);
    }

    #[test]
    fn one_icmp_error_is_retried() {
        // Closed for the first packet (ICMP error), open when the query sends again.
        let addr = closed_port();
        let server = std::thread::spawn(move || {
            std::thread::sleep(UNREACHABLE_PAUSE / 3);
            let socket = UdpSocket::bind(addr).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut buf = [0u8; 1400];
            let (_, from) = socket.recv_from(&mut buf).unwrap();
            socket.send_to(&info_packet(), from).unwrap();
        });
        let (info, _) = query_info(addr, Duration::from_millis(1500)).unwrap();
        assert_eq!(info.name, "My Server");
        server.join().unwrap();
    }
}
