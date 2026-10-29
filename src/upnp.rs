//! Port forwarding in the home router with UPnP IGD (Internet Gateway Device, UPnP
//! Device Architecture 1.0): the router is found with SSDP (multicast `M-SEARCH`), its
//! device description names the control URL of the `WANIPConnection` (or
//! `WANPPPConnection`) service, which then takes the SOAP actions `AddPortMapping`,
//! `DeletePortMapping` and `GetExternalIPAddress` over plain HTTP.
//!
//! Only answers from the local network are used, and the description and control URLs
//! must point to the device that answered the search.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::time::{Duration, Instant};

const SSDP_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(239, 255, 255, 250)), 1900);
const SEARCH_TARGETS: [&str; 2] = [
    "urn:schemas-upnp-org:device:InternetGatewayDevice:1",
    "urn:schemas-upnp-org:device:InternetGatewayDevice:2",
];
/// WAN connection services, preferred first.
const SERVICES: [&str; 3] = [
    "urn:schemas-upnp-org:service:WANIPConnection:2",
    "urn:schemas-upnp-org:service:WANIPConnection:1",
    "urn:schemas-upnp-org:service:WANPPPConnection:1",
];
const HTTP_TIMEOUT: Duration = Duration::from_secs(3);
/// Upper bound for a description or SOAP answer.
const MAX_RESPONSE: u64 = 256 * 1024;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// No UPnP router answered (UPnP off in the router or not supported).
    NotFound,
    /// The router refused the action (UPnP error code and description).
    Upnp {
        code: u16,
        description: String,
    },
    /// Unexpected answer.
    Protocol(String),
}

impl Error {
    /// UPnP error 725: the router only accepts mappings without a lease time.
    fn only_permanent_leases(&self) -> bool {
        matches!(self, Error::Upnp { code: 725, .. })
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => e.fmt(f),
            Error::NotFound => f.write_str("no UPnP router found"),
            Error::Upnp { code, description } => write!(f, "UPnP error {code} {description}"),
            Error::Protocol(msg) => f.write_str(msg),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

/// The WAN connection service of a router.
#[derive(Debug, Clone)]
pub struct Gateway {
    /// Address of the control URL.
    pub addr: SocketAddr,
    /// Path of the control URL.
    control_path: String,
    service_type: String,
}

/// Searches the local network for a UPnP router (waits up to `timeout` for answers).
pub fn discover(timeout: Duration) -> Result<Gateway, Error> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    for st in SEARCH_TARGETS {
        let request = format!(
            "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nST: {st}\r\n\
             MAN: \"ssdp:discover\"\r\nMX: 2\r\n\r\n"
        );
        socket.send_to(request.as_bytes(), SSDP_ADDR)?;
    }
    let deadline = Instant::now() + timeout;
    let mut buf = [0u8; 2048];
    let mut tried = Vec::new();
    let mut last_error = Error::NotFound;
    while let Some(left) = deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
    {
        socket.set_read_timeout(Some(left))?;
        let (len, from) = match socket.recv_from(&mut buf) {
            Ok(r) => r,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(e) => return Err(e.into()),
        };
        if !is_local(from.ip()) {
            continue;
        }
        let Some(location) = ssdp_location(&String::from_utf8_lossy(&buf[..len])) else {
            continue;
        };
        if tried.contains(&location) {
            continue;
        }
        tried.push(location.clone());
        match Gateway::from_description(&location, from.ip()) {
            Ok(gateway) => return Ok(gateway),
            Err(e) => last_error = e,
        }
    }
    Err(last_error)
}

impl Gateway {
    fn from_description(location: &str, responder: IpAddr) -> Result<Self, Error> {
        let (addr, path) = parse_http_url(location)?;
        if addr.ip() != responder {
            return Err(Error::Protocol(format!(
                "description URL points away from the router: {location}"
            )));
        }
        let (status, body) = http(addr, "GET", &path, &[], "")?;
        if status != 200 {
            return Err(Error::Protocol(format!(
                "device description: HTTP {status}"
            )));
        }
        let (service_type, control_url) = find_service(&body)
            .ok_or_else(|| Error::Protocol("router has no WAN connection service".into()))?;
        let base = elements(&body, "URLBase")
            .first()
            .map(|b| unescape(b.trim()))
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| location.to_string());
        let (control_addr, control_path) = resolve_url(&base, &control_url)?;
        if control_addr.ip() != responder {
            return Err(Error::Protocol(format!(
                "control URL points away from the router: {control_url}"
            )));
        }
        Ok(Self {
            addr: control_addr,
            control_path,
            service_type,
        })
    }

    /// The local address the router is reached from (the internal client of a mapping).
    pub fn local_ip(&self) -> io::Result<IpAddr> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.connect(self.addr)?;
        Ok(socket.local_addr()?.ip())
    }

    /// The router's address on the Internet side.
    pub fn external_ip(&self) -> Result<IpAddr, Error> {
        let body = self.soap("GetExternalIPAddress", &[])?;
        let ip = elements(&body, "NewExternalIPAddress")
            .first()
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        ip.parse()
            .map_err(|_| Error::Protocol(format!("unexpected external IP {ip:?}")))
    }

    /// Forwards `external_port` (UDP or TCP) to `internal`. `lease` 0 = until removed; a
    /// router that only knows permanent mappings (error 725) gets lease 0. Returns the
    /// lease that was set.
    pub fn add_port_mapping(
        &self,
        protocol: &str,
        external_port: u16,
        internal: SocketAddr,
        lease: u32,
        description: &str,
    ) -> Result<u32, Error> {
        let add = |lease: u32| {
            let ext = external_port.to_string();
            let int_port = internal.port().to_string();
            let client = internal.ip().to_string();
            let lease = lease.to_string();
            self.soap(
                "AddPortMapping",
                &[
                    ("NewRemoteHost", ""),
                    ("NewExternalPort", &ext),
                    ("NewProtocol", protocol),
                    ("NewInternalPort", &int_port),
                    ("NewInternalClient", &client),
                    ("NewEnabled", "1"),
                    ("NewPortMappingDescription", description),
                    ("NewLeaseDuration", &lease),
                ],
            )
        };
        match add(lease) {
            Err(e) if lease != 0 && e.only_permanent_leases() => add(0).map(|_| 0),
            r => r.map(|_| lease),
        }
    }

    pub fn delete_port_mapping(&self, protocol: &str, external_port: u16) -> Result<(), Error> {
        let ext = external_port.to_string();
        self.soap(
            "DeletePortMapping",
            &[
                ("NewRemoteHost", ""),
                ("NewExternalPort", &ext),
                ("NewProtocol", protocol),
            ],
        )
        .map(|_| ())
    }

    fn soap(&self, action: &str, args: &[(&str, &str)]) -> Result<String, Error> {
        let args: String = args
            .iter()
            .map(|(k, v)| format!("<{k}>{}</{k}>", escape(v)))
            .collect();
        let body = format!(
            "<?xml version=\"1.0\"?>\r\n<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
             s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body>\
             <u:{action} xmlns:u=\"{service}\">{args}</u:{action}></s:Body></s:Envelope>\r\n",
            service = self.service_type
        );
        let soap_action = format!("\"{}#{action}\"", self.service_type);
        let (status, answer) = http(
            self.addr,
            "POST",
            &self.control_path,
            &[
                ("Content-Type", "text/xml; charset=\"utf-8\""),
                ("SOAPAction", &soap_action),
            ],
            &body,
        )?;
        if status == 200 {
            return Ok(answer);
        }
        let code = elements(&answer, "errorCode")
            .first()
            .and_then(|c| c.trim().parse().ok());
        match code {
            Some(code) => Err(Error::Upnp {
                code,
                description: elements(&answer, "errorDescription")
                    .first()
                    .map(|d| unescape(d.trim()))
                    .unwrap_or_default(),
            }),
            None => Err(Error::Protocol(format!("{action}: HTTP {status}"))),
        }
    }
}

/// Addresses a router may answer from: private, link-local or CGNAT IPv4.
fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_link_local()
                || v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64
        }
        IpAddr::V6(_) => false,
    }
}

/// The `LOCATION` header of an SSDP answer.
fn ssdp_location(response: &str) -> Option<String> {
    let mut lines = response.lines();
    if !lines.next()?.starts_with("HTTP/1.") {
        return None;
    }
    lines.find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("location")
            .then(|| value.trim().to_string())
    })
}

/// `http://host:port/path` (host as IPv4 address, as routers announce it).
fn parse_http_url(url: &str) -> Result<(SocketAddr, String), Error> {
    let bad = || Error::Protocol(format!("unsupported URL {url:?}"));
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("HTTP://"))
        .ok_or_else(bad)?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().map_err(|_| bad())?),
        None => (authority, 80),
    };
    let ip: Ipv4Addr = host.parse().map_err(|_| bad())?;
    Ok((SocketAddr::new(IpAddr::V4(ip), port), path.to_string()))
}

/// A control URL from the description: absolute, or relative to `base`.
fn resolve_url(base: &str, url: &str) -> Result<(SocketAddr, String), Error> {
    if url.contains("://") {
        return parse_http_url(url);
    }
    let (addr, _) = parse_http_url(base)?;
    let path = if url.starts_with('/') {
        url.to_string()
    } else {
        format!("/{url}")
    };
    Ok((addr, path))
}

/// Service type and control URL of the preferred WAN connection service.
fn find_service(description: &str) -> Option<(String, String)> {
    let services: Vec<(String, String)> = elements(description, "service")
        .into_iter()
        .filter_map(|s| {
            let service_type = unescape(elements(s, "serviceType").first()?.trim());
            let control = unescape(elements(s, "controlURL").first()?.trim());
            Some((service_type, control))
        })
        .collect();
    SERVICES
        .iter()
        .find_map(|want| services.iter().find(|(t, _)| t == want).cloned())
}

/// Contents of every element with this local name (any namespace prefix).
fn elements<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let mut found = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find('<') {
        rest = &rest[start + 1..];
        let end = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..end];
        let tag_name = tag
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("");
        let local = tag_name.rsplit(':').next().unwrap_or("");
        if local != name || tag.ends_with('/') || tag.starts_with('/') {
            continue;
        }
        let content = &rest[(end + 1).min(rest.len())..];
        let close = format!("</{tag_name}>");
        // No closing tag further on: later elements of this name cannot have one either.
        let Some(close_at) = content.find(&close) else {
            break;
        };
        found.push(&content[..close_at]);
        rest = &content[close_at + close.len()..];
    }
    found
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// One HTTP/1.1 request (`Connection: close`). Returns status and body.
fn http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> Result<(u16, String), Error> {
    let mut stream = TcpStream::connect_timeout(&addr, HTTP_TIMEOUT)?;
    stream.set_read_timeout(Some(HTTP_TIMEOUT))?;
    stream.set_write_timeout(Some(HTTP_TIMEOUT))?;
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    if method == "POST" {
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream.write_all(request.as_bytes())?;
    let mut response = Vec::new();
    stream.take(MAX_RESPONSE).read_to_end(&mut response)?;
    parse_http_response(&response)
}

fn parse_http_response(response: &[u8]) -> Result<(u16, String), Error> {
    let text = String::from_utf8_lossy(response);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| Error::Protocol("incomplete HTTP answer".into()))?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| Error::Protocol("bad HTTP status line".into()))?;
    let chunked = head.lines().any(|l| {
        l.split_once(':').is_some_and(|(n, v)| {
            n.trim().eq_ignore_ascii_case("transfer-encoding")
                && v.trim().eq_ignore_ascii_case("chunked")
        })
    });
    let body = if chunked {
        dechunk(body)
    } else {
        body.to_string()
    };
    Ok((status, body))
}

fn dechunk(mut body: &str) -> String {
    let mut out = String::new();
    while let Some((size, rest)) = body.split_once("\r\n") {
        let size = size.split(';').next().unwrap_or("").trim();
        let Ok(size) = usize::from_str_radix(size, 16) else {
            break;
        };
        if size == 0 {
            break;
        }
        if rest.len() < size || !rest.is_char_boundary(size) {
            out.push_str(rest);
            break;
        }
        out.push_str(&rest[..size]);
        body = rest[size..].strip_prefix("\r\n").unwrap_or(&rest[size..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    const DESCRIPTION: &str = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
<device><deviceType>urn:schemas-upnp-org:device:InternetGatewayDevice:1</deviceType>
<serviceList><service>
<serviceType>urn:schemas-upnp-org:service:Layer3Forwarding:1</serviceType>
<controlURL>/ctl/L3F</controlURL></service></serviceList>
<deviceList><device><deviceList><device><serviceList>
<service><serviceType>urn:schemas-upnp-org:service:WANPPPConnection:1</serviceType>
<controlURL>/ctl/PPP</controlURL></service>
<service><serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>
<controlURL>/igdupnp/control/WANIPConn1</controlURL></service>
</serviceList></device></deviceList></device></deviceList></device></root>"#;

    #[test]
    fn finds_wan_ip_service() {
        assert_eq!(
            find_service(DESCRIPTION),
            Some((
                "urn:schemas-upnp-org:service:WANIPConnection:1".into(),
                "/igdupnp/control/WANIPConn1".into()
            ))
        );
        assert_eq!(find_service("<root></root>"), None);
    }

    #[test]
    fn parses_ssdp_answer() {
        let answer = "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\n\
                      Location: http://192.168.0.1:49000/igddesc.xml\r\n\
                      ST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\r\n";
        assert_eq!(
            ssdp_location(answer).as_deref(),
            Some("http://192.168.0.1:49000/igddesc.xml")
        );
        assert_eq!(ssdp_location("NOTIFY * HTTP/1.1\r\nLOCATION: x\r\n"), None);
    }

    #[test]
    fn parses_urls() {
        let (addr, path) = parse_http_url("http://192.168.0.1:49000/igddesc.xml").unwrap();
        assert_eq!(addr, "192.168.0.1:49000".parse().unwrap());
        assert_eq!(path, "/igddesc.xml");
        assert_eq!(
            resolve_url("http://10.0.0.1:5000/rootDesc.xml", "ctl/IPConn").unwrap(),
            ("10.0.0.1:5000".parse().unwrap(), "/ctl/IPConn".into())
        );
        assert!(parse_http_url("https://192.168.0.1/").is_err());
        assert!(parse_http_url("http://router.example/").is_err());
        assert!(is_local("192.168.0.1".parse().unwrap()));
        assert!(!is_local("203.0.113.1".parse().unwrap()));
    }

    #[test]
    fn reads_soap_answers() {
        let fault = "<s:Envelope><s:Body><s:Fault><detail><UPnPError xmlns=\"urn:schemas-upnp-org:control-1-0\">\
                     <errorCode>718</errorCode><errorDescription>ConflictInMappingEntry</errorDescription>\
                     </UPnPError></detail></s:Fault></s:Body></s:Envelope>";
        assert_eq!(elements(fault, "errorCode"), vec!["718"]);
        let ok = "<u:GetExternalIPAddressResponse xmlns:u=\"x\"><NewExternalIPAddress>203.0.113.7</NewExternalIPAddress></u:GetExternalIPAddressResponse>";
        assert_eq!(elements(ok, "NewExternalIPAddress"), vec!["203.0.113.7"]);
        // Unclosed elements end the search instead of rescanning the rest each time.
        assert!(elements(&"<a>".repeat(50_000), "a").is_empty());
        assert_eq!(escape("a<b>&\"c\""), "a&lt;b&gt;&amp;&quot;c&quot;");
        assert_eq!(unescape("a&amp;lt;"), "a&lt;");
    }

    #[test]
    fn decodes_chunked_bodies() {
        let (status, body) = parse_http_response(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n",
        )
        .unwrap();
        assert_eq!((status, body.as_str()), (200, "Wikipedia"));
    }

    /// A fake router that answers one SOAP request.
    fn fake_router(
        status: &'static str,
        answer: &'static str,
    ) -> (Gateway, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            conn.set_read_timeout(Some(Duration::from_millis(500)))
                .unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            while let Ok(n) = conn.read(&mut buf) {
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&request);
                if text.contains("</s:Envelope>")
                    || text.starts_with("GET") && text.contains("\r\n\r\n")
                {
                    break;
                }
            }
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/xml\r\nContent-Length: {}\r\n\r\n{answer}",
                answer.len()
            );
            conn.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        let gateway = Gateway {
            addr,
            control_path: "/ctl/IPConn".into(),
            service_type: "urn:schemas-upnp-org:service:WANIPConnection:1".into(),
        };
        (gateway, server)
    }

    #[test]
    fn reads_device_description() {
        let (router, server) = fake_router("200 OK", DESCRIPTION);
        let location = format!("http://{}/igddesc.xml", router.addr);
        let gateway = Gateway::from_description(&location, router.addr.ip()).unwrap();
        assert!(
            server
                .join()
                .unwrap()
                .starts_with("GET /igddesc.xml HTTP/1.1\r\n")
        );
        assert_eq!(gateway.addr, router.addr);
        assert_eq!(gateway.control_path, "/igdupnp/control/WANIPConn1");
        assert_eq!(
            gateway.service_type,
            "urn:schemas-upnp-org:service:WANIPConnection:1"
        );
        // The description must come from the device that answered the search.
        assert!(Gateway::from_description(&location, "192.168.0.1".parse().unwrap()).is_err());
    }

    #[test]
    fn adds_port_mapping() {
        let (gateway, server) = fake_router(
            "200 OK",
            "<s:Envelope><s:Body><u:AddPortMappingResponse/></s:Body></s:Envelope>",
        );
        let lease = gateway
            .add_port_mapping(
                "UDP",
                27500,
                "192.168.0.20:27500".parse().unwrap(),
                3600,
                "openHLSW",
            )
            .unwrap();
        assert_eq!(lease, 3600);
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /ctl/IPConn HTTP/1.1\r\n"));
        assert!(request.contains(
            "SOAPAction: \"urn:schemas-upnp-org:service:WANIPConnection:1#AddPortMapping\""
        ));
        assert!(request.contains("<NewExternalPort>27500</NewExternalPort>"));
        assert!(request.contains("<NewProtocol>UDP</NewProtocol>"));
        assert!(request.contains("<NewInternalClient>192.168.0.20</NewInternalClient>"));
        assert!(request.contains("<NewLeaseDuration>3600</NewLeaseDuration>"));
    }

    #[test]
    fn reports_upnp_errors() {
        let (gateway, server) = fake_router(
            "500 Internal Server Error",
            "<s:Envelope><s:Body><s:Fault><detail><UPnPError><errorCode>718</errorCode>\
             <errorDescription>ConflictInMappingEntry</errorDescription></UPnPError></detail>\
             </s:Fault></s:Body></s:Envelope>",
        );
        let err = gateway.delete_port_mapping("UDP", 27500).unwrap_err();
        server.join().unwrap();
        assert!(matches!(err, Error::Upnp { code: 718, .. }), "{err:?}");
        assert_eq!(err.to_string(), "UPnP error 718 ConflictInMappingEntry");
    }
}
