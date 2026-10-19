//! Global Player ID Database (HLSW "Global Player ID Database Editor"): a local list of
//! Steam IDs with the names seen, a type (Normal, Friend, Lamer, …), a comment and a
//! banned flag. Stored next to the config as `iddb.json`; banned IDs can be imported from
//! and exported to a `banned.cfg`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use openhlsw::steamid::SteamId;

/// Type of an entry; colours the player name like HLSW does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IdKind {
    #[default]
    Normal,
    Friend,
    Lamer,
    Blue,
    Brown,
}

impl IdKind {
    pub const ALL: [IdKind; 5] = [
        IdKind::Normal,
        IdKind::Friend,
        IdKind::Lamer,
        IdKind::Blue,
        IdKind::Brown,
    ];

    /// English name (translated with `tr!` by the caller).
    pub fn name(self) -> &'static str {
        match self {
            IdKind::Normal => "Normal",
            IdKind::Friend => "Friend",
            IdKind::Lamer => "Lamer",
            IdKind::Blue => "Blue",
            IdKind::Brown => "Brown",
        }
    }

    /// Name colour (RGB); `None` for normal entries.
    pub fn color(self) -> Option<[u8; 3]> {
        match self {
            IdKind::Normal => None,
            IdKind::Friend => Some([0x20, 0xA0, 0x20]),
            IdKind::Lamer => Some([0xD0, 0x30, 0x30]),
            IdKind::Blue => Some([0x30, 0x60, 0xE0]),
            IdKind::Brown => Some([0xA0, 0x60, 0x20]),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct IdEntry {
    /// Names seen with this ID, most recent last.
    pub names: Vec<String>,
    pub kind: IdKind,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub comment: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub banned: bool,
    /// Added automatically from a players list (View → Hide Auto Added).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub auto: bool,
}

impl IdEntry {
    pub fn names_text(&self) -> String {
        self.names.join(", ")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IdDb {
    /// Normalised ID (`STEAM_0:Y:Z`) → entry.
    pub entries: BTreeMap<String, IdEntry>,
}

/// Normalises a Steam ID of any form to `STEAM_0:Y:Z`; `None` for bots, LAN, pending.
pub fn normalize(id: &str) -> Option<String> {
    SteamId::parse(id).map(|s| SteamId { universe: 0, ..s }.steam2())
}

pub fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("openHLSW").join("iddb.json"))
}

impl IdDb {
    pub fn load() -> Self {
        path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = path() else { return Ok(()) };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(tmp, path)
    }

    pub fn get(&self, id: &str) -> Option<&IdEntry> {
        self.entries.get(&normalize(id)?)
    }

    /// Records a player name seen with `id`. With `add` new IDs are added (marked as auto
    /// added); otherwise only known entries learn new names. Returns true if anything changed.
    pub fn observe(&mut self, id: &str, name: &str, add: bool) -> bool {
        let Some(key) = normalize(id) else {
            return false;
        };
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let entry = match self.entries.get_mut(&key) {
            Some(e) => e,
            None if add => self.entries.entry(key).or_insert_with(|| IdEntry {
                auto: true,
                ..Default::default()
            }),
            None => return false,
        };
        if entry.names.iter().any(|n| n == name) {
            return false;
        }
        entry.names.push(name.to_string());
        true
    }

    /// Reads `banid <minutes> <id>` lines of a `banned.cfg`; returns the number of IDs
    /// marked as banned.
    pub fn import_banned_cfg(&mut self, text: &str) -> usize {
        let mut count = 0;
        for line in text.lines() {
            let mut words = line.split_whitespace();
            if !words
                .next()
                .is_some_and(|w| w.eq_ignore_ascii_case("banid"))
            {
                continue;
            }
            let Some(key) = words.find_map(normalize) else {
                continue;
            };
            let e = self.entries.entry(key).or_default();
            if !e.banned {
                e.banned = true;
                count += 1;
            }
        }
        count
    }

    /// `banned.cfg` with all banned IDs (permanent bans).
    pub fn export_banned_cfg(&self) -> String {
        self.entries
            .iter()
            .filter(|(_, e)| e.banned)
            .map(|(id, _)| format!("banid 0.0 {id}\n"))
            .collect()
    }

    /// IDs of all banned entries.
    pub fn banned(&self) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .filter(|(_, e)| e.banned)
            .map(|(id, _)| id.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observe_and_banned_cfg() {
        let mut db = IdDb::default();
        assert!(!db.observe("STEAM_1:0:11101", "Alice", false));
        assert!(db.observe("STEAM_1:0:11101", "Alice", true));
        assert!(!db.observe("[U:1:22202]", "Alice", true));
        assert!(db.observe("[U:1:22202]", "Alice2", false));
        assert!(!db.observe("BOT", "Bot", true));
        let e = db.get("76561197960287930").unwrap();
        assert_eq!(e.names, ["Alice", "Alice2"]);
        assert!(e.auto);

        let n = db.import_banned_cfg("banid 0.0 STEAM_0:1:5\n// x\nbanid 30 STEAM_0:0:11101\n");
        assert_eq!(n, 2);
        assert_eq!(
            db.export_banned_cfg(),
            "banid 0.0 STEAM_0:0:11101\nbanid 0.0 STEAM_0:1:5\n"
        );
        assert_eq!(db.banned().count(), 2);
    }
}
