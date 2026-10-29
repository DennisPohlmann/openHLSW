//! Background work for the GUI: A2S queries on short-lived threads, one
//! long-lived RCON thread per server and one UDP listener for server logs.
//! Results come back as [`Event`]s over a channel; every send also wakes the UI.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;

use openhlsw::a2s::{self, Player, QueryMeta, Rule, ServerInfo};
use openhlsw::address::ServerAddress;
use openhlsw::gamespy2;
use openhlsw::query::{self, Protocol};
use openhlsw::rcon::{GoldSrcRcon, RconClient, RconError};
use openhlsw::upnp;

pub type ServerId = u64;

const QUERY_TIMEOUT: Duration = Duration::from_millis(1500);
const RCON_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const RCON_COMMAND_TIMEOUT: Duration = Duration::from_secs(15);

pub enum Event {
    Info {
        id: ServerId,
        result: Result<(Box<ServerInfo>, QueryMeta, SocketAddr), String>,
        /// Protocol found by auto detection (servers added without a game).
        detected: Option<Protocol>,
    },
    Players {
        id: ServerId,
        result: Result<(Vec<Player>, QueryMeta), String>,
    },
    Rules {
        id: ServerId,
        result: Result<(Vec<Rule>, QueryMeta), String>,
    },
    Rcon {
        id: ServerId,
        event: RconEvent,
    },
    /// Result of a server location lookup.
    Geo(Result<Vec<(IpAddr, crate::geo::GeoInfo)>, String>),
    /// One line from a server log stream (`logaddress_add`).
    Log {
        from: SocketAddr,
        line: String,
    },
    /// Result of forwarding the log port in the router (UPnP), see [`PortMapper`].
    PortMap(Result<PortMapping, String>),
}

/// The log port forwarded in the router.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortMapping {
    pub port: u16,
    /// The router's Internet address, if it told us.
    pub external_ip: Option<IpAddr>,
}

/// Who asked for an RCON command, so the answer lands in the right tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RconTag {
    Console,
    /// No own handling: the answer is only shown in the console (say, banid, writeid, ...).
    Silent,
    BanIds,
    BanIps,
    Maps,
    /// `listmaps` (map cycle), merged into the map list.
    MapCycle,
    /// `status` before "Kick all on map change".
    KickAllStatus,
    /// Periodic `status` for Steam IDs, IPs, ping and loss of the players.
    PlayerStatus,
}

pub enum RconEvent {
    Status(RconStatus),
    Output {
        tag: RconTag,
        text: String,
    },
    Error(String),
    /// Result of a Get Log request: the address the server now logs to.
    LogStarted(Result<SocketAddr, String>),
    /// A command the worker sent on its own (Get Log) and the server's answer, for the console.
    Sent {
        command: String,
        answer: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RconStatus {
    Unknown,
    Connecting,
    Ok,
    BadPassword,
    Error(String),
}

/// Sends events to the UI thread and requests a repaint.
#[derive(Clone)]
pub struct Notifier {
    tx: Sender<Event>,
    ctx: egui::Context,
}

impl Notifier {
    pub fn new(ctx: egui::Context) -> (Self, Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        (Self { tx, ctx }, rx)
    }

    fn send(&self, event: Event) {
        let _ = self.tx.send(event);
        self.ctx.request_repaint();
    }
}

// --------------------------------------------------------------------------
// A2S queries
// --------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub enum QueryKind {
    Info,
    Players,
    Rules,
}

/// The server a query goes to and how it is asked.
pub struct QueryTarget {
    pub id: ServerId,
    pub address: ServerAddress,
    pub protocol: Protocol,
    /// HLSW key of the chosen game ("" = none); picks the usual query port.
    pub game: &'static str,
    /// No game chosen and the protocol not known yet: Valve A2S, GameSpy 2 and GameSpy 1
    /// are tried in turn.
    pub detect: bool,
}

/// Time for the other protocols: they may try several query ports.
const OTHER_TIMEOUT: Duration = Duration::from_millis(2500);

/// Runs one query on a short-lived thread. `challenge` is the last A2S challenge the
/// server accepted; reusing it saves a round trip (srcds rate-limits queries per IP).
pub fn spawn_query(
    notifier: &Notifier,
    target: QueryTarget,
    kind: QueryKind,
    challenge: Option<[u8; 4]>,
) {
    let n = notifier.clone();
    let QueryTarget {
        id,
        address,
        protocol,
        game,
        detect,
    } = target;
    thread::spawn(move || {
        if detect && matches!(kind, QueryKind::Info) {
            n.send(detect_protocol(id, &address, challenge));
            return;
        }
        if !matches!(protocol, Protocol::A2s | Protocol::GameSpy2) {
            n.send(other_protocol(id, &address, protocol, game, kind));
            return;
        }
        let addr = address.query_address().map_err(|e| format!("DNS: {e}"));
        // ip:port:queryport servers speak GameSpy 2, everything else Valve A2S.
        let gamespy = protocol == Protocol::GameSpy2;
        let event = match kind {
            QueryKind::Info => Event::Info {
                id,
                detected: None,
                result: addr.and_then(|a| {
                    let r = if gamespy {
                        gamespy2::query_info(a, QUERY_TIMEOUT)
                    } else {
                        a2s::query_info_with(a, QUERY_TIMEOUT, challenge)
                    };
                    r.map(|(info, meta)| (Box::new(info), meta, a))
                        .map_err(|e| e.to_string())
                }),
            },
            QueryKind::Players => Event::Players {
                id,
                result: addr.and_then(|a| {
                    if gamespy {
                        gamespy2::query_players(a, QUERY_TIMEOUT)
                    } else {
                        a2s::query_players_with(a, QUERY_TIMEOUT, challenge)
                    }
                    .map_err(|e| e.to_string())
                }),
            },
            QueryKind::Rules => Event::Rules {
                id,
                result: addr.and_then(|a| {
                    if gamespy {
                        gamespy2::query_rules(a, QUERY_TIMEOUT)
                    } else {
                        a2s::query_rules_with(a, QUERY_TIMEOUT, challenge)
                    }
                    .map_err(|e| e.to_string())
                }),
            },
        };
        n.send(event);
    });
}

/// Auto detection for servers added without a game: Valve A2S, GameSpy 2 (on the query
/// port, or the game port) and GameSpy 1 (game port + 1, + 10, the game port). With a query
/// port in the address GameSpy 2 goes first.
fn detect_protocol(id: ServerId, address: &ServerAddress, challenge: Option<[u8; 4]>) -> Event {
    let addr = match address.query_address() {
        Ok(a) => a,
        Err(e) => {
            return Event::Info {
                id,
                result: Err(format!("DNS: {e}")),
                detected: None,
            };
        }
    };
    let order = if address.query_port.is_some() {
        [Protocol::GameSpy2, Protocol::A2s, Protocol::GameSpy1]
    } else {
        [Protocol::A2s, Protocol::GameSpy2, Protocol::GameSpy1]
    };
    let mut last = String::from("Timeout");
    for protocol in order {
        let result = match protocol {
            Protocol::A2s => a2s::query_info_with(addr, QUERY_TIMEOUT, challenge)
                .map(|(info, meta)| (Box::new(info), meta, addr))
                .map_err(|e| e.to_string()),
            Protocol::GameSpy2 => gamespy2::query_info(addr, QUERY_TIMEOUT)
                .map(|(info, meta)| (Box::new(info), meta, addr))
                .map_err(|e| e.to_string()),
            _ => match other_protocol(id, address, protocol, "", QueryKind::Info) {
                Event::Info { result, .. } => result,
                _ => continue,
            },
        };
        match result {
            Ok(found) => {
                return Event::Info {
                    id,
                    result: Ok(found),
                    detected: Some(protocol),
                };
            }
            Err(e) => {
                // The first protocol's error is the most telling (A2S timeouts, DNS …).
                if protocol == order[0] {
                    last = e;
                }
            }
        }
    }
    Event::Info {
        id,
        result: Err(last),
        detected: None,
    }
}

/// Quake 3, GameSpy 1, Unreal 2, Doom 3, TeamSpeak … (see `openhlsw::query`). One query
/// answers info, players and rules; the library keeps it briefly for the other two.
fn other_protocol(
    id: ServerId,
    address: &ServerAddress,
    protocol: Protocol,
    game: &str,
    kind: QueryKind,
) -> Event {
    let target = query::Target {
        host: address.host.clone(),
        port: address.port,
        query_port: address.query_port,
        game: game.to_string(),
    };
    let result = query::query(protocol, &target, OTHER_TIMEOUT).map_err(|e| e.to_string());
    match kind {
        QueryKind::Info => Event::Info {
            id,
            detected: None,
            result: result.and_then(|(status, meta)| {
                let game_addr = (address.host.as_str(), address.port)
                    .to_socket_addrs()
                    .ok()
                    .and_then(|mut a| a.next())
                    .ok_or_else(|| format!("DNS: {} not found", address.host))?;
                Ok((Box::new(status.info), meta, game_addr))
            }),
        },
        QueryKind::Players => Event::Players {
            id,
            result: result.map(|(status, meta)| (status.players, meta)),
        },
        QueryKind::Rules => Event::Rules {
            id,
            result: result.map(|(status, meta)| (status.rules, meta)),
        },
    }
}

/// Looks up the location of up to 100 server IPs (ip-api.com batch request).
pub fn spawn_geo_lookup(notifier: &Notifier, ips: Vec<IpAddr>) {
    let n = notifier.clone();
    thread::spawn(move || n.send(Event::Geo(crate::geo::lookup_batch(&ips))));
}

// --------------------------------------------------------------------------
// Server log receiver
// --------------------------------------------------------------------------

/// Binds the UDP log port and forwards every received log line.
pub fn spawn_log_listener(notifier: &Notifier, port: u16) -> std::io::Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", port))?;
    let n = notifier.clone();
    thread::Builder::new()
        .name("log-listener".into())
        .spawn(move || {
            let mut buf = vec![0u8; 65536];
            loop {
                let Ok((len, from)) = socket.recv_from(&mut buf) else {
                    continue;
                };
                openhlsw::stats::add_received(len + 28);
                if let Some(line) = parse_log_packet(&buf[..len]) {
                    n.send(Event::Log { from, line });
                }
            }
        })?;
    Ok(())
}

// --------------------------------------------------------------------------
// Port forwarding in the router (UPnP)
// --------------------------------------------------------------------------

/// Lease asked for; the mapping is renewed after half of it (and after a failure).
const MAPPING_LEASE: u32 = 3600;
const MAPPING_RENEW: Duration = Duration::from_secs(MAPPING_LEASE as u64 / 2);
const MAPPING_RETRY: Duration = Duration::from_secs(600);

/// Thread that keeps the UDP log port forwarded in the router while it is wanted and
/// removes the forwarding when it is no longer wanted or the program ends.
pub struct PortMapper {
    tx: Option<Sender<Option<u16>>>,
    done: Receiver<()>,
}

impl PortMapper {
    pub fn spawn(notifier: &Notifier) -> Self {
        let (tx, rx) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        let n = notifier.clone();
        thread::Builder::new()
            .name("port-mapper".into())
            .spawn(move || {
                port_mapper(&n, &rx);
                let _ = done_tx.send(());
            })
            .expect("spawn port mapper thread");
        Self { tx: Some(tx), done }
    }

    /// Forward `port` (`None`: remove the forwarding).
    pub fn set(&self, port: Option<u16>) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(port);
        }
    }

    /// Removes the forwarding and waits up to `timeout` for the router's answer.
    pub fn shutdown(&mut self, timeout: Duration) {
        self.tx = None;
        let _ = self.done.recv_timeout(timeout);
    }
}

fn port_mapper(n: &Notifier, rx: &Receiver<Option<u16>>) {
    let mut wanted: Option<u16> = None;
    let mut mapped: Option<(upnp::Gateway, u16)> = None;
    let mut next_try: Option<Instant> = None;
    loop {
        let request = match next_try {
            Some(at) => rx.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        };
        match request {
            Ok(port) => {
                if port == wanted {
                    continue;
                }
                wanted = port;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {} // renew or retry
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Some((gateway, port)) = mapped.take() {
                    let _ = gateway.delete_port_mapping("UDP", port);
                }
                return;
            }
        }
        if let Some((gateway, port)) = mapped.take_if(|(_, port)| Some(*port) != wanted) {
            let _ = gateway.delete_port_mapping("UDP", port);
        }
        next_try = None;
        let Some(port) = wanted else { continue };
        let gateway = match mapped.take() {
            Some((gateway, _)) => Ok(gateway),
            None => upnp::discover(Duration::from_secs(2)),
        };
        let result = gateway.and_then(|gateway| {
            let local = gateway.local_ip()?;
            gateway.add_port_mapping(
                "UDP",
                port,
                SocketAddr::new(local, port),
                MAPPING_LEASE,
                "openHLSW server log",
            )?;
            Ok(gateway)
        });
        match result {
            Ok(gateway) => {
                let external_ip = gateway.external_ip().ok();
                // Permanent mappings (lease 0) are refreshed too, e.g. after a router restart.
                next_try = Some(Instant::now() + MAPPING_RENEW);
                mapped = Some((gateway, port));
                n.send(Event::PortMap(Ok(PortMapping { port, external_ip })));
            }
            Err(e) => {
                next_try = Some(Instant::now() + MAPPING_RETRY);
                n.send(Event::PortMap(Err(e.to_string())));
            }
        }
    }
}

/// srcds log packets: `FF FF FF FF 'R' "L 10/18/2026 - 19:22:33: text\n\0"`
/// ('S' + secret instead of 'R' when sv_logsecret is set).
fn parse_log_packet(data: &[u8]) -> Option<String> {
    let rest = data.strip_prefix(&[0xFF, 0xFF, 0xFF, 0xFF])?;
    let text = String::from_utf8_lossy(rest);
    let start = text.find("L ")?;
    let line = text[start + 2..]
        .trim_end_matches(['\0', '\n', '\r'])
        .to_string();
    (!line.is_empty()).then_some(line)
}

// --------------------------------------------------------------------------
// RCON
// --------------------------------------------------------------------------

pub enum RconRequest {
    /// Sets address + password; drops the connection if either changed.
    Configure {
        address: ServerAddress,
        password: String,
        /// Half-Life 1 server: UDP RCON instead of TCP.
        goldsrc: bool,
    },
    /// Reconnects and authenticates.
    Test,
    Exec {
        command: String,
        tag: RconTag,
    },
    /// `logaddress_add <our ip>:<port>` + `log on`.
    StartLog {
        internet: bool,
        port: u16,
        public_ip: Option<IpAddr>,
    },
    Disconnect,
}

/// Handle to the RCON thread of one server. The thread exits when this is dropped.
pub struct RconHandle {
    tx: Sender<RconRequest>,
}

impl RconHandle {
    pub fn spawn(notifier: &Notifier, id: ServerId) -> Self {
        let (tx, rx) = mpsc::channel();
        let n = notifier.clone();
        thread::Builder::new()
            .name(format!("rcon-{id}"))
            .spawn(move || {
                RconWorker {
                    n,
                    id,
                    target: None,
                    goldsrc: false,
                    conn: None,
                    rejected: false,
                }
                .run(rx)
            })
            .expect("spawn rcon thread");
        Self { tx }
    }

    pub fn send(&self, request: RconRequest) {
        let _ = self.tx.send(request);
    }

    pub fn exec(&self, command: impl Into<String>, tag: RconTag) {
        self.send(RconRequest::Exec {
            command: command.into(),
            tag,
        });
    }
}

struct RconWorker {
    n: Notifier,
    id: ServerId,
    target: Option<(ServerAddress, String)>,
    goldsrc: bool,
    conn: Option<Conn>,
    /// The server refused the password: no more automatic logins with it (servers ban
    /// addresses after a few failed RCON logins) until Test is clicked or the password changes.
    rejected: bool,
}

enum Conn {
    Source(RconClient),
    GoldSrc(GoldSrcRcon),
}

impl Conn {
    fn exec(&mut self, command: &str) -> Result<String, RconError> {
        match self {
            Conn::Source(c) => c.exec(command),
            Conn::GoldSrc(c) => c.exec(command),
        }
    }
}

impl RconWorker {
    fn emit(&self, event: RconEvent) {
        self.n.send(Event::Rcon { id: self.id, event });
    }

    fn run(mut self, rx: Receiver<RconRequest>) {
        while let Ok(request) = rx.recv() {
            match request {
                RconRequest::Configure {
                    address,
                    password,
                    goldsrc,
                } => {
                    let new = Some((address, password));
                    if new != self.target || goldsrc != self.goldsrc {
                        if new != self.target {
                            self.rejected = false;
                        }
                        self.conn = None;
                        self.target = new;
                        self.goldsrc = goldsrc;
                    }
                }
                RconRequest::Disconnect => {
                    self.conn = None;
                    self.emit(RconEvent::Status(RconStatus::Unknown));
                }
                RconRequest::Test => {
                    self.conn = None; // close the old connection before logging in again
                    self.rejected = false;
                    self.connect();
                }
                RconRequest::Exec { command, tag } => {
                    if let Some(text) = self.exec(&command) {
                        self.emit(RconEvent::Output { tag, text });
                    }
                }
                RconRequest::StartLog {
                    internet,
                    port,
                    public_ip,
                } => {
                    let result = self.start_log(internet, port, public_ip);
                    self.emit(RconEvent::LogStarted(result));
                }
            }
        }
    }

    fn connect(&mut self) -> bool {
        let Some((address, password)) = self.target.clone() else {
            return false;
        };
        if address.query_port.is_some() {
            let msg = tr!("RCON is not supported for GameSpy servers");
            self.emit(RconEvent::Status(RconStatus::Error(msg.into())));
            self.emit(RconEvent::Error(msg.into()));
            return false;
        }
        if password.is_empty() {
            self.emit(RconEvent::Status(RconStatus::Error(
                tr!("No password").into(),
            )));
            self.emit(RconEvent::Error(
                tr!("No RCON password set (see the Rcon Config tab)").into(),
            ));
            return false;
        }
        self.emit(RconEvent::Status(RconStatus::Connecting));
        let goldsrc = self.goldsrc;
        let result = address
            .resolve()
            .map_err(|e| RconError::Protocol(format!("DNS: {e}")))
            .and_then(|addr| {
                if goldsrc {
                    GoldSrcRcon::connect(addr, &password, RCON_CONNECT_TIMEOUT).map(|mut c| {
                        c.set_timeout(RCON_COMMAND_TIMEOUT);
                        Conn::GoldSrc(c)
                    })
                } else {
                    match RconClient::connect(addr, &password, RCON_CONNECT_TIMEOUT) {
                        Ok(c) => {
                            let _ = c.set_timeout(RCON_COMMAND_TIMEOUT);
                            Ok(Conn::Source(c))
                        }
                        // No TCP RCON port: probably HLDS before its server info arrived.
                        Err(RconError::Io(e))
                            if e.kind() == std::io::ErrorKind::ConnectionRefused =>
                        {
                            GoldSrcRcon::connect(addr, &password, RCON_CONNECT_TIMEOUT)
                                .map(|mut c| {
                                    c.set_timeout(RCON_COMMAND_TIMEOUT);
                                    Conn::GoldSrc(c)
                                })
                                .map_err(|_| RconError::Io(e))
                        }
                        Err(e) => Err(e),
                    }
                }
            });
        match result {
            Ok(client) => {
                self.goldsrc = matches!(client, Conn::GoldSrc(_));
                self.conn = Some(client);
                self.emit(RconEvent::Status(RconStatus::Ok));
                true
            }
            Err(RconError::BadPassword) => {
                self.rejected = true;
                self.emit(RconEvent::Status(RconStatus::BadPassword));
                self.emit(RconEvent::Error(tr!("Bad RCON password").into()));
                false
            }
            Err(e) => {
                self.emit(RconEvent::Status(RconStatus::Error(e.to_string())));
                self.emit(RconEvent::Error(e.to_string()));
                false
            }
        }
    }

    /// Runs a command, reconnecting once if the connection was dropped.
    fn exec(&mut self, command: &str) -> Option<String> {
        if self.conn.is_none() && self.rejected {
            // Not again with a password the server refused (it would ban us).
            self.emit(RconEvent::Error(
                tr!("Bad RCON password: enter the right one and click Test").into(),
            ));
            return None;
        }
        if self.conn.is_none() && !self.connect() {
            return None;
        }
        match self.conn.as_mut()?.exec(command) {
            Ok(output) => Some(output),
            Err(e) if e.is_connection_lost() => {
                // srcds drops idle connections on map change etc.; reconnect once.
                if !self.connect() {
                    return None;
                }
                match self.conn.as_mut()?.exec(command) {
                    Ok(output) => Some(output),
                    Err(e) => {
                        self.conn = None;
                        self.emit(RconEvent::Error(e.to_string()));
                        None
                    }
                }
            }
            Err(e) => {
                self.conn = None;
                self.emit(RconEvent::Status(RconStatus::Error(e.to_string())));
                self.emit(RconEvent::Error(e.to_string()));
                None
            }
        }
    }

    fn start_log(
        &mut self,
        internet: bool,
        port: u16,
        public_ip: Option<IpAddr>,
    ) -> Result<SocketAddr, String> {
        let (address, _) = self.target.clone().ok_or(tr!("No RCON password set"))?;
        let server = address.resolve().map_err(|e| format!("DNS: {e}"))?;
        let ip = if internet {
            match public_ip {
                Some(ip) => ip,
                None => public_ip_lookup()
                    .map_err(|e| tr!("Could not determine public IP: {e}", e = e))?,
            }
        } else {
            local_ip_towards(server).map_err(|e| tr!("Could not determine LAN IP: {e}", e = e))?
        };
        let target = SocketAddr::new(ip, port);
        // HLDS wants "ip port", srcds "ip:port".
        let cmd = if self.goldsrc {
            format!("logaddress_add {ip} {port}")
        } else {
            format!("logaddress_add {target}")
        };
        for command in [cmd.as_str(), "log on"] {
            let answer = self.exec(command).ok_or(tr!("RCON failed"))?;
            self.emit(RconEvent::Sent {
                command: command.to_string(),
                answer,
            });
        }
        Ok(target)
    }
}

/// The local address the OS would use to reach `server` (no packet is sent).
fn local_ip_towards(server: SocketAddr) -> std::io::Result<IpAddr> {
    let socket = UdpSocket::bind(if server.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    })?;
    socket.connect(server)?;
    Ok(socket.local_addr()?.ip())
}

/// Asks api.ipify.org (plain HTTP) for our public IPv4 address.
fn public_ip_lookup() -> std::io::Result<IpAddr> {
    use std::net::ToSocketAddrs;
    let addr = ("api.ipify.org", 80)
        .to_socket_addrs()?
        .find(|a| a.is_ipv4())
        .ok_or_else(|| std::io::Error::other("no address"))?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(b"GET / HTTP/1.0\r\nHost: api.ipify.org\r\nConnection: close\r\n\r\n")?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let body = response.split("\r\n\r\n").nth(1).unwrap_or_default().trim();
    body.parse()
        .map_err(|_| std::io::Error::other(format!("unexpected answer {body:?}")))
}

#[cfg(test)]
mod tests {
    use super::parse_log_packet;

    #[test]
    fn parses_goldsrc_log_packets() {
        let pkt =
            b"\xFF\xFF\xFF\xFFlog L 10/18/2026 - 21:00:00: World triggered \"Round_Start\"\n\0";
        assert_eq!(
            parse_log_packet(pkt).as_deref(),
            Some("10/18/2026 - 21:00:00: World triggered \"Round_Start\"")
        );
    }

    #[test]
    fn parses_log_packets() {
        let pkt =
            b"\xFF\xFF\xFF\xFFRL 10/18/2026 - 19:22:33: \"Bob<2><STEAM_1:0:1><CT>\" say \"hi\"\n\0";
        assert_eq!(
            parse_log_packet(pkt).as_deref(),
            Some("10/18/2026 - 19:22:33: \"Bob<2><STEAM_1:0:1><CT>\" say \"hi\"")
        );
        assert!(parse_log_packet(b"garbage").is_none());
    }
}
