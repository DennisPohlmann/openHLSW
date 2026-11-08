//! Server logs: the UDP listener for `logaddress_add` (and the HTTP log of Counter-Strike 2
//! on the same TCP port number), the port forwarding in the router (UPnP), the
//! `logaddress_add` command and parsing of log lines. Shared by the GUI and openhlsw-tui.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crate::upnp;

/// The log port forwarded in the router.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortMapping {
    pub port: u16,
    /// The TCP port (HTTP log, Counter-Strike 2) was asked for and is forwarded too.
    pub tcp: Option<bool>,
    /// The router's Internet address, if it told us.
    pub external_ip: Option<IpAddr>,
}

// --------------------------------------------------------------------------
// Server log receiver
// --------------------------------------------------------------------------

/// The running log listener.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogListener {
    /// UDP port of the log, and TCP port of the HTTP log (the same number).
    pub port: u16,
    /// Whether the HTTP log (Counter-Strike 2) runs on TCP `port`, or why not.
    pub http: Result<(), String>,
}

/// Binds the UDP log port (0: a free port chosen by the OS) and the same TCP port for the
/// HTTP log of Counter-Strike 2 ([`openhlsw::httplog`], requests must carry `token`), and
/// forwards every received log line. A random port is one that is free for both.
/// `on_line` gets the sender, the server key from the HTTP log URL (`None` for UDP)
/// and the line.
pub fn spawn_listener<F>(port: u16, token: String, on_line: F) -> std::io::Result<LogListener>
where
    F: Fn(SocketAddr, Option<u64>, String) + Send + Sync + 'static,
{
    let on_line = Arc::new(on_line);
    let (socket, tcp) = if port != 0 {
        let socket = UdpSocket::bind(("0.0.0.0", port))?;
        (socket, TcpListener::bind(("0.0.0.0", port)))
    } else {
        let mut tries = 0;
        loop {
            let socket = UdpSocket::bind(("0.0.0.0", 0))?;
            let tcp = TcpListener::bind(("0.0.0.0", socket.local_addr()?.port()));
            tries += 1;
            if tcp.is_ok() || tries >= 20 {
                break (socket, tcp);
            }
        }
    };
    let port = socket.local_addr()?.port();
    let http = tcp.and_then(|listener| {
        let n = on_line.clone();
        crate::httplog::serve(listener, token, move |id, from, line| {
            n(from, Some(id), line)
        })
    });
    thread::Builder::new()
        .name("log-listener".into())
        .spawn(move || {
            let mut buf = vec![0u8; 65536];
            loop {
                let Ok((len, from)) = socket.recv_from(&mut buf) else {
                    continue;
                };
                crate::stats::add_received(len + 28);
                if let Some(line) = parse_log_packet(&buf[..len]) {
                    on_line(from, None, line);
                }
            }
        })?;
    Ok(LogListener {
        port,
        http: http.map_err(|e| e.to_string()),
    })
}

// --------------------------------------------------------------------------
// Port forwarding in the router (UPnP)
// --------------------------------------------------------------------------

/// Lease asked for; the mapping is renewed after half of it (and after a failure).
const MAPPING_LEASE: u32 = 3600;
const MAPPING_RENEW: Duration = Duration::from_secs(MAPPING_LEASE as u64 / 2);
const MAPPING_RETRY: Duration = Duration::from_secs(120);

/// Thread that keeps the UDP log port (and the TCP port of the HTTP log) forwarded in
/// the router while it is wanted and removes the forwarding when it is no longer wanted
/// or the program ends.
pub struct PortMapper {
    tx: Option<Sender<Option<MappedPort>>>,
    done: Receiver<()>,
}

impl PortMapper {
    /// `on_result` gets the result of every (re)try.
    pub fn spawn(on_result: impl Fn(Result<PortMapping, String>) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        thread::Builder::new()
            .name("port-mapper".into())
            .spawn(move || {
                port_mapper(&on_result, &rx);
                let _ = done_tx.send(());
            })
            .expect("spawn port mapper thread");
        Self { tx: Some(tx), done }
    }

    /// Forward `port` (`None`: remove the forwarding).
    pub fn set(&self, port: Option<MappedPort>) {
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

/// The log port to forward, and whether its TCP port (HTTP log) is wanted too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MappedPort {
    pub port: u16,
    pub tcp: bool,
}

impl MappedPort {
    fn delete(self, gateway: &upnp::Gateway) {
        let _ = gateway.delete_port_mapping("UDP", self.port);
        if self.tcp {
            let _ = gateway.delete_port_mapping("TCP", self.port);
        }
    }
}

fn port_mapper(n: &dyn Fn(Result<PortMapping, String>), rx: &Receiver<Option<MappedPort>>) {
    let mut wanted: Option<MappedPort> = None;
    let mut mapped: Option<(upnp::Gateway, MappedPort)> = None;
    let mut next_try: Option<Instant> = None;
    loop {
        let request = match next_try {
            Some(at) => rx.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        };
        match request {
            Ok(port) => {
                // The same port again after a failure (Get Log pressed) retries right away.
                if port == wanted && (mapped.is_some() || port.is_none()) {
                    continue;
                }
                wanted = port;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {} // renew or retry
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Some((gateway, port)) = mapped.take() {
                    port.delete(&gateway);
                }
                return;
            }
        }
        if let Some((gateway, port)) = mapped.take_if(|(_, port)| Some(*port) != wanted) {
            port.delete(&gateway);
        }
        next_try = None;
        let Some(want) = wanted else { continue };
        let port = want.port;
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
            // The UDP log works without it: a refused TCP forwarding is only reported.
            let tcp = want.tcp.then(|| {
                gateway
                    .add_port_mapping(
                        "TCP",
                        port,
                        SocketAddr::new(local, port),
                        MAPPING_LEASE,
                        "openHLSW server log (HTTP)",
                    )
                    .is_ok()
            });
            Ok((gateway, tcp))
        });
        match result {
            Ok((gateway, tcp)) => {
                let external_ip = gateway.external_ip().ok();
                // Permanent mappings (lease 0) are refreshed too, e.g. after a router restart.
                next_try = Some(Instant::now() + MAPPING_RENEW);
                mapped = Some((gateway, want));
                n(Ok(PortMapping {
                    port,
                    tcp,
                    external_ip,
                }));
            }
            Err(e) => {
                next_try = Some(Instant::now() + MAPPING_RETRY);
                n(Err(e.to_string()));
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

/// The RCON command that makes a server send its log to `target`: HLDS wants "ip port",
/// srcds "ip:port", Counter-Strike 2 an URL (`http_path`, HTTP only).
pub fn logaddress_command(target: SocketAddr, http_path: Option<&str>, goldsrc: bool) -> String {
    if let Some(path) = http_path {
        format!("logaddress_add_http \"http://{target}{path}\"")
    } else if goldsrc {
        format!("logaddress_add {} {}", target.ip(), target.port())
    } else {
        format!("logaddress_add {target}")
    }
}

/// Whether `ip` is reachable from the Internet (not private, loopback, CGNAT …).
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64) // CGNAT 100.64/10
        }
        IpAddr::V6(v6) => {
            !(v6.is_loopback()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80)
        }
    }
}

/// The local address the OS would use to reach `server` (no packet is sent).
pub fn local_ip_towards(server: SocketAddr) -> std::io::Result<IpAddr> {
    let socket = UdpSocket::bind(if server.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    })?;
    socket.connect(server)?;
    Ok(socket.local_addr()?.ip())
}

/// Asks api.ipify.org (plain HTTP) for our public IPv4 address.
pub fn public_ip_lookup() -> std::io::Result<IpAddr> {
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

/// A chat message from the server log.
#[derive(Clone)]
pub struct ChatLine {
    /// From the server log's own stamp (when it has one), else when it arrived.
    pub time: chrono::NaiveDateTime,
    pub name: String,
    pub team: String,
    pub team_only: bool,
    pub text: String,
    /// Sent from openHLSW (Message to all players) and not yet echoed by the server log.
    pub own: bool,
}

/// Splits a server log line into its date/time prefix and the rest:
/// "[L ]MM/DD/YYYY - HH:MM:SS: rest". None if the line has no valid prefix.
pub fn parse_log_stamp(line: &str) -> Option<(chrono::NaiveDateTime, &str)> {
    let line = line.strip_prefix("L ").unwrap_or(line);
    // The prefix has a fixed width: "10/29/2026 - 09:19:46" is 21 bytes, then ": ".
    let stamp = line.get(..21)?;
    let rest = line.get(21..)?.strip_prefix(": ")?;
    let time = chrono::NaiveDateTime::parse_from_str(stamp, "%m/%d/%Y - %H:%M:%S").ok()?;
    Some((time, rest))
}

/// `10/18/2026 - 19:22:33: "Bob<2><STEAM_1:0:1><CT>" say "hi"` → chat line.
pub fn parse_chat_line(line: &str) -> Option<ChatLine> {
    let (time, rest) = match parse_log_stamp(line) {
        Some(stamped) => stamped,
        None => (chrono::Local::now().naive_local(), line.split_once(": ")?.1),
    };
    // HLDS logs `rcon say` as: Server say "text"
    if let Some(text) = rest.strip_prefix("Server say \"") {
        return Some(ChatLine {
            time,
            name: "Console".into(),
            team: String::new(),
            team_only: false,
            text: text.strip_suffix('"').unwrap_or(text).to_string(),
            own: false,
        });
    }
    let (player, team_only, text) = if let Some((p, t)) = rest.split_once("\" say_team \"") {
        (p, true, t)
    } else {
        let (p, t) = rest.split_once("\" say \"")?;
        (p, false, t)
    };
    let player = player.strip_prefix('"')?;
    // name<userid><steamid><team> — the name itself may contain '<', so split from the right.
    let mut parts = player.rsplitn(4, '<');
    let team = parts.next()?.trim_end_matches('>').to_string();
    parts.next()?;
    parts.next()?;
    let name = parts.next()?.to_string();
    let text = text.strip_suffix('"').unwrap_or(text).to_string();
    Some(ChatLine {
        time,
        name,
        team,
        team_only,
        text,
        own: false,
    })
}

/// HLDS logs every RCON command with its password: `Rcon: "rcon <challenge> "<password>"
/// <command>" from ...` (also `Bad Rcon: ...`). The password becomes `***` before the line
/// is shown or saved.
pub fn mask_rcon_password(line: String) -> String {
    let Some(at) = line.find("Rcon: \"rcon ") else {
        return line;
    };
    let rest = at + "Rcon: \"rcon ".len();
    let Some(open) = line[rest..].find(" \"").map(|i| rest + i + 2) else {
        return line;
    };
    let Some(close) = line[open..].find('"').map(|i| open + i) else {
        return line;
    };
    format!("{}***{}", &line[..open], &line[close..])
}

#[cfg(test)]
mod tests {
    use super::{logaddress_command, parse_log_packet};

    #[test]
    fn logaddress_command_per_engine() {
        let t = "198.51.100.1:27500".parse().unwrap();
        assert_eq!(
            logaddress_command(t, None, false),
            "logaddress_add 198.51.100.1:27500"
        );
        assert_eq!(
            logaddress_command(t, None, true),
            "logaddress_add 198.51.100.1 27500"
        );
        assert_eq!(
            logaddress_command(t, Some("/log/x/1"), false),
            "logaddress_add_http \"http://198.51.100.1:27500/log/x/1\""
        );
    }

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
