//! Persistent settings: server list, saved RCON passwords, refresh options.
//! Stored as JSON in the user's config directory
//! (Windows: %APPDATA%\openHLSW\config.json, Linux: ~/.config/openHLSW/config.json).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::settings::Settings;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Servers of the current server list.
    pub servers: Vec<SavedServer>,
    /// Name of the current server list.
    pub list_name: String,
    /// All other server lists.
    pub other_lists: Vec<SavedList>,
    /// "View Connect Log": newest last.
    pub connect_log: Vec<String>,
    /// RCON console command history, newest last; see [`history_to_save`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub console_history: Vec<String>,
    pub selected: Option<String>,
    /// The old global "Hide": only fills in servers of a config written before the RCON
    /// settings were kept per server (each server keeps its own in [`SavedServer`]). New
    /// servers always start with Hide on.
    pub hide_rcon_password: bool,
    /// The old global "Auto Update Status", like `hide_rcon_password`.
    pub auto_update_status: bool,
    #[serde(flatten)]
    pub settings: Settings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
            list_name: DEFAULT_LIST_NAME.into(),
            other_lists: Vec::new(),
            connect_log: Vec::new(),
            console_history: Vec::new(),
            selected: None,
            hide_rcon_password: true,
            auto_update_status: true,
            settings: Settings::default(),
        }
    }
}

pub const DEFAULT_LIST_NAME: &str = "Custom server list";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedList {
    pub name: String,
    #[serde(default)]
    pub servers: Vec<SavedServer>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SavedServer {
    pub address: String,
    /// HLSW key of the game chosen when the server was added (decides the query
    /// protocol); missing = detected automatically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
    /// Only present when "Save" is checked. Stored in plain text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rcon_password: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// Extra start parameters for this server (Server Properties → Additional parameters).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub params: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_connected: Option<String>,
    /// Last known server info (name, map, game, players, ...), shown while the server is
    /// offline or not answered yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<openhlsw::a2s::ServerInfo>,
    /// Last successful query (RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<String>,
    /// Rcon Config → Hide; missing = [`Config::hide_rcon_password`] for entries of this
    /// config, on for imported servers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hide_rcon_password: Option<bool>,
    /// Rcon Config → Auto Update Status; missing = [`Config::auto_update_status`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_update_status: Option<bool>,
    /// Seconds between two automatic `status` commands; missing = the default (30 s).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_interval: Option<u32>,
}

/// Most console commands kept across restarts.
pub const CONSOLE_HISTORY_MAX: usize = 200;

/// Whether a console command may carry a secret (password, login, token) and so must never
/// be written to disk.
pub fn is_secret_command(command: &str) -> bool {
    let lower = command.to_lowercase();
    let mut words = lower.split_whitespace();
    let first = words.next().unwrap_or("");
    matches!(
        first,
        "rcon_password" | "sv_password" | "password" | "login" | "sv_setsteamaccount"
    ) || (first == "rcon" && words.next().is_some())
        || ["password", "passwd", "token"]
            .iter()
            .any(|w| lower.contains(w))
}

/// The console history as stored in the config: no empty lines, no commands that may carry
/// a secret, no consecutive duplicates, at most [`CONSOLE_HISTORY_MAX`] entries (the newest).
pub fn history_to_save(history: &[String]) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for command in history.iter().map(|c| c.trim()) {
        if command.is_empty() || is_secret_command(command) {
            continue;
        }
        if kept.last().map(String::as_str) != Some(command) {
            kept.push(command.to_string());
        }
    }
    let excess = kept.len().saturating_sub(CONSOLE_HISTORY_MAX);
    kept.drain(..excess);
    kept
}

pub fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("openHLSW").join("config.json"))
}

pub fn load() -> Config {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(config: &Config) -> std::io::Result<()> {
    let Some(path) = path() else { return Ok(()) };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(config).map_err(std::io::Error::other)?;
    // Write to a temp file first so a crash never leaves a truncated config behind.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn secret_commands_are_not_saved() {
        for c in [
            "rcon_password PLACEHOLDER",
            "SV_PASSWORD PLACEHOLDER",
            "password x",
            "login USER PLACEHOLDER",
            "rcon pw status",
            "sv_setsteamaccount PLACEHOLDER",
            "tv_password x",
            "admin_passwd y",
            "set_token z",
        ] {
            assert!(is_secret_command(c), "{c}");
        }
        for c in ["status", "changelevel de_dust2", "rcon", "users", "sv_cheats 0"] {
            assert!(!is_secret_command(c), "{c}");
        }
    }

    #[test]
    fn history_is_filtered_and_capped() {
        let history = strings(&["status", "  ", "status", "rcon_password x", "status", "users"]);
        assert_eq!(history_to_save(&history), strings(&["status", "users"]));

        let long: Vec<String> = (0..CONSOLE_HISTORY_MAX + 50).map(|i| format!("say {i}")).collect();
        let saved = history_to_save(&long);
        assert_eq!(saved.len(), CONSOLE_HISTORY_MAX);
        assert_eq!(saved.first().unwrap(), "say 50");
        assert_eq!(saved.last().unwrap(), &format!("say {}", CONSOLE_HISTORY_MAX + 49));
    }

    #[test]
    fn old_config_without_history_loads() {
        let config: Config = serde_json::from_str(r#"{"servers": []}"#).unwrap();
        assert!(config.console_history.is_empty());
    }
}
