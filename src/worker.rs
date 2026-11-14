//! Background work for the GUI: A2S queries on short-lived threads, one
//! long-lived RCON thread per server and one listener for server logs (UDP, and HTTP
//! on the same TCP port number for Counter-Strike 2).
//! Results come back as [`Event`]s over a channel; every send also wakes the UI.

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use eframe::egui;

use openhlsw::a2s::{self, Player, QueryMeta, Rule, ServerInfo};
use openhlsw::address::ServerAddress;
use openhlsw::gamespy2;
use openhlsw::query::{self, Protocol};
use openhlsw::rcon::{GoldSrcRcon, RconClient, RconError};
use openhlsw::serverlog::{self, LogListener, local_ip_towards, public_ip_lookup};
pub use openhlsw::serverlog::{MappedPort, PortMapper, PortMapping};

pub type ServerId = u64;

const QUERY_TIMEOUT: Duration = Duration::from_millis(1500);
const RCON_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const RCON_COMMAND_TIMEOUT: Duration = Duration::from_secs(15);

pub enum Event {
    Info {
        id: ServerId,
        result: Result<(Box<ServerInfo>, QueryMeta, SocketAddr), QueryFailure>,
        /// Protocol found by auto detection (servers added without a game).
        detected: Option<Protocol>,
    },
    Players {
        id: ServerId,
        result: Result<(Vec<Player>, QueryMeta), QueryFailure>,
    },
    Rules {
        id: ServerId,
        result: Result<(Vec<Rule>, QueryMeta), QueryFailure>,
    },
    Rcon {
        id: ServerId,
        event: RconEvent,
    },
    /// Result of a server location lookup.
    Geo(Result<Vec<(IpAddr, crate::geo::GeoInfo)>, String>),
    /// Tag of the latest openHLSW release (update check), `None` when GitHub gave none.
    LatestRelease(Option<String>),
    /// One line from a server log stream (`logaddress_add`, `logaddress_add_http`).
    Log {
        from: SocketAddr,
        /// The secret key from the log URL of the server it was given to (HTTP, see the
        /// app's `log_key`); `None`: find the server by `from` (UDP).
        server: Option<u64>,
        line: String,
    },
    /// Result of forwarding the log port in the router (UPnP), see [`PortMapper`].
    PortMap(Result<PortMapping, String>),
    /// A server was asked for its log but nothing arrived: the firewall found on this
    /// computer that may block `port`, if any.
    NoLog {
        id: ServerId,
        port: u16,
        /// The log was asked for over HTTP (TCP port), not UDP.
        http: bool,
        firewall: Option<crate::firewall::Firewall>,
    },
}

/// Why a server query failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryFailure {
    pub message: String,
    /// An ICMP error came back (port unreachable, administratively prohibited …): the
    /// server's address may be rejecting us for sending too much, see
    /// [`a2s::QueryError::Unreachable`].
    pub unreachable: bool,
}

impl From<String> for QueryFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            unreachable: false,
        }
    }
}

impl From<a2s::QueryError> for QueryFailure {
    fn from(e: a2s::QueryError) -> Self {
        Self {
            unreachable: matches!(e, a2s::QueryError::Unreachable(_)),
            message: e.to_string(),
        }
    }
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
        let addr = address
            .query_address()
            .map_err(|e| QueryFailure::from(format!("DNS: {e}")));
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
                        .map_err(QueryFailure::from)
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
                    .map_err(QueryFailure::from)
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
                    .map_err(QueryFailure::from)
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
                result: Err(format!("DNS: {e}").into()),
                detected: None,
            };
        }
    };
    let order = if address.query_port.is_some() {
        [Protocol::GameSpy2, Protocol::A2s, Protocol::GameSpy1]
    } else {
        [Protocol::A2s, Protocol::GameSpy2, Protocol::GameSpy1]
    };
    // Not `unreachable`: closed ports are expected while probing the protocols.
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
                Event::Info { result, .. } => result.map_err(|e| e.message),
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
        result: Err(last.into()),
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
    // Not `unreachable`: several ports may be tried, and closed ones answer with ICMP.
    let result = query::query(protocol, &target, OTHER_TIMEOUT)
        .map_err(|e| QueryFailure::from(e.to_string()));
    match kind {
        QueryKind::Info => Event::Info {
            id,
            detected: None,
            result: result.and_then(|(status, meta)| {
                let game_addr = (address.host.as_str(), address.port)
                    .to_socket_addrs()
                    .ok()
                    .and_then(|mut a| a.next())
                    .ok_or_else(|| {
                        QueryFailure::from(format!("DNS: {} not found", address.host))
                    })?;
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

/// Checks the local firewall for the log port after a server's log did not arrive.
pub fn spawn_no_log_check(notifier: &Notifier, id: ServerId, port: u16, http: bool) {
    let n = notifier.clone();
    thread::spawn(move || {
        let firewall = crate::firewall::check(port, if http { "tcp" } else { "udp" });
        n.send(Event::NoLog {
            id,
            port,
            http,
            firewall,
        });
    });
}

/// Looks up the location of up to 100 server IPs (ip-api.com batch request).
pub fn spawn_geo_lookup(notifier: &Notifier, ips: Vec<IpAddr>) {
    let n = notifier.clone();
    thread::spawn(move || n.send(Event::Geo(crate::geo::lookup_batch(&ips))));
}

/// Asks GitHub for the latest release of openHLSW (see [`crate::update`]).
pub fn spawn_update_check(notifier: &Notifier, homepage: &'static str) {
    let n = notifier.clone();
    thread::spawn(move || {
        let tag = crate::update::latest_release(homepage)
            .map_err(|e| {
                if cfg!(debug_assertions) {
                    eprintln!("update check: {e}");
                }
            })
            .ok();
        n.send(Event::LatestRelease(tag));
    });
}

/// The log listener ([`serverlog::spawn_listener`]); every line becomes an [`Event::Log`].
pub fn spawn_log_listener(
    notifier: &Notifier,
    port: u16,
    token: String,
) -> std::io::Result<LogListener> {
    let n = notifier.clone();
    serverlog::spawn_listener(port, token, move |from, server, line| {
        n.send(Event::Log { from, server, line });
    })
}

/// The port forwarding in the router; every result becomes an [`Event::PortMap`].
pub fn spawn_port_mapper(notifier: &Notifier) -> PortMapper {
    let n = notifier.clone();
    PortMapper::spawn(move |result| n.send(Event::PortMap(result)))
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
    /// Runs a command for the MCP server: the answer (or why it failed) goes to `reply`,
    /// command and answer to the console.
    ExecReply {
        command: String,
        reply: Sender<Result<String, String>>,
    },
    /// `logaddress_add <our ip>:<port>` + `log on`; with `http_path` (Counter-Strike 2)
    /// `logaddress_add_http "http://<our ip>:<port><http_path>"` instead.
    StartLog {
        internet: bool,
        port: u16,
        public_ip: Option<IpAddr>,
        http_path: Option<String>,
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
                    last_error: Default::default(),
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
    /// The last error sent to the console, for the answer to an MCP client.
    last_error: std::cell::Cell<Option<String>>,
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
        if let RconEvent::Error(msg) = &event {
            self.last_error.set(Some(msg.clone()));
        }
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
                RconRequest::ExecReply { command, reply } => {
                    self.last_error.take();
                    let answer = self.exec(&command);
                    if let Some(answer) = &answer {
                        self.emit(RconEvent::Sent {
                            command: command.clone(),
                            answer: answer.clone(),
                        });
                    }
                    let _ = reply.send(answer.ok_or_else(|| {
                        self.last_error
                            .take()
                            .unwrap_or_else(|| "RCON failed".to_string())
                    }));
                }
                RconRequest::StartLog {
                    internet,
                    port,
                    public_ip,
                    http_path,
                } => {
                    let result = self.start_log(internet, port, public_ip, http_path.as_deref());
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
        http_path: Option<&str>,
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
        let cmd = serverlog::logaddress_command(target, http_path, self.goldsrc);
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
