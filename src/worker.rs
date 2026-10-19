//! Background work for the GUI: A2S queries on short-lived threads, one
//! long-lived RCON thread per server and one UDP listener for server logs.
//! Results come back as [`Event`]s over a channel; every send also wakes the UI.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use eframe::egui;

use openhlsw::a2s::{self, Player, QueryMeta, Rule, ServerInfo};
use openhlsw::address::ServerAddress;
use openhlsw::gamespy2;
use openhlsw::rcon::{GoldSrcRcon, RconClient, RconError};

pub type ServerId = u64;

const QUERY_TIMEOUT: Duration = Duration::from_millis(1500);
const RCON_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const RCON_COMMAND_TIMEOUT: Duration = Duration::from_secs(15);

pub enum Event {
    Info {
        id: ServerId,
        result: Result<(Box<ServerInfo>, QueryMeta, SocketAddr), String>,
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
}

/// Who asked for an RCON command, so the answer lands in the right tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RconTag {
    Console,
    /// Output is dropped (say, banid, writeid, ...).
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

/// Runs one A2S query on a short-lived thread. `challenge` is the last challenge the
/// server accepted; reusing it saves a round trip (srcds rate-limits queries per IP).
pub fn spawn_query(
    notifier: &Notifier,
    id: ServerId,
    address: ServerAddress,
    kind: QueryKind,
    challenge: Option<[u8; 4]>,
) {
    let n = notifier.clone();
    thread::spawn(move || {
        let addr = address.query_address().map_err(|e| format!("DNS: {e}"));
        // ip:port:queryport servers speak GameSpy 2, everything else Valve A2S.
        let gamespy = address.query_port.is_some();
        let event = match kind {
            QueryKind::Info => Event::Info {
                id,
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
            let msg = "RCON is not supported for GameSpy servers";
            self.emit(RconEvent::Status(RconStatus::Error(msg.into())));
            self.emit(RconEvent::Error(msg.into()));
            return false;
        }
        if password.is_empty() {
            self.emit(RconEvent::Status(RconStatus::Error("No password".into())));
            self.emit(RconEvent::Error(
                "No RCON password set (see the Rcon Config tab)".into(),
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
                self.emit(RconEvent::Status(RconStatus::BadPassword));
                self.emit(RconEvent::Error("Bad RCON password".into()));
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
        let (address, _) = self.target.clone().ok_or("No RCON password set")?;
        let server = address.resolve().map_err(|e| format!("DNS: {e}"))?;
        let ip = if internet {
            match public_ip {
                Some(ip) => ip,
                None => {
                    public_ip_lookup().map_err(|e| format!("Could not determine public IP: {e}"))?
                }
            }
        } else {
            local_ip_towards(server).map_err(|e| format!("Could not determine LAN IP: {e}"))?
        };
        let target = SocketAddr::new(ip, port);
        // HLDS wants "ip port", srcds "ip:port".
        let cmd = if self.goldsrc {
            format!("logaddress_add {ip} {port}")
        } else {
            format!("logaddress_add {target}")
        };
        self.exec(&cmd).ok_or("RCON failed")?;
        self.exec("log on").ok_or("RCON failed")?;
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
