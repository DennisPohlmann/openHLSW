use std::fmt;
use std::net::{SocketAddr, ToSocketAddrs};

pub const DEFAULT_PORT: u16 = 27015;

/// "host:port" as entered by the user. Accepts IPv4, hostnames, `[IPv6]:port`
/// and `steam://connect/...` links. The port defaults to 27015.
/// `host:port:queryport` is a GameSpy 2 server (e.g. Deer Hunter 2005) whose query
/// port differs from the game port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerAddress {
    pub host: String,
    pub port: u16,
    pub query_port: Option<u16>,
}

impl ServerAddress {
    pub fn parse(text: &str) -> Option<Self> {
        let mut s = text.trim();
        if s.len() >= 16 && s[..16].eq_ignore_ascii_case("steam://connect/") {
            s = &s[16..];
        }
        let s = s.trim_end_matches('/');
        if s.is_empty() || s.contains(char::is_whitespace) {
            return None;
        }

        // host:gameport:queryport (GameSpy 2)
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() == 3 && !parts[0].is_empty() && !s.starts_with('[') {
            let port = |p: &str| p.parse::<u16>().ok().filter(|&p| p != 0);
            if let (Some(port), Some(query)) = (port(parts[1]), port(parts[2])) {
                return Some(Self {
                    host: parts[0].to_string(),
                    port,
                    query_port: Some(query),
                });
            }
        }

        let (host, port) = if let Some(rest) = s.strip_prefix('[') {
            let end = rest.find(']')?;
            let host = &rest[..end];
            let tail = &rest[end + 1..];
            if tail.is_empty() {
                (host, None)
            } else {
                (host, Some(tail.strip_prefix(':')?))
            }
        } else if s.matches(':').count() == 1 {
            let (h, p) = s.split_once(':')?;
            (h, Some(p))
        } else {
            (s, None) // plain host or bare IPv6 without port
        };

        if host.is_empty() {
            return None;
        }
        let port = match port {
            Some(p) => match p.parse::<u16>() {
                Ok(0) | Err(_) => return None,
                Ok(p) => p,
            },
            None => DEFAULT_PORT,
        };
        Some(Self {
            host: host.to_string(),
            port,
            query_port: None,
        })
    }

    /// The address queries go to: the query port if there is one, else the game port.
    pub fn query_address(&self) -> std::io::Result<SocketAddr> {
        let mut addr = self.resolve()?;
        if let Some(q) = self.query_port {
            addr.set_port(q);
        }
        Ok(addr)
    }

    /// "host:port" without the query port (for connecting with the game).
    pub fn game_address(&self) -> String {
        Self {
            query_port: None,
            ..self.clone()
        }
        .to_string()
    }

    /// DNS lookup (blocking). Prefers IPv4, like the game client does.
    pub fn resolve(&self) -> std::io::Result<SocketAddr> {
        let addrs: Vec<SocketAddr> = (self.host.as_str(), self.port).to_socket_addrs()?.collect();
        addrs
            .iter()
            .find(|a| a.is_ipv4())
            .or_else(|| addrs.first())
            .copied()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "host has no address"))
    }
}

impl fmt::Display for ServerAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.host.contains(':') {
            write!(f, "[{}]:{}", self.host, self.port)?;
        } else {
            write!(f, "{}:{}", self.host, self.port)?;
        }
        if let Some(q) = self.query_port {
            write!(f, ":{q}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_forms() {
        let a = ServerAddress::parse("1.2.3.4:27016").unwrap();
        assert_eq!((a.host.as_str(), a.port), ("1.2.3.4", 27016));
        let a = ServerAddress::parse(" v3.elite-hunterz.info ").unwrap();
        assert_eq!((a.host.as_str(), a.port), ("v3.elite-hunterz.info", 27015));
        let a = ServerAddress::parse("steam://connect/1.2.3.4:27045/").unwrap();
        assert_eq!(a.port, 27045);
        let a = ServerAddress::parse("[::1]:27020").unwrap();
        assert_eq!((a.host.as_str(), a.port), ("::1", 27020));
        assert_eq!(a.to_string(), "[::1]:27020");
        assert!(ServerAddress::parse("host:0").is_none());
        assert!(ServerAddress::parse("host:99999").is_none());
        assert!(ServerAddress::parse("").is_none());
        assert!(ServerAddress::parse("a b").is_none());
        let a = ServerAddress::parse("116.203.178.232:23459:34567").unwrap();
        assert_eq!((a.port, a.query_port), (23459, Some(34567)));
        assert_eq!(a.to_string(), "116.203.178.232:23459:34567");
        assert_eq!(a.game_address(), "116.203.178.232:23459");
        let a = ServerAddress::parse("::1").unwrap();
        assert_eq!((a.host.as_str(), a.query_port), ("::1", None));
    }
}
