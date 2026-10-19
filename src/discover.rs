//! Finding servers outside the server list: Steam favorites / history
//! (`serverbrowser_hist.vdf`) and a LAN search (A2S_INFO broadcast).

use std::collections::BTreeSet;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Ports a LAN search asks on (HLDS / srcds defaults and common alternatives).
pub const LAN_PORTS: std::ops::RangeInclusive<u16> = 27015..=27030;

/// A server found in the Steam server browser files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamServer {
    pub name: String,
    pub address: String,
    /// From the "Favorites" section (otherwise "History").
    pub favorite: bool,
}

/// Steam installation folders that exist on this computer.
fn steam_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for var in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(p) = std::env::var_os(var) {
            dirs.push(PathBuf::from(p).join("Steam"));
        }
    }
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".steam").join("steam"));
        dirs.push(home.join(".local").join("share").join("Steam"));
        dirs.push(home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
        dirs.push(home.join("Library/Application Support/Steam"));
    }
    let mut seen = BTreeSet::new();
    dirs.into_iter()
        .filter(|d| d.is_dir())
        .filter(|d| seen.insert(d.canonicalize().unwrap_or_else(|_| d.clone())))
        .collect()
}

/// All `serverbrowser_hist.vdf` files of all Steam accounts on this computer.
pub fn steam_history_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in steam_dirs() {
        let Ok(users) = std::fs::read_dir(dir.join("userdata")) else {
            continue;
        };
        for user in users.flatten() {
            let f = user.path().join("7/remote/serverbrowser_hist.vdf");
            if f.is_file() {
                files.push(f);
            }
        }
    }
    files
}

/// Splits VDF text into quoted strings and braces.
fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' => out.push(c.to_string()),
            '"' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => {
                            if let Some(n) = chars.next() {
                                s.push(n);
                            }
                        }
                        '"' => break,
                        _ => s.push(c),
                    }
                }
                out.push(s);
            }
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Servers of a `serverbrowser_hist.vdf` ("Favorites" and "History" sections).
pub fn parse_server_browser(text: &str) -> Vec<SteamServer> {
    let toks = tokens(text);
    let mut out = Vec::new();
    let mut path: Vec<String> = Vec::new();
    let mut pending_key: Option<String> = None;
    let mut current: Option<(String, String)> = None;
    for t in toks {
        match t.as_str() {
            "{" => {
                path.push(pending_key.take().unwrap_or_default());
                if path.len() == 3 {
                    current = Some(Default::default());
                }
            }
            "}" => {
                if path.len() == 3
                    && let Some((name, address)) = current.take()
                    && !address.is_empty()
                {
                    out.push(SteamServer {
                        name,
                        address,
                        favorite: path[1].eq_ignore_ascii_case("Favorites"),
                    });
                }
                path.pop();
            }
            _ => match pending_key.take() {
                None => pending_key = Some(t),
                Some(key) => {
                    if let Some((name, address)) = &mut current {
                        if key.eq_ignore_ascii_case("name") {
                            *name = t;
                        } else if key.eq_ignore_ascii_case("address") {
                            *address = t;
                        }
                    }
                }
            },
        }
    }
    out
}

pub fn read_server_browser(path: &Path) -> std::io::Result<Vec<SteamServer>> {
    Ok(parse_server_browser(&std::fs::read_to_string(path)?))
}

/// Broadcasts A2S_INFO on [`LAN_PORTS`] and collects every address that answers within
/// `wait`. Blocking; run it on a thread.
pub fn lan_search(wait: Duration) -> std::io::Result<Vec<SocketAddr>> {
    const QUERY: &[u8] = b"\xFF\xFF\xFF\xFFTSource Engine Query\0";
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_broadcast(true)?;
    for port in LAN_PORTS {
        // A failing port (e.g. no broadcast route) should not stop the others.
        let _ = socket.send_to(QUERY, (Ipv4Addr::BROADCAST, port));
    }
    let start = Instant::now();
    let mut found = BTreeSet::new();
    let mut buf = [0u8; 1400];
    while let Some(left) = wait.checked_sub(start.elapsed()) {
        if left.is_zero() {
            break;
        }
        socket.set_read_timeout(Some(left.max(Duration::from_millis(10))))?;
        match socket.recv_from(&mut buf) {
            // Info ('I'), old GoldSrc info ('m') or a challenge ('A') all mean a server.
            Ok((n, from)) if n >= 5 && buf[..4] == [0xFF; 4] => {
                if matches!(buf[4], b'I' | b'm' | b'A') {
                    found.insert(from);
                }
            }
            Ok(_) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(_) => {}
        }
    }
    Ok(found.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_server_browser() {
        let text = r#"
"Filters"
{
	"Favorites"
	{
		"0"
		{
			"name"		"My \"cs\" server"
			"address"		"192.0.2.1:27015"
			"LastPlayed"		"0"
		}
	}
	"History"
	{
		"0"
		{
			"name"		"Other"
			"address"		"192.0.2.2:27016"
		}
	}
	"AppIDs" { "10" "1" }
}
"#;
        let s = parse_server_browser(text);
        assert_eq!(
            s,
            [
                SteamServer {
                    name: "My \"cs\" server".into(),
                    address: "192.0.2.1:27015".into(),
                    favorite: true
                },
                SteamServer {
                    name: "Other".into(),
                    address: "192.0.2.2:27016".into(),
                    favorite: false
                },
            ]
        );
    }
}
