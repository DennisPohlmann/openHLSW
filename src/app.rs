//! The openHLSW main window, laid out like HLSW 1.4:
//!
//! ```text
//! ┌ menu ─────────────────────────────────────────────────────────┐
//! │ Server list: [Custom server list]                  N servers  │
//! │ IP/Port: [Half-Life 2] [address                             ] │
//! │ toolbar · Password · Search                                   │
//! ├ server list ──────────────────────────────────────────────────┤
//! ├ server info ───────┬ players ─────────────────────────────────┤
//! │                    ├ rules (key / value) ─────────────────────┤
//! ├ Rcon Config / Console content ────────────────────────────────┤
//! │ Rcon Config | Console | Chat | …                              │
//! └ status bar ───────────────────────────────────────────────────┘
//! ```

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Button, Color32, DragValue, Frame, Key, Label, Layout, Margin, RichText,
    ScrollArea, Sense, Stroke, StrokeKind, TextEdit, TextStyle, TextWrapMode, Ui, WidgetText,
};
use egui_extras::{Column, TableBuilder, TableRow};

use openhlsw::a2s::{Player, QueryMeta, Rule, ServerInfo};
use openhlsw::address::ServerAddress;
use openhlsw::steamid::SteamId;

use crate::config::{self, Config, SavedList, SavedServer};
use crate::discover;
use crate::flags::Flags;
use crate::game_icons::{self, GameIcons};
use crate::games;
use crate::geo::{self, GeoInfo};
use crate::iddb::{self, IdDb, IdEntry, IdKind};
use crate::native_window::NativeWindow;
use crate::settings::{
    DialogResult, OnConnect, Settings, SettingsDialog, SettingsTab, SoundEvent, StartupSelection,
};
use crate::theme::{self, Palette, Theme};
use crate::worker::{
    self, Event, Notifier, QueryKind, RconEvent, RconHandle, RconRequest, RconStatus, RconTag,
    ServerId,
};

/// Rules (cvars) change rarely; they are queried less often than info/players.
const RULES_REFRESH: Duration = Duration::from_secs(10);
/// A server that has not answered for this long is shown as offline (red line).
const OFFLINE_AFTER: Duration = Duration::from_secs(10);
const CHAT_MAX_LINES: usize = 1000;
/// Samples kept for the ping graph (about two minutes at the default interval).
const PING_HISTORY: usize = 300;
const ROW_HEIGHT: f32 = 17.0;
const HEADER_HEIGHT: f32 = 20.0;

// --------------------------------------------------------------------------
// Server state
// --------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum QueryState {
    Unknown,
    Online,
    Offline(String),
}

struct Server {
    id: ServerId,
    address: ServerAddress,
    comment: String,
    /// Extra start parameters for Connect (Server Properties).
    params: String,
    info: Option<ServerInfo>,
    ping: Option<u32>,
    /// Ping per info query, `None` for a timeout (drawn as a red bar).
    ping_history: VecDeque<Option<u32>>,
    /// Kills, deaths, team, damage … per player name, from the server log.
    log_players: HashMap<String, LogPlayer>,
    state: QueryState,
    players: Vec<Player>,
    rules: Vec<Rule>,
    /// In-flight queries: info, players, rules.
    busy: [bool; 3],
    rcon_password: String,
    save_password: bool,
    rcon_status: RconStatus,
    rcon: RconHandle,
    console: VecDeque<ConsoleLine>,
    console_cache: usize,
    resolved: Option<SocketAddr>,
    /// Last answer, or when the server was added / the program started.
    last_ok: Instant,
    last_seen: Option<chrono::DateTime<chrono::Local>>,
    /// When we last joined this server via Connect (saved in the config).
    last_connected: Option<String>,
    /// Last A2S challenge the server accepted (reused to save a round trip).
    challenge: Option<[u8; 4]>,
    /// Token bucket for the per-server query limit (srcds sv_max_queries_sec).
    budget: f32,
    /// 0.25 … 1.0: query rate is halved after each timeout and recovers slowly, so a server
    /// (or a firewall in front of it) that drops our queries gets fewer of them.
    rate_factor: f32,
    budget_at: Instant,
    last_rules: Option<Instant>,
    bans: Vec<BanEntry>,
    ban_selected: Option<usize>,
    ban_filter: String,
    maps: Vec<String>,
    map_selected: Option<String>,
    /// Shown in the Change Level tab ("Loading…", "42 maps", errors).
    maps_status: String,
    maps_loading: bool,
    /// Map to change to once the players have been kicked.
    pending_change: Option<String>,
    /// Players from RCON `status` (Steam ID, IP, ping, loss).
    status_players: Vec<StatusPlayer>,
    last_status: Option<Instant>,
    /// Last automatic Get Log request (Auto Update Status).
    log_requested: Option<Instant>,
    chat: Vec<ChatLine>,
    /// Where the server currently sends its log, and whether it was the Internet variant.
    log_target: Option<(SocketAddr, bool)>,
}

#[derive(Clone)]
struct BanEntry {
    is_ip: bool,
    ident: String,
    duration: String,
}

/// One player line of RCON `status`.
#[derive(Clone, Debug, PartialEq)]
struct StatusPlayer {
    userid: u32,
    name: String,
    steam_id: String,
    connected: String,
    ping: String,
    loss: String,
    state: String,
    ip: Option<IpAddr>,
    port: Option<u16>,
    /// Server slot (CS:GO / HLDS `status`).
    slot: Option<u32>,
    /// Network rate (CS:GO / CS2 `status`).
    rate: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LineKind {
    /// Command we sent (dark highlight).
    Command,
    /// Server answer to a command (light highlight, hidden with "F").
    Response,
    /// Server log line (logaddress_add).
    Log,
    /// Message from openHLSW itself.
    Note,
}

struct ConsoleLine {
    time: chrono::DateTime<chrono::Local>,
    kind: LineKind,
    automatic: bool,
    text: String,
}

/// Per player statistics from the server log (since the current map started).
#[derive(Clone, Debug, Default, PartialEq)]
struct LogPlayer {
    team: String,
    kills: u32,
    deaths: u32,
    teamkills: u32,
    suicides: u32,
    damage_hp: u32,
    damage_armor: u32,
    /// Health and armor after the last hit taken.
    health: Option<(i32, i32)>,
}

#[derive(Clone)]
struct ChatLine {
    time: String,
    name: String,
    team: String,
    team_only: bool,
    text: String,
    /// Sent from openHLSW (Message to all players) and not yet echoed by the server log.
    own: bool,
}

impl Server {
    fn new(id: ServerId, address: ServerAddress, notifier: &Notifier) -> Self {
        Self {
            id,
            address,
            comment: String::new(),
            params: String::new(),
            info: None,
            ping: None,
            ping_history: VecDeque::new(),
            log_players: HashMap::new(),
            state: QueryState::Unknown,
            players: Vec::new(),
            rules: Vec::new(),
            busy: [false; 3],
            rcon_password: String::new(),
            save_password: false,
            rcon_status: RconStatus::Unknown,
            rcon: RconHandle::spawn(notifier, id),
            console: VecDeque::new(),
            console_cache: 1000,
            resolved: None,
            last_ok: Instant::now(),
            last_seen: None,
            last_connected: None,
            challenge: None,
            budget: 2.0,
            rate_factor: 1.0,
            budget_at: Instant::now(),
            last_rules: None,
            bans: Vec::new(),
            ban_selected: None,
            ban_filter: String::new(),
            maps: Vec::new(),
            map_selected: None,
            maps_status: String::new(),
            maps_loading: false,
            pending_change: None,
            status_players: Vec::new(),
            last_status: None,
            log_requested: None,
            chat: Vec::new(),
            log_target: None,
        }
    }

    /// Accounts for the packets a query really needed (challenge round trips, retries)
    /// and remembers the accepted challenge.
    fn charge(&mut self, meta: &QueryMeta) {
        self.budget -= meta.packets.saturating_sub(1) as f32;
        if meta.challenge.is_some() {
            self.challenge = meta.challenge;
        }
    }

    /// Source and Half-Life 1 servers have RCON; GameSpy servers (ip:port:queryport) not.
    fn supports_rcon(&self) -> bool {
        self.address.query_port.is_none() && !self.info.as_ref().is_some_and(|i| i.gamespy)
    }

    fn is_offline(&self) -> bool {
        matches!(self.state, QueryState::Offline(_))
    }

    fn rule(&self, key: &str) -> Option<&str> {
        self.rules
            .iter()
            .find(|r| r.key.eq_ignore_ascii_case(key))
            .map(|r| r.value.as_str())
    }

    fn push_ping(&mut self, ping: Option<u32>) {
        self.ping_history.push_back(ping);
        while self.ping_history.len() > PING_HISTORY {
            self.ping_history.pop_front();
        }
    }

    fn avg_ping(&self) -> Option<f32> {
        let (sum, n) = self
            .ping_history
            .iter()
            .flatten()
            .fold((0u32, 0u32), |(s, n), &p| (s + p, n + 1));
        (n > 0).then(|| sum as f32 / n as f32)
    }

    /// "34, Ø42.2" / "Timeout, Ø61.5" like HLSW.
    fn ping_text(&self) -> String {
        let avg = self
            .avg_ping()
            .map(|a| format!(", Ø{a:.1}"))
            .unwrap_or_default();
        match (&self.state, self.ping) {
            (QueryState::Online, Some(p)) => format!("{p}{avg}"),
            (QueryState::Offline(reason), _) if reason == "Timeout" => {
                format!("{}{avg}", tr!("Timeout"))
            }
            (QueryState::Offline(reason), _) => reason.clone(),
            _ => String::new(),
        }
    }

    /// "HL2 - CS:S" style short game name.
    fn game_short(&self) -> String {
        let Some(i) = &self.info else {
            return String::new();
        };
        if i.gamespy {
            // Like HLSW: "BF2 - Special Forces Expansion (gpm_coop)", "BF2 (gpm_cq)".
            let folder = i.folder.to_ascii_lowercase();
            let (gamename, mod_dir) = folder.split_once('/').unwrap_or((folder.as_str(), ""));
            let short = match gamename {
                "battlefield2" => Some("BF2"),
                "stella" => Some("BF2142"),
                _ => None,
            };
            let mut text = match short {
                Some(short) => {
                    let m = openhlsw::gamespy2::bf_mod_name(mod_dir);
                    if m.is_empty() {
                        short.to_string()
                    } else {
                        format!("{short} - {m}")
                    }
                }
                None => i.game.clone(),
            };
            if let Some(t) = self.rule("gametype").filter(|t| !t.is_empty()) {
                text.push_str(&format!(" ({t})"));
            }
            return text;
        }
        if i.goldsrc {
            let short = match i.folder.to_ascii_lowercase().as_str() {
                "cstrike" => "CS",
                "czero" => "CZ",
                "dod" => "DoD",
                "tfc" => "TFC",
                "dmc" => "DMC",
                "valve" => "DM",
                "gearbox" => "OpFor",
                "ricochet" => "Ricochet",
                "ns" => "NS",
                "ts" => "TS",
                _ => i.game.as_str(),
            };
            return format!("HL - {short}");
        }
        let short = match i.real_app_id() {
            240 => "CS:S",
            730 => return "CS2".into(),
            440 => "TF2",
            320 => "DM",
            300 => "DoD:S",
            4000 => "GMod",
            500 => return "L4D".into(),
            550 => return "L4D2".into(),
            _ => i.game.as_str(),
        };
        format!("HL2 - {short}")
    }

    fn players_text(&self) -> String {
        let Some(i) = &self.info else {
            return String::new();
        };
        if i.bots > 0 {
            format!(
                "{} ({}) / {}",
                i.players.saturating_sub(i.bots),
                i.players,
                i.max_players
            )
        } else {
            format!("{} / {}", i.players, i.max_players)
        }
    }

    fn humans(&self) -> u8 {
        self.info
            .as_ref()
            .map_or(0, |i| i.players.saturating_sub(i.bots))
    }

    /// Plugins detected from public cvars: (short, long) names like HLSW shows them.
    fn addon_list(&self) -> Vec<(String, String)> {
        const SOURCE: [(&str, &str, &str); 5] = [
            ("metamod_version", "MM:S", "MetaMod:Source"),
            ("sourcemod_version", "SMod", "SourceMod"),
            ("stripper_version", "STRP", "Stripper:Source"),
            ("mani_admin_plugin_version", "Mani", "Mani Admin Plugin"),
            ("eventscripts_ver", "ES", "EventScripts"),
        ];
        // Half-Life 1 (order like HLSW: "AMXX v1.8.2, ATAC v3.0.1, PTB v1.8b3, MM v1.21.1-am").
        const GOLDSRC: [(&str, &str, &str); 7] = [
            ("amxmodx_version", "AMXX", "AMX Mod X"),
            ("amx_version", "AMX", "AMX Mod"),
            ("atac_version", "ATAC", "Advanced Team Attack Control"),
            ("amx_ptb_version", "PTB", "Ptahhotep's Team Balancer"),
            ("adminmod_version", "AM", "Admin Mod"),
            ("dproto_version", "DPROTO", "dproto"),
            ("metamod_version", "MM", "Metamod"),
        ];
        let known: &[(&str, &str, &str)] = if self.is_goldsrc() { &GOLDSRC } else { &SOURCE };
        known
            .iter()
            .filter_map(|(key, short, long)| {
                self.rule(key)
                    .map(|v| (format!("{short} v{v}"), format!("{long} v{v}")))
            })
            .collect()
    }

    fn addons_short(&self) -> String {
        if self.is_gamespy() {
            // Battlefield 2 / 2142: built-in voice chat, like HLSW shows it.
            return match self.rule("bf2_voip").or(self.rule("bf2142_voip")) {
                Some("1") => "VOIP".into(),
                Some(_) => tr!("None").into(),
                None => String::new(),
            };
        }
        if self.rules.is_empty() {
            return String::new();
        }
        let list = self.addon_list();
        if list.is_empty() {
            return tr!("None").into();
        }
        list.into_iter()
            .map(|(s, _)| s)
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// "MM:S 1.12.0, SM 1.13.0.7427, HLX:CE 1.7.0" for the info panel.
    fn addons_compact(&self) -> String {
        if self.is_gamespy() {
            return self.addons_short();
        }
        if self.is_goldsrc() {
            return self
                .addon_list()
                .into_iter()
                .map(|(s, _)| s)
                .collect::<Vec<_>>()
                .join(", ");
        }
        const KNOWN: [(&str, &str); 6] = [
            ("metamod_version", "MM:S"),
            ("sourcemod_version", "SM"),
            ("stripper_version", "Stripper"),
            ("hlxce_version", "HLX:CE"),
            ("mani_admin_plugin_version", "Mani"),
            ("eventscripts_ver", "ES"),
        ];
        KNOWN
            .iter()
            .filter_map(|(key, name)| self.rule(key).map(|v| format!("{name} {v}")))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn ranking(&self) -> (String, String) {
        if let Some(v) = self.rule("hlxce_version") {
            return (format!("HLX:CE v{v}"), format!("HLstatsX:CE v{v}"));
        }
        if self.rules.is_empty() || self.is_gamespy() {
            (String::new(), String::new())
        } else {
            (tr!("None").into(), tr!("None").into())
        }
    }

    /// "VAC", plus server side anti-cheat plugins found in the rules (HLGuard, sXe, ...).
    fn anticheat(&self) -> String {
        let Some(i) = &self.info else {
            return String::new();
        };
        if i.gamespy {
            // Battlefield 2 / 2142: PunkBuster.
            return match self.rule("bf2_anticheat").or(self.rule("bf2142_anticheat")) {
                Some("1") => "PunkBuster".into(),
                Some(_) => tr!("None").into(),
                None => String::new(),
            };
        }
        const PLUGINS: [(&str, &str); 4] = [
            ("hlg_version", "HLGuard"),
            ("__sxei", "sXe Injected"),
            ("sxei_version", "sXe Injected"),
            ("smac_version", "SMAC"),
        ];
        let mut list: Vec<&str> = Vec::new();
        if i.vac {
            list.push("VAC");
        }
        for (key, name) in PLUGINS {
            if self.rule(key).is_some() && !list.contains(&name) {
                list.push(name);
            }
        }
        if list.is_empty() {
            tr!("None").into()
        } else {
            list.join(", ")
        }
    }

    /// Query limit the server announces in its rules (`sv_max_queries_sec` on srcds,
    /// `max_queries_sec` on HLDS) and the averaging window in seconds.
    fn announced_query_limit(&self) -> Option<(f32, f32)> {
        let num = |keys: [&str; 2]| {
            keys.iter()
                .find_map(|k| self.rule(k))
                .and_then(|v| v.trim().parse::<f32>().ok())
                .filter(|v| *v > 0.0)
        };
        let rate = num(["sv_max_queries_sec", "max_queries_sec"])?;
        let window = num(["sv_max_queries_window", "max_queries_window"]).unwrap_or(30.0);
        Some((rate, window))
    }

    fn limits(&self) -> String {
        let mut parts = Vec::new();
        let mut add = |key: &str, unit: &str| {
            if let Some(v) = self.rule(key).filter(|v| !v.is_empty() && *v != "0") {
                parts.push(format!("{v} {unit}"));
            }
        };
        add("mp_timelimit", tr!("min"));
        add("mp_maxrounds", tr!("rounds"));
        add("mp_winlimit", tr!("wins"));
        add("mp_fraglimit", tr!("frags"));
        if self.is_gamespy() {
            add("timelimit", tr!("min"));
            add("fraglimit", tr!("frags"));
        }
        if parts.is_empty() && !self.rules.is_empty() && !self.is_gamespy() {
            return tr!("None").into();
        }
        parts.join(", ")
    }

    fn version_text(&self) -> String {
        self.info
            .as_ref()
            .filter(|i| !i.version.is_empty())
            .map(|i| format!("v{}", i.version))
            .unwrap_or_default()
    }

    /// Adds console lines. The prefix decides the kind: "] " sent command,
    /// "*** " note from openHLSW, "L " server log line, anything else a note.
    fn append_console(&mut self, text: &str) {
        if let Some(cmd) = text.strip_prefix("] ") {
            self.console_add(LineKind::Command, cmd, false);
        } else if let Some(note) = text.strip_prefix("*** ") {
            self.console_add(LineKind::Note, note, false);
        } else if let Some(log) = text.strip_prefix("L ") {
            self.console_add(LineKind::Log, log, false);
        } else {
            self.console_add(LineKind::Note, text, false);
        }
    }

    /// One entry per line; `automatic` = caused by Auto Update Status (hidden with "A").
    /// Sends an RCON command and shows it in the console; the server's answer is shown there
    /// too, so every command a button triggers can be followed. Periodic `status` counts as
    /// automatic (console switch "A" hides it).
    fn rcon_send(&mut self, command: impl Into<String>, tag: RconTag) {
        let command = command.into();
        self.console_add(LineKind::Command, &command, tag == RconTag::PlayerStatus);
        self.rcon.exec(command, tag);
    }

    fn console_add(&mut self, kind: LineKind, text: &str, automatic: bool) {
        let time = chrono::Local::now();
        for line in text.trim_end_matches(['\n', '\r']).split('\n') {
            self.console.push_back(ConsoleLine {
                time,
                kind,
                automatic,
                text: line.trim_end_matches('\r').to_string(),
            });
        }
        self.trim_console(self.console_cache);
    }

    /// Keeps the last `max_lines` console lines ("Cache size" in the Rcon settings).
    fn trim_console(&mut self, max_lines: usize) {
        self.console_cache = max_lines.max(1);
        while self.console.len() > self.console_cache {
            self.console.pop_front();
        }
    }

    fn configure_rcon(&self) {
        self.rcon.send(RconRequest::Configure {
            address: self.address.clone(),
            password: self.rcon_password.clone(),
            goldsrc: self.is_goldsrc(),
        });
    }

    fn is_gamespy(&self) -> bool {
        self.info.as_ref().is_some_and(|i| i.gamespy)
    }

    /// Half-Life 1 engine server (UDP RCON, `kick #userid`, `logaddress_add ip port`).
    fn is_goldsrc(&self) -> bool {
        self.info.as_ref().is_some_and(|i| i.goldsrc)
    }

    /// Kick command for this engine (HLDS has no `kickid`).
    fn kick_cmd(&self, userid: u32, reason: &str) -> String {
        if self.is_goldsrc() {
            format!("kick #{userid} \"{reason}\"")
        } else {
            format!("kickid {userid} \"{reason}\"")
        }
    }

    fn matches(&self, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        let hit = |s: &str| s.to_lowercase().contains(needle);
        hit(&self.address.to_string())
            || hit(&self.comment)
            || self
                .info
                .as_ref()
                .is_some_and(|i| hit(&i.name) || hit(&i.map) || hit(&i.game))
    }
}

// --------------------------------------------------------------------------
// App
// --------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    RconConfig,
    Console,
    Chat,
    BanList,
    ChangeLevel,
}

/// Bottom tabs like HLSW; `None` = not implemented yet (shown greyed out).
const TABS: [(&str, Option<Tab>); 5] = [
    ("Rcon Config", Some(Tab::RconConfig)),
    ("Console", Some(Tab::Console)),
    ("Chat", Some(Tab::Chat)),
    ("Ban List", Some(Tab::BanList)),
    ("Change Level", Some(Tab::ChangeLevel)),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Col {
    Index,
    Address,
    Name,
    Game,
    Map,
    Players,
    Ping,
    Version,
    Location,
    AddOn,
    AntiCheat,
    Ranking,
    Limits,
    Comment,
    /// Sort-only: game, then name / then address (Settings > Sort Server List).
    GameName,
    GameAddress,
}

const SERVER_COLUMNS: [(Col, &str, f32); 14] = [
    (Col::Index, "#", 58.0),
    (Col::Address, "Addr", 130.0),
    (Col::Name, "Name", 285.0),
    (Col::Game, "Game", 130.0),
    (Col::Map, "Map", 145.0),
    (Col::Players, "Players", 62.0),
    (Col::Ping, "Ping", 100.0),
    (Col::Version, "Version", 78.0),
    (Col::Location, "Location", 128.0),
    (Col::AddOn, "AddOn", 320.0),
    (Col::AntiCheat, "Anti Cheat Tool", 72.0),
    (Col::Ranking, "Ranking", 76.0),
    (Col::Limits, "Limits", 160.0),
    (Col::Comment, "Comment", 120.0),
];

/// Players are sorted by the title of a column.
type PlayerCol = &'static str;

enum Action {
    Select(ServerId),
    Add,
    Refresh(ServerId),
    RefreshAll,
    /// Asks first ("Delete … ?"), then `RemoveConfirmed`.
    Remove(ServerId),
    RemoveConfirmed(ServerId),
    /// Connect with HLSW's checks (password needed, waiting for a free slot).
    Connect(ServerId),
    /// Connect although the server wants a password and none is entered.
    ConnectAnyway(ServerId),
    /// Connect right away (no checks).
    ConnectNow(ServerId),
    /// Stop waiting for a free slot.
    CancelWait,
    /// Select the next server of the Find window (F3).
    FindNext,
    /// Toolbar: show / hide the Rcon section, server list or rules.
    ToggleView(crate::toolbar_icons::ToolIcon),
    /// Toolbar: About openHLSW.
    ShowAbout,
    /// Toolbar: switch "Auto refresh every" on / off.
    ToggleAutoRefresh,
    CopyText(String),
    /// Opens a web page in the default browser.
    OpenUrl(String),
    EditComment(ServerId),
    NewList(String),
    OpenList(String),
    DeleteList,
    RenameList(String),
    ImportList,
    /// Server list: add the servers of Steam's favorites / history.
    ImportSteamFavorites,
    /// File → Advanced.
    ClearGeoCache,
    ResetAllPings,
    /// Server list: broadcast for servers in the LAN.
    LanSearch,
    ExportList,
    MultiInsert(String),
    MoveToList(ServerId, String),
    CopyToList(ServerId, String),
    FocusSearch,
    OpenDialog(Dialog),
    ApplyServerProps(ServerId, ServerPropsDraft),
    TraceRoute(ServerId),
    ResetPing(ServerId),
    /// "Rcon Status Command": run `status` and show it in the console.
    RconStatus(ServerId),
    Minimize {
        no_traffic: bool,
    },
    OpenAppData,
    RconConfigure(ServerId),
    RconTest(ServerId),
    RconDisconnect(ServerId),
    SendConsole(ServerId),
    ClearConsole(ServerId),
    SwitchTab(Tab),
    FetchBans(ServerId),
    /// Open the Global Player ID Database Editor, optionally on an ID.
    OpenIdDb(Option<String>),
    /// Player menu: add the player to the ID database, then set the type (`Some`) or open
    /// the editor on it (`None`).
    IdDbSetKind {
        id: String,
        name: String,
        kind: Option<IdKind>,
    },
    IdDbImport,
    IdDbExport,
    /// Ban list: mark the banned IDs of the server as banned in the ID database.
    BansToIdDb(ServerId),
    /// Ban list: ban all IDs marked as banned in the ID database on the server.
    BansFromIdDb(ServerId),
    /// Rcon command on the ban list (write / reload), then re-fetch.
    BanCommand(ServerId, &'static str),
    FetchMaps(ServerId),
    Unban(ServerId),
    AddBan {
        id: ServerId,
        ip: bool,
    },
    /// Asks first ("Change level to …?"), then `ChangeLevelConfirmed`.
    ChangeLevel(ServerId, String),
    ChangeLevelConfirmed(ServerId, String),
    Say(ServerId),
    StartLog {
        id: ServerId,
        internet: bool,
    },
    KickPlayer(ServerId, u32),
    /// Kick the player, then connect to the server ourselves (HLSW "Kick & Connect").
    KickAndConnect(ServerId, u32),
    /// minutes 0 = permanent. `target` is the Steam ID (or userid for players without
    /// one) for ID bans, the IP address for IP bans.
    BanPlayer {
        id: ServerId,
        userid: u32,
        target: String,
        ip: bool,
        kick: bool,
        minutes: u32,
    },
    /// Header menu: show/hide a column of the server list (`players == false`) or players.
    ToggleColumn {
        players: bool,
        title: String,
    },
    /// "Change ID → Increase / Decrease": move the server one place down / up in the list.
    MoveServer(ServerId, i32),
    /// "Change ID → Set": move the server to this number (1-based).
    SetServerId(ServerId, usize),
    /// "Set all server IDs to current sequence": number the servers as currently sorted.
    RenumberServers,
    /// Header menu: "Reset column width".
    ResetColumns {
        players: bool,
    },
    /// Set a cvar via RCON ("Change Value..." in the rules list).
    SetCvar(ServerId, String, String),
    SetTheme(Theme),
    OpenSettings(SettingsTab),
    ApplySettings(Box<Settings>),
    SaveConfig,
    Quit,
}

fn ui_modifiers(resp: &egui::Response) -> egui::Modifiers {
    resp.ctx.input(|i| i.modifiers)
}

/// The list the arrow keys move in: the one clicked last ("servers", "players", "rules").
fn active_list(ctx: &egui::Context) -> &'static str {
    ctx.data(|d| d.get_temp(egui::Id::unique("active_list")))
        .unwrap_or("servers")
}

fn set_active_list(ctx: &egui::Context, list: &'static str) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("active_list"), list));
}

/// Arrow up / down (with Shift) for the list `name`, when no text field has the focus.
fn arrow_keys(ctx: &egui::Context, name: &str) -> Option<(bool, bool)> {
    // A clicked row also takes the focus; only a text field should keep the keys.
    if active_list(ctx) != name || ctx.text_edit_focused() {
        return None;
    }
    ctx.input(|i| {
        let shift = i.modifiers.shift;
        if i.modifiers.command || i.modifiers.alt {
            None
        } else if i.key_pressed(Key::ArrowDown) {
            Some((true, shift))
        } else if i.key_pressed(Key::ArrowUp) {
            Some((false, shift))
        } else {
            None
        }
    })
}

/// Marked rows of a list: a click marks one row, Ctrl+click adds / removes a row,
/// Shift+click marks the range from the last clicked row (like in HLSW / Windows lists).
#[derive(Default)]
struct Selection {
    keys: HashSet<String>,
    anchor: Option<String>,
    /// Row moved with the arrow keys.
    cursor: Option<String>,
}

impl Selection {
    fn contains(&self, key: &str) -> bool {
        self.keys.contains(key)
    }

    /// Left click on `key`; `order` are the keys as shown.
    fn click(&mut self, key: &str, order: &[&str], mods: egui::Modifiers) {
        let range = mods
            .shift
            .then(|| {
                let a = order
                    .iter()
                    .position(|k| Some(*k) == self.anchor.as_deref())?;
                let b = order.iter().position(|k| *k == key)?;
                Some(order[a.min(b)..=a.max(b)].iter().map(|k| k.to_string()))
            })
            .flatten();
        if let Some(range) = range {
            if !mods.command {
                self.keys.clear();
            }
            self.keys.extend(range);
            return;
        }
        if mods.command {
            if !self.keys.remove(key) {
                self.keys.insert(key.to_string());
            }
        } else {
            self.keys.clear();
            self.keys.insert(key.to_string());
        }
        self.anchor = Some(key.to_string());
        self.cursor = Some(key.to_string());
    }

    /// Arrow key: moves the mark one row up / down (with Shift the marked range grows).
    /// Returns the new row index.
    fn step(&mut self, order: &[&str], down: bool, shift: bool) -> Option<usize> {
        if order.is_empty() {
            return None;
        }
        let current = self
            .cursor
            .as_deref()
            .and_then(|c| order.iter().position(|k| *k == c));
        let new = match current {
            Some(i) if down => (i + 1).min(order.len() - 1),
            Some(i) => i.saturating_sub(1),
            None if down => 0,
            None => order.len() - 1,
        };
        let key = order[new].to_string();
        let anchor = self
            .anchor
            .as_deref()
            .and_then(|a| order.iter().position(|k| *k == a));
        match (shift, anchor) {
            (true, Some(a)) => {
                self.keys = order[a.min(new)..=a.max(new)]
                    .iter()
                    .map(|k| k.to_string())
                    .collect();
            }
            _ => {
                self.keys.clear();
                self.keys.insert(key.clone());
                self.anchor = Some(key.clone());
            }
        }
        self.cursor = Some(key);
        Some(new)
    }

    /// Right click: keeps a marked group, otherwise marks just this row.
    fn right_click(&mut self, key: &str) {
        if !self.keys.contains(key) {
            self.keys.clear();
            self.keys.insert(key.to_string());
            self.anchor = Some(key.to_string());
            self.cursor = Some(key.to_string());
        }
    }

    /// Drops marks of rows that are gone.
    fn retain(&mut self, order: &[&str]) {
        self.keys.retain(|k| order.contains(&k.as_str()));
    }
}

/// Game chosen in the "IP/Port" box (HLSW's game list); `None` = detect automatically.
type AddGame = Option<&'static games::Game>;

/// Hint of the address field for the chosen game.
fn add_game_hint(game: AddGame) -> String {
    match game {
        Some(g) if g.protocol == games::Protocol::GameSpy2 && g.default_query_port > 0 => tr!(
            "ip:gameport (query port {port}) or ip:gameport:queryport, Enter to add",
            port = g.default_query_port
        ),
        Some(g) if g.protocol == games::Protocol::GameSpy2 => {
            tr!("ip:gameport:queryport, e.g. 1.2.3.4:23459:34567 (Deer Hunter 2005), Enter to add")
                .into()
        }
        _ => tr!("host:port or steam://connect/… and press Enter to add").into(),
    }
}

/// Game of the list that matches a detected server (shown when it is selected).
fn add_game_of(info: &ServerInfo) -> AddGame {
    if info.gamespy {
        None
    } else if info.goldsrc {
        games::by_key("HL")
    } else {
        games::by_key("HL2")
    }
}

const COPYRIGHT_HOLDER: &str = "the openHLSW contributors";
const HOMEPAGE: &str = "https://github.com/DennisPohlmann/openHLSW";
const LICENSE_TEXT: &str = include_str!("../LICENSE");
const GAME_ICONS_NOTICE: &str = include_str!("../assets/games/NOTICE.txt");
const FLAGS_NOTICE: &str = include_str!("../assets/FLAGS-LICENSE.txt");
const FONT_NOTICE: &str = include_str!("../assets/fonts/NOTICE.txt");
const OFL_TEXT: &str = include_str!("../assets/fonts/OFL.txt");
const LGPL_TEXT: &str = include_str!("../assets/fonts/LGPL-2.1.txt");

/// Joins the hard-wrapped lines of a notice file into paragraphs that wrap with the window.
fn unwrap_paragraphs(text: &str) -> String {
    text.trim()
        .split("\n\n")
        .map(|p| p.lines().map(str::trim).collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// "? → License": the program's license, the notice for the game icons and the other
/// third-party parts.
/// "About openHLSW", laid out like HLSW's About window: a banner, the program details in a
/// white box, the homepage link and Close.
fn about_view(
    ui: &mut Ui,
    icon: Option<&egui::TextureHandle>,
    close: &mut bool,
    open_license: &mut bool,
    actions: &mut Vec<Action>,
) {
    let width = 400.0;
    // Banner: dark red to orange with soft light curves, the icon and the name.
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 120.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let mut mesh = egui::Mesh::default();
    mesh.add_colored_rect(rect, Color32::WHITE);
    let corners = [
        Color32::from_rgb(140, 24, 14),  // top left
        Color32::from_rgb(232, 96, 40),  // top right
        Color32::from_rgb(245, 150, 70), // bottom right
        Color32::from_rgb(178, 40, 22),  // bottom left
    ];
    for (v, c) in mesh.vertices.iter_mut().zip(corners) {
        v.color = c;
    }
    painter.add(mesh);
    for i in 0..9 {
        let t = i as f32 / 8.0;
        let points: Vec<egui::Pos2> = (0..=24)
            .map(|k| {
                let x = rect.left() + rect.width() * k as f32 / 24.0;
                let phase = k as f32 / 24.0 * std::f32::consts::PI;
                let y = rect.bottom() - 10.0 - t * 70.0 - phase.sin() * (18.0 + t * 22.0);
                egui::pos2(x, y)
            })
            .collect();
        painter.add(egui::Shape::line(
            points,
            Stroke::new(1.0, Color32::from_white_alpha(28 + (t * 40.0) as u8)),
        ));
    }
    let mut x = rect.left() + 20.0;
    if let Some(icon) = icon {
        let r = egui::Rect::from_min_size(
            egui::pos2(x, rect.center().y - 32.0),
            egui::vec2(64.0, 64.0),
        );
        painter.image(
            icon.id(),
            r,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        x = r.right() + 14.0;
    }
    let shadow = Color32::from_black_alpha(90);
    for (text, size, dy) in [("openHLSW", 30.0, -26.0), ("Gameserver Tool", 15.0, 10.0)] {
        let font = egui::FontId::proportional(size);
        let pos = egui::pos2(x, rect.center().y + dy);
        painter.text(
            pos + egui::vec2(1.0, 1.0),
            egui::Align2::LEFT_TOP,
            text,
            font.clone(),
            shadow,
        );
        painter.text(pos, egui::Align2::LEFT_TOP, text, font, Color32::WHITE);
    }

    // Details in a white box.
    Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .stroke(Stroke::new(1.0, Color32::from_gray(170)))
        .inner_margin(Margin::symmetric(10, 8))
        .outer_margin(Margin::symmetric(8, 8))
        .show(ui, |ui| {
            ui.set_width(width - 36.0);
            ScrollArea::vertical()
                .min_scrolled_height(170.0)
                .max_height(170.0)
                .show(ui, |ui| {
                egui::Grid::new("about_grid")
                    .num_columns(2)
                    .spacing([16.0, 2.0])
                    .show(ui, |ui| {
                        ui.label("openHLSW");
                        ui.label(format!("v{}", env!("CARGO_PKG_VERSION")));
                        ui.end_row();
                        ui.label(tr!("Copyright"));
                        ui.label(format!("(c) 2026 {COPYRIGHT_HOLDER}"));
                        ui.end_row();
                        ui.label(tr!("System"));
                        ui.label(format!("{} {}", std::env::consts::OS, std::env::consts::ARCH));
                        ui.end_row();
                        ui.label(tr!("License"));
                        if ui.link("GNU GPL v3.0 or later").clicked() {
                            *open_license = true;
                        }
                        ui.end_row();
                    });
                ui.add_space(8.0);
                ui.label(tr!("Free game server browser and RCON admin tool."));
                ui.label(tr!(
                    "A remake of HLSW. Not affiliated with the original HLSW authors."
                ));
                ui.add_space(6.0);
                ui.label(tr!(
                    "openHLSW only talks to the servers in your lists. Server and player locations are looked up at ip-api.com (can be switched off under Settings → Misc). No account, no master server, no tracking."
                ));
                if let Some(path) = config::path() {
                    ui.add_space(6.0);
                    ui.weak(tr!("Settings: {path}", path = path.display()));
                }
            });
        });

    // Homepage and Close.
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(tr!("Visit openHLSW homepage:"));
        if ui.link("GitHub").clicked() {
            actions.push(Action::OpenUrl(HOMEPAGE.into()));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(10.0);
            if ui
                .add(Button::new(tr!("Close")).min_size(egui::vec2(75.0, 22.0)))
                .clicked()
            {
                *close = true;
            }
        });
    });
    ui.add_space(8.0);
}

fn license_view(ui: &mut Ui) {
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.heading(tr!("openHLSW"));
        ui.label(format!(
            "Copyright (C) 2026 {COPYRIGHT_HOLDER}\n\n\
             This program is free software: you can redistribute it and/or modify it under the \
             terms of the GNU General Public License as published by the Free Software \
             Foundation, either version 3 of the License, or (at your option) any later version.\n\n\
             This program is distributed in the hope that it will be useful, but WITHOUT ANY \
             WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A \
             PARTICULAR PURPOSE. See the GNU General Public License for more details."
        ));
        ui.add_space(8.0);
        ui.heading(tr!("Game icons"));
        ui.label(unwrap_paragraphs(GAME_ICONS_NOTICE));
        ui.add_space(8.0);
        ui.heading(tr!("Country flags"));
        ui.label(unwrap_paragraphs(FLAGS_NOTICE));
        ui.add_space(8.0);
        ui.heading(tr!("Fonts"));
        ui.label(unwrap_paragraphs(FONT_NOTICE));
        egui::CollapsingHeader::new(tr!("SIL Open Font License 1.1 (full text)"))
            .default_open(false)
            .show(ui, |ui| {
                ui.label(RichText::new(OFL_TEXT).monospace().size(11.0));
            });
        egui::CollapsingHeader::new(tr!("GNU Lesser General Public License v2.1 (full text)"))
            .default_open(false)
            .show(ui, |ui| {
                ui.label(RichText::new(LGPL_TEXT).monospace().size(11.0));
            });
        ui.add_space(8.0);
        ui.heading(tr!("Other components"));
        ui.label(
            tr!("openHLSW is built with Rust libraries (egui/eframe, serde and others) under the MIT \
             and/or Apache-2.0 licenses, and egui's bundled fonts (Ubuntu Font Licence, \
             SIL Open Font License, MIT). Fallback fonts for other scripts come from your \
             system. Server locations come from ip-api.com."),
        );
        ui.add_space(8.0);
        egui::CollapsingHeader::new(tr!("GNU General Public License v3.0 (full text)"))
            .default_open(false)
            .show(ui, |ui| {
                ui.label(RichText::new(LICENSE_TEXT).monospace().size(11.0));
            });
    });
}

/// Small modal windows opened from the menus.
#[derive(Clone)]
enum Dialog {
    NewList(String),
    ListProperties(String),
    MultiInsert(String),
    ServerProperties(ServerId, ServerPropsDraft),
    ConfirmDeleteList,
    ConnectLog,
    /// "Change ID → Set": server and the new number being typed.
    SetId(ServerId, String),
    /// Server, cvar, new value.
    ChangeRule(ServerId, String, String),
    /// Yes/No question before an action.
    Confirm(Confirm),
}

/// Questions HLSW asks before doing something.
#[derive(Clone)]
enum Confirm {
    DeleteServer(ServerId),
    ChangeLevel(ServerId, String),
    /// The server is password protected and no password is entered.
    ConnectNoPassword(ServerId),
}

#[derive(Clone)]
struct ServerPropsDraft {
    address: String,
    comment: String,
    params: String,
    rcon_password: String,
    save_password: bool,
}

struct Traffic {
    last: (u64, u64),
    at: Instant,
    rate: (u64, u64),
}

pub struct OpenHlswApp {
    servers: Vec<Server>,
    next_id: ServerId,
    selected: Option<ServerId>,

    notifier: Notifier,
    events: Receiver<Event>,

    settings: Settings,
    settings_dialog: Option<SettingsDialog>,
    list_name: String,
    other_lists: Vec<SavedList>,
    connect_log: Vec<String>,
    dialog: Option<Dialog>,
    /// The Trace Route window, while open.
    trace: Option<crate::trace::TraceRoute>,
    search_id: Option<egui::Id>,
    flags: Option<Flags>,
    game_icons: Option<GameIcons>,
    geo: HashMap<IpAddr, GeoInfo>,
    geo_pending: HashSet<IpAddr>,
    last_geo_request: Option<Instant>,
    /// Row order shown last frame (kept while hovering if keep_order_on_hover).
    list_order: Vec<ServerId>,
    /// Queries paused because we minimized for a game (OnConnect::MinimizeNoTraffic).
    paused_for_game: bool,
    console_input_id: Option<egui::Id>,
    console_autoscroll: bool,
    /// Find bar of the Rcon console.
    console_find: Option<ConsoleFind>,
    address_input: String,
    add_game: AddGame,
    search: String,
    join_password: String,
    hide_join_password: bool,
    status_message: String,
    auto_update_status: bool,
    hide_password: bool,
    last_list_refresh: Option<Instant>,
    traffic: Traffic,
    log_listener: Option<Result<u16, String>>,
    chat_input: String,
    ban_input: String,
    ban_minutes: u32,

    sort: (Col, bool),
    player_sort: (PlayerCol, bool),
    player_filter: String,
    /// Marked players (by name) and rules (by key): click, Ctrl+click, Shift+click.
    player_sel: Selection,
    rule_sel: Selection,
    rules_filter: String,
    tab: Tab,
    console_input: String,
    history: Vec<String>,
    history_pos: Option<usize>,
    show_about: bool,
    /// Program icon for the About window (loaded when it is first opened).
    about_icon: Option<egui::TextureHandle>,
    toolbar_icons: Option<crate::toolbar_icons::ToolbarIcons>,
    show_license: bool,
    editing_comment: Option<(ServerId, String)>,
    /// "Connecting..." window: server we wait on for a free slot, since when.
    waiting: Option<(ServerId, Instant)>,
    /// Filter properties window open.
    show_filter: bool,
    /// Find player / rule window.
    find: Option<Find>,
    /// Global Player ID Database.
    iddb: IdDb,
    iddb_dirty: bool,
    iddb_saved: Instant,
    /// Global Player ID Database Editor window.
    iddb_editor: Option<IdDbEditor>,
    /// Running LAN search.
    lan_search: Option<Receiver<std::io::Result<Vec<SocketAddr>>>>,
}

/// Find bar of the Rcon console.
#[derive(Default)]
struct ConsoleFind {
    text: String,
    /// Current hit (index into all hits of the shown lines).
    pos: usize,
    focus: bool,
    /// Scroll to the current hit on the next frame.
    scroll: bool,
}

/// Byte ranges of `needle` in `hay`, ignoring ASCII case.
fn find_ascii_ci(hay: &str, needle: &str) -> Vec<std::ops::Range<usize>> {
    if needle.is_empty() {
        return Vec::new();
    }
    let h = hay.to_ascii_lowercase();
    let n = needle.to_ascii_lowercase();
    h.match_indices(&n).map(|(i, m)| i..i + m.len()).collect()
}

/// State of the Global Player ID Database Editor.
#[derive(Default)]
struct IdDbEditor {
    selected: Option<String>,
    filter: String,
    hide_auto: bool,
    id: String,
    kind: IdKind,
    names: String,
    comment: String,
    banned: bool,
    message: String,
}

impl IdDbEditor {
    fn select(&mut self, db: &IdDb, id: &str) {
        let key = iddb::normalize(id).unwrap_or_else(|| id.trim().to_string());
        let e = db.entries.get(&key).cloned().unwrap_or_default();
        self.selected = Some(key.clone());
        self.id = key;
        self.kind = e.kind;
        self.names = e.names_text();
        self.comment = e.comment;
        self.banned = e.banned;
        self.message.clear();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FindKind {
    Player,
    Rule,
}

/// "Find → Player / Rule": searches all servers of the list.
struct Find {
    kind: FindKind,
    text: String,
    /// Index of the last selected result (F3 goes on from here).
    pos: Option<usize>,
    focus: bool,
}

impl Find {
    fn new(kind: FindKind) -> Self {
        Self {
            kind,
            text: String::new(),
            pos: None,
            focus: true,
        }
    }
}

impl OpenHlswApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (notifier, events) = Notifier::new(cc.egui_ctx.clone());
        let mut config = config::load();
        crate::i18n::set_language(config.settings.language);
        // Old configs: switch once to HLSW's look (Tahoma, 12 px), the sharpest text.
        if config.settings.appearance < crate::settings::APPEARANCE {
            config.settings.font = crate::theme::UiFont::Tahoma;
            config.settings.text_size = crate::theme::TextSize::Px12;
            config.settings.appearance = crate::settings::APPEARANCE;
        }
        theme::install_fonts(&cc.egui_ctx, config.settings.font);
        theme::apply(&cc.egui_ctx, config.settings.theme);
        cc.egui_ctx
            .set_zoom_factor(config.settings.text_size.zoom());

        let mut app = Self {
            servers: Vec::new(),
            next_id: 1,
            selected: None,
            notifier,
            events,
            settings: {
                // Older configs stored the old defaults 0.2 / 0.4 s; the default is now 1 s.
                let mut st = config.settings.clone();
                if st.defaults_revision < 1 {
                    if [0.2, 0.4]
                        .iter()
                        .any(|d| (st.refresh_interval - d).abs() < 1e-4)
                    {
                        st.refresh_interval = 1.0;
                    }
                    st.defaults_revision = 1;
                }
                st.refresh_interval =
                    sanitize_interval(st.refresh_interval, st.max_queries_per_sec);
                st
            },
            settings_dialog: None,
            list_name: if config.list_name.trim().is_empty() {
                config::DEFAULT_LIST_NAME.into()
            } else {
                config.list_name.clone()
            },
            other_lists: config.other_lists.clone(),
            connect_log: config.connect_log.clone(),
            dialog: None,
            trace: None,
            search_id: None,
            flags: Flags::load(&cc.egui_ctx),
            game_icons: GameIcons::load(&cc.egui_ctx),
            geo: geo::load_cache(),
            geo_pending: HashSet::new(),
            last_geo_request: None,
            list_order: Vec::new(),
            paused_for_game: false,
            console_input_id: None,
            console_autoscroll: true,
            console_find: None,
            address_input: String::new(),
            add_game: None,
            search: String::new(),
            join_password: String::new(),
            hide_join_password: true,
            status_message: tr!("Ready").into(),
            auto_update_status: config.auto_update_status,
            hide_password: config.hide_rcon_password,
            last_list_refresh: None,
            log_listener: None,
            chat_input: String::new(),
            ban_input: String::new(),
            ban_minutes: 0,
            traffic: Traffic {
                last: openhlsw::stats::totals(),
                at: Instant::now(),
                rate: (0, 0),
            },
            sort: (Col::Index, true),
            player_sort: ("#", true),
            player_filter: String::new(),
            player_sel: Selection::default(),
            rule_sel: Selection::default(),
            rules_filter: String::new(),
            tab: Tab::RconConfig,
            console_input: String::new(),
            history: Vec::new(),
            history_pos: None,
            show_about: false,
            about_icon: None,
            toolbar_icons: crate::toolbar_icons::ToolbarIcons::load(&cc.egui_ctx),
            show_license: false,
            editing_comment: None,
            waiting: None,
            show_filter: false,
            iddb: IdDb::load(),
            iddb_dirty: false,
            iddb_saved: Instant::now(),
            iddb_editor: None,
            lan_search: None,
            find: None,
        };

        app.add_saved_servers(&config.servers);
        let startup = match config.settings.startup {
            StartupSelection::None => None,
            StartupSelection::LastUsed => config.selected.clone(),
            StartupSelection::This => Some(config.settings.startup_address.clone()),
        };
        app.selected = startup
            .as_deref()
            .and_then(ServerAddress::parse)
            .and_then(|a| app.servers.iter().find(|s| s.address == a))
            .map(|s| s.id);
        app.play(SoundEvent::StartUp);
        app
    }

    /// Adds servers from the config / an import. Duplicates are skipped.
    /// Returns how many were added.
    fn add_saved_servers(&mut self, saved: &[SavedServer]) -> usize {
        let mut added = 0;
        for saved in saved {
            let Some(address) = ServerAddress::parse(&saved.address) else {
                continue;
            };
            if self.servers.iter().any(|s| s.address == address) {
                continue;
            }
            let id = self.alloc_id();
            let mut server = Server::new(id, address, &self.notifier);
            server.comment = saved.comment.clone();
            server.params = saved.params.clone();
            server.last_connected = saved.last_connected.clone();
            server.info = saved.info.clone();
            server.last_seen = saved
                .last_seen
                .as_deref()
                .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                .map(|t| t.with_timezone(&chrono::Local));
            server.console_cache = self.settings.console_cache.max(1);
            if let Some(pw) = &saved.rcon_password {
                server.rcon_password = pw.clone();
                server.save_password = true;
            }
            self.servers.push(server);
            added += 1;
        }
        added
    }

    fn saved_server(s: &Server) -> SavedServer {
        SavedServer {
            address: s.address.to_string(),
            rcon_password: s.save_password.then(|| s.rcon_password.clone()),
            comment: s.comment.clone(),
            params: s.params.clone(),
            last_connected: s.last_connected.clone(),
            info: s.info.clone(),
            last_seen: s.last_seen.map(|t| t.to_rfc3339()),
        }
    }

    fn list_names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::iter::once(self.list_name.clone())
            .chain(self.other_lists.iter().map(|l| l.name.clone()))
            .collect();
        names.sort_by_key(|n| n.to_lowercase());
        names
    }

    /// Makes another server list the current one. `keep_current` = false drops the
    /// current list (Server list > Delete).
    fn switch_list(&mut self, name: &str, keep_current: bool) {
        let Some(pos) = self.other_lists.iter().position(|l| l.name == name) else {
            return;
        };
        let target = self.other_lists.remove(pos);
        if keep_current {
            let current = SavedList {
                name: std::mem::take(&mut self.list_name),
                servers: self.servers.iter().map(Self::saved_server).collect(),
            };
            self.other_lists.push(current);
        }
        self.servers.clear();
        self.selected = None;
        self.list_order.clear();
        self.list_name = target.name;
        self.add_saved_servers(&target.servers);
        self.selected = self.servers.first().map(|s| s.id);
        self.last_list_refresh = None;
        self.status_message = tr!("Server list: {list_name}", list_name = self.list_name);
        self.save_config();
    }

    fn alloc_id(&mut self) -> ServerId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn index_of(&self, id: ServerId) -> Option<usize> {
        self.servers.iter().position(|s| s.id == id)
    }

    fn server(&self, id: ServerId) -> Option<&Server> {
        self.servers.iter().find(|s| s.id == id)
    }

    fn server_mut(&mut self, id: ServerId) -> Option<&mut Server> {
        self.servers.iter_mut().find(|s| s.id == id)
    }

    fn selected_index(&self) -> Option<usize> {
        self.selected.and_then(|id| self.index_of(id))
    }

    /// Info always; players for the selected server; rules every [`RULES_REFRESH`]
    /// (the list shows AddOn/Ranking/Limits from them) or when `force_details`.
    fn refresh(&mut self, id: ServerId, force_details: bool) {
        let notifier = self.notifier.clone();
        let selected = self.selected == Some(id);
        let auto_details = self.auto_update_status;
        let base_rate = self.settings.max_queries_per_sec.max(0.1);
        let Some(s) = self.server_mut(id) else { return };
        // Never more than the server itself announces (HLDS often uses max_queries_sec 1 with a
        // 1 s window: a short burst would already be dropped).
        let (limit, window) = s.announced_query_limit().unwrap_or((f32::INFINITY, 30.0));
        let rate = base_rate.min(limit * 0.9) * s.rate_factor;

        // Refill the token bucket. A short burst is fine for srcds: it averages
        // queries over sv_max_queries_window (30 s).
        let burst = (rate * window.min(2.0)).max(1.0);
        s.budget = (s.budget + s.budget_at.elapsed().as_secs_f32() * rate).min(burst);
        s.budget_at = Instant::now();

        let rules_due = s.last_rules.is_none_or(|t| t.elapsed() >= RULES_REFRESH);
        // Priority: info first (ping, map, players count must stay current even on servers
        // that allow only one query per second, like many HLDS), then the rules when due
        // (every 10 s), then the players of the selected server.
        let mut kinds = vec![(0, QueryKind::Info)];
        if force_details || rules_due {
            kinds.push((2, QueryKind::Rules));
        }
        if force_details || (selected && auto_details) {
            kinds.push((1, QueryKind::Players));
        }
        for (slot, kind) in kinds {
            if s.busy[slot] {
                continue;
            }
            // Explicit refreshes (click, F5) always go out; the budget may go negative
            // and the automatic refresh then waits until it has recovered.
            if !force_details && s.budget < 1.0 {
                break;
            }
            s.budget -= 1.0;
            s.busy[slot] = true;
            if slot == 2 {
                s.last_rules = Some(Instant::now());
            }
            worker::spawn_query(&notifier, s.id, s.address.clone(), kind, s.challenge);
        }
    }

    fn refresh_all(&mut self, force_details: bool) {
        let mut ids: Vec<ServerId> = self.servers.iter().map(|s| s.id).collect();
        // The selected server first.
        ids.sort_by_key(|id| Some(*id) != self.selected);
        for id in ids {
            let force = force_details && Some(id) == self.selected;
            self.refresh(id, force);
        }
        self.last_list_refresh = Some(Instant::now());
    }

    /// "Auto Update Status": every 5 s RCON `status` for the selected server (Steam IDs,
    /// IPs, ping) and make sure the server sends us its log (`logaddress_add`), so
    /// the log shows up in the Console and Chat tabs.
    /// Never retries after a bad password (srcds bans IPs after repeated failed logins).
    fn poll_player_status(&mut self, ctx: &egui::Context) {
        if !self.auto_update_status {
            return;
        }
        let Some(id) = self.selected else { return };
        let Some(s) = self.server_mut(id) else { return };
        if s.rcon_password.is_empty()
            || matches!(
                s.rcon_status,
                RconStatus::BadPassword | RconStatus::Connecting
            )
        {
            return;
        }
        // After an error (server down, connection refused) wait longer before retrying.
        let wait = if matches!(s.rcon_status, RconStatus::Error(_)) {
            30
        } else {
            5
        };
        if s.last_status
            .is_some_and(|t| t.elapsed() < Duration::from_secs(wait))
        {
            return;
        }
        s.last_status = Some(Instant::now());
        s.configure_rcon();
        s.rcon_send("status", RconTag::PlayerStatus);

        let wants_log = s.rcon_status == RconStatus::Ok
            && s.log_target.is_none()
            && s.log_requested
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(60));
        if wants_log {
            s.log_requested = Some(Instant::now());
            // LAN for servers in the local network, otherwise our public IP.
            let internet = s.resolved.is_some_and(|a| geo::is_public(a.ip()));
            self.apply(Action::StartLog { id, internet }, ctx);
        }
    }

    /// Asks ip-api.com for the location of server IPs we do not know yet
    /// (one batch request at most every 5 s; the free API allows 15 per minute).
    fn lookup_locations(&mut self) {
        if !self.settings.geo_lookup
            || self
                .last_geo_request
                .is_some_and(|t| t.elapsed() < Duration::from_secs(5))
        {
            return;
        }
        let mut ips: Vec<IpAddr> = self
            .servers
            .iter()
            .filter_map(|s| s.resolved.map(|a| a.ip()))
            .chain(
                self.selected_index()
                    .map(|i| {
                        self.servers[i]
                            .status_players
                            .iter()
                            .filter_map(|p| p.ip)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
            )
            .chain(
                self.trace
                    .iter()
                    .flat_map(|t| t.hops.iter().filter_map(|h| h.ip)),
            )
            .filter(|ip| {
                geo::is_public(*ip) && !self.geo.contains_key(ip) && !self.geo_pending.contains(ip)
            })
            .collect();
        ips.sort();
        ips.dedup();
        ips.truncate(100);
        if ips.is_empty() {
            return;
        }
        self.geo_pending.extend(ips.iter().copied());
        self.last_geo_request = Some(Instant::now());
        worker::spawn_geo_lookup(&self.notifier, ips);
    }

    /// Location of a server: `Some(None)` = private/LAN address.
    fn location_of(&self, s: &Server) -> Option<Option<&GeoInfo>> {
        let ip = s.resolved?.ip();
        if !geo::is_public(ip) {
            return Some(None);
        }
        self.geo.get(&ip).map(Some)
    }

    fn ensure_log_listener(&mut self) -> Result<u16, String> {
        if let Some(Ok(port)) = &self.log_listener
            && *port == self.settings.log_port
        {
            return Ok(*port);
        }
        let result = worker::spawn_log_listener(&self.notifier, self.settings.log_port)
            .map(|_| self.settings.log_port)
            .map_err(|e| {
                tr!(
                    "Cannot listen on UDP port {log_port}: {e}",
                    log_port = self.settings.log_port,
                    e = e
                )
            });
        self.log_listener = Some(result.clone());
        result
    }

    fn handle_log_line(&mut self, from: SocketAddr, line: String) {
        // Match by address; srcds sends from its game port, fall back to the IP alone.
        let idx = self
            .servers
            .iter()
            .position(|s| s.resolved == Some(from))
            .or_else(|| {
                self.servers
                    .iter()
                    .position(|s| s.resolved.map(|a| a.ip()) == Some(from.ip()))
            });
        let Some(idx) = idx else { return };
        let chat = parse_chat_line(&line);
        let joined = line.contains("\" entered the game");
        let address = self.servers[idx].address.to_string();
        if self.settings.save_server_log {
            append_log_file(&address, &line, "log");
        }
        if self.settings.save_chat_log && chat.is_some() {
            append_log_file(&address, &line, "chat.log");
        }

        let s = &mut self.servers[idx];
        apply_log_stats(&mut s.log_players, &line);
        s.append_console(&format!("L {line}"));
        let mut sound = joined.then_some(SoundEvent::PlayerJoin);
        let mut admin_call = None;
        if let Some(chat) = chat {
            if chat.name != "Console" {
                sound = Some(SoundEvent::IncomingMessage);
                if self.settings.admin_notification
                    && let Some(text) = chat.text.trim_start().strip_prefix('@')
                {
                    sound = Some(SoundEvent::Notification);
                    admin_call = Some(tr!(
                        "{server}: {name} wrote: {text}",
                        server = s
                            .info
                            .as_ref()
                            .map(|i| i.name.clone())
                            .unwrap_or_else(|| s.address.to_string()),
                        name = chat.name,
                        text = text.trim()
                    ));
                }
            }
            // Our own message is shown right away when sent; the server log repeats it as
            // "Console" – take that as confirmation instead of showing it twice.
            let echo = (chat.name == "Console")
                .then(|| {
                    s.chat
                        .iter_mut()
                        .rev()
                        .take(20)
                        .find(|c| c.own && c.text == chat.text)
                })
                .flatten();
            if let Some(own) = echo {
                own.own = false;
                own.time = chat.time;
            } else {
                s.chat.push(chat);
                if s.chat.len() > CHAT_MAX_LINES {
                    s.chat.remove(0);
                }
            }
        }
        if let Some(msg) = admin_call {
            self.status_message = msg;
        }
        if let Some(sound) = sound {
            self.play(sound);
        }
    }

    fn play(&self, event: SoundEvent) {
        if let Some(file) = self.settings.sound_file(event) {
            play_sound(file);
        }
    }

    /// Joins a server: game executable from "My Games" or steam://connect, then the
    /// "On Connection" action and the user's "Execute applications".
    /// Connect like HLSW: ask when the server wants a password and none is entered, and wait
    /// for a free slot when the server is full ("Connecting..." window).
    fn request_connect(&mut self, id: ServerId, password_checked: bool, ctx: &egui::Context) {
        let Some(s) = self.server(id) else { return };
        if let Some(info) = &s.info {
            if !password_checked && info.passworded && self.join_password.trim().is_empty() {
                self.dialog = Some(Dialog::Confirm(Confirm::ConnectNoPassword(id)));
                return;
            }
            if info.max_players > 0 && s.humans() >= info.max_players && !s.is_offline() {
                self.waiting = Some((id, Instant::now()));
                self.status_message = tr!("Waiting for free slot").into();
                return;
            }
        }
        self.connect(id, ctx);
    }

    fn connect(&mut self, id: ServerId, ctx: &egui::Context) {
        let Some(s) = self.server(id) else { return };
        if s.address.query_port.is_some() {
            // GameSpy games have no connect URL: start the game, join from its server list.
            let game = s.info.as_ref().map(|i| i.game.clone()).unwrap_or_default();
            let addr = s.address.game_address();
            let exe = crate::settings::game_by_name(&game)
                .and_then(|g| self.settings.game_paths.get(g.name))
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty());
            self.status_message = match exe {
                Some(exe) => {
                    let mut cmd = std::process::Command::new(&exe);
                    cmd.args(self.start_params(&game, s));
                    cmd.args(self.battlefield_join_args(s));
                    if let Some(dir) = std::path::Path::new(&exe).parent() {
                        cmd.current_dir(dir);
                    }
                    match cmd.spawn() {
                        Ok(_) => tr!(
                            "Started {game} – join {addr} in the game",
                            game = game,
                            addr = addr
                        ),
                        Err(e) => tr!("Could not start {exe}: {e}", exe = exe, e = e),
                    }
                }
                None => tr!(
                    "Set the path of {game} in Settings → My Games to start it (server: {addr})",
                    game = if game.is_empty() {
                        tr!("the game")
                    } else {
                        &game
                    },
                    addr = addr
                ),
            };
            return;
        }
        let addr = s.address.to_string();
        let (host, port) = (s.address.host.clone(), s.address.port.to_string());
        let app_id = s.info.as_ref().map(|i| i.real_app_id());
        let password = self.join_password.trim().to_string();
        let game_def = app_id.and_then(crate::settings::game_by_app_id);
        // My Games parameters, player name, server parameters.
        let mut params = self.start_params(game_def.map(|g| g.name).unwrap_or(""), s);
        let base_params = if password.is_empty() { 2 } else { 4 };
        params.extend(["+connect".to_string(), addr.clone()]);
        if !password.is_empty() {
            params.extend(["+password".to_string(), password.clone()]);
        }

        let game_exe = game_def
            .and_then(|g| self.settings.game_paths.get(g.name))
            .filter(|p| !p.trim().is_empty());
        let result = if let Some(exe) = game_exe {
            let mut cmd = std::process::Command::new(exe.trim());
            cmd.args(&params);
            if let Some(dir) = std::path::Path::new(exe.trim()).parent() {
                cmd.current_dir(dir);
            }
            cmd.spawn().map(|_| ())
        } else if let Some(app_id) = app_id.filter(|_| params.len() > base_params) {
            // Extra parameters or a player name: let Steam start the game with them.
            let args: Vec<String> = params.iter().map(|a| quote_arg(a)).collect();
            open_url(&format!(
                "steam://run/{app_id}//{}/",
                args.join(" ").replace(' ', "%20").replace('"', "%22")
            ))
        } else {
            let mut url = format!("steam://connect/{addr}");
            if !password.is_empty() {
                url = format!("{url}/{password}");
            }
            open_url(&url)
        };
        if let Err(e) = result {
            self.status_message = tr!("Could not start the game: {e}", e = e);
            return;
        }
        self.status_message = tr!("Connecting to {addr}", addr = addr);
        let now = chrono::Local::now().format("%d.%m.%Y %H:%M:%S").to_string();
        if let Some(s) = self.server_mut(id) {
            s.last_connected = Some(now.clone());
            let name = s.info.as_ref().map(|i| i.name.clone()).unwrap_or_default();
            self.connect_log.push(format!("{now}   {addr}   {name}"));
            if self.connect_log.len() > 500 {
                self.connect_log.remove(0);
            }
        }
        self.save_config();

        for app in self
            .settings
            .connect_apps
            .iter()
            .filter(|a| a.enabled && !a.command.trim().is_empty())
        {
            let command = app
                .command
                .replace("%addr%", &addr)
                .replace("%ip%", &host)
                .replace("%port%", &port);
            if let Err(e) = run_shell(&command) {
                self.status_message = tr!("Could not run {command}: {e}", command = command, e = e);
            }
        }
        match self.settings.on_connect {
            OnConnect::Nothing => {}
            OnConnect::Minimize => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
            OnConnect::MinimizeNoTraffic => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                self.paused_for_game = true;
            }
            OnConnect::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    /// Start parameters for Connect: the game's (My Games), the player name, the server's.
    /// HLSW placeholders are replaced: `<SERVERIP>`, `<SERVERPORT>`, `<SERVERADDR>`,
    /// `<CONNECTADDRPORT>`, `<PASSWORD>`, `<RCONPASSWORD>`, `<PLAYERNAME>`, `<RULE(cvar)>`.
    /// Battlefield 2 / 2142 join the server directly (HLSW's "BF" connect type):
    /// `+modPath mods/<mod> +restart 1 +joinServer <ip> +port <port> [+password …]`.
    fn battlefield_join_args(&self, s: &Server) -> Vec<String> {
        let Some(i) = &s.info else { return Vec::new() };
        let folder = i.folder.to_ascii_lowercase();
        let (gamename, mod_dir) = folder.split_once('/').unwrap_or((folder.as_str(), ""));
        let default_mod = match gamename {
            "battlefield2" => "bf2",
            "stella" => "bf2142",
            _ => return Vec::new(),
        };
        let ip = s
            .resolved
            .map(|a| a.ip().to_string())
            .unwrap_or_else(|| s.address.host.clone());
        let port = i.game_port.unwrap_or(s.address.port).to_string();
        let mut args = vec![
            "+modPath".to_string(),
            format!(
                "mods/{}",
                if mod_dir.is_empty() {
                    default_mod
                } else {
                    mod_dir
                }
            ),
            "+restart".into(),
            "1".into(),
            "+joinServer".into(),
            ip,
            "+port".into(),
            port,
        ];
        let password = self.join_password.trim();
        if !password.is_empty() {
            args.extend(["+password".to_string(), password.to_string()]);
        }
        args
    }

    fn start_params(&self, game: &str, s: &Server) -> Vec<String> {
        let mut out = split_args(
            self.settings
                .game_params
                .get(game)
                .map_or("", |p| p.as_str()),
        );
        let name = self.settings.player_name.trim();
        if !name.is_empty() {
            // Battlefield 2 / 2142 take +playerName, the Source and HL1 games +name.
            let bf = s.info.as_ref().is_some_and(|i| {
                let f = i.folder.to_ascii_lowercase();
                f.starts_with("battlefield2") || f.starts_with("stella")
            });
            let flag = if bf { "+playerName" } else { "+name" };
            out.extend([flag.to_string(), name.to_string()]);
        }
        out.extend(split_args(&s.params));
        let addr = s.address.to_string();
        out.into_iter()
            .map(|arg| {
                let mut arg = arg
                    .replace("<SERVERIP>", &s.address.host)
                    .replace("<SERVERPORT>", &s.address.port.to_string())
                    .replace("<SERVERADDR>", &addr)
                    .replace("<CONNECTADDRPORT>", &addr)
                    .replace("<PASSWORD>", self.join_password.trim())
                    .replace("<RCONPASSWORD>", &s.rcon_password)
                    .replace("<PLAYERNAME>", name);
                while let Some(start) = arg.find("<RULE(") {
                    let Some(end) = arg[start..].find(")>") else {
                        break;
                    };
                    let key = arg[start + 6..start + end].to_string();
                    let value = s.rule(&key).unwrap_or_default().to_string();
                    arg.replace_range(start..start + end + 2, &value);
                }
                arg
            })
            .collect()
    }

    fn process_events(&mut self) {
        let mut went_offline = false;
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Info { id, result } => {
                    let Some(s) = self.servers.iter_mut().find(|s| s.id == id) else {
                        continue;
                    };
                    s.busy[0] = false;
                    match result {
                        Ok((info, meta, addr)) => {
                            let ping = meta.rtt.as_millis() as u32;
                            s.charge(&meta);
                            let map_changed =
                                s.info.as_ref().is_some_and(|old| old.map != info.map);
                            s.resolved = Some(addr);
                            s.last_ok = Instant::now();
                            if map_changed {
                                s.last_rules = None; // limits / next map may have changed
                            }
                            if map_changed
                                && self.settings.log_on_map_change
                                && let Some((_, internet)) = s.log_target
                            {
                                s.rcon.send(RconRequest::StartLog {
                                    internet,
                                    port: self.settings.log_port,
                                    public_ip: self
                                        .settings
                                        .public_ip
                                        .trim()
                                        .parse::<IpAddr>()
                                        .ok(),
                                });
                            }
                            let engine_changed = s.is_goldsrc() != info.goldsrc;
                            s.info = Some(*info);
                            if engine_changed && !s.rcon_password.is_empty() {
                                s.configure_rcon(); // GoldSrc uses UDP RCON
                            }
                            s.ping = Some(ping);
                            s.push_ping(Some(ping));
                            s.rate_factor = (s.rate_factor + 0.02).min(1.0);
                            s.state = QueryState::Online;
                            s.last_seen = Some(chrono::Local::now());
                        }
                        Err(e) => {
                            s.budget -= 1.0; // the timeout retry
                            if s.info.is_some() {
                                s.push_ping(None);
                                s.rate_factor = (s.rate_factor * 0.5).max(0.25);
                            }
                            if s.last_ok.elapsed() >= OFFLINE_AFTER {
                                if s.state == QueryState::Online {
                                    went_offline = true;
                                }
                                s.ping = None;
                                s.state = QueryState::Offline(e);
                            }
                        }
                    }
                }
                Event::Players { id, result } => {
                    let Some(s) = self.servers.iter_mut().find(|s| s.id == id) else {
                        continue;
                    };
                    s.busy[1] = false;
                    match result {
                        Ok((players, meta)) => {
                            s.charge(&meta);
                            s.players = players;
                        }
                        Err(_) => s.budget -= 1.0,
                    }
                }
                Event::Rules { id, result } => {
                    let Some(s) = self.servers.iter_mut().find(|s| s.id == id) else {
                        continue;
                    };
                    s.busy[2] = false;
                    match result {
                        Ok((rules, meta)) => {
                            s.charge(&meta);
                            s.rules = rules;
                        }
                        Err(_) => s.budget -= 1.0,
                    }
                }
                Event::Rcon { id, event } => {
                    let Some(s) = self.servers.iter_mut().find(|s| s.id == id) else {
                        continue;
                    };
                    match event {
                        RconEvent::Status(status) => {
                            if status == RconStatus::Ok {
                                s.last_status = None; // fetch player details right away
                            }
                            s.rcon_status = status;
                        }
                        RconEvent::Output { tag, text } => match tag {
                            RconTag::Console => {
                                if !text.trim().is_empty() {
                                    s.console_add(LineKind::Response, &text, false);
                                }
                            }
                            RconTag::Silent => {
                                if !text.trim().is_empty() {
                                    s.console_add(LineKind::Response, &text, false);
                                }
                            }
                            RconTag::BanIds | RconTag::BanIps => {
                                if !text.trim().is_empty() {
                                    s.console_add(LineKind::Response, &text, false);
                                }
                                let is_ip = tag == RconTag::BanIps;
                                s.bans.retain(|b| b.is_ip != is_ip);
                                s.bans.extend(parse_ban_list(&text, is_ip));
                                s.ban_selected = None;
                            }
                            RconTag::Maps => {
                                if !text.trim().is_empty() {
                                    s.console_add(LineKind::Response, &text, false);
                                }
                                let found = parse_map_list(&text);
                                // The answer is in the console already; only an empty one needs a note.
                                if found.is_empty() && text.trim().is_empty() {
                                    s.append_console(&tr!("*** maps * returned no map names. Server answer:\n{answer}", answer = if text.trim().is_empty() {
                                            tr!("(empty)")
                                        } else {
                                            text.trim_end()
                                        }));
                                }
                                merge_maps(&mut s.maps, found);
                                // `maps *` does not list maps inside the game's VPK files, so
                                // add the stock maps of the game.
                                if let Some(i) = &s.info {
                                    let stock = stock_maps(i.real_app_id(), &i.folder);
                                    merge_maps(
                                        &mut s.maps,
                                        stock.iter().map(|m| m.to_string()).collect(),
                                    );
                                }
                            }
                            RconTag::MapCycle => {
                                if !text.trim().is_empty() {
                                    s.console_add(LineKind::Response, &text, false);
                                }
                                merge_maps(&mut s.maps, parse_map_cycle(&text));
                                s.maps_loading = false;
                                s.maps_status = if s.maps.is_empty() {
                                    tr!("No maps found. The server's answer is in the Console tab.")
                                        .into()
                                } else {
                                    tr!("{count} maps", count = s.maps.len())
                                };
                            }
                            RconTag::PlayerStatus => {
                                s.console_add(LineKind::Response, &text, true);
                                s.status_players = parse_status_players(&text);
                                if self.settings.iddb_auto_add {
                                    for pl in &s.status_players {
                                        if self.iddb.observe(&pl.steam_id, &pl.name, true) {
                                            self.iddb_dirty = true;
                                        }
                                    }
                                }
                            }
                            RconTag::KickAllStatus => {
                                if !text.trim().is_empty() {
                                    s.console_add(LineKind::Response, &text, false);
                                }
                                let ids = parse_status_userids(&text);
                                for uid in &ids {
                                    let cmd = s.kick_cmd(*uid, tr!("Map change"));
                                    s.rcon_send(cmd, RconTag::Silent);
                                }
                                if let Some(map) = s.pending_change.take() {
                                    s.append_console(&tr!(
                                        "*** Kicked {count} players, changelevel {map}",
                                        count = ids.len(),
                                        map = map
                                    ));
                                    s.rcon_send(format!("changelevel {map}"), RconTag::Console);
                                }
                            }
                        },
                        RconEvent::Error(msg) => {
                            if s.maps_loading {
                                s.maps_loading = false;
                                s.maps_status = tr!("Could not load maps: {msg}", msg = msg);
                            }
                            s.pending_change = None;
                            s.append_console(&format!("*** {msg}"));
                        }
                        RconEvent::Sent { command, answer } => {
                            s.console_add(LineKind::Command, &command, false);
                            if !answer.trim().is_empty() {
                                s.console_add(LineKind::Response, &answer, false);
                            }
                        }
                        RconEvent::LogStarted(Ok(target)) => {
                            let internet = s.log_target.map(|(_, i)| i).unwrap_or(false);
                            s.log_target = Some((target, internet));
                            s.append_console(&tr!(
                                "*** Server log is now sent to {target}",
                                target = target
                            ));
                            self.status_message = tr!(
                                "Receiving log of {address} on {target}",
                                address = s.address,
                                target = target
                            );
                        }
                        RconEvent::LogStarted(Err(e)) => {
                            s.log_target = None;
                            s.append_console(&tr!("*** Get Log failed: {e}", e = e));
                            self.status_message = tr!("Get Log failed: {e}", e = e);
                        }
                    }
                }
                Event::Log { from, line } => self.handle_log_line(from, line),
                Event::Geo(result) => match result {
                    Ok(list) => {
                        for (ip, info) in list {
                            self.geo_pending.remove(&ip);
                            self.geo.insert(ip, info);
                        }
                        geo::save_cache(&self.geo);
                    }
                    Err(e) => {
                        self.geo_pending.clear(); // try again on the next round
                        self.status_message = tr!("Location lookup failed: {e}", e = e);
                    }
                },
            }
        }
        if went_offline {
            self.play(SoundEvent::Notification);
        }
    }

    fn tick(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.lan_search
            && let Ok(result) = rx.try_recv()
        {
            self.lan_search = None;
            match result {
                Ok(found) => {
                    let saved: Vec<SavedServer> = found
                        .iter()
                        .map(|a| SavedServer {
                            address: a.to_string(),
                            ..Default::default()
                        })
                        .collect();
                    let added = self.add_saved_servers(&saved);
                    self.status_message = tr!(
                        "LAN search: {count} servers found, {added} added",
                        count = found.len(),
                        added = added
                    );
                    if added > 0 {
                        self.last_list_refresh = None;
                        self.save_config();
                    }
                }
                Err(e) => self.status_message = tr!("LAN search failed: {e}", e = e),
            }
        }
        if self.iddb_dirty && self.iddb_saved.elapsed() >= Duration::from_secs(10) {
            self.save_iddb();
        }
        if self.paused_for_game {
            if ctx.input(|i| i.viewport().minimized) == Some(true) {
                return; // OnConnect::MinimizeNoTraffic
            }
            self.paused_for_game = false;
        }
        let interval = Duration::from_secs_f32(self.settings.refresh_interval);
        let due = match self.last_list_refresh {
            None => true,
            Some(t) => t.elapsed() >= interval,
        };
        if due {
            if self.last_list_refresh.is_none() || self.settings.auto_refresh {
                self.refresh_all(false);
            } else if self.auto_update_status
                && let Some(id) = self.selected
            {
                self.refresh(id, false);
                self.last_list_refresh = Some(Instant::now());
            }
        }

        self.lookup_locations();
        self.poll_player_status(ctx);

        // "Connecting...": join as soon as the full server has a free slot.
        if let Some((id, _)) = self.waiting {
            match self.server(id) {
                None => self.waiting = None,
                Some(s) => {
                    let free = s
                        .info
                        .as_ref()
                        .is_some_and(|i| i.max_players == 0 || s.humans() < i.max_players);
                    if free && !s.is_offline() {
                        self.waiting = None;
                        self.play(SoundEvent::Notification);
                        self.connect(id, ctx);
                    } else if due && !self.settings.auto_refresh {
                        self.refresh(id, false);
                    }
                }
            }
        }

        let elapsed = self.traffic.at.elapsed();
        if elapsed >= Duration::from_secs(1) {
            let now = openhlsw::stats::totals();
            let secs = elapsed.as_secs_f64();
            self.traffic.rate = (
                ((now.0 - self.traffic.last.0) as f64 / secs) as u64,
                ((now.1 - self.traffic.last.1) as f64 / secs) as u64,
            );
            self.traffic.last = now;
            self.traffic.at = Instant::now();
        }
    }

    fn save_config(&mut self) {
        let config = Config {
            servers: self.servers.iter().map(Self::saved_server).collect(),
            list_name: self.list_name.clone(),
            other_lists: self.other_lists.clone(),
            connect_log: self.connect_log.clone(),
            selected: self
                .selected
                .and_then(|id| self.server(id))
                .map(|s| s.address.to_string()),
            hide_rcon_password: self.hide_password,
            auto_update_status: self.auto_update_status,
            settings: self.settings.clone(),
        };
        if let Err(e) = config::save(&config) {
            self.status_message = tr!("Could not save settings: {e}", e = e);
        }
    }

    fn apply(&mut self, action: Action, ctx: &egui::Context) {
        let action_name = match &action {
            Action::MoveToList(..) => "move",
            _ => "",
        };
        match action {
            Action::Select(id) => {
                if let Some(info) = self.server(id).and_then(|s| s.info.as_ref()) {
                    self.add_game = add_game_of(info);
                }
                if self.selected != Some(id) {
                    self.selected = Some(id);
                    self.history_pos = None;
                    self.refresh(id, true);
                    self.save_config();
                }
            }
            Action::Add => {
                let input = self.address_input.trim().to_string();
                if input.is_empty() {
                    return;
                }
                let Some(mut address) = ServerAddress::parse(&input) else {
                    self.status_message = tr!("Invalid address: {input}", input = input);
                    return;
                };
                let gamespy = self
                    .add_game
                    .is_some_and(|g| g.protocol == games::Protocol::GameSpy2);
                if gamespy && address.query_port.is_none() {
                    // Default query port of the game (e.g. 2303 for ArmA), if HLSW knows one.
                    if let Some(port) = self
                        .add_game
                        .map(|g| g.default_query_port)
                        .filter(|&p| p > 0)
                    {
                        address.query_port = Some(port);
                    }
                }
                if gamespy && address.query_port.is_none() {
                    // GameSpy 2 answers on a separate query port.
                    self.status_message = tr!(
                        "GameSpy 2 servers need the query port: ip:gameport:queryport (e.g. {host}:{port}:34567)",
                        host = address.host,
                        port = address.port
                    );
                    return;
                }
                if let Some(existing) = self.servers.iter().find(|s| s.address == address) {
                    let id = existing.id;
                    self.status_message =
                        tr!("{address} is already in the list", address = address);
                    self.apply(Action::Select(id), ctx);
                } else {
                    let id = self.alloc_id();
                    self.status_message = tr!("Added {address}", address = address);
                    let mut server = Server::new(id, address, &self.notifier);
                    server.console_cache = self.settings.console_cache.max(1);
                    self.servers.push(server);
                    self.apply(Action::Select(id), ctx);
                    self.save_config();
                }
                self.address_input.clear();
            }
            Action::Refresh(id) => self.refresh(id, true),
            Action::RefreshAll => self.refresh_all(true),
            Action::Remove(id) => {
                if self.server(id).is_some() {
                    self.dialog = Some(Dialog::Confirm(Confirm::DeleteServer(id)));
                }
            }
            Action::RemoveConfirmed(id) => {
                let Some(idx) = self.index_of(id) else { return };
                let removed = self.servers.remove(idx);
                self.status_message = tr!("Removed {address}", address = removed.address);
                if self.selected == Some(id) {
                    self.selected = self
                        .servers
                        .get(idx.min(self.servers.len().saturating_sub(1)))
                        .map(|s| s.id);
                }
                self.save_config();
            }
            Action::Connect(id) => self.request_connect(id, false, ctx),
            Action::ConnectAnyway(id) => self.request_connect(id, true, ctx),
            Action::ConnectNow(id) => {
                self.waiting = None;
                self.connect(id, ctx);
            }
            Action::CancelWait => self.waiting = None,
            Action::FindNext => {
                let results = self.find_results();
                if let Some(find) = &mut self.find
                    && !results.is_empty()
                {
                    let next = find.pos.map_or(0, |p| (p + 1) % results.len());
                    find.pos = Some(next);
                    let (id, _) = results[next];
                    match find.kind {
                        FindKind::Player => self.player_filter = find.text.clone(),
                        FindKind::Rule => self.rules_filter = find.text.clone(),
                    }
                    self.apply(Action::Select(id), ctx);
                }
            }
            Action::CopyText(text) => {
                self.status_message = tr!("Copied {text}", text = text);
                ctx.copy_text(text);
            }
            Action::OpenUrl(url) => {
                self.status_message = match open_url(&url) {
                    Ok(()) => tr!("Opened {url}", url = url),
                    Err(e) => tr!("Could not open {url}: {e}", url = url, e = e),
                };
            }
            Action::NewList(name) => {
                let name = name.trim().to_string();
                if name.is_empty() || self.list_names().contains(&name) {
                    self.status_message =
                        tr!("A server list named \"{name}\" already exists", name = name);
                    return;
                }
                let current = SavedList {
                    name: std::mem::replace(&mut self.list_name, name),
                    servers: self.servers.iter().map(Self::saved_server).collect(),
                };
                self.other_lists.push(current);
                self.servers.clear();
                self.selected = None;
                self.list_order.clear();
                self.status_message = tr!(
                    "Created server list {list_name}",
                    list_name = self.list_name
                );
                self.save_config();
            }
            Action::OpenList(name) => self.switch_list(&name, true),
            Action::DeleteList => {
                let next = self.list_names().into_iter().find(|n| *n != self.list_name);
                match next {
                    Some(next) => {
                        let old = self.list_name.clone();
                        self.switch_list(&next, false);
                        self.status_message = tr!("Deleted server list {old}", old = old);
                    }
                    None => {
                        self.status_message = tr!("The last server list cannot be deleted").into()
                    }
                }
            }
            Action::RenameList(name) => {
                let name = name.trim().to_string();
                if !name.is_empty() && !self.other_lists.iter().any(|l| l.name == name) {
                    self.list_name = name;
                    self.save_config();
                }
            }
            Action::ClearGeoCache => {
                self.geo.clear();
                self.geo_pending.clear();
                geo::save_cache(&self.geo);
                self.status_message = tr!("Location and provider cache cleared").into();
            }
            Action::ResetAllPings => {
                for s in &mut self.servers {
                    s.ping_history.clear();
                }
            }
            Action::ImportSteamFavorites => {
                let mut files = discover::steam_history_files();
                if files.is_empty() {
                    let Some(path) = rfd::FileDialog::new()
                        .set_title(tr!("Import Steam Favorites"))
                        .set_file_name("serverbrowser_hist.vdf")
                        .add_filter("serverbrowser_hist.vdf", &["vdf"])
                        .pick_file()
                    else {
                        return;
                    };
                    files.push(path);
                }
                let mut servers = Vec::new();
                for f in &files {
                    match discover::read_server_browser(f) {
                        Ok(list) => servers.extend(list.into_iter().filter(|s| s.favorite)),
                        Err(e) => {
                            self.status_message =
                                tr!("Could not read {path}: {e}", path = f.display(), e = e);
                            return;
                        }
                    }
                }
                let saved: Vec<SavedServer> = servers
                    .iter()
                    .map(|s| SavedServer {
                        address: s.address.clone(),
                        comment: s.name.clone(),
                        ..Default::default()
                    })
                    .collect();
                let added = self.add_saved_servers(&saved);
                self.status_message = tr!(
                    "Imported {added} of {count} Steam favorites",
                    added = added,
                    count = saved.len()
                );
                if added > 0 {
                    self.last_list_refresh = None;
                    self.save_config();
                }
            }
            Action::LanSearch => {
                if self.lan_search.is_some() {
                    return;
                }
                let (tx, rx) = std::sync::mpsc::channel();
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(discover::lan_search(Duration::from_millis(1500)));
                    ctx.request_repaint();
                });
                self.lan_search = Some(rx);
                self.status_message = tr!("Searching the LAN for servers…").into();
            }
            Action::ImportList => {
                let Some(path) = rfd::FileDialog::new()
                    .set_title(tr!("Import server list"))
                    .add_filter(tr!("Server list"), &["txt", "json", "cfg"])
                    .add_filter(tr!("All files"), &["*"])
                    .pick_file()
                else {
                    return;
                };
                match std::fs::read_to_string(&path) {
                    Ok(text) => {
                        let saved = parse_server_list_file(&text);
                        let added = self.add_saved_servers(&saved);
                        self.status_message = tr!(
                            "Imported {added} of {count} servers from {path}",
                            added = added,
                            count = saved.len(),
                            path = path.display()
                        );
                        self.last_list_refresh = None;
                        self.save_config();
                    }
                    Err(e) => {
                        self.status_message =
                            tr!("Could not read {path}: {e}", path = path.display(), e = e)
                    }
                }
            }
            Action::ExportList => {
                let Some(path) = rfd::FileDialog::new()
                    .set_title(tr!("Export server list"))
                    .set_file_name(format!("{}.txt", self.list_name))
                    .add_filter(tr!("Text (one address per line)"), &["txt"])
                    .add_filter(tr!("openHLSW JSON"), &["json"])
                    .save_file()
                else {
                    return;
                };
                let is_json = path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("json"));
                let content = if is_json {
                    let list = SavedList {
                        name: self.list_name.clone(),
                        servers: self
                            .servers
                            .iter()
                            .map(|s| SavedServer {
                                rcon_password: None,
                                ..Self::saved_server(s)
                            })
                            .collect(),
                    };
                    serde_json::to_string_pretty(&list).unwrap_or_default()
                } else {
                    self.servers
                        .iter()
                        .map(|s| {
                            if s.comment.is_empty() {
                                s.address.to_string()
                            } else {
                                format!("{} {}", s.address, s.comment)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                        + "\n"
                };
                self.status_message = match std::fs::write(&path, content) {
                    Ok(()) => tr!(
                        "Exported {count} servers to {path}",
                        count = self.servers.len(),
                        path = path.display()
                    ),
                    Err(e) => tr!("Could not write {path}: {e}", path = path.display(), e = e),
                };
            }
            Action::MultiInsert(text) => {
                let saved = parse_server_list_file(&text);
                let added = self.add_saved_servers(&saved);
                self.status_message = tr!("Added {added} servers", added = added);
                self.last_list_refresh = None;
                self.save_config();
            }
            Action::MoveToList(id, target) | Action::CopyToList(id, target) => {
                let moving = matches!(action_name, "move");
                let Some(s) = self.server(id) else { return };
                let saved = Self::saved_server(s);
                let Some(list) = self.other_lists.iter_mut().find(|l| l.name == target) else {
                    return;
                };
                if !list.servers.iter().any(|x| x.address == saved.address) {
                    list.servers.push(saved);
                }
                if moving {
                    self.servers.retain(|s| s.id != id);
                    if self.selected == Some(id) {
                        self.selected = self.servers.first().map(|s| s.id);
                    }
                    self.status_message = tr!("Moved server to {target}", target = target);
                } else {
                    self.status_message = tr!("Copied server to {target}", target = target);
                }
                self.save_config();
            }
            Action::FocusSearch => {
                self.settings.view.filter = true;
                if let Some(search) = self.search_id {
                    ctx.memory_mut(|m| m.request_focus(search));
                }
            }
            Action::OpenDialog(dialog) => self.dialog = Some(dialog),
            Action::ApplyServerProps(id, draft) => {
                let address = ServerAddress::parse(&draft.address);
                if let Some(s) = self.server_mut(id) {
                    s.comment = draft.comment.trim().to_string();
                    s.params = draft.params.trim().to_string();
                    s.save_password = draft.save_password;
                    if s.rcon_password != draft.rcon_password {
                        s.rcon_password = draft.rcon_password;
                        s.rcon_status = RconStatus::Unknown;
                        s.configure_rcon();
                    }
                    if let Some(address) = address
                        && address != s.address
                    {
                        s.address = address;
                        s.info = None;
                        s.resolved = None;
                        s.challenge = None;
                        s.ping_history.clear();
                        s.state = QueryState::Unknown;
                        s.configure_rcon();
                    }
                }
                self.refresh(id, true);
                self.save_config();
            }
            Action::ResetPing(id) => {
                if let Some(s) = self.server_mut(id) {
                    s.ping_history.clear();
                    if let Some(p) = s.ping {
                        s.ping_history.push_back(Some(p));
                    }
                }
            }
            Action::RconStatus(id) => {
                if let Some(s) = self.server_mut(id) {
                    s.configure_rcon();
                    s.rcon_send("status", RconTag::Console);
                }
                self.tab = Tab::Console;
                self.settings.view.rcon_section = true;
            }
            Action::TraceRoute(id) => {
                if let Some(s) = self.server(id) {
                    let host = s
                        .resolved
                        .map(|a| a.ip().to_string())
                        .unwrap_or_else(|| s.address.host.clone());
                    let mut t = crate::trace::TraceRoute::new(host);
                    t.start(ctx);
                    self.trace = Some(t);
                }
            }
            Action::Minimize { no_traffic } => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                self.paused_for_game = no_traffic;
            }
            Action::OpenAppData => {
                if let Some(dir) = config::path().and_then(|p| p.parent().map(|d| d.to_path_buf()))
                {
                    let _ = std::fs::create_dir_all(&dir);
                    if let Err(e) = open_url(&dir.display().to_string()) {
                        self.status_message =
                            tr!("Could not open {dir}: {e}", dir = dir.display(), e = e);
                    }
                }
            }
            Action::EditComment(id) => {
                if let Some(s) = self.server(id) {
                    self.editing_comment = Some((id, s.comment.clone()));
                }
            }
            Action::RconConfigure(id) => {
                if let Some(s) = self.server_mut(id) {
                    s.rcon_status = RconStatus::Unknown;
                    s.configure_rcon();
                }
            }
            Action::RconTest(id) => {
                if let Some(s) = self.server(id) {
                    s.configure_rcon();
                    s.rcon.send(RconRequest::Test);
                }
            }
            Action::RconDisconnect(id) => {
                if let Some(s) = self.server(id) {
                    s.rcon.send(RconRequest::Disconnect);
                }
            }
            Action::SendConsole(id) => {
                let command = self.console_input.trim().to_string();
                if command.is_empty() {
                    return;
                }
                if self.history.last() != Some(&command) {
                    self.history.push(command.clone());
                }
                self.history_pos = None;
                self.console_input.clear();
                if let Some(s) = self.server_mut(id) {
                    s.configure_rcon();
                    s.rcon_send(command, RconTag::Console);
                }
            }
            Action::ClearConsole(id) => {
                if let Some(s) = self.server_mut(id) {
                    s.console.clear();
                }
            }
            Action::SwitchTab(tab) => {
                self.tab = tab;
                // Load ban list / map list the first time the tab is opened.
                if let Some(s) = self.selected.and_then(|id| self.server(id))
                    && !s.rcon_password.is_empty()
                {
                    match tab {
                        Tab::BanList if s.bans.is_empty() => {
                            self.apply(Action::FetchBans(s.id), ctx)
                        }
                        Tab::ChangeLevel if s.maps.is_empty() && !s.maps_loading => {
                            self.apply(Action::FetchMaps(s.id), ctx)
                        }
                        _ => {}
                    }
                }
            }
            Action::BanCommand(id, cmd) => {
                if let Some(s) = self.server_mut(id) {
                    s.configure_rcon();
                    for c in cmd.split(';') {
                        s.rcon_send(c, RconTag::Console);
                    }
                }
                self.apply(Action::FetchBans(id), ctx);
            }
            Action::FetchBans(id) => {
                // Shown in the console like in HLSW, so the raw server answer can be checked.
                if let Some(s) = self.server_mut(id) {
                    s.configure_rcon();
                    s.rcon_send("listid", RconTag::BanIds);
                    s.rcon_send("listip", RconTag::BanIps);
                }
            }
            Action::FetchMaps(id) => {
                if let Some(s) = self.server_mut(id) {
                    if s.rcon_password.is_empty() {
                        s.maps_status = tr!("No RCON password set (Rcon Config tab).").into();
                        return;
                    }
                    s.maps.clear();
                    s.maps_loading = true;
                    s.maps_status = tr!("Loading maps…").into();
                    s.configure_rcon();
                    s.rcon_send("maps *", RconTag::Maps);
                    s.rcon_send("listmaps", RconTag::MapCycle);
                }
            }
            Action::Unban(id) => {
                if let Some(s) = self.server_mut(id)
                    && let Some(ban) = s.ban_selected.and_then(|i| s.bans.get(i)).cloned()
                {
                    let (remove, write) = if ban.is_ip {
                        ("removeip", "writeip")
                    } else {
                        ("removeid", "writeid")
                    };
                    s.configure_rcon();
                    s.rcon_send(format!("{remove} {}", ban.ident), RconTag::Console);
                    s.rcon_send(write, RconTag::Silent);
                    self.apply(Action::FetchBans(id), ctx);
                }
            }
            Action::OpenIdDb(id) => {
                let ed = self.iddb_editor.get_or_insert_with(IdDbEditor::default);
                if let Some(id) = id {
                    ed.select(&self.iddb, &id);
                }
            }
            Action::IdDbSetKind { id, name, kind } => {
                let Some(key) = iddb::normalize(&id) else {
                    return;
                };
                let e = self.iddb.entries.entry(key.clone()).or_default();
                e.auto = false;
                if !e.names.contains(&name) && !name.trim().is_empty() {
                    e.names.push(name);
                }
                match kind {
                    Some(k) => e.kind = k,
                    None => self.apply(Action::OpenIdDb(Some(key)), ctx),
                }
                self.save_iddb();
            }
            Action::IdDbImport => {
                let Some(path) = rfd::FileDialog::new()
                    .set_title(tr!("Import banned.cfg"))
                    .add_filter("banned.cfg", &["cfg", "txt"])
                    .add_filter(tr!("All files"), &["*"])
                    .pick_file()
                else {
                    return;
                };
                let msg = match std::fs::read_to_string(&path) {
                    Ok(text) => {
                        let n = self.iddb.import_banned_cfg(&text);
                        self.save_iddb();
                        tr!("Imported {count} banned IDs", count = n)
                    }
                    Err(e) => tr!("Could not read {path}: {e}", path = path.display(), e = e),
                };
                if let Some(ed) = &mut self.iddb_editor {
                    ed.message = msg.clone();
                }
                self.status_message = msg;
            }
            Action::IdDbExport => {
                let Some(path) = rfd::FileDialog::new()
                    .set_title(tr!("Export banned.cfg"))
                    .set_file_name("banned.cfg")
                    .add_filter("banned.cfg", &["cfg"])
                    .save_file()
                else {
                    return;
                };
                let msg = match std::fs::write(&path, self.iddb.export_banned_cfg()) {
                    Ok(()) => tr!(
                        "Exported {count} banned IDs to {path}",
                        count = self.iddb.banned().count(),
                        path = path.display()
                    ),
                    Err(e) => tr!("Could not write {path}: {e}", path = path.display(), e = e),
                };
                if let Some(ed) = &mut self.iddb_editor {
                    ed.message = msg.clone();
                }
                self.status_message = msg;
            }
            Action::BansToIdDb(id) => {
                let Some(s) = self.servers.iter().find(|s| s.id == id) else {
                    return;
                };
                let mut count = 0;
                for b in s.bans.iter().filter(|b| !b.is_ip) {
                    if let Some(key) = iddb::normalize(&b.ident) {
                        let e = self.iddb.entries.entry(key).or_default();
                        if !e.banned {
                            e.banned = true;
                            count += 1;
                        }
                    }
                }
                self.save_iddb();
                self.status_message = tr!(
                    "Added {count} bans to the Global ID Database",
                    count = count
                );
            }
            Action::BansFromIdDb(id) => {
                let ids: Vec<String> = self.iddb.banned().map(str::to_string).collect();
                if let Some(s) = self.server_mut(id) {
                    s.configure_rcon();
                    for i in &ids {
                        s.rcon_send(format!("banid 0 {i}"), RconTag::Silent);
                    }
                    s.rcon_send("writeid", RconTag::Silent);
                    self.status_message = tr!(
                        "Banned {count} IDs from the Global ID Database",
                        count = ids.len()
                    );
                    self.apply(Action::FetchBans(id), ctx);
                }
            }
            Action::AddBan { id, ip } => {
                let ident = self.ban_input.trim().to_string();
                if ident.is_empty() {
                    return;
                }
                let minutes = self.ban_minutes;
                if let Some(s) = self.server_mut(id) {
                    let (add, write) = if ip {
                        ("addip", "writeip")
                    } else {
                        ("banid", "writeid")
                    };
                    s.configure_rcon();
                    s.rcon_send(format!("{add} {minutes} {ident}"), RconTag::Console);
                    s.rcon_send(write, RconTag::Silent);
                    self.ban_input.clear();
                    self.apply(Action::FetchBans(id), ctx);
                }
            }
            Action::ChangeLevel(id, map) => {
                self.dialog = Some(Dialog::Confirm(Confirm::ChangeLevel(id, map)));
            }
            Action::ChangeLevelConfirmed(id, map) => {
                let kick_all = self.settings.kick_all_on_map_change;
                if let Some(s) = self.server_mut(id) {
                    if s.rcon_password.is_empty() {
                        s.maps_status = tr!("No RCON password set (Rcon Config tab).").into();
                        return;
                    }
                    s.configure_rcon();
                    if kick_all {
                        // Kick everybody (userids from `status`), then change the level.
                        s.pending_change = Some(map.clone());
                        s.rcon_send("status", RconTag::KickAllStatus);
                    } else {
                        s.rcon_send(format!("changelevel {map}"), RconTag::Console);
                    }
                    s.maps_status = tr!("Changing level to {map}…", map = map);
                    self.status_message = tr!("Changing level to {map}", map = map);
                }
            }
            Action::Say(id) => {
                let text = self.chat_input.trim().replace('"', "'");
                if text.is_empty() {
                    return;
                }
                self.chat_input.clear();
                if let Some(s) = self.server_mut(id) {
                    // Show the message right away: the server log may not reach us (no Get
                    // Log, firewall, NAT). If it does, its echo only confirms this line.
                    s.chat.push(ChatLine {
                        time: chrono::Local::now().format("%H:%M:%S").to_string(),
                        name: "Console".into(),
                        team: String::new(),
                        team_only: false,
                        text: text.clone(),
                        own: true,
                    });
                    if s.chat.len() > CHAT_MAX_LINES {
                        s.chat.remove(0);
                    }
                    s.configure_rcon();
                    s.rcon_send(format!("say {text}"), RconTag::Silent);
                }
                self.play(SoundEvent::SendingMessage);
            }
            Action::StartLog { id, internet } => {
                if let Err(e) = self.ensure_log_listener() {
                    self.status_message = e;
                    return;
                }
                let port = self.settings.log_port;
                let public_ip = self.settings.public_ip.trim().parse::<IpAddr>().ok();
                if let Some(s) = self.server_mut(id) {
                    s.log_target = Some((SocketAddr::from(([0, 0, 0, 0], port)), internet));
                    s.configure_rcon();
                    s.rcon.send(RconRequest::StartLog {
                        internet,
                        port,
                        public_ip,
                    });
                    self.status_message = tr!("Requesting log of {address}…", address = s.address);
                }
            }
            Action::KickPlayer(id, userid) => {
                if let Some(s) = self.server_mut(id) {
                    let cmd = s.kick_cmd(userid, tr!("Kicked by admin"));
                    s.configure_rcon();
                    s.rcon_send(cmd, RconTag::Console);
                    s.last_status = None;
                }
            }
            Action::KickAndConnect(id, userid) => {
                self.apply(Action::KickPlayer(id, userid), ctx);
                self.apply(Action::Connect(id), ctx);
            }
            Action::BanPlayer {
                id,
                userid,
                target,
                ip,
                kick,
                minutes,
            } => {
                if let Some(s) = self.server_mut(id) {
                    s.configure_rcon();
                    let mut cmds = Vec::new();
                    if ip {
                        cmds.push(format!("addip {minutes} {target}"));
                        if kick {
                            cmds.push(s.kick_cmd(userid, tr!("Banned by admin")));
                        }
                    } else {
                        // banid with the Steam ID keeps working after the player left.
                        let target = if target.starts_with("STEAM_") || target.starts_with("[U:") {
                            target
                        } else {
                            userid.to_string()
                        };
                        cmds.push(if kick {
                            format!("banid {minutes} {target} kick")
                        } else {
                            format!("banid {minutes} {target}")
                        });
                    }
                    for cmd in cmds {
                        s.rcon_send(cmd, RconTag::Console);
                    }
                    s.rcon_send(if ip { "writeip" } else { "writeid" }, RconTag::Silent);
                    s.last_status = None;
                    s.bans.clear(); // reload the ban list next time
                }
            }
            Action::MoveServer(id, delta) => {
                if let Some(i) = self.index_of(id) {
                    let j =
                        (i as i64 + delta as i64).clamp(0, self.servers.len() as i64 - 1) as usize;
                    if i != j {
                        let s = self.servers.remove(i);
                        self.servers.insert(j, s);
                        self.save_config();
                    }
                }
            }
            Action::SetServerId(id, n) => {
                if let Some(i) = self.index_of(id) {
                    let j = n.clamp(1, self.servers.len()) - 1;
                    let s = self.servers.remove(i);
                    self.servers.insert(j, s);
                    self.save_config();
                }
            }
            Action::RenumberServers => {
                let order = self.list_order.clone();
                self.servers.sort_by_key(|s| {
                    order
                        .iter()
                        .position(|id| *id == s.id)
                        .unwrap_or(usize::MAX)
                });
                self.save_config();
                self.status_message = tr!("Server IDs set to the current sequence").into();
            }
            Action::ToggleColumn { players, title } => {
                let v = &mut self.settings.view;
                if !players && title == "Comment" {
                    v.comment = !v.comment;
                } else {
                    // Server list: list of hidden columns; players: list of shown columns.
                    let list = if players {
                        &mut v.player_columns
                    } else {
                        &mut v.hidden_server_columns
                    };
                    if let Some(i) = list.iter().position(|h| *h == title) {
                        list.remove(i);
                    } else {
                        list.push(title);
                    }
                }
                self.save_config();
            }
            Action::ResetColumns { players } => {
                let v = &mut self.settings.view;
                if players {
                    v.player_columns_reset = v.player_columns_reset.wrapping_add(1);
                } else {
                    v.server_columns_reset = v.server_columns_reset.wrapping_add(1);
                }
                self.save_config();
            }
            Action::SetCvar(id, key, value) => {
                if let Some(s) = self.server_mut(id) {
                    let cmd = format!("{key} \"{}\"", value.replace('"', "'"));
                    s.configure_rcon();
                    s.rcon_send(cmd, RconTag::Console);
                    s.last_rules = None; // show the new value soon
                }
            }
            Action::SetTheme(theme) => {
                if self.settings.theme != theme {
                    self.settings.theme = theme;
                    theme::apply(ctx, theme);
                    self.save_config();
                }
            }
            Action::OpenSettings(tab) => {
                self.settings_dialog = Some(SettingsDialog::new(&self.settings, tab))
            }
            Action::ApplySettings(new) => {
                let mut new = *new;
                new.refresh_interval =
                    sanitize_interval(new.refresh_interval, new.max_queries_per_sec);
                if new.theme != self.settings.theme {
                    theme::apply(ctx, new.theme);
                }
                if new.text_size != self.settings.text_size {
                    ctx.set_zoom_factor(new.text_size.zoom());
                }
                if new.font != self.settings.font {
                    theme::install_fonts(ctx, new.font);
                }
                if new.language != self.settings.language {
                    crate::i18n::set_language(new.language);
                }
                let port_changed = new.log_port != self.settings.log_port;
                self.settings = new;
                if let Some(dialog) = &mut self.settings_dialog {
                    dialog.draft = self.settings.clone();
                }
                if port_changed && self.log_listener.is_some() {
                    self.status_message = match self.ensure_log_listener() {
                        Ok(port) => {
                            tr!("Listening for server logs on UDP port {port}", port = port)
                        }
                        Err(e) => e,
                    };
                }
                let cache = self.settings.console_cache;
                for s in &mut self.servers {
                    s.trim_console(cache);
                }
                self.save_config();
            }
            Action::ToggleView(icon) => {
                use crate::toolbar_icons::ToolIcon;
                let v = &mut self.settings.view;
                match icon {
                    ToolIcon::Rcon => v.rcon_section = !v.rcon_section,
                    ToolIcon::ServerList => v.server_list = !v.server_list,
                    ToolIcon::Rules => v.rules = !v.rules,
                    _ => {}
                }
                self.save_config();
            }
            Action::ShowAbout => self.show_about = true,
            Action::ToggleAutoRefresh => {
                self.settings.auto_refresh = !self.settings.auto_refresh;
                self.save_config();
            }
            Action::SaveConfig => self.save_config(),
            Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }
}

impl eframe::App for OpenHlswApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.iddb_dirty {
            self.save_iddb();
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        theme::snap_text_sizes(ui.ctx());
        self.process_events();
        self.tick(&ui.ctx().clone());
        // While minimized the window has (almost) no size: laying out the panels now would
        // shrink them and that size would be remembered. Draw nothing until it is restored.
        if ui.ctx().input(|i| i.viewport().minimized) == Some(true) {
            return;
        }

        let p = self.settings.theme.palette();
        let mut actions = Vec::new();
        self.shortcuts(ui, &mut actions);

        let chrome = Frame::new()
            .fill(p.window)
            .inner_margin(Margin::symmetric(6, 3));
        // A little more room above the menu, so it does not stick to the window edge.
        let menu_frame = Frame::new().fill(p.window).inner_margin(Margin {
            left: 6,
            right: 6,
            top: 5,
            bottom: 3,
        });
        egui::Panel::top("menu")
            .frame(menu_frame)
            .show(ui, |ui| self.menu_bar(ui, &mut actions));
        egui::Panel::top("toolbar")
            .frame(chrome)
            .show_separator_line(false)
            .show(ui, |ui| self.toolbar(ui, &p, &mut actions));
        let view = self.settings.view.clone();
        if view.status_bar {
            egui::Panel::bottom("statusbar")
                .frame(chrome)
                .show(ui, |ui| self.status_bar(ui, &p));
        }
        if view.rcon_section {
            egui::Panel::bottom("tools")
                .frame(chrome)
                .resizable(true)
                .default_size(150.0)
                .min_size(110.0)
                .show(ui, |ui| self.tools_panel(ui, &p, &mut actions));
        }
        if view.server_list {
            // Like HLSW: the server list takes a bit less than half of the window, the
            // details (info, players, rules) get the rest above the Rcon section.
            let list_height = 300.0;
            egui::Panel::top("server_list")
                .frame(chrome)
                .resizable(true)
                .default_size(list_height)
                .min_size(80.0)
                .show(ui, |ui| {
                    let needle = self.search.trim().to_lowercase();
                    list_frame(ui, &p, |ui| {
                        let locations: Vec<(String, String)> = self
                            .servers
                            .iter()
                            .map(|s| match self.location_of(s) {
                                Some(Some(g)) => (g.country_code.clone(), g.provider().to_string()),
                                Some(None) => (String::new(), tr!("LAN").into()),
                                None => (String::new(), String::new()),
                            })
                            .collect();
                        let other_lists: Vec<String> = self
                            .list_names()
                            .into_iter()
                            .filter(|n| *n != self.list_name)
                            .collect();
                        let opts = ListOptions {
                            game_icons: self.game_icons.as_ref(),
                            other_lists: &other_lists,
                            show_comment: self.settings.view.comment,
                            hidden: &self.settings.view.hidden_server_columns,
                            generation: self.settings.view.server_columns_reset,
                            flags: self.flags.as_ref(),
                            locations: &locations,
                            needle: &needle,
                            filter: &self.settings.filter,
                            selected: self.selected,
                            connect_on_double_click: self.settings.connect_on_double_click,
                            keep_order_on_hover: self.settings.keep_order_on_hover,
                        };
                        server_table(
                            ui,
                            &p,
                            &self.servers,
                            &opts,
                            &mut self.list_order,
                            &mut self.sort,
                            &mut actions,
                        );
                    });
                });
        }
        egui::CentralPanel::default()
            .frame(chrome)
            .show(ui, |ui| self.details(ui, &p, &mut actions));

        let ctx = ui.ctx().clone();
        self.dialogs(&ctx, &mut actions);

        for action in actions {
            self.apply(action, &ctx);
        }
        ctx.request_repaint_after(Duration::from_millis(500));
    }
}

// --------------------------------------------------------------------------
// UI pieces
// --------------------------------------------------------------------------

impl OpenHlswApp {
    fn shortcuts(&mut self, ui: &Ui, actions: &mut Vec<Action>) {
        use egui::{KeyboardShortcut, Modifiers};
        let typing = ui.ctx().memory(|m| m.focused().is_some());
        let selected = self.selected;
        let ctrl = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
        ui.ctx().input_mut(|i| {
            if i.key_pressed(Key::F5) {
                actions.push(Action::RefreshAll);
            }
            if i.key_pressed(Key::F3) {
                if self.find.is_some() {
                    actions.push(Action::FindNext);
                } else if let Some(f) = &mut self.console_find {
                    f.pos += 1;
                    f.scroll = true;
                }
            }
            if i.consume_shortcut(&KeyboardShortcut::new(
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::F,
            )) {
                self.tab = Tab::Console;
                self.console_find.get_or_insert_with(Default::default).focus = true;
            }
            if i.key_pressed(Key::F6)
                && let Some(id) = selected
            {
                actions.push(Action::Refresh(id));
            }
            // While typing in a text field, Ctrl+C/V/X/A/D/... belong to the text field.
            if typing {
                return;
            }
            if i.consume_shortcut(&ctrl(Key::I)) {
                actions.push(Action::OpenDialog(Dialog::MultiInsert(String::new())));
            }
            if i.consume_shortcut(&ctrl(Key::O)) {
                actions.push(Action::OpenDialog(Dialog::ListProperties(
                    self.list_name.clone(),
                )));
            }
            if i.consume_shortcut(&ctrl(Key::F)) {
                actions.push(Action::FocusSearch);
            }
            if i.consume_shortcut(&ctrl(Key::W)) {
                actions.push(Action::OpenIdDb(None));
            }
            if i.consume_shortcut(&ctrl(Key::M)) {
                actions.push(Action::Minimize { no_traffic: true });
            }
            let view = &mut self.settings.view;
            let mut toggled = false;
            for (key, flag) in [
                (Key::R, &mut view.rcon_section),
                (Key::L, &mut view.server_list),
                (Key::U, &mut view.rules),
            ] {
                if i.consume_shortcut(&ctrl(key)) {
                    *flag = !*flag;
                    toggled = true;
                }
            }
            if toggled {
                actions.push(Action::SaveConfig);
            }
            if let Some(id) = selected {
                if i.consume_shortcut(&ctrl(Key::G)) {
                    actions.push(Action::Connect(id));
                }
                if i.consume_shortcut(&ctrl(Key::D)) || i.key_pressed(Key::Delete) {
                    actions.push(Action::Remove(id));
                }
                if i.consume_shortcut(&ctrl(Key::P)) {
                    actions.push(Action::OpenDialog(self.server_props_dialog(id)));
                }
                if i.consume_shortcut(&ctrl(Key::T)) {
                    actions.push(Action::TraceRoute(id));
                }
                // Change ID
                if i.key_pressed(Key::Plus) {
                    actions.push(Action::MoveServer(id, 1));
                }
                if i.key_pressed(Key::Minus) {
                    actions.push(Action::MoveServer(id, -1));
                }
            }
        });
    }

    fn server_props_dialog(&self, id: ServerId) -> Dialog {
        let s = self.server(id);
        Dialog::ServerProperties(
            id,
            ServerPropsDraft {
                address: s.map(|s| s.address.to_string()).unwrap_or_default(),
                comment: s.map(|s| s.comment.clone()).unwrap_or_default(),
                params: s.map(|s| s.params.clone()).unwrap_or_default(),
                rcon_password: s.map(|s| s.rcon_password.clone()).unwrap_or_default(),
                save_password: s.is_some_and(|s| s.save_password),
            },
        )
    }

    fn menu_bar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let selected = self.selected;
        let other_lists: Vec<String> = self
            .list_names()
            .into_iter()
            .filter(|n| *n != self.list_name)
            .collect();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(tr!("File"), |ui| {
                if ui
                    .button(tr!("Global Player ID Database Editor...\tCtrl+W"))
                    .clicked()
                {
                    actions.push(Action::OpenIdDb(None));
                    ui.close();
                }
                ui.separator();
                if ui.button(tr!("View Connect Log")).clicked() {
                    actions.push(Action::OpenDialog(Dialog::ConnectLog));
                    ui.close();
                }
                if ui.button(tr!("View openHLSW AppData Directory")).clicked() {
                    actions.push(Action::OpenAppData);
                    ui.close();
                }
                ui.menu_button(tr!("Advanced"), |ui| {
                    if ui
                        .button(tr!("Clear location and provider cache"))
                        .clicked()
                    {
                        actions.push(Action::ClearGeoCache);
                        ui.close();
                    }
                    if ui.button(tr!("Reset ping graphs of all servers")).clicked() {
                        actions.push(Action::ResetAllPings);
                        ui.close();
                    }
                });
                ui.separator();
                if ui.button(tr!("Minimize (no traffic)\tCtrl+M")).clicked() {
                    actions.push(Action::Minimize { no_traffic: true });
                    ui.close();
                }
                if ui.button(tr!("Minimize")).clicked() {
                    actions.push(Action::Minimize { no_traffic: false });
                    ui.close();
                }
                ui.separator();
                if ui.button(tr!("Exit")).clicked() {
                    actions.push(Action::Quit);
                }
            });
            ui.menu_button(tr!("Server list"), |ui| {
                if ui.button(tr!("New...")).clicked() {
                    actions.push(Action::OpenDialog(Dialog::NewList(String::new())));
                    ui.close();
                }
                ui.add_enabled_ui(!other_lists.is_empty(), |ui| {
                    ui.menu_button(tr!("Open"), |ui| {
                        for name in &other_lists {
                            if ui.button(name).clicked() {
                                actions.push(Action::OpenList(name.clone()));
                                ui.close();
                            }
                        }
                    });
                });
                if ui
                    .add_enabled(!other_lists.is_empty(), Button::new(tr!("Delete")))
                    .clicked()
                {
                    actions.push(Action::OpenDialog(Dialog::ConfirmDeleteList));
                    ui.close();
                }
                ui.separator();
                if ui.button(tr!("Import...")).clicked() {
                    actions.push(Action::ImportList);
                    ui.close();
                }
                if ui.button(tr!("Export...")).clicked() {
                    actions.push(Action::ExportList);
                    ui.close();
                }
                ui.separator();
                if ui.button(tr!("Import Steam Favorites")).clicked() {
                    actions.push(Action::ImportSteamFavorites);
                    ui.close();
                }
                if ui
                    .add_enabled(self.lan_search.is_none(), Button::new(tr!("Search LAN")))
                    .on_hover_text(tr!(
                        "Asks for servers on ports 27015-27030 in the local network"
                    ))
                    .clicked()
                {
                    actions.push(Action::LanSearch);
                    ui.close();
                }
                ui.separator();
                if ui.button(tr!("Multi Server Insert...\tCtrl+I")).clicked() {
                    actions.push(Action::OpenDialog(Dialog::MultiInsert(String::new())));
                    ui.close();
                }
                ui.separator();
                if ui.button(tr!("Refresh\tF5")).clicked() {
                    actions.push(Action::RefreshAll);
                    ui.close();
                }
                if ui
                    .add_enabled(selected.is_some(), Button::new(tr!("Update\tF6")))
                    .clicked()
                {
                    actions.push(Action::Refresh(selected.unwrap_or_default()));
                    ui.close();
                }
                ui.separator();
                if ui.button(tr!("Properties...\tCtrl+O")).clicked() {
                    actions.push(Action::OpenDialog(Dialog::ListProperties(
                        self.list_name.clone(),
                    )));
                    ui.close();
                }
            });
            ui.menu_button(tr!("Server"), |ui| {
                ui.add_enabled_ui(selected.is_some(), |ui| {
                    if let Some(s) = selected.and_then(|id| self.server(id)) {
                        server_menu_items(ui, s, &other_lists, actions);
                    }
                });
                ui.separator();
                ui.menu_button(tr!("Find"), |ui| {
                    if ui.button(tr!("Server Name\tCtrl+F")).clicked() {
                        actions.push(Action::FocusSearch);
                        ui.close();
                    }
                    if ui.button(tr!("Player...")).clicked() {
                        self.find = Some(Find::new(FindKind::Player));
                        ui.close();
                    }
                    if ui.button(tr!("Rule...")).clicked() {
                        self.find = Some(Find::new(FindKind::Rule));
                        ui.close();
                    }
                    if ui
                        .add_enabled(self.find.is_some(), Button::new(tr!("Find Next\tF3")))
                        .clicked()
                    {
                        actions.push(Action::FindNext);
                        ui.close();
                    }
                });
            });
            ui.menu_button(tr!("Settings"), |ui| {
                if ui.button(tr!("Settings...")).clicked() {
                    actions.push(Action::OpenSettings(SettingsTab::Misc));
                    ui.close();
                }
                ui.menu_button(tr!("Sort Server List"), |ui| {
                    for (label, col) in [
                        (tr!("Unsorted"), Col::Index),
                        (tr!("Name"), Col::Name),
                        (tr!("IP/Port"), Col::Address),
                        (tr!("Game + Name"), Col::GameName),
                        (tr!("Game + IP/Port"), Col::GameAddress),
                    ] {
                        if ui.radio(self.sort.0 == col, label).clicked() {
                            self.sort = (col, true);
                            ui.close();
                        }
                    }
                });
            });
            ui.menu_button(tr!("View"), |ui| {
                let v = &mut self.settings.view;
                let mut changed = false;
                changed |= ui
                    .checkbox(&mut v.rcon_section, tr!("Rcon Section\tCtrl+R"))
                    .changed();
                changed |= ui
                    .checkbox(&mut v.server_list, tr!("Server List\tCtrl+L"))
                    .changed();
                changed |= ui.checkbox(&mut v.rules, tr!("Rules\tCtrl+U")).changed();
                ui.separator();
                changed |= ui.checkbox(&mut v.password, tr!("Password")).changed();
                changed |= ui
                    .checkbox(&mut v.player_name, tr!("Player Name"))
                    .changed();
                changed |= ui.checkbox(&mut v.filter, tr!("Filter")).changed();
                changed |= ui.checkbox(&mut v.comment, tr!("Comment")).changed();
                ui.separator();
                changed |= ui.checkbox(&mut v.toolbar, tr!("Toolbar")).changed();
                changed |= ui.checkbox(&mut v.status_bar, tr!("Status Bar")).changed();
                ui.separator();
                changed |= ui.checkbox(&mut v.ping_graph, tr!("Ping Graph")).changed();
                if changed {
                    actions.push(Action::SaveConfig);
                }
                ui.separator();
                ui.menu_button(tr!("Skin"), |ui| {
                    for theme in [Theme::Light, Theme::Dark] {
                        if ui
                            .radio(self.settings.theme == theme, theme.label())
                            .clicked()
                        {
                            actions.push(Action::SetTheme(theme));
                            ui.close();
                        }
                    }
                });
            });
            // Like HLSW's "?" menu.
            ui.menu_button("?", |ui| {
                if ui.button(tr!("About openHLSW...")).clicked() {
                    self.show_about = true;
                    ui.close();
                }
                ui.separator();
                if ui.button(tr!("ReadMe")).clicked() {
                    actions.push(Action::OpenUrl(format!("{HOMEPAGE}#readme")));
                    ui.close();
                }
                if ui.button(tr!("Changelog")).clicked() {
                    actions.push(Action::OpenUrl(format!(
                        "{HOMEPAGE}/blob/main/CHANGELOG.md"
                    )));
                    ui.close();
                }
                if ui.button(tr!("License...")).clicked() {
                    self.show_license = true;
                    ui.close();
                }
                ui.separator();
                if ui.button(tr!("Visit openHLSW homepage")).clicked() {
                    actions.push(Action::OpenUrl(HOMEPAGE.into()));
                    ui.close();
                }
            });
        });
    }

    /// HLSW's icon toolbar: Connect | Rcon section, server list, rules | Delete |
    /// Refresh, Update | server list and server properties | About.
    fn icon_toolbar(
        &self,
        ui: &mut Ui,
        p: &Palette,
        icons: &crate::toolbar_icons::ToolbarIcons,
        actions: &mut Vec<Action>,
    ) {
        use crate::toolbar_icons::ToolIcon;
        let selected = self.selected;
        let id = selected.unwrap_or_default();
        let has = selected.is_some();
        let view = &self.settings.view;
        let gap = ui.spacing().item_spacing.x;
        ui.spacing_mut().item_spacing.x = 1.0;
        if icons
            .button(ui, ToolIcon::Connect, has, false, tr!("Connect to server"))
            .clicked()
        {
            actions.push(Action::Connect(id));
        }
        toolbar_separator(ui, p);
        let toggles = [
            (ToolIcon::Rcon, view.rcon_section, tr!("Toggle Rcon Section")),
            (ToolIcon::ServerList, view.server_list, tr!("Toggle Server List")),
            (ToolIcon::Rules, view.rules, tr!("Toggle Rules")),
        ];
        for (icon, on, tip) in toggles {
            if icons.button(ui, icon, true, on, tip).clicked() {
                actions.push(Action::ToggleView(icon));
            }
        }
        toolbar_separator(ui, p);
        if icons
            .button(ui, ToolIcon::Delete, has, false, tr!("Delete Server"))
            .clicked()
        {
            actions.push(Action::Remove(id));
        }
        toolbar_separator(ui, p);
        // Like HLSW: "Refresh" switches the automatic refresh on and off (the "Auto refresh
        // every" checkbox); "Update" refreshes all servers once while it is off.
        let auto = self.settings.auto_refresh;
        if icons
            .button(ui, ToolIcon::Refresh, true, auto, tr!("Refresh server list"))
            .clicked()
        {
            actions.push(Action::ToggleAutoRefresh);
        }
        if icons
            .button(ui, ToolIcon::Update, !auto, false, tr!("Update server list"))
            .clicked()
        {
            actions.push(Action::RefreshAll);
        }
        toolbar_separator(ui, p);
        if icons
            .button(ui, ToolIcon::ListProperties, true, false, tr!("Server List Properties"))
            .clicked()
        {
            actions.push(Action::OpenDialog(Dialog::ListProperties(
                self.list_name.clone(),
            )));
        }
        if icons
            .button(ui, ToolIcon::ServerProperties, has, false, tr!("Server Properties"))
            .clicked()
        {
            actions.push(Action::OpenDialog(self.server_props_dialog(id)));
        }
        toolbar_separator(ui, p);
        if icons
            .button(ui, ToolIcon::About, true, false, tr!("About"))
            .clicked()
        {
            actions.push(Action::ShowAbout);
        }
        ui.spacing_mut().item_spacing.x = gap;
    }

    /// HLSW's "Playername:" box: an editable dropdown of the names used before and a ✕
    /// that deletes the current name from it.
    fn player_name_box(&mut self, ui: &mut Ui, p: &Palette, actions: &mut Vec<Action>) {
        ui.label(tr!("Player name:"));
        let item_spacing = ui.spacing().item_spacing;
        ui.spacing_mut().item_spacing.x = 0.0;
        let edit = ui
            .add(
                TextEdit::singleline(&mut self.settings.player_name)
                    .align(egui::Align2::LEFT_CENTER)
                    .hint_text(tr!("from the game"))
                    .desired_width(120.0),
            )
            .with_edit_menu()
            .on_hover_text(tr!("Passed to the game on Connect (+name)"));
        // The arrow sits right on the text field, like a Windows combo box.
        let height = edit.rect.height();
        let (rect, arrow) = ui.allocate_exact_size(egui::vec2(16.0, height), Sense::click());
        let visuals = ui.style().interact(&arrow);
        ui.painter().rect(
            rect,
            0.0,
            if arrow.hovered() { visuals.bg_fill } else { p.list_bg },
            Stroke::new(1.0, ui.visuals().widgets.inactive.bg_stroke.color),
            egui::StrokeKind::Inside,
        );
        let c = rect.center();
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                c + egui::vec2(-3.5, -1.5),
                c + egui::vec2(3.5, -1.5),
                c + egui::vec2(0.0, 2.0),
            ],
            p.text,
            Stroke::NONE,
        ));
        ui.spacing_mut().item_spacing = item_spacing;
        ui.add_space(3.0);

        let mut picked = None;
        let names = &self.settings.player_names;
        // Right-aligned to the arrow and as wide as the box: the list opens under the field.
        egui::Popup::menu(&arrow)
            .width(edit.rect.width() + rect.width())
            .align(egui::RectAlign::BOTTOM_END)
            .gap(0.0)
            .show(|ui| {
                ui.set_min_width(edit.rect.width() + rect.width() - 8.0);
                if names.is_empty() {
                    ui.label(RichText::new(tr!("(empty)")).color(ui.visuals().weak_text_color()));
                }
                for name in names {
                    if ui
                        .selectable_label(*name == self.settings.player_name, name)
                        .clicked()
                    {
                        picked = Some(name.clone());
                    }
                }
            });
        let mut changed = false;
        if let Some(name) = picked {
            self.settings.player_name = name;
            changed = true;
        }
        if edit.lost_focus() || changed {
            remember_player_name(&mut self.settings);
            actions.push(Action::SaveConfig);
        }
        let delete = match &self.toolbar_icons {
            Some(icons) => icons.button(
                ui,
                crate::toolbar_icons::ToolIcon::Delete,
                true,
                false,
                tr!("Delete"),
            ),
            None => ui.add(Button::new("✕").frame_when_inactive(false)),
        };
        if delete.clicked() {
            let name = self.settings.player_name.trim().to_string();
            self.settings.player_names.retain(|n| *n != name);
            self.settings.player_name.clear();
            actions.push(Action::SaveConfig);
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, p: &Palette, actions: &mut Vec<Action>) {
        // Right-aligned labels of the first two rows, as wide as the longer (translated) one.
        let label_w = text_width(ui, tr!("Server list:"))
            .max(text_width(ui, tr!("IP/Port:")))
            .max(52.0)
            + 2.0;
        let label = |ui: &mut Ui, text: &str| {
            ui.allocate_ui_with_layout(
                egui::vec2(label_w, 18.0),
                Layout::right_to_left(Align::Center),
                |ui| {
                    ui.label(text);
                },
            );
        };

        // Server list: [Custom server list ▼]                         N servers
        ui.horizontal(|ui| {
            label(ui, tr!("Server list:"));
            let width = (ui.available_width() - 100.0).max(150.0);
            let names = self.list_names();
            egui::ComboBox::from_id_salt("server_list_source")
                .selected_text(self.list_name.clone())
                .width(width)
                .show_ui(ui, |ui| {
                    for name in names {
                        if ui.selectable_label(name == self.list_name, &name).clicked()
                            && name != self.list_name
                        {
                            actions.push(Action::OpenList(name));
                        }
                    }
                });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.settings.filter.enabled || !self.search.trim().is_empty() {
                    ui.label(tr!(
                        "{shown} of {count} servers",
                        shown = self.list_order.len(),
                        count = self.servers.len()
                    ));
                } else {
                    ui.label(tr!("{count} servers", count = self.servers.len()));
                }
            });
        });

        // IP/Port: [Half-Life 2 ▼] [address ...................]
        ui.horizontal(|ui| {
            label(ui, tr!("IP/Port:"));
            if let Some(icons) = &self.game_icons {
                icons.show(ui, self.add_game.map_or("", |g| g.icon), 16.0);
            }
            let selected_text = self.add_game.map_or(tr!("Auto detect"), |g| g.name);
            let add_game = &mut self.add_game;
            let game_icons = self.game_icons.as_ref();
            egui::ComboBox::from_id_salt("game_type")
                .selected_text(selected_text)
                .width(180.0)
                .height(420.0)
                .show_ui(ui, |ui| {
                    // Like HLSW: automatic detection first, then the games that cannot be
                    // detected, then all games.
                    if ui
                        .selectable_label(add_game.is_none(), tr!("Auto detect"))
                        .on_hover_text(tr!(
                            "Source, Half-Life 1 and CS2 servers are detected automatically"
                        ))
                        .clicked()
                    {
                        *add_game = None;
                    }
                    let mut row = |ui: &mut Ui, g: &'static games::Game| {
                        ui.horizontal(|ui| {
                            if let Some(icons) = game_icons {
                                icons.show(ui, g.icon, 16.0);
                            }
                            let selected = add_game.is_some_and(|a| std::ptr::eq(a, g));
                            let r = ui
                                .add_enabled(
                                    g.supported(),
                                    egui::Button::selectable(selected, g.name),
                                )
                                .on_disabled_hover_text(tr!(
                                    "Protocol {protocol} is not supported yet",
                                    protocol = g.hlsw_protocol
                                ));
                            if r.clicked() {
                                *add_game = Some(g);
                                ui.close();
                            }
                        });
                    };
                    ui.label(
                        RichText::new(tr!("--- Not detectable games ---"))
                            .color(ui.visuals().weak_text_color()),
                    );
                    for g in games::GAMES.iter().filter(|g| g.not_detectable) {
                        row(ui, g);
                    }
                    ui.label(
                        RichText::new(tr!("--- All games ---"))
                            .color(ui.visuals().weak_text_color()),
                    );
                    for g in games::GAMES {
                        row(ui, g);
                    }
                });
            let hint = match self.selected.and_then(|id| self.server(id)) {
                Some(s) if self.address_input.is_empty() => format!(
                    "{} {}",
                    s.address,
                    s.info.as_ref().map(|i| i.name.as_str()).unwrap_or_default()
                ),
                _ => add_game_hint(self.add_game),
            };
            let input = ui
                .add(
                    TextEdit::singleline(&mut self.address_input)
                        .align(egui::Align2::LEFT_CENTER)
                        .hint_text(hint)
                        // Leave room for the (translated) "Add" button.
                        .desired_width(
                            ui.available_width()
                                - text_width(ui, tr!("Add"))
                                - 2.0 * ui.spacing().button_padding.x
                                - ui.spacing().item_spacing.x
                                - 2.0,
                        ),
                )
                .with_edit_menu();
            if input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                actions.push(Action::Add);
            }
            if ui.button(tr!("Add")).clicked() {
                actions.push(Action::Add);
            }
        });

        // Toolbar: buttons · Password · Search
        let view = self.settings.view.clone();
        if !view.toolbar && !view.password && !view.filter && !view.player_name {
            ui.add_space(2.0);
            return;
        }
        ui.horizontal(|ui| {
            if view.toolbar {
                let selected = self.selected;
                let id = selected.unwrap_or_default();
                if let Some(icons) = &self.toolbar_icons {
                    self.icon_toolbar(ui, p, icons, actions);
                } else {
                    // Without the icons: flat text buttons.
                    let tool = |text: &str| Button::new(text).frame_when_inactive(false);
                    if ui
                        .add(tool(tr!("Refresh all")))
                        .on_hover_text(tr!("Shift+F5"))
                        .clicked()
                    {
                        actions.push(Action::RefreshAll);
                    }
                    if ui
                        .add_enabled(selected.is_some(), tool(tr!("Connect")))
                        .clicked()
                    {
                        actions.push(Action::Connect(id));
                    }
                    if ui
                        .add_enabled(selected.is_some(), tool(tr!("Remove")))
                        .clicked()
                    {
                        actions.push(Action::Remove(id));
                    }
                }
                toolbar_separator(ui, p);
            }
            if view.password {
                ui.label(tr!("Password:"));
                ui.add(
                    TextEdit::singleline(&mut self.join_password)
                        .align(egui::Align2::LEFT_CENTER)
                        .password(self.hide_join_password)
                        .desired_width(110.0),
                )
                .with_edit_menu()
                .on_hover_text(tr!("Server password used by Connect"));
                ui.checkbox(&mut self.hide_join_password, tr!("Hide"));
                toolbar_separator(ui, p);
            }
            if view.player_name {
                self.player_name_box(ui, p, actions);
                toolbar_separator(ui, p);
            }
            if view.filter {
                ui.label(tr!("Search:"));
                let search = ui
                    .add(
                        TextEdit::singleline(&mut self.search)
                            .align(egui::Align2::LEFT_CENTER)
                            .hint_text(tr!("Name, map, address"))
                            .desired_width(160.0),
                    )
                    .with_edit_menu();
                self.search_id = Some(search.id);
                if ui
                    .checkbox(&mut self.settings.filter.enabled, tr!("Filter"))
                    .on_hover_text(tr!("Server list filter"))
                    .changed()
                {
                    actions.push(Action::SaveConfig);
                }
                if ui
                    .add(Button::new(tr!("Edit filter...")).frame_when_inactive(false))
                    .on_hover_text(tr!("Server List Filter Properties"))
                    .clicked()
                {
                    self.show_filter = true;
                    self.settings.filter.enabled = true;
                }
                toolbar_separator(ui, p);
            }
            if view.toolbar {
                if ui
                    .checkbox(&mut self.settings.auto_refresh, tr!("Auto refresh every"))
                    .changed()
                {
                    actions.push(Action::SaveConfig);
                }
                let rate = self.settings.max_queries_per_sec;
                if interval_edit(ui, &mut self.settings.refresh_interval, rate) {
                    actions.push(Action::SaveConfig);
                }
            }
        });
        ui.add_space(2.0);
    }

    fn status_bar(&self, ui: &mut Ui, p: &Palette) {
        ui.horizontal(|ui| {
            ui.label(&self.status_message);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (sent, recv) = self.traffic.rate;
                ui.label(tr!(
                    "Send: {sent} Recv: {received}",
                    sent = format_rate(sent),
                    received = format_rate(recv)
                ));
                toolbar_separator(ui, p);
                let online = self
                    .servers
                    .iter()
                    .filter(|s| s.state == QueryState::Online)
                    .count();
                ui.label(tr!(
                    "{online}/{count} online",
                    online = online,
                    count = self.servers.len()
                ));
                toolbar_separator(ui, p);
            });
        });
    }

    fn details(&mut self, ui: &mut Ui, p: &Palette, actions: &mut Vec<Action>) {
        let idx = self.selected_index();
        let chrome = Frame::new().fill(p.window).inner_margin(Margin::same(4));
        egui::Panel::left("server_info")
            .frame(chrome)
            .resizable(true)
            .default_size(385.0)
            .min_size(220.0)
            .show(ui, |ui| {
                let location = idx
                    .and_then(|i| self.location_of(&self.servers[i]))
                    .map(|g| g.cloned());
                let flags = self.flags.as_ref();
                let server = idx.map(|i| &mut self.servers[i]);
                let graph = self.settings.view.ping_graph;
                if list_frame_with(ui, p, p.window, |ui| {
                    info_panel(ui, p, server, location, flags, graph)
                }) {
                    actions.push(Action::SaveConfig);
                }
            });
        if self.settings.view.rules {
            egui::Panel::top("players")
                .frame(chrome)
                .resizable(true)
                .default_size(140.0)
                .min_size(50.0)
                .show(ui, |ui| self.players_section(ui, p, idx, actions));
            egui::CentralPanel::default().frame(chrome).show(ui, |ui| {
                let rules = idx
                    .map(|i| self.servers[i].rules.as_slice())
                    .unwrap_or_default();
                let rule_server = idx.map(|i| {
                    let s = &self.servers[i];
                    (s.id, !s.rcon_password.is_empty())
                });
                let needle = filter_bar(
                    ui,
                    p,
                    "rules_filter",
                    &mut self.rules_filter,
                    tr!("cvar or value"),
                    rules.len(),
                );
                list_frame(ui, p, |ui| {
                    rules_table(
                        ui,
                        p,
                        rules,
                        &needle,
                        rule_server,
                        &mut self.rule_sel,
                        actions,
                    )
                });
            });
        } else {
            egui::CentralPanel::default()
                .frame(chrome)
                .show(ui, |ui| self.players_section(ui, p, idx, actions));
        }
    }

    fn players_section(
        &mut self,
        ui: &mut Ui,
        p: &Palette,
        idx: Option<usize>,
        actions: &mut Vec<Action>,
    ) {
        let Some(idx) = idx else {
            let needle = filter_bar(
                ui,
                p,
                "player_filter",
                &mut self.player_filter,
                tr!("player name"),
                0,
            );
            list_frame(ui, p, |ui| {
                players_table(
                    ui,
                    p,
                    &PlayerTableData::default(),
                    &needle,
                    &mut self.player_sort,
                    &mut self.player_sel,
                    actions,
                )
            });
            return;
        };
        let s = &self.servers[idx];
        let rows: Vec<PlayerRow> = s
            .players
            .iter()
            .map(|pl| {
                let st = s.status_players.iter().find(|x| x.name == pl.name);
                let ip = st.and_then(|x| x.ip);
                let g = ip.and_then(|ip| self.geo.get(&ip));
                let lan = ip.is_some_and(|ip| !geo::is_public(ip));
                PlayerRow {
                    player: pl.clone(),
                    status: st.cloned(),
                    country: g.map(|g| g.country_code.clone()).unwrap_or_default(),
                    country_name: g.map(|g| g.country.clone()).unwrap_or_default(),
                    provider: if lan {
                        tr!("LAN").into()
                    } else {
                        g.map(|g| g.provider().to_string()).unwrap_or_default()
                    },
                    log: s.log_players.get(&pl.name).cloned(),
                    db: st.and_then(|x| self.iddb.get(&x.steam_id)).cloned(),
                }
            })
            .collect();
        let data = PlayerTableData {
            server: Some(s.id),
            shown: &self.settings.view.player_columns,
            generation: self.settings.view.player_columns_reset,
            rows,
            flags: self.flags.as_ref(),
            has_status: !s.status_players.is_empty(),
            rcon_ok: s.rcon_status == RconStatus::Ok,
        };
        let total = data.rows.len();
        let needle = filter_bar(
            ui,
            p,
            "player_filter",
            &mut self.player_filter,
            tr!("name, Steam ID or IP"),
            total,
        );
        list_frame(ui, p, |ui| {
            players_table(
                ui,
                p,
                &data,
                &needle,
                &mut self.player_sort,
                &mut self.player_sel,
                actions,
            )
        });
    }

    fn tools_panel(&mut self, ui: &mut Ui, p: &Palette, actions: &mut Vec<Action>) {
        egui::Panel::bottom("tool_tabs")
            .frame(Frame::new().fill(p.window))
            .show_separator_line(false)
            .show(ui, |ui| {
                if let Some(tab) = tab_strip(ui, p, &TABS, self.tab) {
                    actions.push(Action::SwitchTab(tab));
                }
            });

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(p.window)
                    .stroke(Stroke::new(1.0, p.list_border))
                    .inner_margin(6),
            )
            .show(ui, |ui| {
                let Some(idx) = self.selected_index() else {
                    ui.add_enabled_ui(false, |ui| {
                        ui.label(tr!("Select a server in the list first."))
                    });
                    return;
                };
                // GameSpy games (Battlefield 2, Deer Hunter 2005, …) have no RCON that
                // openHLSW speaks: the tabs stay visible but greyed out, nothing can be typed.
                let rcon = self.servers[idx].supports_rcon();
                let resp = ui.add_enabled_ui(rcon, |ui| match self.tab {
                    Tab::RconConfig => self.rcon_config_tab(ui, p, idx, actions),
                    Tab::Console => self.console_tab(ui, p, idx, actions),
                    Tab::Chat => self.chat_tab(ui, p, idx, actions),
                    Tab::BanList => self.ban_tab(ui, p, idx, actions),
                    Tab::ChangeLevel => self.change_level_tab(ui, p, idx, actions),
                });
                if !rcon {
                    resp.response
                        .on_disabled_hover_text(tr!("RCON is not supported for this game"));
                }
            });
    }

    fn rcon_config_tab(&mut self, ui: &mut Ui, p: &Palette, idx: usize, actions: &mut Vec<Action>) {
        let hide = &mut self.hide_password;
        let auto_update = &mut self.auto_update_status;
        let s = &mut self.servers[idx];
        let id = s.id;

        ui.horizontal_top(|ui| {
            group_box(ui, p, tr!("Rcon Password"), 200.0, |ui| {
                egui::Grid::new("rcon_pw_grid")
                    .num_columns(2)
                    .spacing([8.0, 5.0])
                    .show(ui, |ui| {
                        let pw = ui.add(
                            TextEdit::singleline(&mut s.rcon_password).align(egui::Align2::LEFT_CENTER)
                                .password(*hide)
                                .desired_width(105.0),
                        ).with_edit_menu();
                        if pw.changed() {
                            actions.push(Action::RconConfigure(id));
                            if s.save_password {
                                actions.push(Action::SaveConfig);
                            }
                        }
                        if pw.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            actions.push(Action::RconTest(id));
                        }
                        if ui.add(Button::new(tr!("Test")).min_size(egui::vec2(75.0, 18.0))).clicked() {
                            actions.push(Action::RconTest(id));
                        }
                        ui.end_row();

                        ui.horizontal(|ui| {
                            ui.label(tr!("Status:"));
                            ui.label(rcon_status_text(&s.rcon_status, p));
                        });
                        if ui
                            .add(Button::new(tr!("Disconnect")).min_size(egui::vec2(75.0, 18.0)))
                            .clicked()
                        {
                            actions.push(Action::RconDisconnect(id));
                        }
                        ui.end_row();

                        ui.horizontal(|ui| {
                            if ui.checkbox(hide, tr!("Hide")).changed() {
                                actions.push(Action::SaveConfig);
                            }
                            ui.add_space(14.0);
                            if ui
                                .checkbox(&mut s.save_password, tr!("Save"))
                                .on_hover_text(
                                    tr!("Stores the password in plain text in the settings file"),
                                )
                                .changed()
                            {
                                actions.push(Action::SaveConfig);
                            }
                        });
                        if ui
                            .add(Button::new(tr!("Properties...")).min_size(egui::vec2(75.0, 18.0)))
                            .on_hover_text(tr!("Log port and public IP"))
                            .clicked()
                        {
                            actions.push(Action::OpenSettings(SettingsTab::Connections));
                        }
                        ui.end_row();
                    });
            });

            let log_on_map_change = &mut self.settings.log_on_map_change;
            group_box(ui, p, tr!("Receive Server Log"), 120.0, |ui| {
                let active = s.log_target.is_some();
                if ui
                    .add(Button::new(tr!("Get Log (Internet)")).min_size(egui::vec2(110.0, 18.0)))
                    .on_hover_text(
                        tr!("Server sends its log to your public IP (port must be forwarded)"),
                    )
                    .clicked()
                {
                    actions.push(Action::StartLog { id, internet: true });
                }
                if ui
                    .add(Button::new(tr!("Get Log (LAN)")).min_size(egui::vec2(110.0, 18.0)))
                    .on_hover_text(tr!("Server sends its log to your local IP"))
                    .clicked()
                {
                    actions.push(Action::StartLog {
                        id,
                        internet: false,
                    });
                }
                if ui
                    .checkbox(log_on_map_change, tr!("Retrieve log on map\nchange"))
                    .changed()
                {
                    actions.push(Action::SaveConfig);
                }
                if active && let Some((target, _)) = s.log_target {
                    ui.label(RichText::new(format!("→ {target}")).color(p.green).small());
                }
            });

            ui.vertical(|ui| {
                ui.add_space(10.0);
                if ui
                    .checkbox(auto_update, tr!("Auto Update Status"))
                    .on_hover_text(
                        tr!("Regularly sends `status` (Steam IDs, IPs, ping) and sets up the server log (logaddress_add) for the Console and Chat tabs"),
                    )
                    .changed()
                {
                    actions.push(Action::SaveConfig);
                }
                if ui
                    .checkbox(
                        &mut self.settings.admin_notification,
                        tr!("Enable Admin Notification"),
                    )
                    .on_hover_text(tr!(
                        "Chat messages starting with @ play the notification sound and are shown in the status bar (needs the server log)"
                    ))
                    .changed()
                {
                    actions.push(Action::SaveConfig);
                }
            });
        });
        // Keep the panel height stable when switching tabs.
        ui.allocate_space(ui.available_size());
    }

    /// Tab completion of the console input ("Complete" button / Tab key).
    fn complete_console_input(&mut self, idx: usize, ctx: &egui::Context) {
        let (completed, candidates) = autocomplete(&self.console_input, &self.history);
        if let Some(text) = completed {
            self.console_input = text;
            if let Some(input_id) = self.console_input_id
                && let Some(mut state) = egui::TextEdit::load_state(ctx, input_id)
            {
                let end = egui::text::CCursor::new(self.console_input.chars().count());
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(end)));
                state.store(ctx, input_id);
            }
        }
        if candidates.len() > 1 {
            self.servers[idx].console_add(LineKind::Note, &candidates.join("  "), false);
        }
    }

    fn console_tab(&mut self, ui: &mut Ui, p: &Palette, idx: usize, actions: &mut Vec<Action>) {
        let id = self.servers[idx].id;
        // Tab key: must be handled before the TextEdit so it does not move the focus.
        if self.settings.autocomplete
            && let Some(input_id) = self.console_input_id
            && ui.ctx().memory(|m| m.has_focus(input_id))
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Tab))
        {
            self.complete_console_input(idx, &ui.ctx().clone());
        }

        // Switches on the left, like HLSW.
        let mut options_changed = false;
        egui::Panel::left("console_switches")
            .frame(Frame::new())
            .exact_size(20.0)
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let o = &mut self.settings.console;
                let switches: [(&str, &str, &mut bool); 6] = [
                    ("C", tr!("Translate Log"), &mut o.translate_log),
                    ("*", tr!("Hide rcon password"), &mut o.hide_rcon_password),
                    ("F", tr!("Hide rcon feedback"), &mut o.hide_feedback),
                    ("D", tr!("Show date"), &mut o.show_date),
                    ("E", tr!("Show empty lines"), &mut o.show_empty),
                    ("A", tr!("Hide automatic"), &mut o.hide_automatic),
                ];
                for (label, tip, flag) in switches {
                    if toggle_button(ui, p, label, *flag)
                        .on_hover_text(tip)
                        .clicked()
                    {
                        *flag = !*flag;
                        options_changed = true;
                    }
                }
                let finding = self.console_find.is_some();
                if toggle_button(ui, p, "/", finding)
                    .on_hover_text(tr!("Find\tCtrl+Shift+F, F3"))
                    .clicked()
                {
                    self.console_find = if finding {
                        None
                    } else {
                        Some(ConsoleFind {
                            focus: true,
                            ..Default::default()
                        })
                    };
                }
            });

        // Find bar above the console output.
        if let Some(find) = &mut self.console_find {
            let mut close = false;
            egui::Panel::top("console_find")
                .frame(Frame::new().inner_margin(Margin {
                    bottom: 4,
                    ..Default::default()
                }))
                .show_separator_line(false)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(tr!("Find:"));
                        let r = ui
                            .add(
                                TextEdit::singleline(&mut find.text)
                                    .align(egui::Align2::LEFT_CENTER)
                                    .id_salt("console_find_field")
                                    .desired_width(220.0),
                            )
                            .with_edit_menu();
                        if find.focus {
                            r.request_focus();
                            find.focus = false;
                        }
                        if r.changed() {
                            find.pos = 0;
                            find.scroll = true;
                        }
                        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            find.pos += 1;
                            find.scroll = true;
                            find.focus = true;
                        }
                        if ui.input(|i| i.key_pressed(Key::Escape)) && r.has_focus() {
                            close = true;
                        }
                        if ui
                            .add(Button::new(tr!("Find Next\tF3")).min_size(egui::vec2(80.0, 18.0)))
                            .clicked()
                        {
                            find.pos += 1;
                            find.scroll = true;
                        }
                        if ui
                            .add(Button::new("×").min_size(egui::vec2(20.0, 18.0)))
                            .clicked()
                        {
                            close = true;
                        }
                    });
                });
            if close {
                self.console_find = None;
            }
        }
        if options_changed {
            actions.push(Action::SaveConfig);
        }

        let mut complete = false;
        let input = &mut self.console_input;
        let history = &self.history;
        let history_pos = &mut self.history_pos;
        let input_id = &mut self.console_input_id;
        let autoscroll = &mut self.console_autoscroll;
        egui::Panel::bottom("console_input")
            .frame(Frame::new().inner_margin(Margin {
                top: 4,
                ..Default::default()
            }))
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 210.0).max(100.0);
                    let resp = ui
                        .add(
                            TextEdit::singleline(input)
                                .align(egui::Align2::LEFT_CENTER)
                                .id_salt("console_input_field")
                                .font(TextStyle::Monospace)
                                .hint_text(tr!(
                                    "RCON command, e.g. status  (Up/Down: history, Tab: complete)"
                                ))
                                .desired_width(width),
                        )
                        .with_edit_menu();
                    *input_id = Some(resp.id);
                    // History drop-down (the ▼ of HLSW's combo box).
                    let (arrow_rect, arrow) =
                        ui.allocate_exact_size(egui::vec2(16.0, 18.0), Sense::click());
                    let c = arrow_rect.center();
                    ui.painter().rect_stroke(
                        arrow_rect,
                        0.0,
                        Stroke::new(1.0, p.button_border),
                        StrokeKind::Inside,
                    );
                    ui.painter().add(egui::Shape::convex_polygon(
                        vec![
                            egui::pos2(c.x - 4.0, c.y - 2.0),
                            egui::pos2(c.x + 4.0, c.y - 2.0),
                            egui::pos2(c.x, c.y + 3.0),
                        ],
                        p.text,
                        Stroke::NONE,
                    ));
                    egui::Popup::menu(&arrow).show(|ui| {
                        ui.set_min_width(width);
                        if history.is_empty() {
                            ui.label(tr!("No commands yet"));
                        }
                        for cmd in history.iter().rev().take(30) {
                            if ui.button(RichText::new(cmd).monospace()).clicked() {
                                *input = cmd.clone();
                                ui.close();
                            }
                        }
                    });
                    if resp.has_focus() && !history.is_empty() {
                        let (up, down) = ui.input(|i| {
                            (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown))
                        });
                        if up {
                            let pos =
                                history_pos.map_or(history.len() - 1, |p| p.saturating_sub(1));
                            *history_pos = Some(pos);
                            *input = history[pos].clone();
                        } else if down {
                            match *history_pos {
                                Some(p) if p + 1 < history.len() => {
                                    *history_pos = Some(p + 1);
                                    *input = history[p + 1].clone();
                                }
                                _ => {
                                    *history_pos = None;
                                    input.clear();
                                }
                            }
                        }
                    }
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    if ui
                        .add(Button::new(tr!("Send")).min_size(egui::vec2(60.0, 18.0)))
                        .clicked()
                        || enter
                    {
                        actions.push(Action::SendConsole(id));
                        resp.request_focus();
                    }
                    if ui
                        .add(Button::new(tr!("Complete")).min_size(egui::vec2(60.0, 18.0)))
                        .clicked()
                    {
                        complete = true;
                        resp.request_focus();
                    }
                    if toggle_button(ui, p, "S", *autoscroll)
                        .on_hover_text(tr!("Auto scroll"))
                        .clicked()
                    {
                        *autoscroll = !*autoscroll;
                    }
                    if toggle_button(ui, p, "X", false)
                        .on_hover_text(tr!("Clear console"))
                        .clicked()
                    {
                        actions.push(Action::ClearConsole(id));
                    }
                });
            });
        if complete {
            self.complete_console_input(idx, &ui.ctx().clone());
        }

        // Lines to show, after the switches.
        let o = self.settings.console.clone();
        let s = &self.servers[idx];
        let password = (o.hide_rcon_password && s.rcon_password.len() >= 2)
            .then_some(s.rcon_password.as_str());
        let lines: Vec<(String, LineKind)> = s
            .console
            .iter()
            .filter(|l| !(o.hide_feedback && l.kind == LineKind::Response))
            .filter(|l| !(o.hide_automatic && l.automatic))
            .filter(|l| o.show_empty || !l.text.trim().is_empty())
            .map(|l| {
                let time = if o.show_date {
                    l.time.format("%d.%m.%Y %H:%M:%S")
                } else {
                    l.time.format("%H:%M:%S")
                };
                let mut text = match l.kind {
                    LineKind::Log if o.translate_log => translate_log(&l.text),
                    LineKind::Note => format!("openHLSW NOTE: {}", l.text),
                    _ => l.text.clone(),
                };
                if o.hide_rcon_password {
                    text = mask_password(&text, password);
                }
                (format!("{time} {text}"), l.kind)
            })
            .collect();
        let autoscroll = self.console_autoscroll;
        // One read-only text area: scrolling, mouse selection, Ctrl+A / Ctrl+C and the
        // right-click menu work like in any text field. Lines keep their HLSW colours.
        let mut text = String::new();
        let mut job = egui::text::LayoutJob::default();
        let font = TextStyle::Monospace.resolve(ui.style());
        let needle = self
            .console_find
            .as_ref()
            .map(|f| f.text.clone())
            .unwrap_or_default();
        // (line, hit) of every match; the current one is drawn stronger and scrolled to.
        let mut hits: Vec<usize> = Vec::new();
        let hit_count: usize = lines
            .iter()
            .map(|(l, _)| find_ascii_ci(l, &needle).len())
            .sum();
        let current = match &mut self.console_find {
            Some(f) if hit_count > 0 => {
                f.pos %= hit_count;
                Some(f.pos)
            }
            _ => None,
        };
        let highlight = Color32::from_rgba_unmultiplied(255, 200, 0, 90);
        let highlight_current = Color32::from_rgb(255, 170, 0);
        for (i, (line, kind)) in lines.iter().enumerate() {
            let fg = match kind {
                LineKind::Command => p.console_cmd_fg,
                LineKind::Note => p.link,
                _ => p.text,
            };
            let mut piece = line.clone();
            if i + 1 < lines.len() {
                piece.push('\n');
            }
            let mut at = 0;
            for r in find_ascii_ci(line, &needle) {
                let is_current = current == Some(hits.len());
                hits.push(i);
                job.append(
                    &piece[at..r.start],
                    0.0,
                    egui::TextFormat::simple(font.clone(), fg),
                );
                let mut f = egui::TextFormat::simple(
                    font.clone(),
                    if is_current { Color32::BLACK } else { fg },
                );
                f.background = if is_current {
                    highlight_current
                } else {
                    highlight
                };
                job.append(&piece[r.clone()], 0.0, f);
                at = r.end;
            }
            job.append(
                &piece[at..],
                0.0,
                egui::TextFormat::simple(font.clone(), fg),
            );
            text.push_str(&piece);
        }
        let scroll_line = match &mut self.console_find {
            Some(f) if f.scroll => {
                f.scroll = false;
                current.and_then(|c| hits.get(c).copied())
            }
            _ => None,
        };
        // Jumping to a hit stops the auto scroll, or it would pull the view back down.
        if scroll_line.is_some() {
            self.console_autoscroll = false;
        }
        let autoscroll = autoscroll && scroll_line.is_none();
        job.wrap.max_width = f32::INFINITY;
        let job = std::sync::Arc::new(job);
        egui::CentralPanel::default()
            .frame(Frame::new())
            .show(ui, |ui| {
                list_frame(ui, p, |ui| {
                    ScrollArea::both()
                        .id_salt(("console_scroll", id))
                        .stick_to_bottom(autoscroll)
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            // Row backgrounds go below the text: reserve a shape slot first.
                            let bg_slot = ui.painter().add(egui::Shape::Noop);
                            let full_width = ui.available_width();
                            let mut layouter = |ui: &Ui, _: &dyn egui::TextBuffer, _: f32| {
                                ui.fonts_mut(|f| f.layout_job((*job).clone()))
                            };
                            let mut readonly = text.as_str();
                            let output = TextEdit::multiline(&mut readonly)
                                .id_salt(("console_text", id))
                                .frame(egui::Frame::NONE)
                                .desired_width(f32::INFINITY)
                                .desired_rows(1)
                                .layouter(&mut layouter)
                                .show(ui);
                            output.response.response.clone().with_edit_menu();

                            let mut shapes = Vec::new();
                            let mut line = 0;
                            let width = full_width.max(output.galley.rect.width());
                            for row in &output.galley.rows {
                                if let Some((_, kind)) = lines.get(line) {
                                    let bg = match kind {
                                        LineKind::Command => Some(p.console_cmd_bg),
                                        LineKind::Response => Some(p.console_resp_bg),
                                        _ => None,
                                    };
                                    if let Some(bg) = bg {
                                        let r = row.rect().translate(output.galley_pos.to_vec2());
                                        shapes.push(egui::Shape::rect_filled(
                                            egui::Rect::from_min_size(
                                                egui::pos2(output.galley_pos.x, r.top()),
                                                egui::vec2(width, r.height()),
                                            ),
                                            0.0,
                                            bg,
                                        ));
                                    }
                                }
                                if row.ends_with_newline {
                                    line += 1;
                                }
                            }
                            ui.painter().set(bg_slot, egui::Shape::Vec(shapes));
                            if let Some(target) = scroll_line {
                                let mut line = 0;
                                for row in &output.galley.rows {
                                    if line == target {
                                        let r = row.rect().translate(output.galley_pos.to_vec2());
                                        ui.scroll_to_rect(r, Some(Align::Center));
                                        break;
                                    }
                                    if row.ends_with_newline {
                                        line += 1;
                                    }
                                }
                            }
                        });
                });
            });
    }

    /// HLSW's Trace Route window (own native window where supported).
    fn trace_window(&mut self, ctx: &egui::Context) {
        let Some(trace) = &mut self.trace else { return };
        trace.poll();
        let p = self.settings.theme.palette();
        let flags = self.flags.as_ref();
        let geo = &self.geo;
        let mut close = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("trace_route"),
            egui::ViewportBuilder::default()
                .with_title(tr!("openHLSW Trace Route"))
                .with_inner_size([860.0, 560.0])
                .with_min_inner_size([500.0, 300.0]),
            |ui, _class| {
                if ui.input(|i| i.viewport().close_requested()) {
                    close = true;
                }
                egui::CentralPanel::default()
                    .frame(Frame::new().fill(p.window).inner_margin(Margin::same(6)))
                    .show(ui, |ui| trace_view(ui, &p, trace, flags, geo));
            },
        );
        if close {
            self.trace = None;
        }
    }

    /// The small windows opened from the menus.
    fn menu_dialog(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        let mut open = true;
        let mut close = false;
        let title = match &dialog {
            Dialog::NewList(_) => tr!("New server list"),
            Dialog::ListProperties(_) => tr!("Server list properties"),
            Dialog::MultiInsert(_) => tr!("Multi Server Insert"),
            Dialog::ServerProperties(..) => tr!("Server properties"),
            Dialog::ConfirmDeleteList => tr!("Delete server list"),
            Dialog::ConnectLog => tr!("Connect Log"),
            Dialog::ChangeRule(..) => tr!("Change Value"),
            Dialog::SetId(..) => tr!("Change ID"),
            Dialog::Confirm(Confirm::DeleteServer(_)) => tr!("Delete"),
            Dialog::Confirm(Confirm::ChangeLevel(..)) => tr!("Change Level"),
            Dialog::Confirm(Confirm::ConnectNoPassword(_)) => tr!("Password"),
        };
        let confirm_text = match &dialog {
            Dialog::Confirm(Confirm::DeleteServer(id)) => {
                let s = self.server(*id);
                let addr = s.map(|s| s.address.to_string()).unwrap_or_default();
                let name = s
                    .and_then(|s| s.info.as_ref())
                    .map(|i| i.name.clone())
                    .unwrap_or_default();
                tr!("Delete {address} {name}?", address = addr, name = name)
            }
            Dialog::Confirm(Confirm::ChangeLevel(_, map)) => {
                tr!(
                    "Change level to '{map}'?\n\nRcon Command: 'changelevel {map}'",
                    map = map
                )
            }
            Dialog::Confirm(Confirm::ConnectNoPassword(_)) => {
                tr!("The server is password protected and none is specified.\n\nContinue?")
                    .to_string()
            }
            _ => String::new(),
        };
        let list_name = self.list_name.clone();
        let connect_log = &self.connect_log;
        NativeWindow::new(("menu_dialog", title), title)
            .open(&mut open)
            .collapsible(false)
            .resizable(matches!(
                dialog,
                Dialog::MultiInsert(_) | Dialog::ConnectLog
            ))
            .default_width(380.0)
            .show(ctx, |ui| {
                let ok_cancel = |ui: &mut Ui| -> (bool, bool) {
                    ui.add_space(6.0);
                    let mut r = (false, false);
                    ui.horizontal(|ui| {
                        r.0 = ui
                            .add(Button::new(tr!("OK")).min_size(egui::vec2(75.0, 22.0)))
                            .clicked();
                        r.1 = ui
                            .add(Button::new(tr!("Cancel")).min_size(egui::vec2(75.0, 22.0)))
                            .clicked();
                    });
                    r
                };
                match &mut dialog {
                    Dialog::NewList(name) | Dialog::ListProperties(name) => {
                        ui.label(tr!("Name:"));
                        let r = ui
                            .add(
                                TextEdit::singleline(name)
                                    .align(egui::Align2::LEFT_CENTER)
                                    .desired_width(340.0),
                            )
                            .with_edit_menu();
                        // Check Enter before re-requesting focus (request_focus resets lost_focus).
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        if !enter {
                            r.request_focus();
                        }
                        let (ok, cancel) = ok_cancel(ui);
                        if ok || enter {
                            let n = name.clone();
                            actions.push(if matches!(dialog, Dialog::NewList(_)) {
                                Action::NewList(n)
                            } else {
                                Action::RenameList(n)
                            });
                            close = true;
                        }
                        close |= cancel;
                    }
                    Dialog::MultiInsert(text) => {
                        ui.label(tr!(
                            "Paste server addresses (one per line, or separated by commas):"
                        ));
                        ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                            ui.add(
                                TextEdit::multiline(text)
                                    .hint_text(tr!("1.2.3.4:27015\nexample.org:27016 my comment"))
                                    .desired_rows(12)
                                    .desired_width(f32::INFINITY),
                            )
                            .with_edit_menu();
                        });
                        let (ok, cancel) = ok_cancel(ui);
                        if ok {
                            actions.push(Action::MultiInsert(text.clone()));
                            close = true;
                        }
                        close |= cancel;
                    }
                    Dialog::ServerProperties(id, draft) => {
                        egui::Grid::new("server_props")
                            .num_columns(2)
                            .spacing([8.0, 6.0])
                            .show(ui, |ui| {
                                ui.label(tr!("IP/Port:"));
                                ui.add(
                                    TextEdit::singleline(&mut draft.address)
                                        .align(egui::Align2::LEFT_CENTER)
                                        .desired_width(260.0),
                                )
                                .with_edit_menu();
                                ui.end_row();
                                ui.label(tr!("Comment:"));
                                ui.add(
                                    TextEdit::singleline(&mut draft.comment)
                                        .align(egui::Align2::LEFT_CENTER)
                                        .desired_width(260.0),
                                )
                                .with_edit_menu();
                                ui.end_row();
                                ui.label(tr!("Additional parameters:"));
                                ui.add(
                                    TextEdit::singleline(&mut draft.params)
                                        .align(egui::Align2::LEFT_CENTER)
                                        .hint_text(tr!("start parameters of the game"))
                                        .desired_width(260.0),
                                )
                                .with_edit_menu();
                                ui.end_row();
                                let rcon = ServerAddress::parse(&draft.address)
                                    .is_none_or(|a| a.query_port.is_none());
                                let no_rcon = tr!("RCON is not supported for this game");
                                ui.label(tr!("Rcon password:"));
                                ui.add_enabled(
                                    rcon,
                                    TextEdit::singleline(&mut draft.rcon_password)
                                        .align(egui::Align2::LEFT_CENTER)
                                        .password(true)
                                        .desired_width(260.0),
                                )
                                .on_disabled_hover_text(no_rcon)
                                .with_edit_menu();
                                ui.end_row();
                                ui.label("");
                                ui.add_enabled(
                                    rcon,
                                    egui::Checkbox::new(
                                        &mut draft.save_password,
                                        tr!("Save Rcon password"),
                                    ),
                                )
                                .on_disabled_hover_text(no_rcon);
                                ui.end_row();
                            });
                        let valid = ServerAddress::parse(&draft.address).is_some();
                        if !valid {
                            ui.colored_label(
                                Color32::from_rgb(220, 60, 60),
                                tr!("Invalid address"),
                            );
                        }
                        let (ok, cancel) = ok_cancel(ui);
                        if ok && valid {
                            actions.push(Action::ApplyServerProps(*id, draft.clone()));
                            close = true;
                        }
                        close |= cancel;
                    }
                    Dialog::SetId(id, text) => {
                        ui.label(tr!("New ID (position in the server list):"));
                        let r = ui
                            .add(
                                TextEdit::singleline(text)
                                    .align(egui::Align2::LEFT_CENTER)
                                    .desired_width(120.0),
                            )
                            .with_edit_menu();
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        if !enter {
                            r.request_focus();
                        }
                        let (ok, cancel) = ok_cancel(ui);
                        if ok || enter {
                            if let Ok(n) = text.trim().parse::<usize>() {
                                actions.push(Action::SetServerId(*id, n));
                            }
                            close = true;
                        }
                        close |= cancel;
                    }
                    Dialog::ChangeRule(id, key, value) => {
                        ui.label(tr!("New value of {key}:", key = key));
                        let r = ui
                            .add(
                                TextEdit::singleline(value)
                                    .align(egui::Align2::LEFT_CENTER)
                                    .desired_width(340.0),
                            )
                            .with_edit_menu();
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        if !enter {
                            r.request_focus();
                        }
                        let (ok, cancel) = ok_cancel(ui);
                        if ok || enter {
                            actions.push(Action::SetCvar(*id, key.clone(), value.clone()));
                            close = true;
                        }
                        close |= cancel;
                    }
                    Dialog::Confirm(confirm) => {
                        ui.label(&confirm_text);
                        let (ok, cancel) = ok_cancel(ui);
                        if ok {
                            actions.push(match confirm.clone() {
                                Confirm::DeleteServer(id) => Action::RemoveConfirmed(id),
                                Confirm::ChangeLevel(id, map) => {
                                    Action::ChangeLevelConfirmed(id, map)
                                }
                                Confirm::ConnectNoPassword(id) => Action::ConnectAnyway(id),
                            });
                            close = true;
                        }
                        close |= cancel;
                    }
                    Dialog::ConfirmDeleteList => {
                        ui.label(tr!(
                            "Delete the server list \"{list_name}\" and all its servers?",
                            list_name = list_name
                        ));
                        let (ok, cancel) = ok_cancel(ui);
                        if ok {
                            actions.push(Action::DeleteList);
                            close = true;
                        }
                        close |= cancel;
                    }
                    Dialog::ConnectLog => {
                        ScrollArea::vertical()
                            .max_height(300.0)
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                if connect_log.is_empty() {
                                    ui.label(tr!("No connections yet."));
                                }
                                for line in connect_log {
                                    ui.label(RichText::new(line).monospace());
                                }
                            });
                        ui.add_space(4.0);
                        if ui
                            .add(Button::new(tr!("Close")).min_size(egui::vec2(75.0, 22.0)))
                            .clicked()
                        {
                            close = true;
                        }
                    }
                }
            });
        if open && !close {
            self.dialog = Some(dialog);
        }
    }

    fn chat_tab(&mut self, ui: &mut Ui, p: &Palette, idx: usize, actions: &mut Vec<Action>) {
        let id = self.servers[idx].id;
        let input = &mut self.chat_input;
        egui::Panel::bottom("chat_input")
            .frame(Frame::new().inner_margin(Margin {
                top: 4,
                ..Default::default()
            }))
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let resp = ui
                        .add(
                            TextEdit::singleline(input)
                                .align(egui::Align2::LEFT_CENTER)
                                .id_salt("chat_input_field")
                                .hint_text(tr!("Message to all players (rcon say)"))
                                .desired_width((ui.available_width() - 60.0).max(100.0)),
                        )
                        .with_edit_menu();
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    if ui
                        .add(Button::new(tr!("Say")).min_size(egui::vec2(50.0, 18.0)))
                        .clicked()
                        || enter
                    {
                        actions.push(Action::Say(id));
                        resp.request_focus();
                    }
                });
            });

        let s = &self.servers[idx];
        egui::CentralPanel::default().frame(Frame::new()).show(ui, |ui| {
            list_frame(ui, p, |ui| {
                if s.log_target.is_none() && s.chat.is_empty() {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(tr!("Chat is read from the server log. Click \"Get Log\" in Rcon Config to start receiving it."))
                                .color(p.weak_text),
                        );
                    });
                    return;
                }
                styled_table(ui, "chat")
                    .stick_to_bottom(true)
                    .column(Column::initial(60.0).at_least(40.0))
                    .column(Column::initial(160.0).at_least(60.0).clip(true))
                    .column(Column::remainder().clip(true))
                    .header(HEADER_HEIGHT, |mut h| {
                        h.col(|ui| { plain_header(ui, p, tr!("Time")); });
                        h.col(|ui| { plain_header(ui, p, tr!("Name")); });
                        h.col(|ui| { plain_header(ui, p, tr!("Message")); });
                    })
                    .body(|body| {
                        body.rows(ROW_HEIGHT, s.chat.len(), |mut row| {
                            let c = &s.chat[row.index()];
                            let team_color = match c.team.as_str() {
                                "CT" | "Blue" => Some(Color32::from_rgb(60, 110, 230)),
                                "TERRORIST" | "Red" => Some(p.red),
                                _ => None,
                            };
                            row.col(|ui| {
                                ui.label(&c.time);
                            });
                            row.col(|ui| {
                                let mut name = RichText::new(&c.name);
                                if let Some(col) = team_color {
                                    name = name.color(col);
                                }
                                ui.add(Label::new(name).wrap_mode(fit(ui)));
                            });
                            row.col(|ui| {
                                let prefix = if c.team_only { tr!("(Team) ") } else { "" };
                                ui.add(Label::new(format!("{prefix}{}", c.text)).wrap_mode(fit(ui)));
                            });
                        });
                    });
            });
        });
    }

    fn ban_tab(&mut self, ui: &mut Ui, p: &Palette, idx: usize, actions: &mut Vec<Action>) {
        let ban_input = &mut self.ban_input;
        let ban_minutes = &mut self.ban_minutes;
        let s = &mut self.servers[idx];
        let id = s.id;

        egui::Panel::right("ban_controls")
            .frame(Frame::new().inner_margin(Margin {
                left: 8,
                ..Default::default()
            }))
            .show_separator_line(false)
            .exact_size(250.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add(Button::new(tr!("Refresh")).min_size(egui::vec2(80.0, 18.0)))
                        .clicked()
                    {
                        actions.push(Action::FetchBans(id));
                    }
                    if ui
                        .add_enabled(
                            s.ban_selected.is_some(),
                            Button::new(tr!("Unban")).min_size(egui::vec2(80.0, 18.0)),
                        )
                        .clicked()
                    {
                        actions.push(Action::Unban(id));
                    }
                });
                ui.horizontal(|ui| {
                    if ui
                        .add(Button::new(tr!("Write")).min_size(egui::vec2(80.0, 18.0)))
                        .on_hover_text(tr!("Save the ban lists on the server (writeid / writeip)"))
                        .clicked()
                    {
                        actions.push(Action::BanCommand(id, "writeid;writeip"));
                    }
                    if ui
                        .add(Button::new(tr!("Reload")).min_size(egui::vec2(80.0, 18.0)))
                        .on_hover_text(tr!("Reload banned.cfg / listip.cfg on the server"))
                        .clicked()
                    {
                        actions.push(Action::BanCommand(id, "exec banned.cfg;exec listip.cfg"));
                    }
                });
                ui.horizontal(|ui| {
                    if ui
                        .add(Button::new(tr!("To ID Database")).min_size(egui::vec2(80.0, 18.0)))
                        .on_hover_text(tr!("Add the banned IDs to the Global ID Database"))
                        .clicked()
                    {
                        actions.push(Action::BansToIdDb(id));
                    }
                    if ui
                        .add(Button::new(tr!("From ID Database")).min_size(egui::vec2(80.0, 18.0)))
                        .on_hover_text(tr!("Add all bans from Global ID Database"))
                        .clicked()
                    {
                        actions.push(Action::BansFromIdDb(id));
                    }
                });
                ui.add_space(6.0);
                ui.label(tr!("Filter:"));
                ui.add(
                    TextEdit::singleline(&mut s.ban_filter)
                        .align(egui::Align2::LEFT_CENTER)
                        .desired_width(230.0),
                )
                .with_edit_menu();
                ui.add_space(6.0);
                ui.label(tr!("Steam ID or IP:"));
                ui.add(
                    TextEdit::singleline(ban_input)
                        .align(egui::Align2::LEFT_CENTER)
                        .hint_text(tr!("STEAM_1:0:123 / 1.2.3.4"))
                        .desired_width(230.0),
                )
                .with_edit_menu();
                ui.horizontal(|ui| {
                    ui.label(tr!("Minutes:"));
                    ui.add(DragValue::new(ban_minutes).range(0..=525_600));
                    ui.label(RichText::new(tr!("0 = permanent")).color(p.weak_text));
                });
                ui.horizontal(|ui| {
                    if ui
                        .add(Button::new(tr!("Ban ID")).min_size(egui::vec2(80.0, 18.0)))
                        .clicked()
                    {
                        actions.push(Action::AddBan { id, ip: false });
                    }
                    if ui
                        .add(Button::new(tr!("Ban IP")).min_size(egui::vec2(80.0, 18.0)))
                        .clicked()
                    {
                        actions.push(Action::AddBan { id, ip: true });
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(Frame::new())
            .show(ui, |ui| {
                list_frame(ui, p, |ui| {
                    let selected = s.ban_selected;
                    let mut clicked = None;
                    let needle = s.ban_filter.trim().to_lowercase();
                    let shown: Vec<usize> = (0..s.bans.len())
                        .filter(|&i| {
                            needle.is_empty() || s.bans[i].ident.to_lowercase().contains(&needle)
                        })
                        .collect();
                    styled_table(ui, "bans")
                        .sense(Sense::click())
                        .column(Column::exact(20.0))
                        .column(Column::initial(40.0))
                        .column(Column::initial(200.0).at_least(80.0).clip(true))
                        .column(Column::remainder().clip(true))
                        .header(HEADER_HEIGHT, |mut h| {
                            h.col(|ui| {
                                plain_header(ui, p, "#");
                            });
                            h.col(|ui| {
                                plain_header(ui, p, tr!("Type"));
                            });
                            h.col(|ui| {
                                plain_header(ui, p, tr!("Steam ID / IP"));
                            });
                            h.col(|ui| {
                                plain_header(ui, p, tr!("Duration"));
                            });
                        })
                        .body(|body| {
                            body.rows(ROW_HEIGHT, shown.len(), |mut row| {
                                let i = shown[row.index()];
                                let b = &s.bans[i];
                                row.set_selected(selected == Some(i));
                                row.col(|ui| {
                                    ui.label(format!("{:02}", i + 1));
                                });
                                row.col(|ui| {
                                    ui.label(if b.is_ip { tr!("IP") } else { "ID" });
                                });
                                row.col(|ui| {
                                    ui.add(
                                        Label::new(&b.ident).wrap_mode(fit(ui)).selectable(false),
                                    );
                                });
                                row.col(|ui| {
                                    ui.label(&b.duration);
                                });
                                if row.response().clicked() {
                                    clicked = Some(i);
                                }
                            });
                        });
                    if clicked.is_some() {
                        s.ban_selected = clicked;
                    }
                });
            });
    }

    fn change_level_tab(
        &mut self,
        ui: &mut Ui,
        p: &Palette,
        idx: usize,
        actions: &mut Vec<Action>,
    ) {
        let kick_all = &mut self.settings.kick_all_on_map_change;
        let s = &mut self.servers[idx];
        let id = s.id;
        let current = s.info.as_ref().map(|i| i.map.clone()).unwrap_or_default();

        // HLSW layout: map list left, Update / Change Level / Kick all on the right.
        egui::Panel::right("map_controls")
            .frame(Frame::new().inner_margin(Margin {
                left: 8,
                ..Default::default()
            }))
            .show_separator_line(false)
            .exact_size(170.0)
            .show(ui, |ui| {
                if ui
                    .add(Button::new(tr!("Update")).min_size(egui::vec2(150.0, 20.0)))
                    .clicked()
                {
                    actions.push(Action::FetchMaps(id));
                }
                ui.add_space(4.0);
                let target = s.map_selected.clone();
                if ui
                    .add_enabled(
                        target.is_some(),
                        Button::new(tr!("Change Level")).min_size(egui::vec2(150.0, 20.0)),
                    )
                    .clicked()
                    && let Some(map) = target
                {
                    actions.push(Action::ChangeLevel(id, map));
                }
                if ui
                    .checkbox(kick_all, tr!("Kick all on map\nchange"))
                    .changed()
                {
                    actions.push(Action::SaveConfig);
                }
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(tr!("Current:")).color(p.weak_text));
                    ui.label(RichText::new(&current).color(p.red));
                });
                if !s.maps_status.is_empty() {
                    ui.add(Label::new(RichText::new(&s.maps_status).color(p.weak_text)).wrap());
                }
            });

        egui::CentralPanel::default()
            .frame(Frame::new())
            .show(ui, |ui| {
                list_frame(ui, p, |ui| {
                    let selected = s.map_selected.clone();
                    let mut clicked = None;
                    styled_table(ui, "maps")
                        .sense(Sense::click())
                        .column(Column::exact(28.0))
                        .column(Column::remainder().clip(true))
                        .header(HEADER_HEIGHT, |mut h| {
                            h.col(|ui| {
                                plain_header(ui, p, "#");
                            });
                            h.col(|ui| {
                                plain_header(ui, p, tr!("Map"));
                            });
                        })
                        .body(|body| {
                            body.rows(ROW_HEIGHT, s.maps.len(), |mut row| {
                                let m = &s.maps[row.index()];
                                row.set_selected(selected.as_deref() == Some(m.as_str()));
                                let n = row.index() + 1;
                                row.col(|ui| {
                                    ui.label(format!("{n:02}"));
                                });
                                row.col(|ui| {
                                    let mut t = RichText::new(m);
                                    if *m == current {
                                        t = t.strong();
                                    }
                                    ui.add(Label::new(t).selectable(false));
                                });
                                let r = row.response();
                                if r.clicked() {
                                    clicked = Some(m.clone());
                                }
                                if r.double_clicked() {
                                    actions.push(Action::ChangeLevel(id, m.clone()));
                                }
                            });
                        });
                    if clicked.is_some() {
                        s.map_selected = clicked;
                    }
                });
            });
    }

    /// Results of the Find window: server and a description of the hit.
    fn find_results(&self) -> Vec<(ServerId, String)> {
        let Some(find) = &self.find else {
            return Vec::new();
        };
        let needle = find.text.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for s in &self.servers {
            let server = s
                .info
                .as_ref()
                .map(|i| i.name.clone())
                .unwrap_or_else(|| s.address.to_string());
            match find.kind {
                FindKind::Player => {
                    for p in s
                        .players
                        .iter()
                        .filter(|p| p.name.to_lowercase().contains(&needle))
                    {
                        out.push((s.id, format!("{}  —  {server}", p.name)));
                    }
                }
                FindKind::Rule => {
                    for r in s.rules.iter().filter(|r| {
                        r.key.to_lowercase().contains(&needle)
                            || r.value.to_lowercase().contains(&needle)
                    }) {
                        out.push((s.id, format!("{} = {}  —  {server}", r.key, r.value)));
                    }
                }
            }
        }
        out
    }

    fn save_iddb(&mut self) {
        self.iddb_dirty = false;
        self.iddb_saved = Instant::now();
        if let Err(e) = self.iddb.save() {
            self.status_message = tr!("Could not save the Global ID Database: {e}", e = e);
        }
    }

    /// "Global Player ID Database Editor" (File menu, Ctrl+W).
    fn iddb_window(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some(ed) = &mut self.iddb_editor else {
            return;
        };
        let db = &mut self.iddb;
        let mut open = true;
        let mut changed = false;
        NativeWindow::new("iddb", tr!("Global Player ID Database Editor"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([680.0, 440.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(tr!("Import banned.cfg...")).clicked() {
                        actions.push(Action::IdDbImport);
                    }
                    if ui.button(tr!("Export banned.cfg...")).clicked() {
                        actions.push(Action::IdDbExport);
                    }
                    if ui
                        .add_enabled(ed.selected.is_some(), Button::new(tr!("Delete")))
                        .clicked()
                        && let Some(id) = ed.selected.take()
                    {
                        db.entries.remove(&id);
                        changed = true;
                    }
                    ui.checkbox(&mut ed.hide_auto, tr!("Hide Auto Added"));
                });
                ui.horizontal(|ui| {
                    ui.label(tr!("Filter:"));
                    ui.add(
                        TextEdit::singleline(&mut ed.filter)
                            .align(egui::Align2::LEFT_CENTER)
                            .hint_text(tr!("ID, name or comment"))
                            .desired_width(240.0),
                    )
                    .with_edit_menu();
                    ui.label(
                        RichText::new(tr!("{count} entries", count = db.entries.len()))
                            .color(ui.visuals().weak_text_color()),
                    );
                });
                ui.add_space(4.0);

                // Edit form at the bottom, the list fills the rest.
                egui::Panel::bottom("iddb_form")
                    .show_separator_line(false)
                    .show(ui, |ui| {
                        ui.add_space(6.0);
                        egui::Grid::new("iddb_form_grid")
                            .num_columns(2)
                            .spacing([8.0, 4.0])
                            .show(ui, |ui| {
                                ui.label(tr!("Global ID:"));
                                ui.horizontal(|ui| {
                                    ui.add(
                                        TextEdit::singleline(&mut ed.id)
                                            .align(egui::Align2::LEFT_CENTER)
                                            .hint_text("STEAM_0:0:11101")
                                            .desired_width(200.0),
                                    )
                                    .with_edit_menu();
                                    ui.label(tr!("Type:"));
                                    egui::ComboBox::from_id_salt("iddb_kind")
                                        .selected_text(crate::i18n::t(ed.kind.name()))
                                        .show_ui(ui, |ui| {
                                            for k in IdKind::ALL {
                                                ui.selectable_value(
                                                    &mut ed.kind,
                                                    k,
                                                    crate::i18n::t(k.name()),
                                                );
                                            }
                                        });
                                    ui.checkbox(&mut ed.banned, tr!("Banned"));
                                });
                                ui.end_row();
                                ui.label(tr!("Names:"));
                                ui.add(
                                    TextEdit::singleline(&mut ed.names)
                                        .align(egui::Align2::LEFT_CENTER)
                                        .hint_text(tr!("comma separated"))
                                        .desired_width(f32::INFINITY),
                                )
                                .with_edit_menu();
                                ui.end_row();
                                ui.label(tr!("Comment:"));
                                ui.add(
                                    TextEdit::singleline(&mut ed.comment)
                                        .align(egui::Align2::LEFT_CENTER)
                                        .desired_width(f32::INFINITY),
                                )
                                .with_edit_menu();
                                ui.end_row();
                            });
                        ui.horizontal(|ui| {
                            let key = iddb::normalize(&ed.id);
                            let exists = key.as_ref().is_some_and(|k| db.entries.contains_key(k));
                            let entry = |ed: &IdDbEditor| IdEntry {
                                names: ed
                                    .names
                                    .split(',')
                                    .map(|n| n.trim().to_string())
                                    .filter(|n| !n.is_empty())
                                    .collect(),
                                kind: ed.kind,
                                comment: ed.comment.trim().to_string(),
                                banned: ed.banned,
                                auto: false,
                            };
                            if ui
                                .add_enabled(
                                    key.is_some() && !exists,
                                    Button::new(tr!("Add")).min_size(egui::vec2(80.0, 18.0)),
                                )
                                .clicked()
                                && let Some(k) = key.clone()
                            {
                                db.entries.insert(k.clone(), entry(ed));
                                ed.selected = Some(k.clone());
                                ed.id = k;
                                changed = true;
                            }
                            if ui
                                .add_enabled(
                                    exists,
                                    Button::new(tr!("Apply")).min_size(egui::vec2(80.0, 18.0)),
                                )
                                .clicked()
                                && let Some(k) = key.clone()
                            {
                                db.entries.insert(k.clone(), entry(ed));
                                ed.selected = Some(k);
                                changed = true;
                            }
                            if key.is_none() && !ed.id.trim().is_empty() {
                                ui.label(
                                    RichText::new(tr!("Not a valid Steam ID"))
                                        .color(ui.visuals().warn_fg_color),
                                );
                            } else if !ed.message.is_empty() {
                                ui.label(&ed.message);
                            }
                        });
                    });

                let needle = ed.filter.trim().to_lowercase();
                let rows: Vec<(&String, &IdEntry)> = db
                    .entries
                    .iter()
                    .filter(|(_, e)| !(ed.hide_auto && e.auto))
                    .filter(|(id, e)| {
                        needle.is_empty()
                            || id.to_lowercase().contains(&needle)
                            || e.names.iter().any(|n| n.to_lowercase().contains(&needle))
                            || e.comment.to_lowercase().contains(&needle)
                    })
                    .collect();
                let mut clicked = None;
                styled_table(ui, "iddb_table")
                    .sense(Sense::click())
                    .column(Column::initial(150.0).at_least(130.0).clip(true))
                    .column(Column::initial(70.0).at_least(60.0).clip(true))
                    .column(Column::initial(200.0).at_least(160.0).clip(true))
                    .column(Column::initial(60.0).at_least(50.0))
                    .column(Column::remainder().at_least(80.0).clip(true))
                    .header(HEADER_HEIGHT, |mut h| {
                        for t in [
                            tr!("Global ID"),
                            tr!("Type"),
                            tr!("Names"),
                            tr!("Banned"),
                            tr!("Comment"),
                        ] {
                            h.col(|ui| {
                                ui.strong(t);
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(ROW_HEIGHT, rows.len(), |mut row| {
                            let (id, e) = rows[row.index()];
                            row.set_selected(ed.selected.as_ref() == Some(id));
                            let color = e.kind.color().map(|[r, g, b]| Color32::from_rgb(r, g, b));
                            row.col(|ui| {
                                ui.add(Label::new(id.as_str()).selectable(false));
                            });
                            row.col(|ui| {
                                let t = RichText::new(crate::i18n::t(e.kind.name()));
                                ui.add(
                                    Label::new(match color {
                                        Some(c) => t.color(c),
                                        None => t,
                                    })
                                    .selectable(false),
                                );
                            });
                            row.col(|ui| {
                                ui.add(Label::new(e.names_text()).selectable(false));
                            });
                            row.col(|ui| {
                                if e.banned {
                                    ui.add(Label::new(tr!("Yes")).selectable(false));
                                }
                            });
                            row.col(|ui| {
                                ui.add(Label::new(e.comment.as_str()).selectable(false));
                            });
                            if row.response().clicked() {
                                clicked = Some(id.clone());
                            }
                        });
                    });
                if let Some(id) = clicked {
                    ed.select(db, &id);
                }
            });
        if changed {
            self.iddb_dirty = true;
            self.save_iddb();
        }
        if !open {
            self.iddb_editor = None;
        }
    }

    fn find_window(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let results = self.find_results();
        let Some(find) = &mut self.find else { return };
        let mut open = true;
        let title = match find.kind {
            FindKind::Player => tr!("Find player"),
            FindKind::Rule => tr!("Find rule"),
        };
        NativeWindow::new("find", title)
            .open(&mut open)
            .collapsible(false)
            .default_width(380.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr!("Find what:"));
                    let r = ui
                        .add(
                            TextEdit::singleline(&mut find.text)
                                .align(egui::Align2::LEFT_CENTER)
                                .desired_width(200.0),
                        )
                        .with_edit_menu();
                    if find.focus {
                        r.request_focus();
                        find.focus = false;
                    }
                    if r.changed() {
                        find.pos = None;
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        actions.push(Action::FindNext);
                        find.focus = true;
                    }
                    if ui.button(tr!("Find Next\tF3")).clicked() {
                        actions.push(Action::FindNext);
                    }
                });
                ui.label(
                    RichText::new(tr!(
                        "{count} found in all servers of the list",
                        count = results.len()
                    ))
                    .color(ui.visuals().weak_text_color()),
                );
                ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    for (n, (_, text)) in results.iter().enumerate() {
                        if ui.selectable_label(find.pos == Some(n), text).clicked() {
                            // FindNext moves on by one: start just before this hit.
                            find.pos = n.checked_sub(1);
                            actions.push(Action::FindNext);
                        }
                    }
                });
            });
        if !open {
            self.find = None;
        }
    }

    /// "Server List Filter Properties" (HLSW's Filter / Advanced Filter).
    fn filter_window(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        use crate::filter::{PasswordFilter, Range, ServerFilter};
        if !self.show_filter {
            return;
        }
        let mut open = true;
        let mut close = false;
        let f = &mut self.settings.filter;
        NativeWindow::new("filter", tr!("Server List Filter Properties"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.checkbox(&mut f.enabled, tr!("Filter on"));
                ui.add_space(4.0);
                ui.add_enabled_ui(f.enabled, |ui| {
                    ui.checkbox(
                        &mut f.hide_timeout,
                        tr!("Hide servers that do not answer (timeout)"),
                    );
                    egui::Grid::new("filter_grid")
                        .num_columns(4)
                        .spacing([8.0, 5.0])
                        .show(ui, |ui| {
                            ui.label(tr!("Maximum ping:"));
                            ui.horizontal(|ui| {
                                ui.add(egui::DragValue::new(&mut f.max_ping).range(0..=999));
                                ui.label(tr!("ms (0 = off)"));
                            });
                            ui.end_row();
                            ui.label(tr!("Password"));
                            egui::ComboBox::from_id_salt("filter_password")
                                .selected_text(match f.password {
                                    PasswordFilter::Any => tr!("Any"),
                                    PasswordFilter::Without => tr!("Without password"),
                                    PasswordFilter::With => tr!("With password"),
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut f.password,
                                        PasswordFilter::Any,
                                        tr!("Any"),
                                    );
                                    ui.selectable_value(
                                        &mut f.password,
                                        PasswordFilter::Without,
                                        tr!("Without password"),
                                    );
                                    ui.selectable_value(
                                        &mut f.password,
                                        PasswordFilter::With,
                                        tr!("With password"),
                                    );
                                });
                            ui.end_row();
                            for (label, value, hint) in [
                                (tr!("Game:"), &mut f.game, tr!("e.g. cstrike, CS:S, TF2")),
                                (tr!("Map:"), &mut f.map, tr!("e.g. de_, cs_office")),
                                (tr!("Country:"), &mut f.country, tr!("e.g. DE, AT, CH")),
                                (tr!("AddOns:"), &mut f.addons, tr!("e.g. AMXX, SourceMod")),
                            ] {
                                ui.label(label);
                                ui.add(
                                    TextEdit::singleline(value)
                                        .align(egui::Align2::LEFT_CENTER)
                                        .hint_text(hint)
                                        .desired_width(220.0),
                                )
                                .with_edit_menu();
                                ui.end_row();
                            }
                        });
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(tr!("Several values separated by commas; 0 = no limit."))
                            .color(ui.visuals().weak_text_color()),
                    );
                    ui.add_space(4.0);
                    egui::Grid::new("filter_ranges")
                        .num_columns(3)
                        .spacing([8.0, 5.0])
                        .show(ui, |ui| {
                            ui.label("");
                            ui.label(tr!("min"));
                            ui.label(tr!("max"));
                            ui.end_row();
                            let range = |ui: &mut Ui, label: &str, r: &mut Range| {
                                ui.label(label);
                                ui.add(egui::DragValue::new(&mut r.min).range(0..=9999));
                                ui.add(egui::DragValue::new(&mut r.max).range(0..=9999));
                                ui.end_row();
                            };
                            range(ui, tr!("Players:"), &mut f.players);
                            range(ui, tr!("Slots:"), &mut f.slots);
                            range(ui, tr!("Free Slots:"), &mut f.free_slots);
                            range(ui, tr!("Time Limit:"), &mut f.time_limit);
                            range(ui, tr!("Frag Limit:"), &mut f.frag_limit);
                            range(ui, tr!("Round Limit:"), &mut f.round_limit);
                            range(ui, tr!("Team Score:"), &mut f.team_score);
                        });
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(Button::new(tr!("Reset")).min_size(egui::vec2(75.0, 22.0)))
                        .clicked()
                    {
                        *f = ServerFilter {
                            enabled: f.enabled,
                            ..Default::default()
                        };
                    }
                    if ui
                        .add(Button::new(tr!("Close")).min_size(egui::vec2(75.0, 22.0)))
                        .clicked()
                    {
                        close = true;
                    }
                });
            });
        if !open || close {
            self.show_filter = false;
            actions.push(Action::SaveConfig);
        }
    }

    /// HLSW's "Connecting..." window while waiting for a free slot.
    fn wait_window(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some((id, since)) = self.waiting else {
            return;
        };
        let Some(s) = self.server(id) else { return };
        let info = s.info.as_ref();
        let name = info.map(|i| i.name.clone()).unwrap_or_default();
        let map = info.map(|i| i.map.clone()).unwrap_or_default();
        let players = s.players_text();
        let ping = s.ping.map(|p| format!("{p} ms")).unwrap_or_default();
        let waited = since.elapsed().as_secs();
        let mut open = true;
        NativeWindow::new("connecting", tr!("Connecting..."))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(320.0)
            .show(ctx, |ui| {
                egui::Grid::new("wait_for_slot")
                    .num_columns(2)
                    .spacing([10.0, 4.0])
                    .show(ui, |ui| {
                        for (label, value) in [
                            (tr!("Name:"), name),
                            (tr!("Map:"), map),
                            (tr!("Players:"), players),
                            (tr!("Ping:"), ping),
                            (
                                tr!("Time:"),
                                format!("{:02}:{:02}", waited / 60, waited % 60),
                            ),
                            (tr!("Status:"), tr!("Waiting for free slot").to_string()),
                        ] {
                            ui.label(RichText::new(label).color(ui.visuals().weak_text_color()));
                            ui.label(value);
                            ui.end_row();
                        }
                        ui.label(
                            RichText::new(tr!("Password:")).color(ui.visuals().weak_text_color()),
                        );
                        ui.add(
                            TextEdit::singleline(&mut self.join_password)
                                .align(egui::Align2::LEFT_CENTER)
                                .password(self.hide_join_password)
                                .desired_width(160.0),
                        );
                        ui.end_row();
                    });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(Button::new(tr!("Connect now")).min_size(egui::vec2(90.0, 22.0)))
                        .clicked()
                    {
                        actions.push(Action::ConnectNow(id));
                    }
                    if ui
                        .add(Button::new(tr!("Cancel")).min_size(egui::vec2(75.0, 22.0)))
                        .clicked()
                    {
                        actions.push(Action::CancelWait);
                    }
                });
            });
        if !open {
            actions.push(Action::CancelWait);
        }
        ctx.request_repaint_after(Duration::from_millis(500));
    }

    fn dialogs(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        self.iddb_window(ctx, actions);
        self.wait_window(ctx, actions);
        self.filter_window(ctx, actions);
        self.find_window(ctx, actions);
        self.menu_dialog(ctx, actions);
        self.trace_window(ctx);
        if let Some(dialog) = &mut self.settings_dialog {
            let p = self.settings.theme.palette();
            let addresses: Vec<String> =
                self.servers.iter().map(|s| s.address.to_string()).collect();
            match dialog.show(
                ctx,
                &p,
                &self.settings,
                &addresses,
                self.game_icons.as_ref(),
            ) {
                DialogResult::Open => {}
                DialogResult::Apply(new) => actions.push(Action::ApplySettings(Box::new(new))),
                DialogResult::Ok(new) => {
                    actions.push(Action::ApplySettings(Box::new(new)));
                    self.settings_dialog = None;
                }
                DialogResult::Cancel => self.settings_dialog = None,
            }
        }

        let mut show_about = self.show_about;
        if show_about && self.about_icon.is_none() {
            self.about_icon = Some(ctx.load_texture(
                "about_icon",
                egui::ColorImage::from_rgba_unmultiplied([64, 64], game_icons::APP_ICON_RGBA),
                egui::TextureOptions::LINEAR,
            ));
        }
        let icon = self.about_icon.clone();
        let mut close = false;
        let mut open_license = false;
        NativeWindow::new("about", tr!("About openHLSW"))
            .open(&mut show_about)
            .collapsible(false)
            .resizable(false)
            .default_width(400.0)
            .margin(0)
            .show(ctx, |ui| {
                about_view(ui, icon.as_ref(), &mut close, &mut open_license, actions);
            });
        if close {
            show_about = false;
        }
        if open_license {
            self.show_license = true;
        }
        self.show_about = show_about;

        let mut show_license = self.show_license;
        NativeWindow::new("license", tr!("License"))
            .open(&mut show_license)
            .collapsible(false)
            .resizable(true)
            .default_size([620.0, 520.0])
            .show(ctx, license_view);
        self.show_license = show_license;

        let mut close = false;
        if let Some((id, text)) = &mut self.editing_comment {
            let id = *id;
            let mut open = true;
            NativeWindow::new("comment", tr!("Comment"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    let r = ui
                        .add(
                            TextEdit::singleline(text)
                                .align(egui::Align2::LEFT_CENTER)
                                .desired_width(260.0),
                        )
                        .with_edit_menu();
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    if !enter {
                        r.request_focus();
                    }
                    ui.horizontal(|ui| {
                        if ui.button(tr!("OK")).clicked() || enter {
                            if let Some(s) = self.servers.iter_mut().find(|s| s.id == id) {
                                s.comment = text.trim().to_string();
                            }
                            actions.push(Action::SaveConfig);
                            close = true;
                        }
                        if ui.button(tr!("Cancel")).clicked() {
                            close = true;
                        }
                    });
                });
            if !open {
                close = true;
            }
        }
        if close {
            self.editing_comment = None;
        }
    }
}

// --------------------------------------------------------------------------
// Widgets
// --------------------------------------------------------------------------

/// White list-view area with the thin Windows border.
pub(crate) fn list_frame<R>(ui: &mut Ui, p: &Palette, add: impl FnOnce(&mut Ui) -> R) -> R {
    list_frame_with(ui, p, p.list_bg, add)
}

pub(crate) fn list_frame_with<R>(
    ui: &mut Ui,
    p: &Palette,
    fill: Color32,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, p.list_border))
        .inner_margin(1)
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            // Nothing inside (header cells, row highlights) may paint over the border line.
            let inside = ui.max_rect().intersect(ui.clip_rect());
            ui.set_clip_rect(inside);
            add(ui)
        })
        .inner
}

/// Windows-style group box with the title sitting on the top border.
pub(crate) fn group_box(
    ui: &mut Ui,
    p: &Palette,
    title: &str,
    width: f32,
    add: impl FnOnce(&mut Ui),
) {
    let response = Frame::new()
        .stroke(Stroke::new(1.0, p.group_border))
        .inner_margin(Margin {
            left: 8,
            right: 8,
            top: 10,
            bottom: 6,
        })
        .outer_margin(Margin {
            top: 6,
            ..Default::default()
        })
        .show(ui, |ui| {
            ui.set_width(width);
            // Rows as close together as in HLSW's dialogs.
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.vertical(|ui| add(ui));
        })
        .response;
    let galley = WidgetText::from(title).into_galley(
        ui,
        Some(TextWrapMode::Extend),
        f32::INFINITY,
        TextStyle::Body,
    );
    let pos = egui::pos2(
        response.rect.left() + 6.0,
        response.rect.top() + 6.0 - galley.size().y / 2.0,
    );
    let bg = egui::Rect::from_min_size(
        pos - egui::vec2(2.0, 0.0),
        galley.size() + egui::vec2(4.0, 0.0),
    );
    ui.painter().rect_filled(bg, 0.0, p.window);
    ui.painter().galley(pos, galley, p.text);
}

/// Puts the current player name first in the list of names used before (at most 20).
fn remember_player_name(settings: &mut Settings) {
    let name = settings.player_name.trim().to_string();
    if name.is_empty() {
        return;
    }
    settings.player_names.retain(|n| *n != name);
    settings.player_names.insert(0, name);
    settings.player_names.truncate(20);
}

pub(crate) fn toolbar_separator(ui: &mut Ui, p: &Palette) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 16.0), Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        Stroke::new(1.0, p.list_border),
    );
}

/// HLSW's flat bottom tabs: "Rcon Config | Console | Chat | …".
pub(crate) fn tab_strip<T: Copy + PartialEq>(
    ui: &mut Ui,
    p: &Palette,
    tabs: &[(&str, Option<T>)],
    current: T,
) -> Option<T> {
    let mut clicked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for &(title, tab) in tabs {
            let galley = WidgetText::from(crate::i18n::t(title)).into_galley(
                ui,
                Some(TextWrapMode::Extend),
                f32::INFINITY,
                TextStyle::Body,
            );
            let size = galley.size() + egui::vec2(8.0, 4.0);
            let sense = if tab.is_some() {
                Sense::click()
            } else {
                Sense::hover()
            };
            let (rect, resp) = ui.allocate_exact_size(size, sense);
            let selected = tab == Some(current);
            let painter = ui.painter();
            if selected {
                painter.rect_filled(rect, 0.0, p.list_bg);
                painter.rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(1.0, p.list_border),
                    StrokeKind::Inside,
                );
            } else {
                if resp.hovered() && tab.is_some() {
                    painter.rect_filled(rect, 0.0, p.hover_bg);
                }
                painter.vline(
                    rect.right() - 0.5,
                    rect.y_range().shrink(3.0),
                    Stroke::new(1.0, p.list_border),
                );
            }
            let color = if tab.is_some() {
                p.text
            } else {
                p.disabled_text
            };
            painter.galley(rect.min + egui::vec2(4.0, 2.0), galley, color);
            if let Some(t) = tab {
                if resp.clicked() {
                    clicked = Some(t);
                }
            } else {
                resp.on_hover_text(tr!("Not implemented yet"));
            }
        }
    });
    clicked
}

/// Table with HLSW-style header (white, thin separators) and no grid lines in the body.
pub(crate) fn styled_table<'a>(ui: &'a mut Ui, id: &str) -> TableBuilder<'a> {
    ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
    ui.visuals_mut().widgets.noninteractive.bg_stroke = Stroke::NONE;
    // Selectable labels would swallow the clicks meant for the row (selection, context menu).
    ui.style_mut().interaction.selectable_labels = false;
    TableBuilder::new(ui)
        .id_salt(id)
        .striped(false)
        .resizable(true)
        .auto_shrink(false)
        .min_scrolled_height(0.0)
        .cell_layout(Layout::left_to_right(Align::Center))
}

/// Header cell; returns the response of the whole cell (click = sort, double click = auto-size).
pub(crate) fn header_cell(ui: &mut Ui, p: &Palette, add: impl FnOnce(&mut Ui)) -> egui::Response {
    // Cover the gaps between the cells, but never the list frame (first / last column).
    let rect = ui
        .max_rect()
        .expand2(egui::vec2(3.0, 0.0))
        .intersect(ui.clip_rect());
    let response = ui.interact(
        ui.max_rect(),
        ui.scope_id().with("header_cell"),
        Sense::click(),
    );
    ui.painter().rect_filled(rect, 0.0, p.header_bg);
    ui.painter().vline(
        rect.right() - 0.5,
        rect.y_range(),
        Stroke::new(1.0, p.header_border),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.header_border),
    );
    add(ui);
    response
}

/// Double click on the column separator to the right of this header cell (where the
/// resize cursor shows). egui_extras does not detect that itself.
fn separator_double_clicked(ui: &Ui) -> bool {
    let cell = ui.max_rect();
    let line_x = cell.right() + ui.spacing().item_spacing.x / 2.0;
    let grab = ui.style().interaction.resize_grab_radius_side + 1.0;
    ui.input(|i| {
        i.pointer
            .button_double_clicked(egui::PointerButton::Primary)
            && i.pointer.interact_pos().is_some_and(|pos| {
                (pos.x - line_x).abs() <= grab && pos.y >= cell.top() && pos.y <= cell.bottom()
            })
    })
}

/// Double click on a column header widens the column to its longest value: the
/// request is kept in egui's memory and applied to that column on the next frame.
pub(crate) fn take_autosize(ui: &Ui, table: &str) -> Option<usize> {
    ui.ctx()
        .data_mut(|d| d.remove_temp::<usize>(egui::Id::unique(("autosize", table))))
}

pub(crate) fn request_autosize(ui: &Ui, table: &str, col: usize) {
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::unique(("autosize", table)), col));
    ui.ctx().request_repaint();
}

/// Sortable header cell: click sorts, double click (or a double click on the separator)
/// requests auto-size (first value). Also returns the cell's response for its context menu.
fn sort_header<C: PartialEq + Copy + Send + Sync + 'static>(
    ui: &mut Ui,
    p: &Palette,
    title: &str,
    col: C,
    sort: &mut (C, bool),
) -> (bool, egui::Response) {
    let edge = separator_double_clicked(ui);
    let resp = header_cell(ui, p, |ui| {
        ui.add(
            Label::new(crate::i18n::t(title))
                .selectable(false)
                .wrap_mode(fit(ui)),
        );
        if sort.0 == col {
            // Drawn by hand: the bundled UI fonts have no arrow glyphs.
            let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), Sense::hover());
            let c = rect.center();
            let (tip, base) = if sort.1 {
                (c.y - 2.5, c.y + 2.5)
            } else {
                (c.y + 2.5, c.y - 2.5)
            };
            let points = vec![
                egui::pos2(c.x, tip),
                egui::pos2(c.x - 3.5, base),
                egui::pos2(c.x + 3.5, base),
            ];
            ui.painter().add(egui::Shape::convex_polygon(
                points,
                p.weak_text,
                Stroke::NONE,
            ));
        }
    });
    // A double click (auto-size) must not change the sorting: remember the order before
    // the first click and restore it when the second click turns out to be a double click.
    let memory_id = resp.id.with("sort_before_click");
    if resp.double_clicked() {
        if let Some(previous) = ui.ctx().data_mut(|d| d.get_temp::<(C, bool)>(memory_id)) {
            *sort = previous;
        }
    } else if resp.clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(memory_id, *sort));
        *sort = if sort.0 == col {
            (col, !sort.1)
        } else {
            (col, true)
        };
    }
    (resp.double_clicked() || edge, resp)
}

/// Right-click menu of a list header like HLSW: one check box per column, then
/// "Reset column width". `columns` is (title, visible, can be hidden).
fn column_menu(
    ui: &mut Ui,
    players: bool,
    columns: &[(&str, bool, bool)],
    actions: &mut Vec<Action>,
) {
    for &(title, visible, optional) in columns {
        let mut on = visible;
        if ui
            .add_enabled(
                optional,
                egui::Checkbox::new(&mut on, crate::i18n::t(title)),
            )
            .clicked()
        {
            actions.push(Action::ToggleColumn {
                players,
                title: title.to_string(),
            });
        }
    }
    ui.separator();
    if ui.button(tr!("Reset column width")).clicked() {
        actions.push(Action::ResetColumns { players });
        ui.close();
    }
}

/// Header without sorting; returns true on double click (auto-size).
pub(crate) fn plain_header(ui: &mut Ui, p: &Palette, title: &str) -> bool {
    let edge = separator_double_clicked(ui);
    header_cell(ui, p, |ui| {
        ui.add(Label::new(title).selectable(false).wrap_mode(fit(ui)));
    })
    .double_clicked()
        || edge
}

/// Width of `text` in the list font.
fn text_width(ui: &Ui, text: &str) -> f32 {
    let font = TextStyle::Body.resolve(ui.style());
    let job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, Color32::WHITE);
    ui.fonts_mut(|f| f.layout_job(job)).size().x
}

/// Auto-size width of a column: the widest of the header and *all* rows (also the ones
/// scrolled out of view). Each row gives its text plus the width of icons in front of it.
fn fit_column(ui: &Ui, header: &str, rows: impl Iterator<Item = (String, f32)>) -> f32 {
    let header = text_width(ui, header) + 14.0; // room for the sort arrow
    rows.map(|(t, extra)| text_width(ui, &t) + extra)
        .fold(header, f32::max)
        .ceil()
        + 4.0
}

/// A column forced to `width` for one frame (the table keeps it afterwards).
fn sized(column: Column, width: Option<f32>) -> Column {
    match width {
        Some(w) => column.range(w..=w),
        None => column,
    }
}

/// Text in list cells: truncated with "…", except during the auto-size pass of a column
/// (double-click on its header or separator), where the full width must be measured.
pub(crate) fn fit(ui: &Ui) -> egui::TextWrapMode {
    if ui.is_sizing_pass() {
        egui::TextWrapMode::Extend
    } else {
        egui::TextWrapMode::Truncate
    }
}

/// One list cell. Selected rows use the selection text color for everything.
fn cell(
    row: &mut TableRow<'_, '_>,
    text: String,
    color: Option<Color32>,
    bg: Option<Color32>,
    selected: bool,
) -> egui::Rect {
    row.col(|ui| {
        if let Some(bg) = bg {
            let rect = ui.max_rect().expand2(0.5 * egui::vec2(6.0, 0.0));
            ui.painter().rect_filled(rect, 0.0, bg);
        }
        let mut rt = RichText::new(text);
        if let (Some(c), false) = (color, selected) {
            rt = rt.color(c);
        }
        ui.add(Label::new(rt).wrap_mode(fit(ui)).selectable(false));
    })
    .0
}

struct ListOptions<'a> {
    game_icons: Option<&'a GameIcons>,
    other_lists: &'a [String],
    show_comment: bool,
    /// Columns switched off in the header's right-click menu (titles).
    hidden: &'a [String],
    /// Bumped by "Reset column width" (new table id = default widths).
    generation: u32,
    flags: Option<&'a Flags>,
    /// Per server (same order as `servers`): country code, provider.
    locations: &'a [(String, String)],
    needle: &'a str,
    filter: &'a crate::filter::ServerFilter,
    selected: Option<ServerId>,
    connect_on_double_click: bool,
    keep_order_on_hover: bool,
}

fn server_table(
    ui: &mut Ui,
    p: &Palette,
    servers: &[Server],
    opts: &ListOptions<'_>,
    last_order: &mut Vec<ServerId>,
    sort: &mut (Col, bool),
    actions: &mut Vec<Action>,
) {
    let selected = opts.selected;
    let fresh: Vec<usize> = sorted_servers(servers, opts.locations, *sort)
        .into_iter()
        .filter(|&i| {
            servers[i].matches(opts.needle)
                && filter_matches(opts.filter, &servers[i], &opts.locations[i].0)
        })
        .collect();
    let hovered = ui.rect_contains_pointer(ui.max_rect());
    let order: Vec<usize> = if opts.keep_order_on_hover && hovered && !last_order.is_empty() {
        // Keep rows where they are while the mouse is over the list (new servers go to the end).
        let mut kept: Vec<usize> = last_order
            .iter()
            .filter_map(|id| fresh.iter().copied().find(|&i| servers[i].id == *id))
            .collect();
        let new: Vec<usize> = fresh
            .iter()
            .copied()
            .filter(|i| !kept.contains(i))
            .collect();
        kept.extend(new);
        // Servers that went offline still move to the end right away.
        kept.sort_by_key(|&i| servers[i].is_offline());
        kept
    } else {
        fresh
    };
    *last_order = order.iter().map(|&i| servers[i].id).collect();

    let visible = |c: Col, title: &str| match c {
        Col::Comment => opts.show_comment,
        Col::Index | Col::Name => true,
        _ => !opts.hidden.iter().any(|h| h == title),
    };
    let columns: Vec<(Col, &str, f32)> = SERVER_COLUMNS
        .iter()
        .copied()
        .filter(|&(c, t, _)| visible(c, t))
        .collect();
    let menu_items: Vec<(&str, bool, bool)> = SERVER_COLUMNS
        .iter()
        .map(|&(c, t, _)| (t, visible(c, t), !matches!(c, Col::Index | Col::Name)))
        .collect();
    let autosize = take_autosize(ui, "servers");
    // Double click on a header / separator: fit the column to all servers in the list.
    let fit_width = autosize
        .and_then(|i| columns.get(i))
        .map(|&(col, title, _)| {
            let spacing = 6.0;
            fit_column(
                ui,
                title,
                order.iter().map(|&idx| {
                    let s = &servers[idx];
                    let info = s.info.as_ref();
                    match col {
                        Col::Index => {
                            let star = if s.rcon_password.is_empty() {
                                0.0
                            } else {
                                12.0 + spacing
                            };
                            ((idx + 1).to_string(), 16.5 + spacing + star)
                        }
                        Col::Address => (s.address.to_string(), 0.0),
                        Col::Name => (
                            info.map(|i| i.name.clone()).unwrap_or_default(),
                            14.0 + spacing,
                        ),
                        Col::Game => (s.game_short(), 0.0),
                        Col::Map => (info.map(|i| i.map.clone()).unwrap_or_default(), 0.0),
                        Col::Players => (s.players_text(), 0.0),
                        Col::Ping => (s.ping_text(), 0.0),
                        Col::Version => (s.version_text(), 0.0),
                        Col::Location => (opts.locations[idx].1.clone(), 0.0),
                        Col::AddOn => (s.addons_short(), 0.0),
                        Col::AntiCheat => (s.anticheat(), 0.0),
                        Col::Ranking => (s.ranking().0, 0.0),
                        Col::Limits => (s.limits(), 0.0),
                        Col::Comment => (s.comment.clone(), 0.0),
                        Col::GameName | Col::GameAddress => (String::new(), 0.0),
                    }
                }),
            )
        });
    let table_id = format!("servers{}", opts.generation);
    let arrows = arrow_keys(ui.ctx(), "servers");
    let mut table = styled_table(ui, &table_id).sense(Sense::click());
    // Arrow up / down select the previous / next server.
    if let Some((down, _)) = arrows
        && !order.is_empty()
    {
        let current = order.iter().position(|&i| Some(servers[i].id) == selected);
        let new = match current {
            Some(i) if down => (i + 1).min(order.len() - 1),
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        actions.push(Action::Select(servers[order[new]].id));
        table = table.scroll_to_row(new, None);
    }
    for (i, (_, _, width)) in columns.iter().enumerate() {
        let column = if i + 1 == columns.len() {
            Column::remainder().at_least(60.0)
        } else {
            sized(
                Column::initial(*width).at_least(20.0),
                fit_width.filter(|_| autosize == Some(i)),
            )
        };
        table = table.column(column);
    }
    let mut autosize_request = None;

    table
        .header(HEADER_HEIGHT, |mut header| {
            for (i, &(col, title, _)) in columns.iter().enumerate() {
                header.col(|ui| {
                    let (auto, resp) = sort_header(ui, p, title, col, sort);
                    if auto {
                        autosize_request = Some(i);
                    }
                    resp.context_menu(|ui| column_menu(ui, false, &menu_items, actions));
                });
            }
        })
        .body(|body| {
            body.rows(ROW_HEIGHT, order.len(), |mut row| {
                let idx = order[row.index()];
                let s = &servers[idx];
                let sel = Some(s.id) == selected;
                row.set_selected(sel);
                let offline = s.is_offline();
                let bg = (offline && !sel).then_some(p.offline_bg);
                // Offline: the whole line in red (with the last known infos).
                let red = offline.then_some(p.red);
                let info = s.info.as_ref();
                let players_color = if offline {
                    Some(p.red)
                } else if s.humans() > 0 {
                    Some(p.green)
                } else {
                    None
                };

                let (country, provider) = &opts.locations[idx];
                let mut comment_rect = egui::Rect::NOTHING;
                for &(col, _, _) in &columns {
                    match col {
                        // Country flag + number (+ star when an RCON password is stored).
                        Col::Index => {
                            row.col(|ui| {
                                if let Some(bg) = bg {
                                    ui.painter().rect_filled(
                                        ui.max_rect().expand2(egui::vec2(3.0, 0.0)),
                                        0.0,
                                        bg,
                                    );
                                }
                                match opts.flags {
                                    Some(flags) if flags.has(country) => {
                                        flags
                                            .show(ui, country, 11.0)
                                            .on_hover_text(country.to_uppercase());
                                    }
                                    _ => {
                                        // Same space as a flag, so the numbers line up.
                                        ui.allocate_exact_size(
                                            egui::vec2(16.5, 11.0),
                                            Sense::hover(),
                                        );
                                    }
                                }
                                let mut num = RichText::new((idx + 1).to_string());
                                if let (Some(c), false) = (red, sel) {
                                    num = num.color(c);
                                }
                                ui.add(Label::new(num).selectable(false));
                                if !s.rcon_password.is_empty() {
                                    star(ui, 12.0).on_hover_text(tr!("RCON password set"));
                                }
                            });
                        }
                        Col::Address => {
                            cell(&mut row, s.address.to_string(), red, bg, sel);
                        }
                        // Name with the game icon in front, like HLSW.
                        Col::Name => {
                            row.col(|ui| {
                                if let Some(bg) = bg {
                                    ui.painter().rect_filled(
                                        ui.max_rect().expand2(egui::vec2(3.0, 0.0)),
                                        0.0,
                                        bg,
                                    );
                                }
                                if let (Some(icons), Some(i)) = (opts.game_icons, info) {
                                    let icon = if i.gamespy {
                                        game_icons::icon_for_gamespy(&i.folder)
                                    } else {
                                        game_icons::icon_for_server(i.real_app_id(), &i.folder)
                                    };
                                    icons.show(ui, icon, 14.0);
                                }
                                let mut name =
                                    RichText::new(info.map(|i| i.name.clone()).unwrap_or_default());
                                if let (Some(c), false) = (red, sel) {
                                    name = name.color(c);
                                }
                                ui.add(Label::new(name).wrap_mode(fit(ui)).selectable(false));
                            });
                        }
                        Col::Game => {
                            cell(&mut row, s.game_short(), Some(p.red), bg, sel);
                        }
                        Col::Map => {
                            let map = info.map(|i| i.map.clone()).unwrap_or_default();
                            cell(&mut row, map, Some(p.red), bg, sel);
                        }
                        Col::Players => {
                            cell(&mut row, s.players_text(), players_color, bg, sel);
                        }
                        Col::Ping => {
                            cell(&mut row, s.ping_text(), red, bg, sel);
                        }
                        Col::Version => {
                            cell(&mut row, s.version_text(), red, bg, sel);
                        }
                        Col::Location => {
                            cell(&mut row, provider.clone(), red, bg, sel);
                        }
                        Col::AddOn => {
                            cell(&mut row, s.addons_short(), red, bg, sel);
                        }
                        Col::AntiCheat => {
                            cell(&mut row, s.anticheat(), red, bg, sel);
                        }
                        Col::Ranking => {
                            cell(&mut row, s.ranking().0, red, bg, sel);
                        }
                        Col::Limits => {
                            cell(&mut row, s.limits(), red, bg, sel);
                        }
                        Col::Comment => {
                            comment_rect = cell(&mut row, s.comment.clone(), red, bg, sel);
                        }
                        Col::GameName | Col::GameAddress => {}
                    }
                }

                let response = row.response();
                let on_comment = response
                    .interact_pointer_pos()
                    .or(response.hover_pos())
                    .is_some_and(|pos| comment_rect.contains(pos));
                if response.clicked() || response.secondary_clicked() {
                    actions.push(Action::Select(s.id));
                    set_active_list(&response.ctx, "servers");
                }
                if response.double_clicked() {
                    if on_comment {
                        actions.push(Action::EditComment(s.id));
                    } else if opts.connect_on_double_click {
                        actions.push(Action::Connect(s.id));
                    }
                }
                response.context_menu(|ui| server_context_menu(ui, s, opts.other_lists, actions));
            });
        });
    if let Some(i) = autosize_request {
        request_autosize(ui, "servers", i);
    }
}

/// "Change ID" submenu: the # of a server is its position in the list.
fn change_id_menu(ui: &mut Ui, id: ServerId, actions: &mut Vec<Action>) {
    ui.menu_button(tr!("Change ID"), |ui| {
        if ui.button(tr!("Increase\t+")).clicked() {
            actions.push(Action::MoveServer(id, 1));
            ui.close();
        }
        if ui.button(tr!("Decrease\t-")).clicked() {
            actions.push(Action::MoveServer(id, -1));
            ui.close();
        }
        if ui.button(tr!("Set")).clicked() {
            actions.push(Action::OpenDialog(Dialog::SetId(id, String::new())));
            ui.close();
        }
        ui.separator();
        if ui
            .button(tr!("Set all server IDs to current sequence"))
            .clicked()
        {
            actions.push(Action::RenumberServers);
            ui.close();
        }
    });
}

/// Right-click menu of the server list, laid out like HLSW's.
fn server_context_menu(ui: &mut Ui, s: &Server, other_lists: &[String], actions: &mut Vec<Action>) {
    let id = s.id;
    if ui.button(tr!("Connect...")).clicked() {
        actions.push(Action::Connect(id));
        ui.close();
    }
    copy_menu(ui, s, actions);
    ui.separator();
    if ui.button(tr!("Delete\tDel")).clicked() {
        actions.push(Action::Remove(id));
        ui.close();
    }
    list_menus(ui, id, other_lists, actions);
    ui.separator();
    change_id_menu(ui, id, actions);
    if ui.button(tr!("Reset Average Ping")).clicked() {
        actions.push(Action::ResetPing(id));
        ui.close();
    }
    ui.separator();
    if ui.button(tr!("Properties...")).clicked() {
        actions.push(Action::OpenDialog(props_dialog(s)));
        ui.close();
    }
    if ui.button(tr!("Trace Route...")).clicked() {
        actions.push(Action::TraceRoute(id));
        ui.close();
    }
    ui.separator();
    if ui
        .add_enabled(s.supports_rcon(), Button::new(tr!("Rcon Status Command")))
        .on_disabled_hover_text(tr!("RCON is not supported for this game"))
        .clicked()
    {
        actions.push(Action::RconStatus(id));
        ui.close();
    }
}

fn copy_menu(ui: &mut Ui, s: &Server, actions: &mut Vec<Action>) {
    ui.menu_button(tr!("Copy"), |ui| {
        let name = s.info.as_ref().map(|i| i.name.clone()).unwrap_or_default();
        let items = [
            (tr!("Address"), s.address.to_string()),
            (tr!("Name"), name.clone()),
            (tr!("Address + Name"), format!("{} {name}", s.address)),
            (
                tr!("steam://connect link"),
                format!("steam://connect/{}", s.address),
            ),
        ];
        for (label, text) in items {
            if ui.button(label).clicked() {
                actions.push(Action::CopyText(text));
                ui.close();
            }
        }
    });
}

fn list_menus(ui: &mut Ui, id: ServerId, other_lists: &[String], actions: &mut Vec<Action>) {
    ui.add_enabled_ui(!other_lists.is_empty(), |ui| {
        ui.menu_button(tr!("Move to Server List"), |ui| {
            for name in other_lists {
                if ui.button(name).clicked() {
                    actions.push(Action::MoveToList(id, name.clone()));
                    ui.close();
                }
            }
        });
        ui.menu_button(tr!("Copy to Server List"), |ui| {
            for name in other_lists {
                if ui.button(name).clicked() {
                    actions.push(Action::CopyToList(id, name.clone()));
                    ui.close();
                }
            }
        });
    });
}

fn props_dialog(s: &Server) -> Dialog {
    Dialog::ServerProperties(
        s.id,
        ServerPropsDraft {
            address: s.address.to_string(),
            comment: s.comment.clone(),
            params: s.params.clone(),
            rcon_password: s.rcon_password.clone(),
            save_password: s.save_password,
        },
    )
}

/// Entries of the "Server" menu.
fn server_menu_items(ui: &mut Ui, s: &Server, other_lists: &[String], actions: &mut Vec<Action>) {
    let id = s.id;
    if ui.button(tr!("Connect...\tCtrl+G")).clicked() {
        actions.push(Action::Connect(id));
        ui.close();
    }
    copy_menu(ui, s, actions);
    ui.separator();
    if ui.button(tr!("Delete\tCtrl+D")).clicked() {
        actions.push(Action::Remove(id));
        ui.close();
    }
    list_menus(ui, id, other_lists, actions);
    ui.separator();
    if ui.button(tr!("Edit comment...")).clicked() {
        actions.push(Action::EditComment(id));
        ui.close();
    }
    change_id_menu(ui, id, actions);
    if ui.button(tr!("Reset Average Ping")).clicked() {
        actions.push(Action::ResetPing(id));
        ui.close();
    }
    if ui.button(tr!("Properties...\tCtrl+P")).clicked() {
        actions.push(Action::OpenDialog(props_dialog(s)));
        ui.close();
    }
    if ui.button(tr!("Trace Route...\tCtrl+T")).clicked() {
        actions.push(Action::TraceRoute(id));
        ui.close();
    }
    ui.separator();
    if ui
        .add_enabled(s.supports_rcon(), Button::new(tr!("Rcon Status Command")))
        .on_disabled_hover_text(tr!("RCON is not supported for this game"))
        .clicked()
    {
        actions.push(Action::RconStatus(id));
        ui.close();
    }
}

fn sorted_servers(
    servers: &[Server],
    locations: &[(String, String)],
    (col, ascending): (Col, bool),
) -> Vec<usize> {
    fn text(s: Option<&String>) -> String {
        s.map(|s| s.to_lowercase()).unwrap_or_default()
    }
    let mut order: Vec<usize> = (0..servers.len()).collect();
    order.sort_by(|&a, &b| {
        let (x, y) = (&servers[a], &servers[b]);
        let (xi, yi) = (x.info.as_ref(), y.info.as_ref());
        let ord = match col {
            Col::Index => a.cmp(&b),
            Col::Address => x.address.to_string().cmp(&y.address.to_string()),
            Col::Name => text(xi.map(|i| &i.name)).cmp(&text(yi.map(|i| &i.name))),
            Col::Game => x.game_short().cmp(&y.game_short()),
            Col::Map => text(xi.map(|i| &i.map)).cmp(&text(yi.map(|i| &i.map))),
            Col::Players => xi.map(|i| i.players).cmp(&yi.map(|i| i.players)),
            Col::Ping => x.ping.unwrap_or(u32::MAX).cmp(&y.ping.unwrap_or(u32::MAX)),
            Col::Version => text(xi.map(|i| &i.version)).cmp(&text(yi.map(|i| &i.version))),
            Col::Location => locations[a]
                .1
                .to_lowercase()
                .cmp(&locations[b].1.to_lowercase()),
            Col::AddOn => x.addons_short().cmp(&y.addons_short()),
            Col::AntiCheat => x.anticheat().cmp(&y.anticheat()),
            Col::Ranking => x.ranking().0.cmp(&y.ranking().0),
            Col::Limits => x.limits().cmp(&y.limits()),
            Col::Comment => x.comment.to_lowercase().cmp(&y.comment.to_lowercase()),
            Col::GameName => x
                .game_short()
                .cmp(&y.game_short())
                .then_with(|| text(xi.map(|i| &i.name)).cmp(&text(yi.map(|i| &i.name)))),
            Col::GameAddress => x
                .game_short()
                .cmp(&y.game_short())
                .then_with(|| x.address.to_string().cmp(&y.address.to_string())),
        };
        let ord = if ascending { ord } else { ord.reverse() };
        // Offline servers always go to the end (sorted among themselves the same way) and
        // move back up as soon as they answer again.
        x.is_offline()
            .cmp(&y.is_offline())
            .then(ord)
            .then(a.cmp(&b))
    });
    order
}

/// Returns true when the comment was edited.
/// `location`: `Some(None)` = LAN address, `None` = not known (yet).
fn info_panel(
    ui: &mut Ui,
    p: &Palette,
    server_mut: Option<&mut Server>,
    location: Option<Option<GeoInfo>>,
    flags: Option<&Flags>,
    show_graph: bool,
) -> bool {
    let mut comment_changed = false;
    ScrollArea::vertical()
        .id_salt("info_scroll")
        .auto_shrink(false)
        .show(ui, |ui| {
            let server = server_mut.as_deref();
            ui.add_space(2.0);
            // Title (HLSW shows the server name as a link) and the ping graph.
            let title = server.map(|s| {
                s.info
                    .as_ref()
                    .map(|i| i.name.clone())
                    .unwrap_or_else(|| s.address.to_string())
            });
            // Like HLSW: the name in link colour, underlined, in the normal font size.
            ui.horizontal(|ui| {
                ui.add_space(3.0);
                match &title {
                    Some(t) => ui.add(
                        Label::new(RichText::new(t).color(p.link).underline())
                            .wrap()
                            .selectable(false),
                    ),
                    None => ui.label(""),
                };
            });
            ui.add_space(2.0);
            if show_graph {
                ui.horizontal(|ui| {
                    ui.add_space(3.0);
                    // The graph grows with the panel (drag the panel border to enlarge it).
                    let width = (ui.available_width() - 6.0).max(100.0);
                    let height = (width * 0.09).clamp(36.0, 110.0);
                    ping_graph(
                        ui,
                        p,
                        egui::vec2(width, height),
                        server.map(|s| &s.ping_history),
                        PING_HISTORY,
                    );
                });
            }
            ui.add_space(5.0);

            let Some(s) = server else {
                ui.label(RichText::new(tr!("No server selected.")).color(p.weak_text));
                return;
            };
            let text = |t: String| RichText::new(if t.is_empty() { "-".to_string() } else { t });

            ui.spacing_mut().interact_size.y = 14.0;
            egui::Grid::new("info_grid")
                .num_columns(2)
                .spacing([10.0, 2.0])
                .show(ui, |ui| {
                    let row = |ui: &mut Ui, key: &str, value: RichText| {
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            ui.label(RichText::new(key).color(p.text));
                        });
                        ui.add(Label::new(value).wrap());
                        ui.end_row();
                    };
                    row(ui, tr!("Address"), text(s.address.to_string()));
                    let ping = match (&s.state, s.ping) {
                        (QueryState::Online, Some(ms)) => {
                            let avg = s
                                .avg_ping()
                                .map(|a| format!(" (Ø {a:.1} ms)"))
                                .unwrap_or_default();
                            RichText::new(format!("{ms} ms{avg}"))
                        }
                        (QueryState::Offline(e), _) => RichText::new(e).color(p.red),
                        _ => RichText::new(tr!("querying…")).color(p.weak_text),
                    };
                    row(ui, tr!("Ping"), ping);
                    match &location {
                        Some(Some(g)) if !g.country.is_empty() => {
                            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                ui.label(RichText::new(tr!("Location")).color(p.text));
                            });
                            ui.horizontal(|ui| {
                                if let Some(f) = flags.filter(|f| f.has(&g.country_code)) {
                                    f.show(ui, &g.country_code, 11.0);
                                }
                                ui.label(g.place());
                            });
                            ui.end_row();
                            let provider = if g.asn.is_empty() {
                                g.provider().to_string()
                            } else {
                                format!("{} ({})", g.provider(), g.asn)
                            };
                            row(ui, tr!("Provider"), text(provider));
                        }
                        Some(Some(_)) => row(ui, tr!("Location"), text(tr!("unknown").into())),
                        Some(None) => row(ui, tr!("Location"), text(tr!("Local network").into())),
                        None => {}
                    }
                    if let Some(i) = &s.info {
                        row(
                            ui,
                            tr!("Game"),
                            text(format!(
                                "{} ({}, AppID {})",
                                i.game,
                                i.folder,
                                i.real_app_id()
                            )),
                        );
                        let mut players = format!("{} / {}", i.players, i.max_players);
                        if i.bots > 0 {
                            players += &tr!(" ({bots} bots)", bots = i.bots);
                        }
                        let pc = if s.humans() > 0 { p.green } else { p.text };
                        row(ui, tr!("Players"), RichText::new(players).color(pc));
                        let next = s
                            .rule("sm_nextmap")
                            .filter(|n| !n.is_empty())
                            .map(|n| tr!("  ({n} next)", n = n))
                            .unwrap_or_default();
                        row(ui, tr!("Map"), text(format!("{}{next}", i.map)));
                        row(ui, tr!("Limits"), text(s.limits()));
                        let ff = match s
                            .rule("mp_friendlyfire")
                            .or(s.rule("bf2_friendlyfire"))
                            .or(s.rule("bf2142_friendlyfire"))
                        {
                            Some("0") => tr!("No"),
                            Some(_) => tr!("Yes"),
                            None => "",
                        };
                        row(ui, tr!("Friendly Fire"), text(ff.into()));
                        row(
                            ui,
                            tr!("Server Type"),
                            text(format!("{}, {}", i.server_type_name(), i.os_name())),
                        );
                        row(ui, tr!("Version"), text(i.version.clone()));
                        row(ui, tr!("AddOns"), text(s.addons_compact()));
                        row(
                            ui,
                            tr!("Anti-Cheat"),
                            text(if i.vac {
                                tr!("Valve Anti-Cheat").into()
                            } else {
                                tr!("None").into()
                            }),
                        );
                        row(
                            ui,
                            tr!("Password"),
                            text(if i.passworded {
                                tr!("Yes").into()
                            } else {
                                tr!("No").into()
                            }),
                        );
                        if let Some(k) = &i.keywords {
                            row(ui, tr!("Tags"), text(k.clone()));
                        }
                        if let Some(id) = i.steam_id {
                            row(ui, tr!("Steam ID"), text(id.to_string()));
                        }
                        if let Some(port) = i.tv_port {
                            row(
                                ui,
                                tr!("SourceTV"),
                                text(tr!(
                                    "{name} (port {port})",
                                    name = i.tv_name.as_deref().unwrap_or(""),
                                    port = port
                                )),
                            );
                        }
                    }
                    if let Some(t) = s.last_seen {
                        let ago = (chrono::Local::now() - t).num_seconds().max(0);
                        row(ui, tr!("Last Seen"), text(tr!("{ago} s ago", ago = ago)));
                    }
                    if let Some(c) = &s.last_connected {
                        row(ui, tr!("Last Connected"), text(c.clone()));
                    }
                });

            if let Some(s) = server_mut {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(80.0, 18.0),
                        Layout::right_to_left(Align::Center),
                        |ui| {
                            ui.label(RichText::new(tr!("Comment")).color(p.text));
                        },
                    );
                    let r = ui
                        .add(
                            TextEdit::singleline(&mut s.comment)
                                .align(egui::Align2::LEFT_CENTER)
                                .id_salt("info_comment")
                                .hint_text(tr!("add a note for this server"))
                                .desired_width(ui.available_width() - 8.0),
                        )
                        .with_edit_menu();
                    if r.lost_focus() || (r.changed() && !r.has_focus()) {
                        comment_changed = true;
                    }
                });
            }
        });
    comment_changed
}

/// The hatched ping strip from HLSW's info panel, with the ping history drawn on top.
/// Contents of the Trace Route window.
fn trace_view(
    ui: &mut Ui,
    p: &Palette,
    trace: &mut crate::trace::TraceRoute,
    flags: Option<&Flags>,
    geo: &HashMap<IpAddr, GeoInfo>,
) {
    ui.horizontal(|ui| {
        let running = trace.running();
        let r = ui
            .add(
                TextEdit::singleline(&mut trace.target)
                    .align(egui::Align2::LEFT_CENTER)
                    .hint_text(tr!("IP address or host name"))
                    .desired_width(ui.available_width() - 90.0),
            )
            .with_edit_menu();
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let label = if running { tr!("Stop") } else { tr!("Start") };
        if ui
            .add(Button::new(label).min_size(egui::vec2(80.0, 22.0)))
            .clicked()
            || enter
        {
            if running && !enter {
                trace.stop_trace();
                trace.status = tr!("Stopped").into();
            } else {
                trace.start(ui.ctx());
            }
        }
    });
    ui.add_space(4.0);
    egui::Panel::bottom("trace_legend")
        .frame(Frame::new().fill(p.window))
        .show_separator_line(false)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (color, text) in [
                    (Color32::BLACK, tr!("Different Route")),
                    (Color32::from_rgb(255, 0, 0), tr!("Timeout")),
                ] {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(32.0, 14.0), Sense::hover());
                    ui.painter().rect_filled(rect, 0.0, color);
                    ui.label(text);
                    ui.add_space(30.0);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(&trace.status).color(p.weak_text));
                });
            });
        });
    egui::CentralPanel::default()
        .frame(Frame::new().fill(p.window))
        .show(ui, |ui| {
            list_frame(ui, p, |ui| {
                let row_h = 32.0;
                let mut prev_avg: Option<f32> = None;
                let rows: Vec<_> = trace
                    .hops
                    .iter()
                    .map(|h| {
                        let avg = h.avg();
                        let diff = match (avg, prev_avg) {
                            (Some(a), Some(b)) => Some(a - b),
                            (Some(a), None) => Some(a),
                            _ => None,
                        };
                        if avg.is_some() {
                            prev_avg = avg;
                        }
                        (h, avg, diff)
                    })
                    .collect();
                let autosize = take_autosize(ui, "trace");
                let widths = [24.0, 50.0, 50.0, 44.0, 110.0, 200.0, 170.0];
                let mut table = styled_table(ui, "trace");
                for (i, w) in widths.iter().enumerate() {
                    table = table.column(
                        Column::initial(*w)
                            .at_least(20.0)
                            .clip(true)
                            .auto_size_this_frame(autosize == Some(i)),
                    );
                }
                let mut autosize_request = None;
                table
                    .column(Column::remainder().at_least(120.0))
                    .header(HEADER_HEIGHT, |mut h| {
                        for (i, title) in [
                            "#",
                            tr!("Ping"),
                            tr!("Ø Ping"),
                            tr!("Diff"),
                            tr!("IP"),
                            tr!("Reverse DNS"),
                            tr!("Provider"),
                            tr!("Ping Graph"),
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            h.col(|ui| {
                                if plain_header(ui, p, title) {
                                    autosize_request = Some(i);
                                }
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(row_h, rows.len(), |mut row| {
                            let i = row.index();
                            let (h, avg, diff) = &rows[i];
                            let right = |ui: &mut Ui, text: RichText| {
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(text);
                                });
                            };
                            row.col(|ui| {
                                if h.changed {
                                    ui.painter().rect_filled(
                                        ui.max_rect().expand2(egui::vec2(3.0, 0.0)),
                                        0.0,
                                        Color32::BLACK,
                                    );
                                    ui.label(
                                        RichText::new((i + 1).to_string()).color(Color32::WHITE),
                                    );
                                } else {
                                    ui.label((i + 1).to_string());
                                }
                            });
                            row.col(|ui| match h.last() {
                                Some(Some(ms)) => right(ui, RichText::new(ms.to_string())),
                                Some(None) => right(ui, RichText::new(tr!("Timeout")).color(p.red)),
                                None => {}
                            });
                            row.col(|ui| {
                                right(
                                    ui,
                                    RichText::new(
                                        avg.map(|a| format!("{a:.1}")).unwrap_or_default(),
                                    ),
                                )
                            });
                            row.col(|ui| {
                                right(
                                    ui,
                                    RichText::new(
                                        diff.map(|d| format!("{d:.1}")).unwrap_or_default(),
                                    ),
                                )
                            });
                            row.col(|ui| {
                                let text =
                                    h.ip.map(|ip| ip.to_string()).unwrap_or_else(|| "*".into());
                                right(ui, RichText::new(text));
                            });
                            row.col(|ui| {
                                ui.add(Label::new(&h.rdns).wrap_mode(fit(ui)));
                            });
                            row.col(|ui| {
                                let Some(ip) = h.ip else { return };
                                if !geo::is_public(ip) {
                                    ui.label(tr!("LAN"));
                                } else if let Some(g) = geo.get(&ip) {
                                    if let Some(f) = flags.filter(|f| f.has(&g.country_code)) {
                                        f.show(ui, &g.country_code, 11.0).on_hover_text(g.place());
                                    }
                                    ui.add(Label::new(g.provider()).wrap_mode(fit(ui)));
                                }
                            });
                            row.col(|ui| {
                                let size = egui::vec2(ui.available_width().max(60.0), row_h - 4.0);
                                ping_graph(ui, p, size, Some(&h.pings), crate::trace::HISTORY);
                            });
                        });
                    });
                if let Some(i) = autosize_request {
                    request_autosize(ui, "trace", i);
                }
            });
        });
}

/// Bar color like HLSW: green for normal ping, orange for a raised one, red for a high one.
/// A ping counts as raised/high when it is high in absolute terms (≥ 100 / 200 ms) or
/// compared to the average of the graph (spikes).
/// Ping that is drawn in the middle of the graph and turns the bars yellow; twice this is
/// red and the top of the default scale.
const PING_HIGH: f32 = 100.0;

/// Colour of the ping graph at a ping: green at 0 ms, yellow at 100 ms, red from 200 ms.
fn ping_color(ms: f32) -> Color32 {
    let green = Color32::from_rgb(30, 200, 40);
    let yellow = Color32::from_rgb(245, 215, 0);
    let red = Color32::from_rgb(230, 30, 25);
    let t = (ms / PING_HIGH).clamp(0.0, 2.0);
    if t <= 1.0 {
        green.lerp_to_gamma(yellow, t)
    } else {
        yellow.lerp_to_gamma(red, t - 1.0)
    }
}

/// HLSW ping graph: hatched gray area with one bar per query, fills from the left and
/// scrolls once full. Timeouts are full-height red bars.
fn ping_graph(
    ui: &mut Ui,
    p: &Palette,
    size: egui::Vec2,
    history: Option<&VecDeque<Option<u32>>>,
    capacity: usize,
) {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    let empty: VecDeque<Option<u32>> = VecDeque::new();
    let history = history.unwrap_or(&empty);
    let step = rect.width() / capacity.max(2) as f32;
    let filled = rect.left() + step * history.len() as f32;

    // Hatched gray background over the whole graph, vertical gradient like HLSW.
    {
        let mut mesh = egui::Mesh::default();
        mesh.add_colored_rect(rect, p.graph_bg);
        let (top, bottom) = (p.graph_bg, p.graph_hatch);
        for (i, v) in mesh.vertices.iter_mut().enumerate() {
            v.color = if i < 2 { top } else { bottom };
        }
        painter.add(mesh);
        let mut x = rect.left() - rect.height();
        while x < rect.right() {
            painter.line_segment(
                [
                    egui::pos2(x, rect.bottom()),
                    egui::pos2(x + rect.height(), rect.top()),
                ],
                Stroke::new(1.0, p.graph_hatch),
            );
            x += 4.0;
        }
    }
    // Empty part: blue base line with tick marks.
    if filled < rect.right() {
        let y = rect.bottom() - 2.0;
        painter.hline(
            egui::Rangef::new(filled, rect.right()),
            y,
            Stroke::new(1.0, p.graph_line),
        );
        let mut tx = rect.left() + 12.0;
        while tx < rect.right() {
            if tx > filled {
                painter.vline(
                    tx,
                    egui::Rangef::new(y - 3.0, y),
                    Stroke::new(1.0, p.graph_line),
                );
            }
            tx += 12.0;
        }
    }

    // Fixed scale like a traffic light: 10 ms stays near the bottom, 100 ms reaches the
    // middle, 200 ms the top. Only servers that are slow all the time (90th percentile above
    // 200 ms) widen it, so their bars are not all cut off. Bars above the scale are clipped
    // at the top; their peak label shows the real value.
    let max = {
        let mut sorted: Vec<u32> = history.iter().flatten().copied().collect();
        sorted.sort_unstable();
        let p90 = sorted
            .get((sorted.len() * 9 / 10).min(sorted.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0);
        (p90 as f32 * 1.25).max(PING_HIGH * 2.0)
    };
    let (sum, count) = history
        .iter()
        .flatten()
        .fold((0u64, 0u64), |(s, n), &p| (s + p as u64, n + 1));
    let avg = if count > 0 {
        sum as f32 / count as f32
    } else {
        0.0
    };
    let bar_w = step.max(1.0);
    // 3D look: every bar casts a soft shadow to the upper right (depth) and has a vertical
    // gradient (light top, dark foot) with a bright cap; wide bars also get lit / shaded
    // edges. Everything goes into two meshes, so long histories stay cheap.
    let depth = (rect.height() * 0.06).clamp(1.5, 3.0);
    let mut shadow = egui::Mesh::default();
    let mut bars = egui::Mesh::default();
    let shade = |c: Color32, t: f32| c.lerp_to_gamma(Color32::BLACK, t);
    let light = |c: Color32, t: f32| c.lerp_to_gamma(Color32::WHITE, t);
    let gradient = |mesh: &mut egui::Mesh, r: egui::Rect, top: Color32, bottom: Color32| {
        let start = mesh.vertices.len();
        mesh.add_colored_rect(r, top);
        for v in &mut mesh.vertices[start + 2..] {
            v.color = bottom;
        }
    };
    let y_of = |ms: f32| rect.bottom() - 1.0 - (ms / max).min(1.0) * (rect.height() - 2.0);
    for (i, sample) in history.iter().enumerate() {
        let x = rect.left() + step * i as f32;
        let (top, ms) = match sample {
            // at least 2 px, so a 0 ms answer is still visible
            Some(ping) => (y_of(*ping as f32).min(rect.bottom() - 3.0), *ping as f32),
            None => (rect.top(), f32::INFINITY),
        };
        // Timeouts are wider so a single one is visible, like in HLSW.
        let w = if sample.is_none() {
            bar_w.max(4.0)
        } else {
            bar_w
        };
        let bar = egui::Rect::from_min_max(egui::pos2(x, top), egui::pos2(x + w, rect.bottom()));
        shadow.add_colored_rect(
            bar.translate(egui::vec2(depth, -depth)).intersect(rect),
            Color32::from_black_alpha(45),
        );
        if sample.is_none() {
            let red = Color32::from_rgb(230, 30, 25);
            gradient(&mut bars, bar, light(red, 0.3), shade(red, 0.3));
        } else {
            // Colour by height: green at the foot, yellow at 100 ms, red towards 200 ms, so
            // every bar shows on its own how far up it reaches.
            let top_ms = ms.min(max);
            let mut stops = vec![0.0];
            for s in [PING_HIGH, PING_HIGH * 2.0] {
                if s < top_ms {
                    stops.push(s);
                }
            }
            stops.push(top_ms);
            for pair in stops.windows(2) {
                let (lo, hi) = (pair[0], pair[1]);
                let r = egui::Rect::from_min_max(
                    egui::pos2(x, y_of(hi).max(top)),
                    egui::pos2(x + w, if lo == 0.0 { rect.bottom() } else { y_of(lo) }),
                );
                // A slightly darker foot gives the bars depth.
                let foot = if lo == 0.0 {
                    shade(ping_color(lo), 0.25)
                } else {
                    ping_color(lo)
                };
                gradient(&mut bars, r, ping_color(hi), foot);
            }
        }
        let color = ping_color(ms.min(max));
        if bar.height() >= 3.0 {
            // bright cap on top
            bars.add_colored_rect(
                egui::Rect::from_min_size(bar.min, egui::vec2(w, 1.0)),
                light(color, 0.6),
            );
        }
        if w >= 3.0 {
            // lit left edge, shaded right edge
            bars.add_colored_rect(
                egui::Rect::from_min_size(bar.min, egui::vec2(1.0, bar.height())),
                Color32::from_white_alpha(70),
            );
            bars.add_colored_rect(
                egui::Rect::from_min_max(egui::pos2(bar.right() - 1.0, bar.top()), bar.max),
                Color32::from_black_alpha(70),
            );
        }
    }
    painter.add(shadow);
    painter.add(bars);

    // Glossy reflection over the upper half.
    let gloss = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.center().y));
    let mut mesh = egui::Mesh::default();
    gradient(
        &mut mesh,
        gloss,
        Color32::from_white_alpha(70),
        Color32::from_white_alpha(0),
    );
    painter.add(mesh);

    // Sunken frame: dark top / left, light bottom / right.
    let dark = Stroke::new(1.0, shade(p.list_border, 0.35));
    let lit = Stroke::new(1.0, Color32::from_white_alpha(200));
    let r = rect.shrink(0.5);
    painter.line_segment([r.left_bottom(), r.left_top()], dark);
    painter.line_segment([r.left_top(), r.right_top()], dark);
    painter.line_segment([r.right_top(), r.right_bottom()], lit);
    painter.line_segment([r.right_bottom(), r.left_bottom()], lit);
    let inner = rect.shrink(1.5);
    painter.line_segment(
        [inner.left_bottom(), inner.left_top()],
        Stroke::new(1.0, Color32::from_black_alpha(40)),
    );
    painter.line_segment(
        [inner.left_top(), inner.right_top()],
        Stroke::new(1.0, Color32::from_black_alpha(40)),
    );

    graph_labels(
        ui, &painter, &response, rect, history, step, max, avg, capacity,
    );
}

/// Numbers in the ping graph: scale lines, the average, labelled peaks, the current
/// value, and the value under the mouse. Small graphs (trace route rows) only get the
/// peaks and the hover value.
#[allow(clippy::too_many_arguments)]
fn graph_labels(
    ui: &Ui,
    painter: &egui::Painter,
    response: &egui::Response,
    rect: egui::Rect,
    history: &VecDeque<Option<u32>>,
    step: f32,
    max: f32,
    avg: f32,
    capacity: usize,
) {
    if history.is_empty() {
        return;
    }
    let y_of = |v: f32| rect.bottom() - 1.0 - (v / max).min(1.0) * (rect.height() - 2.0);
    let x_of = |i: usize| rect.left() + step * i as f32 + step.max(1.0) / 2.0;
    let small = egui::FontId::proportional(9.0);
    let ink = Color32::from_gray(40);
    // Where a label goes: anchored at `pos`, moved inside the graph.
    let fit = |pos: egui::Pos2, align: egui::Align2, text: &str| {
        let size = painter
            .layout_no_wrap(text.to_string(), small.clone(), ink)
            .size();
        let r = align.anchor_size(pos, size).expand2(egui::vec2(2.0, 0.0));
        let dx =
            (rect.left() + 1.0 - r.left()).max(0.0) - (r.right() - rect.right() + 1.0).max(0.0);
        let dy =
            (rect.top() + 1.0 - r.top()).max(0.0) - (r.bottom() - rect.bottom() + 1.0).max(0.0);
        r.translate(egui::vec2(dx, dy))
    };
    // Text with a light halo in the rect from `fit`.
    let draw = |r: egui::Rect, text: String, color: Color32| {
        let galley = painter.layout_no_wrap(text, small.clone(), color);
        painter.rect_filled(r, 2.0, Color32::from_white_alpha(170));
        painter.galley(r.min + egui::vec2(2.0, 0.0), galley, color);
        r
    };
    let label = |pos: egui::Pos2, align: egui::Align2, text: String, color: Color32| {
        draw(fit(pos, align, &text), text, color)
    };
    let tall = rect.height() >= 40.0;
    let mut placed: Vec<egui::Rect> = Vec::new();

    // Current value at the newest bar; placed first so the other labels keep clear of it.
    let current = if tall && let Some(last) = history.back() {
        let i = history.len() - 1;
        let (text, y, color) = match last {
            Some(v) => (format!("{v} ms"), y_of(*v as f32), ink),
            None => (
                tr!("Timeout").to_string(),
                rect.top() + 10.0,
                Color32::from_rgb(200, 0, 0),
            ),
        };
        let r = fit(
            egui::pos2(x_of(i) + 4.0, y),
            egui::Align2::LEFT_BOTTOM,
            &text,
        );
        placed.push(r.expand(1.0));
        Some((r, text, color))
    } else {
        None
    };

    if tall {
        // Scale: up to three "nice" grid lines with their value on the left.
        let target = max / 3.0;
        let magnitude = 10f32.powf(target.log10().floor());
        let nice = [1.0, 2.0, 2.5, 5.0, 10.0]
            .iter()
            .map(|m| m * magnitude)
            .find(|s| *s >= target)
            .unwrap_or(target);
        let mut v = nice;
        let mut scale = Vec::new();
        while v < max {
            let y = y_of(v);
            if y > rect.top() + 6.0 {
                painter.hline(
                    rect.x_range(),
                    y,
                    Stroke::new(1.0, Color32::from_black_alpha(25)),
                );
                let text = format!("{v}");
                let r = fit(
                    egui::pos2(rect.left() + 3.0, y),
                    egui::Align2::LEFT_CENTER,
                    &text,
                );
                if !placed.iter().any(|p| p.intersects(r)) {
                    scale.push(draw(r, text, Color32::from_gray(110)));
                }
            }
            v += nice;
        }
        placed.extend(scale);
        // Average: dashed line; its value goes right above the line, or – when the current
        // value sits there – below it, further left, or on the left side.
        if avg > 0.0 {
            let y = y_of(avg);
            painter.add(egui::Shape::dashed_line(
                &[egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                Stroke::new(1.0, Color32::from_rgb(30, 90, 200)),
                5.0,
                3.0,
            ));
            let text = format!("Ø {avg:.0} ms");
            let right = rect.right() - 3.0;
            let left = rect.left() + 40.0;
            let candidates = [
                (egui::pos2(right, y - 1.0), egui::Align2::RIGHT_BOTTOM),
                (egui::pos2(right, y + 1.0), egui::Align2::RIGHT_TOP),
                (
                    egui::pos2(right - 70.0, y - 1.0),
                    egui::Align2::RIGHT_BOTTOM,
                ),
                (egui::pos2(right - 70.0, y + 1.0), egui::Align2::RIGHT_TOP),
                (egui::pos2(left, y - 1.0), egui::Align2::LEFT_BOTTOM),
                (egui::pos2(left, y + 1.0), egui::Align2::LEFT_TOP),
            ];
            let r = candidates
                .iter()
                .map(|&(pos, align)| fit(pos, align, &text))
                .find(|r| !placed.iter().any(|p| p.intersects(*r)));
            if let Some(r) = r {
                placed.push(draw(r, text, Color32::from_rgb(30, 70, 170)));
            }
        }
    }

    // Peaks: local maxima clearly above the average, biggest first, never overlapping.
    let window = (capacity / 25).max(4);
    let mut peaks: Vec<(usize, u32)> = history
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|v| (i, v)))
        .filter(|&(i, v)| {
            (v as f32) > avg * 1.4 + 8.0
                && history
                    .range(i.saturating_sub(window)..(i + window + 1).min(history.len()))
                    .flatten()
                    .all(|&o| o <= v)
        })
        .collect();
    peaks.sort_by_key(|p| std::cmp::Reverse(p.1));
    for (i, v) in peaks.into_iter().take(if tall { 8 } else { 3 }) {
        let pos = egui::pos2(x_of(i), y_of(v as f32) - 1.0);
        let text = v.to_string();
        let r = fit(pos, egui::Align2::CENTER_BOTTOM, &text);
        if placed.iter().any(|p| p.intersects(r.expand(1.0))) {
            continue;
        }
        placed.push(label(pos, egui::Align2::CENTER_BOTTOM, text, ink));
    }

    if let Some((r, text, color)) = current {
        draw(r, text, color);
    }

    // Mouse over the graph: cursor line and the value of that query.
    if let Some(pos) = response.hover_pos() {
        let i = ((pos.x - rect.left()) / step).floor().max(0.0) as usize;
        if let Some(sample) = history.get(i) {
            let x = x_of(i);
            painter.vline(
                x,
                rect.y_range(),
                Stroke::new(1.0, Color32::from_black_alpha(120)),
            );
            let text = match sample {
                Some(v) => format!("{v} ms"),
                None => tr!("Timeout").to_string(),
            };
            label(
                egui::pos2(x + 4.0, rect.top() + 2.0),
                egui::Align2::LEFT_TOP,
                text,
                Color32::BLACK,
            );
            ui.ctx().request_repaint();
        }
    }
}

/// Search field above a list. Returns the lower-cased search text.
fn filter_bar(
    ui: &mut Ui,
    p: &Palette,
    id: &str,
    text: &mut String,
    hint: &str,
    total: usize,
) -> String {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(RichText::new(tr!("Filter:")).color(p.weak_text));
        ui.add(
            TextEdit::singleline(text)
                .align(egui::Align2::LEFT_CENTER)
                .id_salt(id)
                .hint_text(hint)
                .desired_width(180.0),
        )
        .with_edit_menu();
        if !text.is_empty()
            && ui
                .small_button("x")
                .on_hover_text(tr!("Clear filter"))
                .clicked()
        {
            text.clear();
        }
        ui.label(RichText::new(tr!("{total} total", total = total)).color(p.weak_text));
    });
    ui.add_space(1.0);
    text.trim().to_lowercase()
}

struct PlayerRow {
    player: Player,
    status: Option<StatusPlayer>,
    country: String,
    country_name: String,
    provider: String,
    log: Option<LogPlayer>,
    /// Entry of the Global Player ID Database.
    db: Option<IdEntry>,
}

#[derive(Default)]
struct PlayerTableData<'a> {
    server: Option<ServerId>,
    /// Visible columns (titles), see [`PLAYER_COLUMNS`].
    shown: &'a [String],
    generation: u32,
    rows: Vec<PlayerRow>,
    flags: Option<&'a Flags>,
    has_status: bool,
    rcon_ok: bool,
}

/// All columns of HLSW's players list: (title, available, initial width, minimum width).
/// Columns openHLSW has no data for are shown greyed out in the header menu, like HLSW does
/// for games that do not provide them. Kills, deaths, teams, damage and health come from the
/// server log (Get Log / Auto Update Status), ID, Global ID, ping, loss, rate, slot and
/// address from RCON `status`, country and provider from the address.
const PLAYER_COLUMNS: [(&str, bool, f32, f32); 31] = [
    ("Country", true, 20.0, 20.0),
    ("#", true, 24.0, 20.0),
    ("Name", true, 200.0, 80.0),
    ("Score", true, 40.0, 30.0),
    ("Frags", true, 40.0, 30.0),
    ("Kills", true, 36.0, 30.0),
    ("Deaths", true, 44.0, 30.0),
    ("Skill", true, 40.0, 30.0),
    ("Time", true, 62.0, 50.0),
    ("ID", true, 30.0, 20.0),
    ("Global ID", true, 150.0, 60.0),
    ("Global ID Database Names", true, 120.0, 60.0),
    ("Global ID Database Comment", true, 120.0, 60.0),
    ("Ping", true, 36.0, 30.0),
    ("Loss", true, 34.0, 30.0),
    ("Honor (AAO)", false, 50.0, 30.0),
    ("Rate", true, 56.0, 30.0),
    ("QPort", false, 50.0, 30.0),
    ("Slot", true, 34.0, 24.0),
    ("Addr", true, 140.0, 60.0),
    ("Country Name", true, 100.0, 50.0),
    ("Provider", true, 150.0, 60.0),
    ("Team", true, 70.0, 40.0),
    ("Bot", true, 34.0, 24.0),
    ("Model", false, 70.0, 40.0),
    ("Extra", false, 70.0, 40.0),
    ("Statistics (Frags,Death,Efficiency)", true, 110.0, 60.0),
    ("Statistics - Teamkills", true, 60.0, 40.0),
    ("Statistics - Suicides", true, 60.0, 40.0),
    ("Statistics - Damage (HP,Armor)", true, 90.0, 50.0),
    ("Health (HP,Armor)", true, 70.0, 40.0),
];

/// Sort key of a players list cell.
#[derive(PartialEq, PartialOrd)]
enum CellKey {
    Num(f64),
    Text(String),
}

/// Text and sort key of one players list cell.
fn player_cell(r: &PlayerRow, title: &str) -> (String, CellKey) {
    let st = r.status.as_ref();
    let log = r.log.as_ref();
    let num = |v: Option<f64>| v.map_or(CellKey::Num(-1.0), CellKey::Num);
    let text = |t: String| {
        let k = CellKey::Text(t.to_lowercase());
        (t, k)
    };
    let parsed = |t: String| {
        let k = num(t.trim().trim_end_matches('%').parse().ok());
        (t, k)
    };
    match title {
        "Country" => text(r.country.to_uppercase()),
        "#" => (String::new(), CellKey::Num(r.player.index as f64)),
        "Name" => text(r.player.name.clone()),
        "Score" | "Frags" => (
            r.player.score.to_string(),
            CellKey::Num(r.player.score as f64),
        ),
        "Kills" => match log {
            Some(l) => (l.kills.to_string(), CellKey::Num(l.kills as f64)),
            None => (String::new(), num(None)),
        },
        "Deaths" => match (log, r.player.deaths) {
            (Some(l), _) => (l.deaths.to_string(), CellKey::Num(l.deaths as f64)),
            (None, Some(d)) => (d.to_string(), CellKey::Num(d as f64)),
            (None, None) => (String::new(), num(None)),
        },
        "Skill" => match r.player.skill {
            Some(v) => (v.to_string(), CellKey::Num(v as f64)),
            None => (String::new(), num(None)),
        },
        "Time" => (
            format_duration(r.player.duration),
            CellKey::Num(r.player.duration as f64),
        ),
        "ID" => parsed(st.map(|s| s.userid.to_string()).unwrap_or_default()),
        "Global ID" => text(st.map(|s| s.steam_id.clone()).unwrap_or_default()),
        "Global ID Database Names" => {
            text(r.db.as_ref().map(IdEntry::names_text).unwrap_or_default())
        }
        "Global ID Database Comment" => {
            text(r.db.as_ref().map(|e| e.comment.clone()).unwrap_or_default())
        }
        "Ping" => parsed(
            st.map(|s| s.ping.clone())
                .or_else(|| r.player.ping.map(|p| p.to_string()))
                .unwrap_or_default(),
        ),
        "Loss" => parsed(st.map(|s| s.loss.clone()).unwrap_or_default()),
        "Rate" => parsed(st.map(|s| s.rate.clone()).unwrap_or_default()),
        "Slot" => parsed(
            st.and_then(|s| s.slot)
                .map(|n| n.to_string())
                .unwrap_or_default(),
        ),
        "Addr" => {
            let addr = st
                .and_then(|s| s.ip)
                .map(|ip| match st.and_then(|s| s.port) {
                    Some(port) => SocketAddr::new(ip, port).to_string(),
                    None => ip.to_string(),
                });
            text(addr.unwrap_or_default())
        }
        "Country Name" => text(r.country_name.clone()),
        "Provider" => text(r.provider.clone()),
        "Team" => text(
            log.map(|l| l.team.clone())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| r.player.team.clone()),
        ),
        "Bot" => {
            let bot = r.player.bot
                || st.is_some_and(|s| {
                    s.steam_id.eq_ignore_ascii_case("BOT") || s.state.eq_ignore_ascii_case("BOT")
                });
            text(if bot { "BOT".into() } else { String::new() })
        }
        "Statistics (Frags,Death,Efficiency)" => match log {
            Some(l) => {
                let eff = if l.kills + l.deaths > 0 {
                    l.kills as f64 * 100.0 / (l.kills + l.deaths) as f64
                } else {
                    0.0
                };
                (
                    format!("{} / {} / {eff:.0}%", l.kills, l.deaths),
                    CellKey::Num(eff),
                )
            }
            None => (String::new(), num(None)),
        },
        "Statistics - Teamkills" => match log {
            Some(l) => (l.teamkills.to_string(), CellKey::Num(l.teamkills as f64)),
            None => (String::new(), num(None)),
        },
        "Statistics - Suicides" => match log {
            Some(l) => (l.suicides.to_string(), CellKey::Num(l.suicides as f64)),
            None => (String::new(), num(None)),
        },
        "Statistics - Damage (HP,Armor)" => match log {
            Some(l) => (
                format!("{} / {}", l.damage_hp, l.damage_armor),
                CellKey::Num(l.damage_hp as f64),
            ),
            None => (String::new(), num(None)),
        },
        "Health (HP,Armor)" => match log.and_then(|l| l.health) {
            Some((hp, armor)) => (format!("{hp} / {armor}"), CellKey::Num(hp as f64)),
            None => (String::new(), num(None)),
        },
        _ => (String::new(), num(None)),
    }
}

fn players_table(
    ui: &mut Ui,
    p: &Palette,
    data: &PlayerTableData<'_>,
    needle: &str,
    sort: &mut (PlayerCol, bool),
    sel: &mut Selection,
    actions: &mut Vec<Action>,
) {
    let mut list: Vec<&PlayerRow> = data
        .rows
        .iter()
        .filter(|r| {
            needle.is_empty()
                || r.player.name.to_lowercase().contains(needle)
                || r.status.as_ref().is_some_and(|s| {
                    s.steam_id.to_lowercase().contains(needle)
                        || s.ip.is_some_and(|ip| ip.to_string().contains(needle))
                })
        })
        .collect();
    list.sort_by(|a, b| {
        let ord = player_cell(a, sort.0)
            .1
            .partial_cmp(&player_cell(b, sort.0).1)
            .unwrap_or(Ordering::Equal);
        if sort.1 { ord } else { ord.reverse() }
    });
    let order: Vec<&str> = list.iter().map(|r| r.player.name.as_str()).collect();
    sel.retain(&order);
    let marked: Vec<&PlayerRow> = list
        .iter()
        .copied()
        .filter(|r| sel.contains(&r.player.name))
        .collect();
    // Ctrl+C / Ctrl+A while the mouse is over the list.
    if ui.rect_contains_pointer(ui.max_rect()) && !ui.ctx().text_edit_focused() {
        let (copy, all) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::COMMAND, Key::C),
                i.consume_key(egui::Modifiers::COMMAND, Key::A),
            )
        });
        if copy && !marked.is_empty() {
            actions.push(Action::CopyText(players_as_text(&marked)));
        }
        if all {
            sel.keys = order.iter().map(|k| k.to_string()).collect();
        }
    }
    let mut clicked: Option<(String, egui::Modifiers)> = None;
    let scroll_to =
        arrow_keys(ui.ctx(), "players").and_then(|(down, shift)| sel.step(&order, down, shift));

    let autosize = take_autosize(ui, "players");
    let mut autosize_request = None;
    let visible = |title: &str| title == "Name" || data.shown.iter().any(|h| h == title);
    let columns: Vec<_> = PLAYER_COLUMNS
        .iter()
        .filter(|c| c.1 && visible(c.0))
        .collect();
    // Unavailable columns are listed greyed out, like in HLSW.
    let menu_items: Vec<(&str, bool, bool)> = PLAYER_COLUMNS
        .iter()
        .map(|c| (c.0, c.1 && visible(c.0), c.1 && c.0 != "Name"))
        .collect();
    // Double click on a header / separator: fit the column to all players.
    let fit_width = autosize.and_then(|i| columns.get(i)).map(|&&(title, ..)| {
        fit_column(
            ui,
            title,
            list.iter().enumerate().map(|(n, r)| match title {
                "#" => (format!("{:02}", n + 1), 0.0),
                _ => (player_cell(r, title).0, 0.0),
            }),
        )
    });
    let table_id = format!("players{}", data.generation);
    let mut table = styled_table(ui, &table_id).sense(Sense::click());
    if let Some(i) = scroll_to {
        table = table.scroll_to_row(i, None);
    }
    for (i, &&(title, _, w, min)) in columns.iter().enumerate() {
        table = table.column(if title == "Country" {
            Column::exact(w)
        } else {
            sized(
                Column::initial(w).at_least(min).clip(true),
                fit_width.filter(|_| autosize == Some(i)),
            )
        });
    }
    table
        .column(Column::remainder())
        .header(HEADER_HEIGHT, |mut h| {
            for (i, &&(title, ..)) in columns.iter().enumerate() {
                h.col(|ui| {
                    let label = if title == "Country" { "" } else { title };
                    let (auto, resp) = sort_header(ui, p, label, title, sort);
                    if auto {
                        autosize_request = Some(i);
                    }
                    resp.context_menu(|ui| column_menu(ui, true, &menu_items, actions));
                });
            }
            h.col(|ui| {
                let resp = header_cell(ui, p, |ui| {
                    if !data.has_status && data.server.is_some() {
                        let hint = if data.rcon_ok {
                            tr!("loading status…")
                        } else {
                            tr!("Global ID / Addr / Ping need RCON (Rcon Config → Test)")
                        };
                        ui.add(
                            Label::new(RichText::new(hint).color(p.weak_text)).wrap_mode(fit(ui)),
                        );
                    }
                });
                resp.context_menu(|ui| column_menu(ui, true, &menu_items, actions));
            });
        })
        .body(|body| {
            body.rows(ROW_HEIGHT, list.len(), |mut row| {
                let r = list[row.index()];
                let n = row.index() + 1;
                row.set_selected(sel.contains(&r.player.name));
                for &&(title, ..) in &columns {
                    row.col(|ui| match title {
                        "Country" => {
                            if let Some(f) = data.flags.filter(|f| f.has(&r.country)) {
                                let tip = if r.country_name.is_empty() {
                                    r.country.to_uppercase()
                                } else {
                                    r.country_name.clone()
                                };
                                f.show(ui, &r.country, 11.0).on_hover_text(tip);
                            }
                        }
                        "#" => {
                            ui.label(format!("{n:02}"));
                        }
                        "Name" => {
                            // Global ID Database type colours the name (Friend, Lamer, …).
                            let mut text = RichText::new(&r.player.name);
                            if let Some([red, g, b]) = r.db.as_ref().and_then(|e| e.kind.color()) {
                                text = text.color(Color32::from_rgb(red, g, b));
                            }
                            let resp =
                                ui.add(Label::new(text).wrap_mode(fit(ui)).selectable(false));
                            if let Some(c) = r.db.as_ref().map(|e| &e.comment)
                                && !c.is_empty()
                            {
                                resp.on_hover_text(c);
                            }
                        }
                        _ => {
                            let text = player_cell(r, title).0;
                            ui.add(Label::new(text).wrap_mode(fit(ui)).selectable(false));
                        }
                    });
                }
                row.col(|_| {});

                let resp = row.response();
                if resp.clicked() {
                    clicked = Some((r.player.name.clone(), ui_modifiers(&resp)));
                }
                if resp.secondary_clicked() {
                    sel.right_click(&r.player.name);
                    set_active_list(&resp.ctx, "players");
                }
                let Some(server) = data.server else { return };
                // The menu acts on all marked players (or the one under the mouse).
                let targets: Vec<&PlayerRow> = if sel.contains(&r.player.name) && marked.len() > 1 {
                    marked.clone()
                } else {
                    vec![r]
                };
                resp.context_menu(|ui| {
                    player_context_menu(ui, data, server, &targets, &list, actions)
                });
            });
        });
    if let Some((key, mods)) = clicked {
        sel.click(&key, &order, mods);
        set_active_list(ui.ctx(), "players");
    }
    if let Some(i) = autosize_request {
        request_autosize(ui, "players", i);
    }
}

/// Tab separated copy of the visible player table (HLSW "Copy → Whole Table").
fn players_as_text(list: &[&PlayerRow]) -> String {
    let mut out = String::from("#\tName\tFrags\tTime\tPing\tLoss\tSteam ID\tIP\n");
    for (i, r) in list.iter().enumerate() {
        let st = r.status.as_ref();
        let f = |g: fn(&StatusPlayer) -> String| st.map(g).unwrap_or_default();
        out.push_str(&format!(
            "{:02}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            i + 1,
            r.player.name,
            r.player.score,
            format_duration(r.player.duration),
            f(|s| s.ping.clone()),
            f(|s| s.loss.clone()),
            f(|s| s.steam_id.clone()),
            f(|s| s.ip.map(|ip| ip.to_string()).unwrap_or_default()),
        ));
    }
    out
}

const BAN_DURATIONS: [(&str, u32); 6] = [
    ("Permanent", 0),
    ("5 minutes", 5),
    ("30 minutes", 30),
    ("1 hour", 60),
    ("1 day", 1440),
    ("1 week", 10080),
];

/// Right-click menu of the player list, laid out like HLSW's. Acts on all marked players
/// (`targets`); copies put one line per player.
fn player_context_menu(
    ui: &mut Ui,
    data: &PlayerTableData<'_>,
    server: ServerId,
    targets: &[&PlayerRow],
    list: &[&PlayerRow],
    actions: &mut Vec<Action>,
) {
    let Some(&r) = targets.first() else { return };
    let single = targets.len() == 1;
    let steam_of = |r: &PlayerRow| {
        r.status
            .as_ref()
            .map(|s| s.steam_id.clone())
            .unwrap_or_default()
    };
    let addr_of = |r: &PlayerRow| {
        let st = r.status.as_ref();
        st.and_then(|s| s.ip)
            .map(|ip| match st.and_then(|s| s.port) {
                Some(port) => SocketAddr::new(ip, port).to_string(),
                None => ip.to_string(),
            })
    };
    // One line per marked player; players without that value are skipped.
    let lines = |f: &dyn Fn(&PlayerRow) -> Option<String>| {
        let v: Vec<String> = targets
            .iter()
            .filter_map(|r| f(r))
            .filter(|t| !t.is_empty())
            .collect();
        (!v.is_empty()).then(|| v.join("\n"))
    };
    let mut copy = |ui: &mut Ui, label: &str, text: Option<String>| {
        if ui
            .add_enabled(
                text.as_deref().is_some_and(|t| !t.is_empty()),
                Button::new(label),
            )
            .clicked()
        {
            actions.push(Action::CopyText(text.unwrap_or_default()));
            ui.close();
        }
    };
    let sid_of = |r: &PlayerRow| SteamId::parse(&steam_of(r));
    ui.menu_button(tr!("Copy"), |ui| {
        copy(ui, tr!("Name"), lines(&|r| Some(r.player.name.clone())));
        // Bots and LAN players have no real Steam ID: copy what the server reported.
        copy(
            ui,
            tr!("Steam ID"),
            lines(&|r| Some(sid_of(r).map(|s| s.steam2()).unwrap_or_else(|| steam_of(r)))),
        );
        copy(
            ui,
            tr!("Steam ID3"),
            lines(&|r| sid_of(r).map(|s| s.steam3())),
        );
        copy(
            ui,
            tr!("Steam ID64"),
            lines(&|r| sid_of(r).map(|s| s.steam64().to_string())),
        );
        copy(
            ui,
            tr!("Steam Profile URL"),
            lines(&|r| sid_of(r).map(|s| s.profile_url())),
        );
        copy(ui, tr!("Addr"), lines(&addr_of));
        ui.separator();
        if !single {
            copy(ui, tr!("Marked Rows"), Some(players_as_text(targets)));
        }
        copy(ui, tr!("Whole Table"), Some(players_as_text(list)));
    });
    let profiles: Vec<String> = targets
        .iter()
        .filter_map(|r| sid_of(r).map(|s| s.profile_url()))
        .collect();
    let profile = ui
        .add_enabled(!profiles.is_empty(), Button::new(tr!("Open Steam Profile")))
        .on_disabled_hover_text(if r.status.is_none() {
            tr!("Needs the Steam ID from RCON status (Rcon Config → Test)")
        } else {
            tr!("No Steam account (bot or LAN player)")
        });
    if profile.clicked() {
        actions.extend(profiles.into_iter().map(Action::OpenUrl));
        ui.close();
    }
    let with_id: Vec<(String, String)> = targets
        .iter()
        .filter(|r| sid_of(r).is_some())
        .map(|r| (steam_of(r), r.player.name.clone()))
        .collect();
    ui.add_enabled_ui(!with_id.is_empty(), |ui| {
        ui.menu_button(tr!("Global ID Database"), |ui| {
            if ui
                .add_enabled(
                    with_id.len() == 1,
                    Button::new(tr!("Open in Global ID Database Editor...")),
                )
                .clicked()
            {
                actions.push(Action::IdDbSetKind {
                    id: with_id[0].0.clone(),
                    name: with_id[0].1.clone(),
                    kind: None,
                });
                ui.close();
            }
            ui.separator();
            for k in IdKind::ALL {
                let current = single && r.db.as_ref().is_some_and(|e| e.kind == k);
                if ui
                    .radio(
                        current,
                        tr!("Set to {type}", type = crate::i18n::t(k.name())),
                    )
                    .clicked()
                {
                    for (id, name) in &with_id {
                        actions.push(Action::IdDbSetKind {
                            id: id.clone(),
                            name: name.clone(),
                            kind: Some(k),
                        });
                    }
                    ui.close();
                }
            }
        });
    });
    ui.separator();
    if ui
        .add_enabled(data.rcon_ok, Button::new(tr!("Rcon Status Command")))
        .clicked()
    {
        actions.push(Action::RconStatus(server));
        ui.close();
    }
    ui.separator();
    // Everything below needs the userid from RCON `status`.
    let with_status: Vec<&StatusPlayer> =
        targets.iter().filter_map(|r| r.status.as_ref()).collect();
    if with_status.is_empty() {
        for label in [
            tr!("Kick"),
            tr!("Kick & Connect"),
            tr!("Ban ID"),
            tr!("Ban ID Kick"),
            tr!("Ban IP"),
            tr!("Ban IP Kick"),
        ] {
            ui.add_enabled(false, Button::new(label))
                .on_disabled_hover_text(tr!("Needs RCON (Rcon Config → Test)"));
        }
        return;
    }
    if ui.button(tr!("Kick")).clicked() {
        for st in &with_status {
            actions.push(Action::KickPlayer(server, st.userid));
        }
        ui.close();
    }
    // Takes over one slot: only for a single player.
    if ui
        .add_enabled(with_status.len() == 1, Button::new(tr!("Kick & Connect")))
        .clicked()
    {
        actions.push(Action::KickAndConnect(server, with_status[0].userid));
        ui.close();
    }
    ui.separator();
    for (label, by_ip, kick) in [
        (tr!("Ban ID"), false, false),
        (tr!("Ban ID Kick"), false, true),
        (tr!("Ban IP"), true, false),
        (tr!("Ban IP Kick"), true, true),
    ] {
        let bans: Vec<(u32, String)> = with_status
            .iter()
            .filter_map(|st| {
                let target = if by_ip {
                    st.ip.map(|ip| ip.to_string())
                } else {
                    Some(st.steam_id.clone())
                };
                target.map(|t| (st.userid, t))
            })
            .collect();
        if bans.is_empty() {
            ui.add_enabled(false, Button::new(label));
            continue;
        }
        ui.menu_button(label, |ui| {
            for (dur, minutes) in BAN_DURATIONS {
                if ui.button(crate::i18n::t(dur)).clicked() {
                    for (userid, target) in &bans {
                        actions.push(Action::BanPlayer {
                            id: server,
                            userid: *userid,
                            target: target.clone(),
                            ip: by_ip,
                            kick,
                            minutes,
                        });
                    }
                    ui.close();
                }
            }
        });
    }
}

/// Players from RCON `status`. Handles the Source formats
/// `#  2 "Name" STEAM_1:0:1 01:00 30 0 active 1.2.3.4:27005`,
/// `# 2 1 "Name" [U:1:2] 01:00 30 0 active 196608 1.2.3.4:27005` (CS:GO) and
/// CS2's `  2   01:00  30  0  active 786432 1.2.3.4:27005 'Name'`.
fn parse_status_players(text: &str) -> Vec<StatusPlayer> {
    let parse_ip = |t: &str| -> Option<IpAddr> {
        t.parse::<SocketAddr>()
            .ok()
            .map(|a| a.ip())
            .or_else(|| t.parse().ok())
    };
    let parse_port = |t: &str| t.parse::<SocketAddr>().ok().map(|a| a.port());
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('#') {
            // Source / CS:GO
            let (Some(first), Some(last)) = (rest.find('"'), rest.rfind('"')) else {
                continue;
            };
            if last <= first {
                continue;
            }
            let Some(userid) = rest[..first]
                .split_whitespace()
                .next()
                .and_then(|t| t.parse().ok())
            else {
                continue;
            };
            let name = rest[first + 1..last].to_string();
            let t: Vec<&str> = rest[last + 1..].split_whitespace().collect();
            // GoldSrc: # slot "name" userid uniqueid frag time ping loss adr
            if let Some(uid) = t.first().and_then(|x| x.parse::<u32>().ok())
                && t.len() >= 2
            {
                let get = |i: usize| t.get(i).copied().unwrap_or_default().to_string();
                out.push(StatusPlayer {
                    userid: uid,
                    name,
                    steam_id: get(1),
                    connected: get(3),
                    ping: get(4),
                    loss: get(5),
                    state: String::new(),
                    ip: t.get(6).and_then(|a| parse_ip(a)),
                    port: t.get(6).and_then(|a| parse_port(a)),
                    slot: rest[..first]
                        .split_whitespace()
                        .next()
                        .and_then(|x| x.parse().ok()),
                    rate: String::new(),
                });
                continue;
            }
            let steam_id = t.first().copied().unwrap_or_default().to_string();
            let mut p = StatusPlayer {
                userid,
                name,
                steam_id,
                connected: String::new(),
                ping: String::new(),
                loss: String::new(),
                state: String::new(),
                ip: None,
                port: None,
                slot: rest[..first]
                    .split_whitespace()
                    .nth(1)
                    .and_then(|t| t.parse().ok()),
                rate: String::new(),
            };
            if t.len() >= 5 {
                p.connected = t[1].to_string();
                p.ping = t[2].to_string();
                p.loss = t[3].to_string();
                p.state = t[4].to_string();
                p.ip = t.last().and_then(|a| parse_ip(a));
                p.port = t.last().and_then(|a| parse_port(a));
                if t.len() >= 7 {
                    p.rate = t[5].to_string(); // CS:GO: ... state rate adr
                }
            } else if t.len() >= 2 {
                p.state = t[t.len() - 1].to_string(); // bots: "BOT active"
            }
            out.push(p);
        } else if trimmed.ends_with('\'') && trimmed.contains(" '") {
            // CS2: id time ping loss state rate adr 'name'
            let start = trimmed.find(" '").unwrap_or(0);
            let name = trimmed[start + 2..trimmed.len() - 1].to_string();
            let t: Vec<&str> = trimmed[..start].split_whitespace().collect();
            let Some(userid) = t.first().and_then(|x| x.parse::<u32>().ok()) else {
                continue;
            };
            if userid == 65535 || t.len() < 5 {
                continue; // "[NoChan]" placeholder line
            }
            out.push(StatusPlayer {
                userid,
                name,
                steam_id: String::new(),
                connected: t[1].to_string(),
                ping: t[2].to_string(),
                loss: t[3].to_string(),
                state: t[4].to_string(),
                ip: t.last().and_then(|a| parse_ip(a)),
                port: t.last().and_then(|a| parse_port(a)),
                slot: Some(userid),
                rate: t.get(5).map(|r| r.to_string()).unwrap_or_default(),
            });
        }
    }
    out
}

fn rules_table(
    ui: &mut Ui,
    p: &Palette,
    rules: &[Rule],
    needle: &str,
    server: Option<(ServerId, bool)>,
    sel: &mut Selection,
    actions: &mut Vec<Action>,
) {
    let all = rules;
    let rules: Vec<&Rule> = rules
        .iter()
        .filter(|r| {
            needle.is_empty()
                || r.key.to_lowercase().contains(needle)
                || r.value.to_lowercase().contains(needle)
        })
        .collect();
    let autosize = take_autosize(ui, "rules");
    let mut autosize_request = None;
    // Double click on a header / separator: fit the column to all (filtered) rules.
    let fit_width = |col: usize| {
        (autosize == Some(col)).then(|| {
            let header = ["", tr!("Key"), tr!("Value")][col];
            fit_column(
                ui,
                header,
                rules.iter().enumerate().map(|(n, r)| match col {
                    0 => (format!("{:02}", n + 1), 0.0),
                    1 => (r.key.clone(), 0.0),
                    _ => (r.value.clone(), 0.0),
                }),
            )
        })
    };
    let (fit0, fit1, fit2) = (fit_width(0), fit_width(1), fit_width(2));
    let order: Vec<&str> = rules.iter().map(|r| r.key.as_str()).collect();
    sel.retain(&order);
    let marked: Vec<&Rule> = rules
        .iter()
        .copied()
        .filter(|r| sel.contains(&r.key))
        .collect();
    // Ctrl+C / Ctrl+A while the mouse is over the list.
    if ui.rect_contains_pointer(ui.max_rect()) && !ui.ctx().text_edit_focused() {
        let (copy, all_keys) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::COMMAND, Key::C),
                i.consume_key(egui::Modifiers::COMMAND, Key::A),
            )
        });
        if copy && !marked.is_empty() {
            actions.push(Action::CopyText(rules_as_text(&marked)));
        }
        if all_keys {
            sel.keys = order.iter().map(|k| k.to_string()).collect();
        }
    }
    let scroll_to =
        arrow_keys(ui.ctx(), "rules").and_then(|(down, shift)| sel.step(&order, down, shift));
    let mut clicked: Option<(String, egui::Modifiers)> = None;
    let mut table = styled_table(ui, "rules");
    if let Some(i) = scroll_to {
        table = table.scroll_to_row(i, None);
    }
    table
        .sense(Sense::click())
        .column(sized(Column::initial(20.0).at_least(14.0), fit0))
        .column(sized(
            Column::initial(122.0).at_least(60.0).clip(true),
            fit1,
        ))
        .column(sized(
            Column::initial(240.0).at_least(60.0).clip(true),
            fit2,
        ))
        .column(Column::remainder())
        .header(HEADER_HEIGHT, |mut h| {
            for (i, title) in ["", tr!("Key"), tr!("Value")].into_iter().enumerate() {
                h.col(|ui| {
                    if plain_header(ui, p, title) {
                        autosize_request = Some(i);
                    }
                });
            }
            h.col(|ui| {
                header_cell(ui, p, |_| {});
            });
        })
        .body(|body| {
            body.rows(ROW_HEIGHT, rules.len(), |mut row| {
                let r = rules[row.index()];
                let n = row.index() + 1;
                row.set_selected(sel.contains(&r.key));
                row.col(|ui| {
                    ui.label(format!("{n:02}"));
                });
                row.col(|ui| {
                    ui.add(Label::new(&r.key).wrap_mode(fit(ui)).selectable(false));
                });
                row.col(|ui| {
                    ui.add(Label::new(&r.value).wrap_mode(fit(ui)).selectable(false));
                });
                row.col(|_| {});
                let resp = row.response();
                if resp.clicked() {
                    clicked = Some((r.key.clone(), ui_modifiers(&resp)));
                }
                if resp.secondary_clicked() {
                    sel.right_click(&r.key);
                    set_active_list(&resp.ctx, "rules");
                }
                let targets: Vec<&Rule> = if sel.contains(&r.key) && marked.len() > 1 {
                    marked.clone()
                } else {
                    vec![r]
                };
                resp.context_menu(|ui| rule_context_menu(ui, &targets, all, server, actions));
            });
        });
    if let Some((key, mods)) = clicked {
        sel.click(&key, &order, mods);
        set_active_list(ui.ctx(), "rules");
    }
    if let Some(i) = autosize_request {
        request_autosize(ui, "rules", i);
    }
}

/// Right-click menu of the server rules (cvars), laid out like HLSW's.
fn rule_context_menu(
    ui: &mut Ui,
    targets: &[&Rule],
    all: &[Rule],
    server: Option<(ServerId, bool)>,
    actions: &mut Vec<Action>,
) {
    let Some(&r) = targets.first() else { return };
    let join =
        |f: &dyn Fn(&Rule) -> String| targets.iter().map(|r| f(r)).collect::<Vec<_>>().join("\n");
    ui.menu_button(tr!("Copy"), |ui| {
        let items = [
            (tr!("Key"), join(&|r| r.key.clone())),
            (tr!("Value"), join(&|r| r.value.clone())),
            (tr!("Both"), join(&|r| format!("{} \"{}\"", r.key, r.value))),
        ];
        for (label, text) in items {
            if ui.button(label).clicked() {
                actions.push(Action::CopyText(text));
                ui.close();
            }
        }
        ui.separator();
        if ui.button(tr!("All Rules")).clicked() {
            let text: String = all
                .iter()
                .map(|r| format!("{} \"{}\"\n", r.key, r.value))
                .collect();
            actions.push(Action::CopyText(text));
            ui.close();
        }
        if ui.button(tr!("All Keys")).clicked() {
            let text: String = all.iter().map(|r| format!("{}\n", r.key)).collect();
            actions.push(Action::CopyText(text));
            ui.close();
        }
    });
    let rcon = server.filter(|(_, ok)| *ok);
    let r_btn = ui
        .add_enabled(
            rcon.is_some() && targets.len() == 1,
            Button::new(tr!("Change Value...")),
        )
        .on_disabled_hover_text(tr!("Needs RCON (Rcon Config → Test)"));
    if r_btn.clicked()
        && let Some((id, _)) = rcon
    {
        actions.push(Action::OpenDialog(Dialog::ChangeRule(
            id,
            r.key.clone(),
            r.value.clone(),
        )));
        ui.close();
    }
}

/// `key "value"` per line, like HLSW's copy of rules.
fn rules_as_text(rules: &[&Rule]) -> String {
    rules
        .iter()
        .map(|r| format!("{} \"{}\"\n", r.key, r.value))
        .collect()
}

fn rcon_status_text(status: &RconStatus, p: &Palette) -> RichText {
    match status {
        RconStatus::Unknown => RichText::new(tr!("Unknown")),
        RconStatus::Connecting => RichText::new(tr!("Connecting…")),
        RconStatus::Ok => RichText::new(tr!("OK")).color(p.green),
        RconStatus::BadPassword => RichText::new(tr!("Bad password")).color(p.red),
        RconStatus::Error(e) => RichText::new(e).color(p.red),
    }
}

fn format_duration(seconds: f32) -> String {
    let s = seconds.max(0.0) as u64;
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

fn format_rate(bytes_per_sec: u64) -> String {
    if bytes_per_sec < 1024 {
        format!("{bytes_per_sec} Bytes/s")
    } else {
        format!("{:.2} KiB/s", bytes_per_sec as f64 / 1024.0)
    }
}

fn open_url(url: &str) -> std::io::Result<()> {
    use std::process::Command;
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).spawn()?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(url).spawn()?;
    }
    Ok(())
}

/// Shortest useful refresh interval: one query per tick must fit into the per-server
/// query budget (srcds `sv_max_queries_sec`), otherwise the extra ticks are just skipped.
fn min_interval(max_queries_per_sec: f32) -> f32 {
    (1.0 / max_queries_per_sec.max(0.1)).clamp(0.1, 10.0)
}

fn sanitize_interval(secs: f32, max_queries_per_sec: f32) -> f32 {
    let min = min_interval(max_queries_per_sec);
    if secs.is_finite() {
        secs.clamp(min, 3600.0)
    } else {
        min
    }
}

/// Refresh interval editor (seconds, min_interval … 3600). Returns true when it should be saved.
fn interval_edit(ui: &mut Ui, value: &mut f32, max_queries_per_sec: f32) -> bool {
    let min = min_interval(max_queries_per_sec);
    let r = ui
        .add(DragValue::new(value).range(min..=3600.0).speed(0.05).max_decimals(2).suffix(" s"))
        .on_hover_text(tr!("Refresh interval. srcds answers only sv_max_queries_sec queries per second per IP; \
             with the query limit of {max_queries_per_sec}/s (Settings → Misc) the minimum is {min} s.", max_queries_per_sec = format!("{:.1}", max_queries_per_sec), min = format!("{:.2}", min)));
    *value = sanitize_interval(*value, max_queries_per_sec);
    r.drag_stopped() || r.lost_focus()
}

/// `listid` / `listip` output: "1 STEAM_1:0:123 : permanent" / "2 1.2.3.4 : 30.000 min".
fn parse_ban_list(text: &str, is_ip: bool) -> Vec<BanEntry> {
    text.lines()
        .filter_map(|line| {
            let (left, duration) = line.trim().split_once(" : ")?;
            let (index, rest) = left.trim().split_once(char::is_whitespace)?;
            index.parse::<u32>().ok()?;
            // HLDS pads each octet of an IP to three characters ("193. 24.211.152"), so the
            // address may contain spaces: join everything between the index and " : ".
            let ident: String = rest.split_whitespace().collect();
            if ident.is_empty() {
                return None;
            }
            Some(BanEntry {
                is_ip,
                ident,
                duration: duration.trim().to_string(),
            })
        })
        .collect()
}

/// Maps that ship with the game. srcds' `maps *` skips maps packed in VPK files, so the
/// default maps would be missing from Change Level without this list.
fn stock_maps(app_id: u32, folder: &str) -> &'static [&'static str] {
    match (app_id, folder.to_ascii_lowercase().as_str()) {
        (240, _) => &[
            "cs_assault",
            "cs_compound",
            "cs_havana",
            "cs_italy",
            "cs_militia",
            "cs_office",
            "de_aztec",
            "de_cbble",
            "de_chateau",
            "de_dust",
            "de_dust2",
            "de_inferno",
            "de_nuke",
            "de_piranesi",
            "de_port",
            "de_prodigy",
            "de_tides",
            "de_train",
        ],
        (730, _) => &[
            "ar_baggage",
            "ar_pool_day",
            "ar_shoots",
            "cs_italy",
            "cs_office",
            "de_ancient",
            "de_anubis",
            "de_dust2",
            "de_inferno",
            "de_mirage",
            "de_nuke",
            "de_overpass",
            "de_train",
            "de_vertigo",
        ],
        (300, _) => &[
            "dod_anzio",
            "dod_argentan",
            "dod_avalanche",
            "dod_colmar",
            "dod_donner",
            "dod_flash",
            "dod_jagd",
            "dod_kalt",
            "dod_palermo",
        ],
        (320, _) => &[
            "dm_lockdown",
            "dm_overwatch",
            "dm_powerhouse",
            "dm_resistance",
            "dm_runoff",
            "dm_steamlab",
            "dm_underpass",
            "halls3",
        ],
        (440, _) => &[
            "cp_badlands",
            "cp_dustbowl",
            "cp_granary",
            "cp_gorge",
            "cp_gravelpit",
            "cp_process_final",
            "cp_well",
            "ctf_2fort",
            "ctf_doublecross",
            "ctf_turbine",
            "koth_harvest_final",
            "koth_viaduct",
            "pl_badwater",
            "pl_goldrush",
            "pl_upward",
            "plr_hightower",
        ],
        (_, "cstrike" | "czero") => &[
            "as_oilrig",
            "cs_747",
            "cs_assault",
            "cs_backalley",
            "cs_estate",
            "cs_havana",
            "cs_italy",
            "cs_militia",
            "cs_office",
            "cs_siege",
            "de_airstrip",
            "de_aztec",
            "de_cbble",
            "de_chateau",
            "de_dust",
            "de_dust2",
            "de_inferno",
            "de_nuke",
            "de_piranesi",
            "de_prodigy",
            "de_storm",
            "de_survivor",
            "de_torn",
            "de_train",
            "de_vertigo",
        ],
        _ => &[],
    }
}

/// `maps *` output: every token ending in ".bsp", without path and extension.
fn parse_map_list(text: &str) -> Vec<String> {
    let mut maps: Vec<String> = text
        .split_whitespace()
        .filter_map(|t| t.strip_suffix(".bsp"))
        .map(|t| t.rsplit(['/', '\\']).next().unwrap_or(t).to_string())
        .filter(|m| !m.is_empty() && !m.contains(['*', '?']))
        .collect();
    maps.sort_by_key(|m| m.to_lowercase());
    maps.dedup();
    maps
}

/// The little yellow star HLSW shows for servers with an RCON password.
fn star(ui: &mut Ui, size: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
    let c = rect.center();
    let (outer, inner) = (size * 0.5, size * 0.21);
    let point = |i: usize, r: f32| {
        let a = -std::f32::consts::FRAC_PI_2 + (i % 10) as f32 * std::f32::consts::PI / 5.0;
        egui::pos2(c.x + r * a.cos(), c.y + r * a.sin())
    };
    let fill = Color32::from_rgb(255, 204, 0);
    let stroke = Stroke::new(1.0, Color32::from_rgb(196, 140, 0));
    // A star is not convex: draw the inner pentagon plus five tips.
    let pentagon: Vec<egui::Pos2> = (0..5).map(|k| point(2 * k + 1, inner)).collect();
    ui.painter()
        .add(egui::Shape::convex_polygon(pentagon, fill, Stroke::NONE));
    for k in 0..5 {
        let tip = vec![
            point(2 * k + 9, inner),
            point(2 * k, outer),
            point(2 * k + 1, inner),
        ];
        ui.painter()
            .add(egui::Shape::convex_polygon(tip, fill, Stroke::NONE));
    }
    let outline: Vec<egui::Pos2> = (0..10)
        .map(|i| point(i, if i % 2 == 0 { outer } else { inner }))
        .collect();
    ui.painter().add(egui::Shape::closed_line(outline, stroke));
    response
}

/// Right-click menu for text fields: Cut / Copy / Paste / Select All
/// (Ctrl+X/C/V/A work everywhere anyway).
pub(crate) trait EditMenu {
    fn with_edit_menu(self) -> Self;
}

impl EditMenu for egui::Response {
    fn with_edit_menu(self) -> Self {
        let field = self.clone();
        self.context_menu(|ui| {
            let ctx = ui.ctx().clone();
            let mut command = None;
            if ui.button(tr!("Cut\tCtrl+X")).clicked() {
                command = Some(egui::ViewportCommand::RequestCut);
            }
            if ui.button(tr!("Copy\tCtrl+C")).clicked() {
                command = Some(egui::ViewportCommand::RequestCopy);
            }
            if ui.button(tr!("Paste\tCtrl+V")).clicked() {
                command = Some(egui::ViewportCommand::RequestPaste);
            }
            ui.separator();
            if ui.button(tr!("Select All\tCtrl+A")).clicked() {
                if let Some(mut state) = egui::TextEdit::load_state(&ctx, field.id) {
                    let all = egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(usize::MAX / 2),
                    );
                    state.cursor.set_char_range(Some(all));
                    state.store(&ctx, field.id);
                }
                field.request_focus();
                ui.close();
            }
            if let Some(command) = command {
                // The text field gets the focus back and receives the clipboard event.
                field.request_focus();
                ctx.send_viewport_cmd(command);
                ui.close();
            }
        });
        self
    }
}

/// Small square switch button (console C * F D E A, auto scroll S).
fn toggle_button(ui: &mut Ui, p: &Palette, label: &str, on: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::click());
    let (bg, border) = if on {
        (p.button_active_bg, p.button_hover_border)
    } else if response.hovered() {
        (p.button_hover_bg, p.button_hover_border)
    } else {
        (p.button_bg, p.button_border)
    };
    ui.painter().rect_filled(rect, 0.0, bg);
    ui.painter()
        .rect_stroke(rect, 0.0, Stroke::new(1.0, border), StrokeKind::Inside);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(10.0),
        p.text,
    );
    response
}

/// "C" switch: srcds log line in readable form, e.g.
/// `10/18/2026 - 20:29:31: "Bob<2><STEAM_1:0:1><CT>" say "hi"` → `Bob: hi`.
fn translate_log(line: &str) -> String {
    // drop the date/time prefix (the console shows its own time)
    let mut text = match line.split_once(": ") {
        Some((stamp, rest)) if stamp.contains(" - ") => rest.to_string(),
        _ => line.to_string(),
    };
    if let Some(chat) = parse_chat_line(line) {
        let team = if chat.team_only { "(Team) " } else { "" };
        return format!("{team}{}: {}", chat.name, chat.text);
    }
    // "Name<uid><steamid><team>" -> Name
    let mut out = String::with_capacity(text.len());
    while let Some(start) = text.find('"') {
        let Some(len) = text[start + 1..].find('"') else {
            break;
        };
        let quoted = &text[start + 1..start + 1 + len];
        out.push_str(&text[..start]);
        let mut parts = quoted.rsplitn(4, '<');
        let player = match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(_), Some(_), Some(_), Some(name)) if quoted.ends_with('>') => name.to_string(),
            _ => quoted.to_string(),
        };
        out.push_str(&player);
        text = text[start + len + 2..].to_string();
    }
    out.push_str(&text);
    out
}

/// "*" switch: hide the rcon password in console lines.
fn mask_password(text: &str, password: Option<&str>) -> String {
    let mut out = match password {
        Some(pw) => text.replace(pw, "********"),
        None => text.to_string(),
    };
    if let Some(pos) = out.find("rcon_password ") {
        let start = pos + "rcon_password ".len();
        let end = out[start..]
            .find(char::is_whitespace)
            .map_or(out.len(), |e| start + e);
        out.replace_range(start..end, "********");
    }
    out
}

/// Server list file / pasted text: openHLSW JSON export, or one address per line
/// (`host:port [comment]`; `#` / `//` lines are ignored; commas and semicolons separate too).
fn parse_server_list_file(text: &str) -> Vec<SavedServer> {
    if let Ok(list) = serde_json::from_str::<SavedList>(text) {
        return list.servers;
    }
    if let Ok(servers) = serde_json::from_str::<Vec<SavedServer>>(text) {
        return servers;
    }
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
                        rcon_password: None,
                        comment: String::new(),
                        last_connected: None,
                        info: None,
                        last_seen: None,
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
                rcon_password: None,
                comment: rest.trim().to_string(),
                ..Default::default()
            });
        }
    }
    out
}

fn merge_maps(maps: &mut Vec<String>, new: Vec<String>) {
    maps.extend(new);
    maps.sort_by_key(|m| m.to_lowercase());
    maps.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
}

/// `listmaps` output: one map name per line (mapcycle), header lines are skipped.
fn parse_map_cycle(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .map(|l| l.strip_suffix(".bsp").unwrap_or(l))
        .filter(|l| {
            !l.is_empty()
                && l.len() < 64
                && l.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '$'))
                && l.chars().any(|c| c.is_ascii_alphabetic())
        })
        .map(String::from)
        .collect()
}

/// User ids from `status`: lines like `#  2 "Name" STEAM_1:0:1 ...` or `# 2 1 "Name" [U:1:2] ...`.
fn parse_status_userids(text: &str) -> Vec<u32> {
    parse_status_players(text)
        .iter()
        .map(|p| p.userid)
        .collect()
}

/// `"Name<2><STEAM_1:0:1><CT>"` at the start of `text` → (name, team, rest after it).
fn parse_log_actor(text: &str) -> Option<(String, String, &str)> {
    let text = text.trim_start().strip_prefix('"')?;
    let end = text.find(">\"")?;
    let actor = &text[..end + 1];
    let rest = &text[end + 2..];
    // name<userid><steamid><team> — the name may contain '<', so split from the right.
    let mut parts = actor.rsplitn(4, '<');
    let team = parts.next()?.trim_end_matches('>').to_string();
    parts.next()?;
    parts.next()?;
    let name = parts.next()?.to_string();
    Some((name, team, rest))
}

/// Value of a `(key "value")` property in a log line.
fn log_property<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let start = text.find(&format!("({key} \""))? + key.len() + 3;
    let end = text[start..].find('"')?;
    Some(&text[start..start + end])
}

/// Updates kills, deaths, team kills, suicides, damage, health and teams from one log line
/// (Source and GoldSrc log format). A new map starts the statistics over.
fn apply_log_stats(players: &mut HashMap<String, LogPlayer>, line: &str) {
    let Some((_, event)) = line.split_once(": ") else {
        return;
    };
    if event.starts_with("Started map") || event.starts_with("Loading map") {
        players.clear();
        return;
    }
    let Some((name, team, rest)) = parse_log_actor(event) else {
        return;
    };
    let real_team = |t: &str| !t.is_empty() && t != "Unassigned" && t != "Console";
    if real_team(&team) {
        players.entry(name.clone()).or_default().team = team.clone();
    }
    // Skip the position "[x y z]" that CS:GO / CS2 write after the actor.
    let rest = rest.trim_start();
    let rest = if rest.starts_with('[') {
        rest.split_once(']').map_or(rest, |(_, r)| r.trim_start())
    } else {
        rest
    };
    if let Some(victim) = rest.strip_prefix("killed ") {
        let Some((vname, vteam, _)) = parse_log_actor(victim) else {
            return;
        };
        let killer = players.entry(name.clone()).or_default();
        killer.kills += 1;
        if real_team(&team) && team == vteam && vname != name {
            killer.teamkills += 1;
        }
        let v = players.entry(vname).or_default();
        v.deaths += 1;
        if real_team(&vteam) {
            v.team = vteam;
        }
    } else if rest.starts_with("committed suicide") {
        let p = players.entry(name).or_default();
        p.suicides += 1;
        p.deaths += 1;
    } else if let Some(victim) = rest.strip_prefix("attacked ") {
        let Some((vname, _, props)) = parse_log_actor(victim) else {
            return;
        };
        let num = |k: &str| log_property(props, k).and_then(|v| v.parse::<i32>().ok());
        let a = players.entry(name).or_default();
        a.damage_hp += num("damage").unwrap_or(0).max(0) as u32;
        a.damage_armor += num("damage_armor").unwrap_or(0).max(0) as u32;
        if let Some(hp) = num("health") {
            players.entry(vname).or_default().health = Some((hp, num("armor").unwrap_or(0)));
        }
    } else if let Some(t) = rest.strip_prefix("joined team \"") {
        let t = t.trim_end_matches('"');
        if real_team(t) {
            players.entry(name).or_default().team = t.to_string();
        }
    } else if let Some(new) = rest.strip_prefix("changed name to \"")
        && let Some(stats) = players.remove(&name)
    {
        players.insert(new.trim_end_matches('"').to_string(), stats);
    }
}

/// `10/18/2026 - 19:22:33: "Bob<2><STEAM_1:0:1><CT>" say "hi"` → chat line.
fn parse_chat_line(line: &str) -> Option<ChatLine> {
    let (stamp, rest) = line.split_once(": ")?;
    let time = stamp.rsplit(' ').next().unwrap_or_default().to_string();
    // HLDS logs `rcon say` as: Server say "text"
    if let Some(text) = rest.strip_prefix("Server say \"") {
        return Some(ChatLine {
            time,
            name: "Console".into(),
            team: String::new(),
            team_only: false,
            text: text.strip_suffix('"').unwrap_or(text).to_string(),
            own: false,
        });
    }
    let (player, team_only, text) = if let Some((p, t)) = rest.split_once("\" say_team \"") {
        (p, true, t)
    } else {
        let (p, t) = rest.split_once("\" say \"")?;
        (p, false, t)
    };
    let player = player.strip_prefix('"')?;
    // name<userid><steamid><team> — the name itself may contain '<', so split from the right.
    let mut parts = player.rsplitn(4, '<');
    let team = parts.next()?.trim_end_matches('>').to_string();
    parts.next()?;
    parts.next()?;
    let name = parts.next()?.to_string();
    let text = text.strip_suffix('"').unwrap_or(text).to_string();
    Some(ChatLine {
        time,
        name,
        team,
        team_only,
        text,
        own: false,
    })
}

/// Appends a log line to `<config dir>/logs/<server>/<YYYY-MM-DD>.<ext>`.
fn append_log_file(server: &str, line: &str, ext: &str) {
    use std::io::Write as _;
    let Some(base) = config::path().and_then(|p| p.parent().map(|d| d.join("logs"))) else {
        return;
    };
    let dir = base.join(server.replace([':', '[', ']', '/', '\\'], "_"));
    // Log lines start with "MM/DD/YYYY - hh:mm:ss: ".
    let date = line
        .get(..10)
        .and_then(|d| {
            let mut it = d.split('/');
            Some(format!("{}-{}-{}", it.nth(2)?, d.get(..2)?, d.get(3..5)?))
        })
        .unwrap_or_else(|| "log".into());
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{date}.{ext}")))
    {
        let _ = writeln!(f, "L {line}");
    }
}

/// Plays a .wav file without blocking (PowerShell on Windows, paplay/aplay on Linux).
fn play_sound(file: &str) {
    use std::process::{Command, Stdio};
    let file = file.to_string();
    std::thread::spawn(move || {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            let script = format!(
                "(New-Object Media.SoundPlayer '{}').PlaySync()",
                file.replace('\'', "''")
            );
            let _ = Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                .creation_flags(0x0800_0000)
                .status();
        }
        #[cfg(target_os = "macos")]
        {
            let _ = Command::new("afplay").arg(&file).status();
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let ok = Command::new("paplay")
                .arg(&file)
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if !ok {
                let _ = Command::new("aplay")
                    .args(["-q", &file])
                    .stderr(Stdio::null())
                    .status();
            }
        }
        let _ = Stdio::null();
    });
}

fn run_shell(command: &str) -> std::io::Result<()> {
    use std::process::Command;
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        Command::new("cmd")
            .args(["/C", command])
            .creation_flags(0x0800_0000)
            .spawn()?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("sh").args(["-c", command]).spawn()?;
    }
    Ok(())
}

/// Common srcds console commands and cvars for Tab completion.
/// Whether the server list filter shows this server (`country`: two-letter code).
fn filter_matches(f: &crate::filter::ServerFilter, s: &Server, country: &str) -> bool {
    if !f.enabled {
        return true;
    }
    let info = s.info.as_ref();
    let short = s.game_short();
    let game: Vec<&str> = info
        .map(|i| vec![i.game.as_str(), i.folder.as_str(), short.as_str()])
        .unwrap_or_default();
    let addons = s.addons_short();
    let rule = |key: &str| {
        s.rule(key)
            .and_then(|v| v.trim().parse::<f32>().ok())
            .map(|v| v.max(0.0) as u32)
    };
    f.matches(&crate::filter::FilterInput {
        offline: s.is_offline(),
        has_info: info.is_some(),
        ping: s.ping,
        passworded: info.is_some_and(|i| i.passworded),
        game: &game,
        map: info.map_or("", |i| i.map.as_str()),
        country,
        addons: &addons,
        humans: u32::from(s.humans()),
        max_players: info.map_or(0, |i| u32::from(i.max_players)),
        rule: &rule,
    })
}

/// Splits a parameter line like a shell: spaces separate, "double quotes" group.
fn split_args(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Quotes a parameter that contains spaces (for steam://run).
fn quote_arg(arg: &str) -> String {
    if arg.contains(char::is_whitespace) {
        format!("\"{arg}\"")
    } else {
        arg.to_string()
    }
}

const CONSOLE_COMMANDS: &[&str] = &[
    "addip",
    "banid",
    "bot_add",
    "bot_kick",
    "bot_quota",
    "changelevel",
    "cvarlist",
    "echo",
    "exec",
    "find",
    "help",
    "hostname",
    "kick",
    "kickid",
    "listid",
    "listip",
    "log",
    "logaddress_add",
    "logaddress_del",
    "logaddress_delall",
    "logaddress_list",
    "maps",
    "meta",
    "mp_friendlyfire",
    "mp_maxrounds",
    "mp_restartgame",
    "mp_roundtime",
    "mp_timelimit",
    "mp_warmup_end",
    "mp_winlimit",
    "nextlevel",
    "plugin_print",
    "quit",
    "removeid",
    "removeip",
    "say",
    "sm",
    "sm plugins list",
    "sm_kick",
    "sm_ban",
    "sm_map",
    "sm_rcon",
    "stats",
    "status",
    "sv_alltalk",
    "sv_cheats",
    "sv_gravity",
    "sv_password",
    "sv_tags",
    "tv_record",
    "tv_stoprecord",
    "users",
    "version",
    "writeid",
    "writeip",
];

/// Completes the command word to the longest common prefix of all matches.
/// Returns the new text (if it changed) and the candidates.
fn autocomplete(input: &str, history: &[String]) -> (Option<String>, Vec<String>) {
    let prefix = input.trim_start();
    if prefix.is_empty() {
        return (None, Vec::new());
    }
    let lower = prefix.to_lowercase();
    let mut candidates: Vec<String> = CONSOLE_COMMANDS
        .iter()
        .map(|c| c.to_string())
        .chain(history.iter().rev().cloned())
        .filter(|c| c.to_lowercase().starts_with(&lower) && c.len() > prefix.len())
        .collect();
    candidates.sort();
    candidates.dedup();
    let Some(first) = candidates.first() else {
        return (None, candidates);
    };
    let mut common = first.clone();
    for c in &candidates[1..] {
        let n = common
            .chars()
            .zip(c.chars())
            .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
            .count();
        common = common.chars().take(n).collect();
    }
    if candidates.len() == 1 {
        common.push(' ');
    }
    let completed = (common.len() > prefix.len()).then_some(common);
    (completed, candidates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_click_ctrl_shift_arrows() {
        let order = ["a", "b", "c", "d"];
        let mut sel = Selection::default();
        let none = egui::Modifiers::NONE;
        sel.click("b", &order, none);
        sel.click("d", &order, egui::Modifiers::COMMAND);
        assert!(sel.contains("b") && sel.contains("d") && !sel.contains("c"));
        sel.click("a", &order, egui::Modifiers::SHIFT);
        assert_eq!(sel.keys.len(), 4); // range d..a
        sel.click("c", &order, none);
        assert_eq!(sel.step(&order, true, false), Some(3));
        assert!(sel.contains("d") && sel.keys.len() == 1);
        assert_eq!(sel.step(&order, false, true), Some(2));
        assert!(sel.contains("c") && sel.contains("d") && sel.keys.len() == 2);
        sel.right_click("d");
        assert_eq!(sel.keys.len(), 2); // keeps the marked group
        sel.right_click("a");
        assert!(sel.contains("a") && sel.keys.len() == 1);
    }

    #[test]
    fn parses_chat() {
        let c = parse_chat_line(
            r#"10/18/2026 - 19:22:33: "B<o>b<2><STEAM_1:0:1><CT>" say_team "rush b""#,
        )
        .unwrap();
        assert_eq!(
            (c.time.as_str(), c.name.as_str(), c.team.as_str()),
            ("19:22:33", "B<o>b", "CT")
        );
        assert!(c.team_only);
        assert_eq!(c.text, "rush b");
        assert!(
            parse_chat_line(r#"10/18/2026 - 19:22:33: World triggered "Round_Start""#).is_none()
        );
        // rcon say: srcds and HLDS log it differently, both count as "Console".
        for line in [
            r#"10/18/2026 - 19:22:34: "Console<0><Console><Console>" say "hello all""#,
            r#"10/18/2026 - 19:22:34: Server say "hello all""#,
        ] {
            let c = parse_chat_line(line).unwrap();
            assert_eq!((c.name.as_str(), c.text.as_str()), ("Console", "hello all"));
        }
    }

    #[test]
    fn parses_bans_and_maps() {
        let bans = parse_ban_list(
            "ID filter list: 2 entries\n1 STEAM_1:0:123 : permanent\n2 STEAM_1:1:5 : 30.000 min\n",
            false,
        );
        assert_eq!(bans.len(), 2);
        assert_eq!(
            (bans[1].ident.as_str(), bans[1].duration.as_str()),
            ("STEAM_1:1:5", "30.000 min")
        );
        // HLDS pads the octets of banned IPs.
        let ips = parse_ban_list(
            "IP filter list: 2 entries\n1 193. 24.211.152 : permanent\n2  10.  0.  0.  1 : 5.000 min\n",
            true,
        );
        assert_eq!(ips.len(), 2);
        assert_eq!(ips[0].ident, "193.24.211.152");
        assert_eq!(ips[1].ident, "10.0.0.1");
        let maps = parse_map_list(
            "Dir:  maps/*.bsp\nPENDING:   (fs) de_dust2.bsp\nPENDING:   (fs) cs_office.bsp\n",
        );
        assert_eq!(maps, vec!["cs_office", "de_dust2"]);
    }

    #[test]
    fn parses_cycle_and_status() {
        assert_eq!(
            parse_map_cycle("Map Cycle:\nde_dust2\ncs_office\n\nUnknown command \"x\"\n"),
            vec!["de_dust2", "cs_office"]
        );
        let st = "# userid name uniqueid connected ping loss state rate adr\n#      2 \"Bob\" STEAM_1:0:1 01:00 30 0 active 80000 1.2.3.4:27005\n#  3 \"BOT Ali\" BOT active\n";
        assert_eq!(parse_status_userids(st), vec![2, 3]);
    }

    #[test]
    fn log_statistics() {
        let mut p = HashMap::new();
        let lines = [
            r#"10/18/2026 - 20:00:00: "Bob<2><STEAM_1:0:1><Unassigned>" joined team "CT""#,
            r#"10/18/2026 - 20:00:01: "Bob<2><STEAM_1:0:1><CT>" [1 2 3] attacked "Ann<3><STEAM_1:0:2><TERRORIST>" [4 5 6] with "ak47" (damage "27") (damage_armor "4") (health "73") (armor "96") (hitgroup "chest")"#,
            r#"10/18/2026 - 20:00:02: "Bob<2><STEAM_1:0:1><CT>" [1 2 3] killed "Ann<3><STEAM_1:0:2><TERRORIST>" [4 5 6] with "ak47""#,
            r#"10/18/2026 - 20:00:03: "Bob<2><STEAM_1:0:1><CT>" killed "Cid<4><STEAM_1:0:3><CT>" with "m4a1""#,
            r#"10/18/2026 - 20:00:04: "Ann<3><STEAM_1:0:2><TERRORIST>" committed suicide with "world""#,
        ];
        for l in lines {
            apply_log_stats(&mut p, l);
        }
        let bob = &p["Bob"];
        assert_eq!((bob.kills, bob.deaths, bob.teamkills), (2, 0, 1));
        assert_eq!((bob.damage_hp, bob.damage_armor), (27, 4));
        assert_eq!(bob.team, "CT");
        let ann = &p["Ann"];
        assert_eq!(
            (ann.deaths, ann.suicides, ann.health),
            (2, 1, Some((73, 96)))
        );
        apply_log_stats(
            &mut p,
            r#"10/18/2026 - 20:05:00: Started map "de_dust2" (CRC "1")"#,
        );
        assert!(p.is_empty());
    }

    #[test]
    fn parses_goldsrc_status() {
        let text = "#      name userid uniqueid frag time ping loss adr\n\
                    # 1 \"Bob\"   12 STEAM_0:1:42   7 12:34   48    0 94.31.108.10:27005\n\
                    # 2 \"bot\"   13 BOT            0 12:00    0    0\n2 users\n";
        let p = parse_status_players(text);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].userid, 12);
        assert_eq!(p[0].steam_id, "STEAM_0:1:42");
        assert_eq!(p[0].ping, "48");
        assert_eq!(p[0].port, Some(27005));
        assert_eq!(p[1].steam_id, "BOT");
        assert_eq!(parse_status_userids(text), vec![12, 13]);
    }

    #[test]
    fn parses_status_players() {
        let css = "hostname: x\n# userid name uniqueid connected ping loss state adr\n#      2 \"Bob \\\"the\\\" Builder\" STEAM_1:0:1234 05:23 45 0 active 1.2.3.4:27005\n#      3 \"BOT Ali\" BOT active\n";
        let p = parse_status_players(css);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].userid, 2);
        assert_eq!(p[0].steam_id, "STEAM_1:0:1234");
        assert_eq!(p[0].ping, "45");
        assert_eq!(p[0].ip, Some("1.2.3.4".parse().unwrap()));
        assert_eq!(p[1].steam_id, "BOT");
        let cs2 = "---------players--------\n  id     time ping loss      state   rate adr name\n65535 [NoChan]    0    0 challenging      0unknown ''\n    2    05:23   45    0     active 786432 5.6.7.8:27005 'Ann'\n";
        let p = parse_status_players(cs2);
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].name.as_str(), p[0].ping.as_str()), ("Ann", "45"));
        assert_eq!(p[0].ip, Some("5.6.7.8".parse().unwrap()));
    }

    #[test]
    fn translates_log_lines() {
        assert_eq!(
            translate_log(r#"10/18/2026 - 20:29:31: "Bob<2><STEAM_1:0:1><CT>" say "hi""#),
            "Bob: hi"
        );
        assert_eq!(
            translate_log(
                r#"10/18/2026 - 20:29:31: "Bob<2><STEAM_1:0:1><CT>" killed "Al<3><BOT><TERRORIST>" with "ak47""#
            ),
            "Bob killed Al with ak47"
        );
        assert_eq!(
            mask_password("rcon_password geheim now", None),
            "rcon_password ******** now"
        );
        assert_eq!(
            mask_password("pw is geheim", Some("geheim")),
            "pw is ********"
        );
    }

    #[test]
    fn parses_server_lists() {
        let l = parse_server_list_file(
            "# my servers\n1.2.3.4:27015 Clan server\nsteam://connect/5.6.7.8:27016\n\na:1, b:2\n",
        );
        let addrs: Vec<&str> = l.iter().map(|s| s.address.as_str()).collect();
        assert_eq!(addrs, ["1.2.3.4:27015", "5.6.7.8:27016", "a:1", "b:2"]);
        assert_eq!(l[0].comment, "Clan server");
    }

    #[test]
    fn completes_commands() {
        let (text, _) = autocomplete("chang", &[]);
        assert_eq!(text.as_deref(), Some("changelevel "));
        let (text, cands) = autocomplete("mp_w", &[]);
        assert_eq!(text, None); // mp_warmup_end / mp_winlimit share only "mp_w"
        assert!(cands.len() >= 2);
    }
}
