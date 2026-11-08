//! The server list and console history, read from (and written back to) the config file of
//! the openHLSW GUI, so both show the same servers.
//!
//! Only the keys the console tool changes (`servers`, `console_history`) are touched: the
//! file is read again right before every write and every other setting is kept as it is.
//! A file that cannot be parsed is never written.

use std::io;
use std::path::PathBuf;

use openhlsw::a2s::ServerInfo;
use serde_json::{Map, Value};

/// One server of the list, as far as the console tool needs it.
pub struct Entry {
    pub address: String,
    pub game: Option<String>,
    /// Only present when "Save" is checked in the GUI.
    pub rcon_password: Option<String>,
    /// Last known info (shown until the server answers).
    pub info: Option<ServerInfo>,
    pub comment: String,
    /// Rcon Config → Hide (on when missing).
    pub hide_rcon_password: bool,
}

pub struct Store {
    path: Option<PathBuf>,
    doc: Map<String, Value>,
    /// False when the file exists but is not valid JSON: then nothing is written.
    writable: bool,
}

/// `config.json` of the GUI (Linux: ~/.config/openHLSW, Windows: %APPDATA%\openHLSW).
pub fn default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("openHLSW").join("config.json"))
}

fn read(path: &Option<PathBuf>) -> (Map<String, Value>, bool) {
    let Some(path) = path else {
        return (Map::new(), false);
    };
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => (map, true),
            _ => (Map::new(), false),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => (Map::new(), true),
        Err(_) => (Map::new(), false),
    }
}

impl Store {
    pub fn load(path: Option<PathBuf>) -> Self {
        let (doc, writable) = read(&path);
        Self {
            path,
            doc,
            writable,
        }
    }

    pub fn writable(&self) -> bool {
        self.writable
    }

    pub fn list_name(&self) -> String {
        self.doc
            .get("list_name")
            .and_then(Value::as_str)
            .unwrap_or("Custom server list")
            .to_string()
    }

    pub fn servers(&self) -> Vec<Entry> {
        let Some(Value::Array(list)) = self.doc.get("servers") else {
            return Vec::new();
        };
        list.iter()
            .filter_map(|s| {
                let address = s.get("address")?.as_str()?.to_string();
                let text = |k: &str| s.get(k).and_then(Value::as_str).map(str::to_string);
                Some(Entry {
                    address,
                    game: text("game"),
                    rcon_password: text("rcon_password"),
                    info: s
                        .get("info")
                        .and_then(|i| serde_json::from_value(i.clone()).ok()),
                    comment: text("comment").unwrap_or_default(),
                    hide_rcon_password: s
                        .get("hide_rcon_password")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                })
            })
            .collect()
    }

    /// A number from the GUI's settings (top-level keys of config.json).
    pub fn setting_f64(&self, key: &str) -> Option<f64> {
        self.doc.get(key).and_then(Value::as_f64)
    }

    pub fn history(&self) -> Vec<String> {
        match self.doc.get("console_history") {
            Some(Value::Array(h)) => h
                .iter()
                .filter_map(|c| c.as_str().map(str::to_string))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Reads the file again, lets `change` edit it and writes it back (temp file first, so a
    /// crash never leaves a truncated config behind).
    fn update(&mut self, change: impl FnOnce(&mut Map<String, Value>)) -> io::Result<()> {
        let (doc, writable) = read(&self.path);
        if !writable {
            self.writable = false;
            return Err(io::Error::other(
                "the config file cannot be read, not changed",
            ));
        }
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        self.doc = doc;
        change(&mut self.doc);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(&Value::Object(self.doc.clone()))
            .map_err(io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        openhlsw::write_private(&tmp, json.as_bytes())?;
        std::fs::rename(tmp, path)
    }

    /// Adds a server at the end of the current list (like the GUI's Add: RCON password
    /// hidden, automatic status on).
    pub fn add_server(&mut self, address: &str) -> io::Result<()> {
        self.update(|doc| {
            let list = doc
                .entry("servers")
                .or_insert_with(|| Value::Array(Vec::new()));
            if !list.is_array() {
                *list = Value::Array(Vec::new());
            }
            if let Value::Array(list) = list {
                let mut entry = Map::new();
                entry.insert("address".into(), address.into());
                entry.insert("hide_rcon_password".into(), true.into());
                entry.insert("auto_update_status".into(), true.into());
                list.push(Value::Object(entry));
            }
        })
    }

    /// Removes the first server with this address from the current list.
    pub fn remove_server(&mut self, address: &str) -> io::Result<()> {
        self.update(|doc| {
            if let Some(Value::Array(list)) = doc.get_mut("servers")
                && let Some(i) = list
                    .iter()
                    .position(|s| s.get("address").and_then(Value::as_str) == Some(address))
            {
                list.remove(i);
            }
        })
    }

    /// Rcon Config → Save / Hide of one server: the password is stored (in plain text, like
    /// the GUI does) only with `password` = Some.
    pub fn set_rcon(
        &mut self,
        address: &str,
        password: Option<&str>,
        hide: bool,
    ) -> io::Result<()> {
        self.update(|doc| {
            if let Some(Value::Array(list)) = doc.get_mut("servers")
                && let Some(Value::Object(entry)) = list
                    .iter_mut()
                    .find(|s| s.get("address").and_then(Value::as_str) == Some(address))
            {
                match password {
                    Some(p) => {
                        entry.insert("rcon_password".into(), p.into());
                    }
                    None => {
                        entry.remove("rcon_password");
                    }
                }
                entry.insert("hide_rcon_password".into(), hide.into());
            }
        })
    }

    /// Stores the console history (without commands that may carry a secret).
    pub fn save_history(&mut self, history: &[String]) -> io::Result<()> {
        let kept = openhlsw::history::history_to_save(history);
        self.update(|doc| {
            doc.insert(
                "console_history".into(),
                Value::Array(kept.into_iter().map(Value::from).collect()),
            );
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("openhlsw-tui-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("config.json")
    }

    #[test]
    fn keeps_other_settings_when_adding_and_removing() {
        let path = temp("keep");
        std::fs::write(
            &path,
            r#"{"servers":[{"address":"192.0.2.1:27015","game":"CSS"}],"skin":"Black","font_size":12}"#,
        )
        .unwrap();
        let mut store = Store::load(Some(path.clone()));
        assert_eq!(store.servers().len(), 1);
        assert_eq!(store.servers()[0].game.as_deref(), Some("CSS"));
        store.add_server("192.0.2.2:27016").unwrap();
        store.remove_server("192.0.2.1:27015").unwrap();
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(doc["skin"], "Black");
        assert_eq!(doc["font_size"], 12);
        assert_eq!(doc["servers"].as_array().unwrap().len(), 1);
        assert_eq!(doc["servers"][0]["address"], "192.0.2.2:27016");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn broken_file_is_never_written() {
        let path = temp("broken");
        std::fs::write(&path, "{ not json").unwrap();
        let mut store = Store::load(Some(path.clone()));
        assert!(!store.writable());
        assert!(store.add_server("192.0.2.3:27015").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn secret_commands_stay_out_of_the_history() {
        let path = temp("history");
        let mut store = Store::load(Some(path.clone()));
        store
            .save_history(&["status".into(), "rcon_password x".into(), "users".into()])
            .unwrap();
        assert_eq!(
            Store::load(Some(path.clone())).history(),
            vec!["status", "users"]
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
