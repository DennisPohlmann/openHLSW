//! RCON clients: Source (TCP) and GoldSrc / Half-Life 1 (UDP).
//! <https://developer.valvesoftware.com/wiki/Source_RCON_Protocol>
//!
//! Multi-packet responses are collected with the "empty SERVERDATA_RESPONSE_VALUE"
//! trick: after every command an empty packet is sent, and srcds mirrors it once
//! all output of the command has been delivered.

use std::fmt;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

const SERVERDATA_AUTH: i32 = 3;
const SERVERDATA_AUTH_RESPONSE: i32 = 2;
const SERVERDATA_EXECCOMMAND: i32 = 2;
const SERVERDATA_RESPONSE_VALUE: i32 = 0;
const MAX_PACKET_SIZE: i32 = 1 << 20;
/// Most bytes one command may receive in all (cvarlist is a few 100 KB): the server is
/// untrusted and could otherwise stream packets for ever.
const MAX_OUTPUT: usize = 16 << 20;
/// Most packets skipped while waiting for the login answer.
const MAX_LOGIN_PACKETS: usize = 64;
/// How long the server may stay silent after sending output before we consider
/// the response complete (for servers that do not mirror the terminator packet).
const QUIET_AFTER_OUTPUT: std::time::Duration = std::time::Duration::from_millis(1500);

#[derive(Debug)]
pub enum RconError {
    Io(std::io::Error),
    Timeout,
    BadPassword,
    /// The server closed the connection (srcds does this after too many failed logins).
    Closed,
    Protocol(String),
    /// ICMP error on the UDP RCON socket (GoldSrc): closed port or a firewall rejecting us.
    Unreachable(std::io::Error),
}

impl RconError {
    /// True if the connection is gone and a reconnect may help.
    pub fn is_connection_lost(&self) -> bool {
        matches!(self, RconError::Closed)
            || matches!(self, RconError::Io(e) if matches!(e.kind(),
                ErrorKind::BrokenPipe | ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted | ErrorKind::NotConnected))
    }
}

impl fmt::Display for RconError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RconError::Io(e) => write!(f, "{e}"),
            RconError::Timeout => write!(f, "Timeout"),
            RconError::BadPassword => write!(f, "Bad RCON password"),
            RconError::Closed => write!(f, "Connection closed by server"),
            RconError::Protocol(m) => write!(f, "{m}"),
            RconError::Unreachable(e) => write!(f, "{}", crate::a2s::unreachable_text(e)),
        }
    }
}

impl std::error::Error for RconError {}

impl From<std::io::Error> for RconError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            ErrorKind::WouldBlock | ErrorKind::TimedOut => RconError::Timeout,
            ErrorKind::UnexpectedEof => RconError::Closed,
            _ => RconError::Io(e),
        }
    }
}

struct Packet {
    id: i32,
    kind: i32,
    body: Vec<u8>,
}

pub struct RconClient {
    stream: TcpStream,
    last_id: i32,
}

impl RconClient {
    /// Connects and authenticates.
    pub fn connect(addr: SocketAddr, password: &str, timeout: Duration) -> Result<Self, RconError> {
        let stream = TcpStream::connect_timeout(&addr, timeout)?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;

        let mut client = Self { stream, last_id: 0 };
        let auth_id = client.next_id();
        client.send(auth_id, SERVERDATA_AUTH, password.as_bytes())?;

        for _ in 0..MAX_LOGIN_PACKETS {
            // srcds sends an empty RESPONSE_VALUE before the AUTH_RESPONSE; skip it.
            let packet = client.read_packet().map_err(|e| match e {
                RconError::Closed => RconError::Protocol(
                    "Server closed the connection during login (IP banned?)".into(),
                ),
                other => other,
            })?;
            if packet.kind != SERVERDATA_AUTH_RESPONSE {
                continue;
            }
            if packet.id == -1 {
                return Err(RconError::BadPassword);
            }
            if packet.id == auth_id {
                return Ok(client);
            }
        }
        Err(RconError::Protocol("No login answer".into()))
    }

    /// Sets how long `exec` waits for output (long commands like `cvarlist` may need more).
    pub fn set_timeout(&self, timeout: Duration) -> Result<(), RconError> {
        self.stream.set_read_timeout(Some(timeout))?;
        Ok(())
    }

    /// Runs a command and returns its complete output.
    pub fn exec(&mut self, command: &str) -> Result<String, RconError> {
        let id = self.next_id();
        let terminator = self.next_id();
        self.send(id, SERVERDATA_EXECCOMMAND, command.as_bytes())?;
        self.send(terminator, SERVERDATA_RESPONSE_VALUE, b"")?;

        let mut output = Vec::new();
        let mut received = 0usize;
        let full_timeout = self.stream.read_timeout().ok().flatten();
        let result = loop {
            let packet = match self.read_packet() {
                Ok(p) => p,
                // Some servers (e.g. CS2) do not mirror the terminator packet: once output
                // has arrived and the server stays quiet, take what we have.
                Err(RconError::Timeout) if !output.is_empty() => break Ok(()),
                Err(e) => break Err(e),
            };
            received += 10 + packet.body.len();
            if received > MAX_OUTPUT {
                break Err(RconError::Protocol("Answer too large".into()));
            }
            if packet.kind != SERVERDATA_RESPONSE_VALUE {
                continue;
            }
            if packet.id == id {
                if output.is_empty() {
                    let _ = self.stream.set_read_timeout(Some(QUIET_AFTER_OUTPUT));
                }
                output.extend_from_slice(&packet.body);
            } else if packet.id == terminator {
                break Ok(());
            }
            // Anything else (e.g. the trailing 0x00000100 packet srcds sends after
            // a mirrored terminator of an earlier command) is ignored.
        };
        let _ = self.stream.set_read_timeout(full_timeout);
        result?;
        Ok(String::from_utf8_lossy(&output).into_owned())
    }

    fn next_id(&mut self) -> i32 {
        if self.last_id >= 0x7FFF_FFF0 {
            self.last_id = 0;
        }
        self.last_id += 1;
        self.last_id
    }

    fn send(&mut self, id: i32, kind: i32, body: &[u8]) -> Result<(), RconError> {
        // size | id | type | body | \0 | \0   (size excludes itself)
        let size = body.len() as i32 + 10;
        let mut packet = Vec::with_capacity(size as usize + 4);
        packet.extend_from_slice(&size.to_le_bytes());
        packet.extend_from_slice(&id.to_le_bytes());
        packet.extend_from_slice(&kind.to_le_bytes());
        packet.extend_from_slice(body);
        packet.extend_from_slice(&[0, 0]);
        self.stream.write_all(&packet)?;
        crate::stats::add_sent(packet.len());
        Ok(())
    }

    fn read_packet(&mut self) -> Result<Packet, RconError> {
        let mut size_buf = [0u8; 4];
        self.stream.read_exact(&mut size_buf)?;
        let size = i32::from_le_bytes(size_buf);
        if !(10..=MAX_PACKET_SIZE).contains(&size) {
            return Err(RconError::Protocol(format!("Invalid packet size {size}")));
        }
        let mut data = vec![0u8; size as usize];
        self.stream.read_exact(&mut data)?;
        crate::stats::add_received(4 + data.len());

        let id = i32::from_le_bytes(data[0..4].try_into().unwrap());
        let kind = i32::from_le_bytes(data[4..8].try_into().unwrap());
        let mut body = data[8..].to_vec();
        while body.last() == Some(&0) {
            body.pop();
        }
        Ok(Packet { id, kind, body })
    }
}

/// GoldSrc (Half-Life 1) RCON over UDP:
/// `challenge rcon` → `challenge rcon <n>`, then `rcon <n> "<password>" <command>`.
/// Answers are 'l' packets (sometimes split); there is no end marker, so a response is
/// complete once the server stays quiet for a moment.
pub struct GoldSrcRcon {
    socket: std::net::UdpSocket,
    password: String,
    challenge: String,
    timeout: Duration,
}

const GOLDSRC_QUIET: Duration = Duration::from_millis(300);
const GOLDSRC_NO_OUTPUT: Duration = Duration::from_millis(1200);

impl GoldSrcRcon {
    /// Gets a challenge and checks the password.
    pub fn connect(addr: SocketAddr, password: &str, timeout: Duration) -> Result<Self, RconError> {
        let socket = std::net::UdpSocket::bind(if addr.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        })?;
        socket.connect(addr)?;
        let mut client = Self {
            socket,
            password: password.to_string(),
            challenge: String::new(),
            timeout,
        };
        client.fetch_challenge()?;
        // A harmless command: HLDS answers "Bad rcon_password." if the password is wrong.
        client.exec("echo openHLSW")?;
        Ok(client)
    }

    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    fn send_raw(&self, body: &[u8]) -> Result<(), RconError> {
        let mut packet = vec![0xFF; 4];
        packet.extend_from_slice(body);
        self.socket.send(&packet).map_err(|e| {
            if crate::a2s::is_unreachable(e.kind()) {
                RconError::Unreachable(e)
            } else {
                e.into()
            }
        })?;
        crate::stats::add_sent(packet.len() + 28);
        Ok(())
    }

    fn recv(&self, buf: &mut [u8], wait: Duration) -> Result<Option<usize>, RconError> {
        self.socket
            .set_read_timeout(Some(wait.max(Duration::from_millis(1))))?;
        match self.socket.recv(buf) {
            Ok(n) => {
                crate::stats::add_received(n + 28);
                Ok(Some(n))
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => Ok(None),
            Err(e) if crate::a2s::is_unreachable(e.kind()) => Err(RconError::Unreachable(e)),
            Err(e) => Err(e.into()),
        }
    }

    fn fetch_challenge(&mut self) -> Result<(), RconError> {
        let mut buf = vec![0u8; 4096];
        for attempt in 0..2 {
            // An ICMP error (for this or an earlier packet) counts like a lost packet.
            let retry = |e: RconError| match e {
                RconError::Unreachable(_) if attempt == 0 => {
                    std::thread::sleep(crate::a2s::UNREACHABLE_PAUSE);
                    Ok(None)
                }
                e => Err(e),
            };
            if let Err(e) = self.send_raw(b"challenge rcon\n") {
                retry(e)?;
                continue;
            }
            let deadline = std::time::Instant::now() + self.timeout.min(Duration::from_secs(5));
            while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
                let Some(n) = self.recv(&mut buf, left).or_else(retry)? else {
                    break;
                };
                let text = String::from_utf8_lossy(&buf[..n]);
                if let Some(rest) = text.split("challenge rcon ").nth(1) {
                    let c: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                    if !c.is_empty() {
                        self.challenge = c;
                        return Ok(());
                    }
                }
            }
        }
        Err(RconError::Timeout)
    }

    /// Runs a command and returns its complete output.
    pub fn exec(&mut self, command: &str) -> Result<String, RconError> {
        for attempt in 0..2 {
            let out = match self.exec_once(command) {
                Ok(out) => out,
                // An ICMP error, often left over from an earlier packet (the server was
                // restarting, or a firewall rejected one packet): try once more.
                Err(RconError::Unreachable(_)) if attempt == 0 => {
                    std::thread::sleep(crate::a2s::UNREACHABLE_PAUSE);
                    continue;
                }
                Err(e) => return Err(e),
            };
            let trimmed = out.trim();
            if trimmed.starts_with("Bad rcon_password") {
                return Err(RconError::BadPassword);
            }
            if trimmed.starts_with("Bad challenge") && attempt == 0 {
                self.fetch_challenge()?; // the server restarted or changed the challenge
                continue;
            }
            if trimmed.starts_with("You have been banned") {
                return Err(RconError::Protocol(trimmed.to_string()));
            }
            return Ok(out);
        }
        Err(RconError::Protocol("Bad challenge".into()))
    }

    fn exec_once(&mut self, command: &str) -> Result<String, RconError> {
        let request = format!(
            "rcon {} \"{}\" {command}\n",
            self.challenge,
            self.password.replace('"', "")
        );
        self.send_raw(request.as_bytes())?;

        let mut buf = vec![0u8; 65536];
        let mut output = Vec::new();
        let mut splits: std::collections::HashMap<u32, Vec<(u8, u8, Vec<u8>)>> =
            std::collections::HashMap::new();
        let mut got_any = false;
        let mut received = 0usize;
        let started = std::time::Instant::now();
        let mut last = started;
        loop {
            let wait = if got_any {
                GOLDSRC_QUIET.saturating_sub(last.elapsed())
            } else {
                // Commands without output (say, kick, ...) send nothing at all.
                GOLDSRC_NO_OUTPUT
                    .min(self.timeout)
                    .saturating_sub(started.elapsed())
            };
            if wait.is_zero() {
                break;
            }
            let Some(n) = self.recv(&mut buf, wait)? else {
                break;
            };
            received += n;
            if received > MAX_OUTPUT {
                return Err(RconError::Protocol("Answer too large".into()));
            }
            let data = &buf[..n];
            let payload = if data.starts_with(&[0xFF; 4]) {
                Some(data[4..].to_vec())
            } else if data.len() > 9 && data.starts_with(&(-2i32).to_le_bytes()) {
                let id = u32::from_le_bytes(data[4..8].try_into().unwrap());
                let (num, total) = (data[8] >> 4, data[8] & 0x0F);
                if splits.len() >= 64 && !splits.contains_key(&id) {
                    splits.clear(); // incomplete answers that never finish
                }
                let parts = splits.entry(id).or_default();
                parts.push((num, total, data[9..].to_vec()));
                if total > 0 && parts.len() >= total as usize {
                    let mut parts = splits.remove(&id).unwrap_or_default();
                    parts.sort_by_key(|p| p.0);
                    let full: Vec<u8> = parts.into_iter().flat_map(|p| p.2).collect();
                    full.strip_prefix(&[0xFF; 4][..]).map(<[u8]>::to_vec)
                } else {
                    None
                }
            } else {
                None
            };
            got_any = true;
            last = std::time::Instant::now();
            if let Some(p) = payload
                && let Some(text) = p.strip_prefix(b"l")
            {
                output.extend(text.iter().copied().filter(|&b| b != 0));
            }
        }
        Ok(crate::a2s::decode_text(&output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::UdpSocket;

    fn source_packet(id: i32, kind: i32, body: &[u8]) -> Vec<u8> {
        let mut p = ((body.len() + 10) as i32).to_le_bytes().to_vec();
        p.extend(id.to_le_bytes());
        p.extend(kind.to_le_bytes());
        p.extend(body);
        p.extend([0, 0]);
        p
    }

    #[test]
    fn source_output_is_bounded() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut c, _) = listener.accept().unwrap();
            let _ = c.write_all(&source_packet(1, SERVERDATA_AUTH_RESPONSE, b""));
            // Output for the command (id 2) for ever, never the terminator.
            let chunk = source_packet(2, SERVERDATA_RESPONSE_VALUE, &[b'x'; 1 << 19]);
            while c.write_all(&chunk).is_ok() {}
        });
        let mut client = RconClient::connect(addr, "pw", Duration::from_secs(2)).unwrap();
        assert!(
            matches!(client.exec("cvarlist"), Err(RconError::Protocol(m)) if m == "Answer too large")
        );
    }

    #[test]
    fn source_login_does_not_wait_for_ever() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut c, _) = listener.accept().unwrap();
            let junk = source_packet(9, SERVERDATA_RESPONSE_VALUE, b"");
            while c.write_all(&junk).is_ok() {}
        });
        assert!(RconClient::connect(addr, "pw", Duration::from_secs(2)).is_err());
    }

    #[test]
    fn goldsrc_login_survives_one_icmp_error() {
        // Closed for the first packet (ICMP error), open when the client sends again.
        let addr = UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let server = std::thread::spawn(move || {
            std::thread::sleep(crate::a2s::UNREACHABLE_PAUSE / 3);
            let socket = UdpSocket::bind(addr).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut buf = [0u8; 1400];
            for _ in 0..2 {
                let (n, from) = socket.recv_from(&mut buf).unwrap();
                let text = String::from_utf8_lossy(&buf[4..n]).to_string();
                let answer: &[u8] = if text.starts_with("challenge rcon") {
                    b"\xFF\xFF\xFF\xFFchallenge rcon 12345\n\0"
                } else {
                    assert!(
                        text.starts_with("rcon 12345 \"RCON_PASSWORD\" echo"),
                        "{text}"
                    );
                    b"\xFF\xFF\xFF\xFFlopenHLSW\n\0"
                };
                socket.send_to(answer, from).unwrap();
            }
        });
        let client = GoldSrcRcon::connect(addr, "RCON_PASSWORD", Duration::from_secs(2));
        assert!(client.is_ok(), "{:?}", client.err());
        server.join().unwrap();
    }

    #[test]
    fn goldsrc_closed_port_is_unreachable() {
        let addr = UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let e = GoldSrcRcon::connect(addr, "RCON_PASSWORD", Duration::from_secs(2))
            .err()
            .unwrap();
        assert!(matches!(e, RconError::Unreachable(_)), "{e:?}");
        assert_eq!(e.to_string(), "Port unreachable");
    }
}
