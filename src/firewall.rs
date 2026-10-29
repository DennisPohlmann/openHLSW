//! Looks for a firewall on this computer that may drop the server log (no root needed):
//! ufw (its switch is readable in `/etc/ufw/ufw.conf`, its rules are not) and firewalld
//! (`firewall-cmd --query-port` asks it over D-Bus).

use std::process::{Command, Stdio};

/// A firewall that is on and may block the log port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Firewall {
    /// ufw is enabled; whether the port is allowed cannot be read without root.
    Ufw,
    /// firewalld runs and the port is not open in its active zone.
    FirewalldClosed,
}

/// Checks the firewalls this program knows for UDP `port`. Runs `firewall-cmd`, so call it
/// off the UI thread.
pub fn check(port: u16) -> Option<Firewall> {
    if cfg!(target_os = "linux") {
        if std::fs::read_to_string("/etc/ufw/ufw.conf").is_ok_and(|conf| ufw_enabled(&conf)) {
            return Some(Firewall::Ufw);
        }
        if firewalld_port_closed(port) {
            return Some(Firewall::FirewalldClosed);
        }
    }
    None
}

/// `ENABLED=yes` in ufw.conf (quotes and spaces allowed, comments ignored).
fn ufw_enabled(conf: &str) -> bool {
    conf.lines().any(|line| {
        let line = line.trim();
        !line.starts_with('#')
            && line.split_once('=').is_some_and(|(key, value)| {
                key.trim() == "ENABLED"
                    && value
                        .trim()
                        .trim_matches(['"', '\''])
                        .eq_ignore_ascii_case("yes")
            })
    })
}

/// firewalld answers "no" for a port that is not open; "yes", an error or a missing
/// `firewall-cmd` (firewalld not installed or not running) count as not closed.
fn firewalld_port_closed(port: u16) -> bool {
    Command::new("firewall-cmd")
        .arg(format!("--query-port={port}/udp"))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "no")
}

#[cfg(test)]
mod tests {
    use super::ufw_enabled;

    #[test]
    fn reads_ufw_switch() {
        assert!(ufw_enabled("# comment\nENABLED=yes\nLOGLEVEL=low\n"));
        assert!(ufw_enabled("ENABLED = \"yes\"\n"));
        assert!(!ufw_enabled("ENABLED=no\n"));
        assert!(!ufw_enabled("#ENABLED=yes\n"));
        assert!(!ufw_enabled(""));
    }
}
