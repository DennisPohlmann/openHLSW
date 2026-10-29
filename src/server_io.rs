//! Server list export / import (Server list → Export / Import): openHLSW JSON and CSV
//! with a chosen set of fields, plus the old text lists (one address per line) on import.
//!
//! JSON: `{"openhlsw_servers": 1, "lists": [{"name": "…", "servers": [{"address": …}]}]}`,
//! the servers written like in `config.json` (fields that are not exported are left out).
//! A single server list (`{"name", "servers"}`) or a bare array of servers is read too.
//!
//! CSV: semicolon separated, the first row names the columns (`address` is required, see
//! [`Field::key`]), every value in double quotes (an empty value is `""`), a quote inside a
//! value doubled. The optional column `list` names the server list of each row.

use openhlsw::address::ServerAddress;
use serde::{Deserialize, Serialize};

use crate::config::{SavedList, SavedServer};

/// Server settings that can be exported and imported (the address always is).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Game,
    Comment,
    Params,
    RconPassword,
    HideRconPassword,
    AutoUpdateStatus,
    StatusInterval,
}

impl Field {
    pub const ALL: [Field; 7] = [
        Field::Game,
        Field::Comment,
        Field::Params,
        Field::RconPassword,
        Field::HideRconPassword,
        Field::AutoUpdateStatus,
        Field::StatusInterval,
    ];

    /// Name of the JSON key and CSV column (the key in `config.json`).
    pub fn key(self) -> &'static str {
        match self {
            Field::Game => "game",
            Field::Comment => "comment",
            Field::Params => "params",
            Field::RconPassword => "rcon_password",
            Field::HideRconPassword => "hide_rcon_password",
            Field::AutoUpdateStatus => "auto_update_status",
            Field::StatusInterval => "status_interval",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Field::Game => tr!("Game"),
            Field::Comment => tr!("Comment"),
            Field::Params => tr!("Additional parameters"),
            Field::RconPassword => tr!("Rcon password"),
            Field::HideRconPassword => tr!("Hide Rcon password"),
            Field::AutoUpdateStatus => tr!("Auto Update Status"),
            Field::StatusInterval => tr!("Status interval (seconds)"),
        }
    }

    /// Whether the field is ticked when the export wizard opens: everything but the RCON
    /// password, which would end up in the file in plain text.
    pub fn exported_by_default(self) -> bool {
        self != Field::RconPassword
    }

    fn from_key(key: &str) -> Option<Field> {
        let key = key.trim().to_ascii_lowercase();
        Field::ALL.into_iter().find(|f| f.key() == key)
    }

    /// Whether `server` has a value for this field.
    fn present(self, s: &SavedServer) -> bool {
        match self {
            Field::Game => s.game.is_some(),
            Field::Comment => !s.comment.is_empty(),
            Field::Params => !s.params.is_empty(),
            Field::RconPassword => s.rcon_password.is_some(),
            Field::HideRconPassword => s.hide_rcon_password.is_some(),
            Field::AutoUpdateStatus => s.auto_update_status.is_some(),
            Field::StatusInterval => s.status_interval.is_some(),
        }
    }

    fn clear(self, s: &mut SavedServer) {
        match self {
            Field::Game => s.game = None,
            Field::Comment => s.comment.clear(),
            Field::Params => s.params.clear(),
            Field::RconPassword => s.rcon_password = None,
            Field::HideRconPassword => s.hide_rcon_password = None,
            Field::AutoUpdateStatus => s.auto_update_status = None,
            Field::StatusInterval => s.status_interval = None,
        }
    }

    fn csv_value(self, s: &SavedServer) -> String {
        let flag = |b: Option<bool>| b.map(|b| b.to_string()).unwrap_or_default();
        match self {
            Field::Game => s.game.clone().unwrap_or_default(),
            Field::Comment => s.comment.clone(),
            Field::Params => s.params.clone(),
            Field::RconPassword => s.rcon_password.clone().unwrap_or_default(),
            Field::HideRconPassword => flag(s.hide_rcon_password),
            Field::AutoUpdateStatus => flag(s.auto_update_status),
            Field::StatusInterval => s.status_interval.map(|n| n.to_string()).unwrap_or_default(),
        }
    }

    /// Sets the field from a CSV value; an empty value leaves it unset.
    fn set_csv(self, s: &mut SavedServer, value: &str) -> Result<(), ()> {
        let value = value.trim();
        if value.is_empty() {
            return Ok(());
        }
        match self {
            Field::Game => s.game = Some(value.to_string()),
            Field::Comment => s.comment = value.to_string(),
            Field::Params => s.params = value.to_string(),
            // Passwords may have spaces at the ends.
            Field::RconPassword => s.rcon_password = Some(value.to_string()),
            Field::HideRconPassword => s.hide_rcon_password = Some(parse_bool(value)?),
            Field::AutoUpdateStatus => s.auto_update_status = Some(parse_bool(value)?),
            Field::StatusInterval => s.status_interval = Some(value.parse().map_err(|_| ())?),
        }
        Ok(())
    }
}

fn parse_bool(value: &str) -> Result<bool, ()> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "y" | "on" | "x" | "ja" | "j" => Ok(true),
        "false" | "0" | "no" | "n" | "off" | "nein" | "-" => Ok(false),
        _ => Err(()),
    }
}

/// Header of an openHLSW JSON export.
#[derive(Serialize, Deserialize)]
struct ExportFile {
    openhlsw_servers: u32,
    lists: Vec<SavedList>,
}

/// The servers with only the chosen fields (and the address).
fn keep_fields(server: &SavedServer, fields: &[Field]) -> SavedServer {
    let mut s = SavedServer {
        address: server.address.clone(),
        ..Default::default()
    };
    for &f in fields {
        match f {
            Field::Game => s.game.clone_from(&server.game),
            Field::Comment => s.comment.clone_from(&server.comment),
            Field::Params => s.params.clone_from(&server.params),
            Field::RconPassword => s.rcon_password.clone_from(&server.rcon_password),
            Field::HideRconPassword => s.hide_rcon_password = server.hide_rcon_password,
            Field::AutoUpdateStatus => s.auto_update_status = server.auto_update_status,
            Field::StatusInterval => s.status_interval = server.status_interval,
        }
    }
    s
}

pub fn export_json(lists: &[SavedList], fields: &[Field]) -> String {
    let file = ExportFile {
        openhlsw_servers: 1,
        lists: lists
            .iter()
            .map(|l| SavedList {
                name: l.name.clone(),
                servers: l.servers.iter().map(|s| keep_fields(s, fields)).collect(),
            })
            .collect(),
    };
    serde_json::to_string_pretty(&file).unwrap_or_default() + "\n"
}

/// CSV with a `list` column when more than one list is exported.
pub fn export_csv(lists: &[SavedList], fields: &[Field]) -> String {
    let with_list = lists.len() > 1;
    let mut header: Vec<&str> = Vec::new();
    if with_list {
        header.push("list");
    }
    header.push("address");
    header.extend(fields.iter().map(|f| f.key()));
    let mut out = csv_row(header.iter().copied());
    for list in lists {
        for s in &list.servers {
            let mut row: Vec<String> = Vec::new();
            if with_list {
                row.push(list.name.clone());
            }
            row.push(s.address.clone());
            row.extend(fields.iter().map(|f| f.csv_value(s)));
            out += &csv_row(row.iter().map(String::as_str));
        }
    }
    out
}

/// One CSV line: every value quoted (empty = `""`), separated by semicolons.
fn csv_row<'a>(values: impl Iterator<Item = &'a str>) -> String {
    let mut line = values
        .map(|v| format!("\"{}\"", v.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(";");
    line.push_str("\r\n");
    line
}

/// File format of an import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Csv,
    /// One address per line, optionally followed by a comment.
    Text,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Json => "JSON",
            Format::Csv => "CSV",
            Format::Text => tr!("Text"),
        }
    }
}

/// A read import file.
#[derive(Debug, Clone)]
pub struct Imported {
    pub format: Format,
    /// The servers by server list; the name is `None` where the file names no list.
    pub lists: Vec<(Option<String>, Vec<SavedServer>)>,
    /// Fields that at least one server has a value for.
    pub fields: Vec<Field>,
    /// Rows / entries that were left out (no valid address, bad value), with a reason.
    pub skipped: Vec<String>,
}

impl Imported {
    pub fn server_count(&self) -> usize {
        self.lists.iter().map(|(_, s)| s.len()).sum()
    }

    pub fn has_list_names(&self) -> bool {
        self.lists.iter().any(|(n, _)| n.is_some())
    }

    /// The servers with only the chosen fields kept.
    pub fn filtered(&self, fields: &[Field]) -> Vec<(Option<String>, Vec<SavedServer>)> {
        self.lists
            .iter()
            .map(|(name, servers)| {
                let servers = servers
                    .iter()
                    .map(|s| {
                        let mut s = s.clone();
                        for f in Field::ALL {
                            if !fields.contains(&f) {
                                f.clear(&mut s);
                            }
                        }
                        s
                    })
                    .collect();
                (name.clone(), servers)
            })
            .collect()
    }
}

/// Reads an import file. `csv` = the file name ends in `.csv` (CSV is also detected by
/// its header row). Errors are for the user.
pub fn import(text: &str, csv: bool) -> Result<Imported, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return import_json(text);
    }
    if csv || looks_like_csv_header(text) {
        return import_csv(text);
    }
    import_text(text)
}

fn import_json(text: &str) -> Result<Imported, String> {
    let lists: Vec<(Option<String>, Vec<SavedServer>)> =
        if let Ok(file) = serde_json::from_str::<ExportFile>(text) {
            file.lists
                .into_iter()
                .map(|l| (Some(l.name), l.servers))
                .collect()
        } else if let Ok(list) = serde_json::from_str::<SavedList>(text) {
            vec![(Some(list.name), list.servers)]
        } else {
            match serde_json::from_str::<Vec<SavedServer>>(text) {
                Ok(servers) => vec![(None, servers)],
                Err(e) => return Err(tr!("Not an openHLSW server list: {e}", e = e)),
            }
        };
    let mut skipped = Vec::new();
    let lists = lists
        .into_iter()
        .map(|(name, servers)| {
            let servers = servers
                .into_iter()
                .filter_map(|mut s| match ServerAddress::parse(&s.address) {
                    Some(a) => {
                        s.address = a.to_string();
                        Some(s)
                    }
                    None => {
                        skipped.push(tr!("Invalid address: {address}", address = s.address));
                        None
                    }
                })
                .collect();
            (name.filter(|n| !n.trim().is_empty()), servers)
        })
        .collect();
    Ok(finish(Format::Json, lists, skipped))
}

/// The first line names the `address` column.
fn looks_like_csv_header(text: &str) -> bool {
    let first = text.lines().next().unwrap_or_default();
    let sep = separator(first);
    split_header(first, sep)
        .iter()
        .any(|c| c.eq_ignore_ascii_case("address"))
}

/// Semicolon; a comma only for files without any semicolon in the header.
fn separator(header: &str) -> char {
    if !header.contains(';') && header.contains(',') {
        ','
    } else {
        ';'
    }
}

fn split_header(line: &str, sep: char) -> Vec<String> {
    parse_csv(line, sep)
        .into_iter()
        .next()
        .map(|(_, row)| row.into_iter().map(|c| c.trim().to_string()).collect())
        .unwrap_or_default()
}

/// Splits CSV text into rows (with their line number) of values: quotes around a value
/// are optional, `""` inside quotes is a quote, quoted values may span lines.
fn parse_csv(text: &str, sep: char) -> Vec<(usize, Vec<String>)> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut value = String::new();
    let mut quoted = false;
    let mut line = 1;
    let mut row_line = 1;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    value.push('"');
                }
                '"' => quoted = false,
                _ => {
                    if c == '\n' {
                        line += 1;
                    }
                    value.push(c);
                }
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            c if c == sep => row.push(std::mem::take(&mut value)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut value));
                if !(row.len() == 1 && row[0].trim().is_empty()) {
                    rows.push((row_line, std::mem::take(&mut row)));
                }
                row.clear();
                line += 1;
                row_line = line;
            }
            _ => value.push(c),
        }
    }
    if !value.is_empty() || !row.is_empty() {
        row.push(value);
        if !(row.len() == 1 && row[0].trim().is_empty()) {
            rows.push((row_line, row));
        }
    }
    rows
}

enum Column {
    List,
    Address,
    Field(Field),
    Unknown,
}

fn import_csv(text: &str) -> Result<Imported, String> {
    let sep = separator(text.lines().next().unwrap_or_default());
    let mut rows = parse_csv(text, sep).into_iter();
    let Some((_, header)) = rows.next() else {
        return Err(tr!("The file is empty.").into());
    };
    let columns: Vec<Column> = header
        .iter()
        .map(|h| {
            let h = h.trim();
            if h.eq_ignore_ascii_case("list") {
                Column::List
            } else if h.eq_ignore_ascii_case("address") {
                Column::Address
            } else {
                Field::from_key(h).map_or(Column::Unknown, Column::Field)
            }
        })
        .collect();
    if !columns.iter().any(|c| matches!(c, Column::Address)) {
        return Err(tr!("The first row must name the columns, with a column \"address\".").into());
    }
    let mut lists: Vec<(Option<String>, Vec<SavedServer>)> = Vec::new();
    let mut skipped = Vec::new();
    'rows: for (line, row) in rows {
        let mut s = SavedServer::default();
        let mut list = None;
        for (col, value) in columns.iter().zip(&row) {
            match col {
                Column::List => list = Some(value.trim().to_string()).filter(|v| !v.is_empty()),
                Column::Address => s.address = value.trim().to_string(),
                Column::Field(f) => {
                    if f.set_csv(&mut s, value).is_err() {
                        skipped.push(tr!(
                            "Line {line}: invalid value \"{value}\" for {column}",
                            line = line,
                            value = value,
                            column = f.key()
                        ));
                        continue 'rows;
                    }
                }
                Column::Unknown => {}
            }
        }
        let Some(address) = ServerAddress::parse(&s.address) else {
            skipped.push(tr!(
                "Line {line}: invalid address \"{address}\"",
                line = line,
                address = s.address
            ));
            continue;
        };
        s.address = address.to_string();
        match lists.iter_mut().find(|(n, _)| *n == list) {
            Some((_, servers)) => servers.push(s),
            None => lists.push((list, vec![s])),
        }
    }
    Ok(finish(Format::Csv, lists, skipped))
}

fn import_text(text: &str) -> Result<Imported, String> {
    let servers = parse_text_list(text);
    if servers.is_empty() {
        return Err(tr!("No server addresses found in the file.").into());
    }
    Ok(finish(Format::Text, vec![(None, servers)], Vec::new()))
}

/// One address per line (`host:port [comment]`; `#` / `//` lines are ignored; commas and
/// semicolons separate addresses too). Also used by Multi Server Insert.
pub fn parse_text_list(text: &str) -> Vec<SavedServer> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        // "a:1, b:2" or "a:1 comment"
        if line.contains([',', ';']) {
            for part in line.split([',', ';']) {
                if let Some(addr) = ServerAddress::parse(part.trim()) {
                    out.push(SavedServer {
                        address: addr.to_string(),
                        ..Default::default()
                    });
                }
            }
            continue;
        }
        let (first, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        if let Some(addr) = ServerAddress::parse(first) {
            out.push(SavedServer {
                address: addr.to_string(),
                comment: rest.trim().to_string(),
                ..Default::default()
            });
        }
    }
    out
}

fn finish(
    format: Format,
    lists: Vec<(Option<String>, Vec<SavedServer>)>,
    skipped: Vec<String>,
) -> Imported {
    let lists: Vec<_> = lists.into_iter().filter(|(_, s)| !s.is_empty()).collect();
    let fields = Field::ALL
        .into_iter()
        .filter(|f| lists.iter().flat_map(|(_, s)| s).any(|s| f.present(s)))
        .collect();
    Imported {
        format,
        lists,
        fields,
        skipped,
    }
}

/// Puts imported servers into a saved (not shown) server list. Servers already in it are
/// left alone, or with `overwrite` get the imported values (only those the file has).
/// Returns (added, updated).
pub fn merge_into(
    list: &mut Vec<SavedServer>,
    servers: &[SavedServer],
    overwrite: bool,
) -> (usize, usize) {
    let (mut added, mut updated) = (0, 0);
    for new in servers {
        let same = |s: &&mut SavedServer| {
            ServerAddress::parse(&s.address)
                .map(|a| a.to_string())
                .as_ref()
                == Some(&new.address)
        };
        match list.iter_mut().find(same) {
            Some(old) => {
                if overwrite {
                    update_saved(old, new);
                    updated += 1;
                }
            }
            None => {
                // New servers: Hide and Auto Update Status on unless the file says otherwise.
                let mut s = new.clone();
                s.hide_rcon_password.get_or_insert(true);
                s.auto_update_status.get_or_insert(true);
                list.push(s);
                added += 1;
            }
        }
    }
    (added, updated)
}

/// Copies the values `new` has onto `old`.
fn update_saved(old: &mut SavedServer, new: &SavedServer) {
    for f in Field::ALL {
        if f.present(new) {
            match f {
                Field::Game => old.game.clone_from(&new.game),
                Field::Comment => old.comment.clone_from(&new.comment),
                Field::Params => old.params.clone_from(&new.params),
                Field::RconPassword => old.rcon_password.clone_from(&new.rcon_password),
                Field::HideRconPassword => old.hide_rcon_password = new.hide_rcon_password,
                Field::AutoUpdateStatus => old.auto_update_status = new.auto_update_status,
                Field::StatusInterval => old.status_interval = new.status_interval,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(address: &str) -> SavedServer {
        SavedServer {
            address: address.into(),
            game: Some("cs2".into()),
            rcon_password: Some("example-secret".into()),
            comment: "Say \"hi\"; ok".into(),
            params: String::new(),
            hide_rcon_password: Some(true),
            auto_update_status: Some(false),
            status_interval: Some(45),
            ..Default::default()
        }
    }

    fn lists() -> Vec<SavedList> {
        vec![
            SavedList {
                name: "Main".into(),
                servers: vec![server("203.0.113.10:27015")],
            },
            SavedList {
                name: "Test".into(),
                servers: vec![server("198.51.100.7:27016")],
            },
        ]
    }

    fn default_fields() -> Vec<Field> {
        Field::ALL
            .into_iter()
            .filter(|f| f.exported_by_default())
            .collect()
    }

    #[test]
    fn json_leaves_out_unchosen_fields() {
        let json = export_json(&lists(), &default_fields());
        assert!(!json.contains("\"rcon_password\""));
        assert!(!json.contains("example-secret"));
        assert!(json.contains("\"hide_rcon_password\": true"));
        assert!(json.contains("\"status_interval\": 45"));

        let json = export_json(&lists(), &Field::ALL);
        assert!(json.contains("example-secret"));
    }

    #[test]
    fn json_round_trip() {
        let json = export_json(&lists(), &Field::ALL);
        let imported = import(&json, false).unwrap();
        assert_eq!(imported.format, Format::Json);
        assert_eq!(imported.lists.len(), 2);
        assert_eq!(imported.lists[1].0.as_deref(), Some("Test"));
        let s = &imported.lists[0].1[0];
        assert_eq!(s.rcon_password.as_deref(), Some("example-secret"));
        assert_eq!(s.comment, "Say \"hi\"; ok");
        assert_eq!(s.auto_update_status, Some(false));
        // Params is empty for all servers, so it is not offered.
        assert_eq!(imported.fields.len(), Field::ALL.len() - 1);
        assert!(!imported.fields.contains(&Field::Params));
    }

    #[test]
    fn json_old_formats() {
        let list = r#"{"name":"Old","servers":[{"address":"203.0.113.10:27015"},{"address":"not an address"}]}"#;
        let imported = import(list, false).unwrap();
        assert_eq!(imported.lists[0].0.as_deref(), Some("Old"));
        assert_eq!(imported.server_count(), 1);
        assert_eq!(imported.skipped.len(), 1);

        let bare = r#"[{"address":"203.0.113.10:27015","comment":"x"}]"#;
        let imported = import(bare, false).unwrap();
        assert_eq!(imported.lists[0].0, None);
        assert_eq!(imported.fields, vec![Field::Comment]);

        assert!(import("{ broken", false).is_err());
    }

    #[test]
    fn csv_quotes_every_value() {
        let csv = export_csv(&lists()[..1], &default_fields());
        let mut lines = csv.lines();
        assert_eq!(
            lines.next(),
            Some(
                "\"address\";\"game\";\"comment\";\"params\";\"hide_rcon_password\";\
                 \"auto_update_status\";\"status_interval\""
            )
        );
        assert_eq!(
            lines.next(),
            Some(
                "\"203.0.113.10:27015\";\"cs2\";\"Say \"\"hi\"\"; ok\";\"\";\"true\";\"false\";\"45\""
            )
        );
        // More than one list: a list column first.
        assert!(export_csv(&lists(), &[]).starts_with("\"list\";\"address\"\r\n\"Main\";"));
    }

    #[test]
    fn csv_round_trip() {
        let csv = export_csv(&lists(), &Field::ALL);
        let imported = import(&csv, true).unwrap();
        assert_eq!(imported.format, Format::Csv);
        assert_eq!(imported.lists.len(), 2);
        assert_eq!(imported.lists[0].0.as_deref(), Some("Main"));
        let s = &imported.lists[0].1[0];
        let want = server("203.0.113.10:27015");
        assert_eq!(s.comment, want.comment);
        assert_eq!(s.rcon_password, want.rcon_password);
        assert_eq!(s.hide_rcon_password, Some(true));
        assert_eq!(s.auto_update_status, Some(false));
        assert_eq!(s.status_interval, Some(45));
    }

    #[test]
    fn csv_by_hand() {
        // Header detected without the .csv extension, unquoted values, unknown columns
        // ignored, missing trailing values, a value over two lines, bad rows reported.
        let text = "Address;Comment;Name;auto_update_status\r\n\
                    203.0.113.10:27015;first;x;ja\n\
                    \n\
                    198.51.100.7;\"two\nlines\"\n\
                    not valid:x;;;\n\
                    203.0.113.11:27015;;;maybe\n";
        let imported = import(text, false).unwrap();
        assert_eq!(imported.format, Format::Csv);
        let servers = &imported.lists[0].1;
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].auto_update_status, Some(true));
        assert_eq!(servers[1].comment, "two\nlines");
        assert_eq!(servers[1].auto_update_status, None);
        assert_eq!(imported.skipped.len(), 2);
        assert!(imported.skipped[0].contains('6'), "{:?}", imported.skipped);

        // Comma separated, no address column.
        let comma = import("list,address\nA,203.0.113.10:27015\n", true).unwrap();
        assert_eq!(comma.lists[0].0.as_deref(), Some("A"));
        assert!(import("host;port\n203.0.113.10;27015\n", true).is_err());
    }

    #[test]
    fn text_lists_still_work() {
        let imported = import("# my servers\n203.0.113.10:27015 Clan server\n", false).unwrap();
        assert_eq!(imported.format, Format::Text);
        assert_eq!(imported.lists[0].1[0].comment, "Clan server");
        assert!(import("# only a comment\n", false).is_err());
    }

    #[test]
    fn filtered_drops_unchosen_fields() {
        let json = export_json(&lists(), &Field::ALL);
        let imported = import(&json, false).unwrap();
        let lists = imported.filtered(&[Field::Comment]);
        let s = &lists[0].1[0];
        assert_eq!(s.rcon_password, None);
        assert_eq!(s.game, None);
        assert_eq!(s.hide_rcon_password, None);
        assert!(!s.comment.is_empty());
    }

    #[test]
    fn merge_into_saved_list() {
        let mut list = vec![SavedServer {
            address: "203.0.113.10:27015".into(),
            comment: "keep".into(),
            ..Default::default()
        }];
        let new = vec![
            SavedServer {
                address: "203.0.113.10:27015".into(),
                status_interval: Some(60),
                ..Default::default()
            },
            SavedServer {
                address: "198.51.100.7:27015".into(),
                ..Default::default()
            },
        ];
        assert_eq!(merge_into(&mut list, &new, false), (1, 0));
        assert_eq!(list[0].status_interval, None);
        assert_eq!(list[1].hide_rcon_password, Some(true));
        assert_eq!(merge_into(&mut list, &new, true), (0, 2));
        assert_eq!(list[0].status_interval, Some(60));
        assert_eq!(list[0].comment, "keep");
    }
}
