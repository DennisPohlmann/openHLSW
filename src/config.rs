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
    pub selected: Option<String>,
    pub hide_rcon_password: bool,
    /// Periodically refresh players of the selected server.
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
