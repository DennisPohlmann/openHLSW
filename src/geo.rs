//! Server location (country, city, provider) via ip-api.com, cached on disk.
//!
//! ip-api.com is free without an API key over plain HTTP (45 single / 15 batch
//! requests per minute). Results are cached in `<config dir>/geo_cache.json`
//! so every server IP is looked up only once.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeoInfo {
    pub country: String,
    /// ISO 3166-1 alpha-2, lower case ("de"). Empty when unknown.
    pub country_code: String,
    pub city: String,
    pub isp: String,
    pub org: String,
    /// "AS24940 Hetzner Online GmbH"
    pub asn: String,
}

impl GeoInfo {
    /// Provider shown in the Location column.
    pub fn provider(&self) -> &str {
        if !self.isp.is_empty() {
            &self.isp
        } else {
            &self.org
        }
    }

    pub fn place(&self) -> String {
        match (self.country.is_empty(), self.city.is_empty()) {
            (false, false) => format!("{}, {}", self.country, self.city),
            (false, true) => self.country.clone(),
            _ => String::new(),
        }
    }
}

/// Private, loopback and other non-routable addresses are not looked up.
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

fn cache_path() -> Option<PathBuf> {
    crate::config::path().and_then(|p| p.parent().map(|d| d.join("geo_cache.json")))
}

pub fn load_cache() -> HashMap<IpAddr, GeoInfo> {
    let Some(text) = cache_path().and_then(|p| std::fs::read_to_string(p).ok()) else {
        return HashMap::new();
    };
    let raw: HashMap<String, GeoInfo> = serde_json::from_str(&text).unwrap_or_default();
    raw.into_iter()
        .filter_map(|(k, v)| Some((k.parse().ok()?, v)))
        .collect()
}

pub fn save_cache(cache: &HashMap<IpAddr, GeoInfo>) {
    let Some(path) = cache_path() else { return };
    let raw: HashMap<String, &GeoInfo> = cache.iter().map(|(k, v)| (k.to_string(), v)).collect();
    if let Ok(json) = serde_json::to_string_pretty(&raw) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, json);
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiEntry {
    status: String,
    #[serde(default)]
    query: String,
    #[serde(default)]
    country: String,
    #[serde(default)]
    country_code: String,
    #[serde(default)]
    city: String,
    #[serde(default)]
    isp: String,
    #[serde(default)]
    org: String,
    #[serde(default, rename = "as")]
    asn: String,
}

/// Looks up up to 100 IPs with one request. IPs ip-api.com cannot locate come
/// back with an empty [`GeoInfo`] so they are not asked again.
pub fn lookup_batch(ips: &[IpAddr]) -> Result<Vec<(IpAddr, GeoInfo)>, String> {
    let body = serde_json::to_string(&ips.iter().map(|ip| ip.to_string()).collect::<Vec<_>>())
        .map_err(|e| e.to_string())?;
    let addr = ("ip-api.com", 80)
        .to_socket_addrs()
        .map_err(|e| format!("DNS: {e}"))?
        .next()
        .ok_or("DNS: no address")?;
    let mut stream =
        TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    let request = format!(
        "POST /batch?fields=status,query,country,countryCode,city,isp,org,as HTTP/1.0\r\n\
         Host: ip-api.com\r\nUser-Agent: openHLSW/{}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        env!("CARGO_PKG_VERSION"),
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|e| e.to_string())?;
    let response = String::from_utf8_lossy(&response);
    let (head, json) = response
        .split_once("\r\n\r\n")
        .ok_or("invalid HTTP response")?;
    let status_line = head.lines().next().unwrap_or_default();
    if !status_line.contains(" 200") {
        return Err(format!("ip-api.com: {status_line}"));
    }
    let entries: Vec<ApiEntry> =
        serde_json::from_str(json.trim()).map_err(|e| format!("ip-api.com: {e}"))?;
    Ok(entries
        .into_iter()
        .filter_map(|e| {
            let ip: IpAddr = e.query.parse().ok()?;
            let info = if e.status == "success" {
                GeoInfo {
                    country: e.country,
                    country_code: e.country_code.to_lowercase(),
                    city: e.city,
                    isp: e.isp,
                    org: e.org,
                    asn: e.asn,
                }
            } else {
                GeoInfo::default()
            };
            Some((ip, info))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_ranges() {
        assert!(!is_public("127.0.0.1".parse().unwrap()));
        assert!(!is_public("192.168.1.10".parse().unwrap()));
        assert!(!is_public("10.0.0.1".parse().unwrap()));
        assert!(!is_public("100.64.1.1".parse().unwrap()));
        assert!(is_public("116.203.178.232".parse().unwrap()));
        assert!(is_public("46.174.53.40".parse().unwrap()));
    }
}
