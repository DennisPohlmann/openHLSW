//! State of the console tool and what every key does. The screen is laid out like the GUI
//! (server list on top, details left, players and rules right, the tabs Rcon Config,
//! Console, Chat, Ban List and Change Level at the bottom); it is used like htop and
//! Midnight Commander: Tab moves between the panels, F9 opens the menu bar, F keys below.

use openhlsw::clean_terminal_text as clean;
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use openhlsw::a2s::{Player, Rule, ServerInfo};
use openhlsw::address::ServerAddress;
use openhlsw::query::Protocol;
use openhlsw::rcon_text::{self, BanEntry};
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};

use crate::games::{self, Game};
use crate::net::{self, Event, Id, QueryJob, RconState, Request, Tag};
use crate::store::Store;

/// Lines of console and chat output kept per server.
const CONSOLE_LINES: usize = 5000;
/// Pings kept per server for the graph in the details.
const PING_HISTORY: usize = 300;

// --------------------------------------------------------------------------
// Data
// --------------------------------------------------------------------------

pub struct Server {
    pub id: Id,
    pub address: ServerAddress,
    /// The address as written in the list (also the key in the config file).
    pub address_text: String,
    pub game: Option<&'static Game>,
    pub detected: Option<Protocol>,
    /// RCON password (typed in Rcon Config, or saved in the config with Save).
    pub password: Option<String>,
    pub save_password: bool,
    pub hide_password: bool,
    pub comment: String,
    /// Last known info (from the config until the server answers).
    pub info: Option<ServerInfo>,
    pub state: State,
    pub ping_ms: Option<u32>,
    pub pings: VecDeque<u32>,
    pub players: Vec<Player>,
    pub rules: Vec<Rule>,
    pub last_seen: Option<Instant>,
    busy: bool,
    details_at: Option<Instant>,
    challenge: Option<[u8; 4]>,
    /// Given on the command line, not in the saved list.
    pub temporary: bool,
    pub rcon: Rcon,
}

#[derive(Clone, PartialEq)]
pub enum State {
    Waiting,
    Online,
    Offline(String),
}

pub struct Rcon {
    pub session: Option<Sender<Request>>,
    pub state: RconState,
    pub console: VecDeque<(String, LineKind)>,
    /// Lines scrolled up from the bottom (0 = follow new output).
    pub console_scroll: usize,
    pub chat: VecDeque<ChatLine>,
    pub chat_scroll: usize,
    pub bans: Vec<BanEntry>,
    pub maps: Vec<String>,
    pub maps_asked: bool,
    pub bans_asked: bool,
    pub pending: usize,
}

impl Default for Rcon {
    fn default() -> Self {
        Self {
            session: None,
            state: RconState::Unknown,
            console: VecDeque::new(),
            console_scroll: 0,
            chat: VecDeque::new(),
            chat_scroll: 0,
            bans: Vec::new(),
            maps: Vec::new(),
            maps_asked: false,
            bans_asked: false,
            pending: 0,
        }
    }
}

pub struct ChatLine {
    pub time: String,
    pub name: String,
    pub text: String,
}

#[derive(Clone, Copy, PartialEq)]
pub enum LineKind {
    Command,
    Output,
    Error,
}

fn push_lines(
    lines: &mut VecDeque<(String, LineKind)>,
    scroll: &mut usize,
    text: &str,
    kind: LineKind,
) {
    for line in text.trim_end_matches('\n').split('\n') {
        lines.push_back((clean(line.trim_end_matches('\r')), kind));
        if *scroll > 0 {
            *scroll += 1;
        }
    }
    while lines.len() > CONSOLE_LINES {
        lines.pop_front();
    }
}

impl Server {
    fn protocol(&self) -> Option<Protocol> {
        self.game.map(|g| g.protocol).or(self.detected)
    }

    pub fn name(&self) -> &str {
        self.info
            .as_ref()
            .map(|i| i.name.as_str())
            .filter(|n| !n.is_empty())
            .unwrap_or("")
    }

    pub fn map(&self) -> &str {
        self.info.as_ref().map_or("", |i| i.map.as_str())
    }

    pub fn game_name(&self) -> String {
        match (&self.info, self.game) {
            (Some(i), _) if !i.game.is_empty() => i.game.clone(),
            (_, Some(g)) => g.name.to_string(),
            (Some(i), None) => i.folder.clone(),
            _ => String::new(),
        }
    }

    /// (players, max players), 0/0 while unknown.
    pub fn counts(&self) -> (u32, u32) {
        self.info
            .as_ref()
            .map_or((0, 0), |i| (u32::from(i.players), u32::from(i.max_players)))
    }

    pub fn ping_avg(&self) -> Option<f64> {
        (!self.pings.is_empty()).then(|| {
            self.pings.iter().map(|&p| f64::from(p)).sum::<f64>() / self.pings.len() as f64
        })
    }

    pub fn protocol_name(&self) -> &'static str {
        self.protocol().map_or("detecting", |p| p.name())
    }

    pub fn rule(&self, key: &str) -> Option<&str> {
        self.rules
            .iter()
            .find(|r| r.key.eq_ignore_ascii_case(key))
            .map(|r| r.value.as_str())
    }

    /// "MM:S v1.12, SMod v1.13" like the GUI's AddOn column (from the rules).
    pub fn addons(&self) -> String {
        let mut out = Vec::new();
        if let Some(v) = self.rule("metamod_version") {
            out.push(format!("MM:S v{v}"));
        }
        if let Some(v) = self.rule("sourcemod_version") {
            out.push(format!("SMod v{v}"));
        }
        if let Some(v) = self.rule("amxmodx_version") {
            out.push(format!("AMXX v{v}"));
        }
        out.join(", ")
    }

    fn matches(&self, needle: &str) -> bool {
        let n = needle.to_lowercase();
        [
            self.name().to_string(),
            self.address_text.clone(),
            self.map().to_string(),
            self.game_name(),
            self.comment.clone(),
        ]
        .iter()
        .any(|h| h.to_lowercase().contains(&n))
    }
}

// --------------------------------------------------------------------------
// UI state
// --------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortKey {
    List,
    Address,
    Name,
    Game,
    Map,
    Players,
    Ping,
}

pub const SERVER_SORT: [(SortKey, &str); 7] = [
    (SortKey::List, "#"),
    (SortKey::Address, "Addr"),
    (SortKey::Name, "Name"),
    (SortKey::Game, "Game"),
    (SortKey::Map, "Map"),
    (SortKey::Players, "Players"),
    (SortKey::Ping, "Ping"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlayerSort {
    Index,
    Name,
    Frags,
    Time,
}

pub const PLAYER_SORT: [(PlayerSort, &str); 4] = [
    (PlayerSort::Index, "#"),
    (PlayerSort::Name, "Name"),
    (PlayerSort::Frags, "Frags"),
    (PlayerSort::Time, "Time"),
];

/// The panels, in the order Tab walks through them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane {
    Servers,
    Players,
    Rules,
    Tabs,
}

const PANES: [Pane; 4] = [Pane::Servers, Pane::Players, Pane::Rules, Pane::Tabs];

/// The tabs at the bottom, like HLSW.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    RconConfig,
    Console,
    Chat,
    Bans,
    ChangeLevel,
}

pub const TABS: [(Tab, &str); 5] = [
    (Tab::RconConfig, "Rcon Config"),
    (Tab::Console, "Console"),
    (Tab::Chat, "Chat"),
    (Tab::Bans, "Ban List"),
    (Tab::ChangeLevel, "Change Level"),
];

/// Items of the Rcon Config tab, in order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    Password,
    Test,
    Disconnect,
    Hide,
    Save,
}

pub const FIELDS: [Field; 5] = [
    Field::Password,
    Field::Test,
    Field::Disconnect,
    Field::Hide,
    Field::Save,
];

#[derive(Clone, PartialEq)]
pub enum PromptKind {
    Search,
    Filter(Pane),
    Add,
    Ban,
    BanMinutes(String),
}

impl PromptKind {
    pub fn label(&self) -> &'static str {
        match self {
            PromptKind::Search => "Search: ",
            PromptKind::Filter(Pane::Players) => "Filter players (name): ",
            PromptKind::Filter(Pane::Rules) => "Filter rules (cvar or value): ",
            PromptKind::Filter(_) => "Filter servers: ",
            PromptKind::Add => "Add server (IP:port): ",
            PromptKind::Ban => "Ban Steam ID or IP: ",
            PromptKind::BanMinutes(_) => "Minutes (0 = permanent): ",
        }
    }
}

pub enum Confirm {
    Remove(Id),
    ChangeLevel(Id, String),
    Unban(Id, BanEntry),
}

/// An entry of the menu bar (F9).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Quit,
    Add,
    Remove,
    Refresh,
    Search,
    Filter,
    HideEmpty,
    Sort,
    Invert,
    CopyAddress,
    Tab(Tab),
    Help,
    About,
}

/// A menu entry: label, key, what it does.
pub type MenuItem = (&'static str, &'static str, Action);

pub const MENUS: &[(&str, &[MenuItem])] = &[
    ("File", &[("Quit", "F10", Action::Quit)]),
    (
        "Server list",
        &[
            ("Add server...", "F7", Action::Add),
            ("Remove server", "F8", Action::Remove),
            ("Refresh", "F5", Action::Refresh),
            ("Find...", "F3", Action::Search),
            ("Filter...", "F4", Action::Filter),
            ("Hide empty servers", "e", Action::HideEmpty),
        ],
    ),
    (
        "Server",
        &[
            ("Rcon Config", "Alt+1", Action::Tab(Tab::RconConfig)),
            ("Console", "F2", Action::Tab(Tab::Console)),
            ("Chat", "Alt+3", Action::Tab(Tab::Chat)),
            ("Ban List", "Alt+4", Action::Tab(Tab::Bans)),
            ("Change Level", "Alt+5", Action::Tab(Tab::ChangeLevel)),
            ("Copy address", "y", Action::CopyAddress),
        ],
    ),
    (
        "View",
        &[
            ("Sort by...", "F6", Action::Sort),
            ("Invert sort order", "I", Action::Invert),
        ],
    ),
    (
        "?",
        &[("Keys", "F1", Action::Help), ("About", "", Action::About)],
    ),
];

pub enum Mode {
    Normal,
    Prompt { kind: PromptKind, text: String },
    Confirm(Confirm),
    Sort(usize),
    Help,
    About,
    Menu { menu: usize, item: usize },
}

/// A one-line text field.
#[derive(Default, Clone)]
pub struct Input {
    pub text: String,
    /// Cursor position in characters.
    pub cursor: usize,
}

impl Input {
    pub fn set(&mut self, text: &str) {
        self.text = text.to_string();
        self.cursor = text.chars().count();
    }

    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    /// Editing keys. Returns false for keys it does not use.
    fn key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let len = self.text.chars().count();
        match key.code {
            KeyCode::Char('u') if ctrl => {
                self.text.clear();
                self.cursor = 0;
            }
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('e') if ctrl => self.cursor = len,
            KeyCode::Char(c) if !ctrl && !alt => {
                let at = byte_at(&self.text, self.cursor);
                self.text.insert(at, c);
                self.cursor += 1;
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    let at = byte_at(&self.text, self.cursor);
                    self.text.remove(at);
                }
            }
            KeyCode::Delete => {
                if self.cursor < len {
                    let at = byte_at(&self.text, self.cursor);
                    self.text.remove(at);
                }
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(len),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = len,
            _ => return false,
        }
        true
    }
}

fn byte_at(s: &str, chars: usize) -> usize {
    s.char_indices().nth(chars).map_or(s.len(), |(i, _)| i)
}

/// Where the last frame drew things, for the mouse and for paging.
#[derive(Default)]
pub struct Areas {
    pub servers: Rect,
    pub servers_offset: usize,
    pub players: Rect,
    pub players_offset: usize,
    pub rules: Rect,
    pub rules_offset: usize,
    pub tabs: Rect,
    pub tab_labels: Vec<(Rect, Tab)>,
    pub menu_labels: Vec<Rect>,
    pub map_columns: usize,
    pub bans_offset: usize,
}

fn inside(r: Rect, x: u16, y: u16) -> bool {
    r.contains(Position::new(x, y))
}

pub struct App {
    pub store: Store,
    pub servers: Vec<Server>,
    next_id: Id,
    pub focus: Pane,
    pub tab: Tab,
    pub mode: Mode,
    /// Index into `visible()` of the selected server.
    pub selected: usize,
    selected_id: Option<Id>,
    selected_at: Instant,
    pub sort: SortKey,
    pub reverse: bool,
    pub player_sort: PlayerSort,
    pub player_reverse: bool,
    pub filter: String,
    pub player_filter: String,
    pub rule_filter: String,
    pub search: String,
    pub hide_empty: bool,
    pub player_sel: usize,
    pub rule_sel: usize,
    pub ban_sel: usize,
    pub map_sel: usize,
    pub field: Field,
    pub console_input: Input,
    pub chat_input: Input,
    pub map_search: Input,
    /// Password field of Rcon Config (for the selected server).
    pub password_input: Input,
    pub history: Vec<String>,
    history_pos: Option<usize>,
    pub delay: Duration,
    last_refresh: Option<Instant>,
    /// Status line message and when it was set.
    pub message: Option<(String, Instant)>,
    pub quit: bool,
    pub tx: Sender<Event>,
    rx: Receiver<Event>,
    pub areas: Areas,
    /// Text waiting for the terminal's clipboard (OSC 52), written after the next frame.
    pub clipboard: Option<String>,
}

impl App {
    pub fn new(store: Store, extra: Vec<ServerAddress>, delay: Duration) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let history = store.history();
        let mut app = Self {
            servers: Vec::new(),
            next_id: 1,
            focus: Pane::Servers,
            tab: Tab::RconConfig,
            mode: Mode::Normal,
            selected: 0,
            selected_id: None,
            selected_at: Instant::now(),
            sort: SortKey::List,
            reverse: false,
            player_sort: PlayerSort::Index,
            player_reverse: false,
            filter: String::new(),
            player_filter: String::new(),
            rule_filter: String::new(),
            search: String::new(),
            hide_empty: false,
            player_sel: 0,
            rule_sel: 0,
            ban_sel: 0,
            map_sel: 0,
            field: Field::Password,
            console_input: Input::default(),
            chat_input: Input::default(),
            map_search: Input::default(),
            password_input: Input::default(),
            history,
            history_pos: None,
            delay,
            last_refresh: None,
            message: None,
            quit: false,
            tx,
            rx,
            areas: Areas::default(),
            clipboard: None,
            store,
        };
        for e in app.store.servers() {
            if let Some(address) = ServerAddress::parse(&e.address) {
                let game = e.game.as_deref().and_then(games::by_key);
                let id = app.push_server(address, e.address, game, e.info, false);
                if let Some(s) = app.server_mut(id) {
                    s.save_password = e.rcon_password.as_deref().is_some_and(|p| !p.is_empty());
                    s.password = e.rcon_password.filter(|p| !p.is_empty());
                    s.hide_password = e.hide_rcon_password;
                    s.comment = clean(&e.comment);
                }
            }
        }
        for address in extra {
            let text = address.to_string();
            app.push_server(address, text, None, None, true);
        }
        if !app.store.writable() {
            app.say("The config file cannot be read: the server list is not changed.");
        }
        app.selection_changed();
        app
    }

    fn push_server(
        &mut self,
        address: ServerAddress,
        address_text: String,
        game: Option<&'static Game>,
        info: Option<ServerInfo>,
        temporary: bool,
    ) -> Id {
        let id = self.next_id;
        self.next_id += 1;
        self.servers.push(Server {
            id,
            address,
            address_text,
            game,
            detected: None,
            password: None,
            save_password: false,
            hide_password: true,
            comment: String::new(),
            info,
            state: State::Waiting,
            ping_ms: None,
            pings: VecDeque::new(),
            players: Vec::new(),
            rules: Vec::new(),
            last_seen: None,
            busy: false,
            details_at: None,
            challenge: None,
            temporary,
            rcon: Rcon::default(),
        });
        id
    }

    pub fn say(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), Instant::now()));
    }

    pub fn server(&self, id: Id) -> Option<&Server> {
        self.servers.iter().find(|s| s.id == id)
    }

    fn server_mut(&mut self, id: Id) -> Option<&mut Server> {
        self.servers.iter_mut().find(|s| s.id == id)
    }

    /// Position of a server in the saved list (the # column).
    pub fn index_of(&self, id: Id) -> usize {
        self.servers.iter().position(|s| s.id == id).unwrap_or(0) + 1
    }

    /// Servers shown in the list: filtered and sorted.
    pub fn visible(&self) -> Vec<&Server> {
        let mut list: Vec<&Server> = self
            .servers
            .iter()
            .filter(|s| self.filter.is_empty() || s.matches(&self.filter))
            .filter(|s| !self.hide_empty || s.counts().0 > 0)
            .collect();
        let lower = |s: &str| s.to_lowercase();
        match self.sort {
            SortKey::List => {}
            SortKey::Address => list.sort_by_key(|s| lower(&s.address_text)),
            SortKey::Name => list.sort_by_key(|s| lower(s.name())),
            SortKey::Game => list.sort_by_key(|s| lower(&s.game_name())),
            SortKey::Map => list.sort_by_key(|s| lower(s.map())),
            // Most players first, then the larger servers.
            SortKey::Players => list.sort_by_key(|s| std::cmp::Reverse(s.counts())),
            SortKey::Ping => list.sort_by_key(|s| s.ping_ms.unwrap_or(u32::MAX)),
        }
        if self.reverse {
            list.reverse();
        }
        list
    }

    /// The selected server: the details and all tabs are about it, like in the GUI.
    pub fn current(&self) -> Option<&Server> {
        self.selected_id.and_then(|id| self.server(id))
    }

    fn current_mut(&mut self) -> Option<&mut Server> {
        let id = self.selected_id?;
        self.server_mut(id)
    }

    /// Keeps `selected` in range and notices when another server got selected.
    fn selection_changed(&mut self) {
        let n = self.visible().len();
        if self.selected >= n {
            self.selected = n.saturating_sub(1);
        }
        let id = self.visible().get(self.selected).map(|s| s.id);
        if id != self.selected_id {
            self.selected_id = id;
            self.selected_at = Instant::now();
            self.player_sel = 0;
            self.rule_sel = 0;
            self.ban_sel = 0;
            self.map_sel = 0;
            let pw = self
                .current()
                .and_then(|s| s.password.clone())
                .unwrap_or_default();
            self.password_input.set(&pw);
            self.tab_opened();
        }
    }

    fn select_id(&mut self, id: Id) {
        if let Some(i) = self.visible().iter().position(|s| s.id == id) {
            self.selected = i;
        }
        self.selection_changed();
    }

    pub fn shown_players(&self, s: &Server) -> Vec<(usize, Player)> {
        let n = self.player_filter.to_lowercase();
        let mut p: Vec<(usize, Player)> = s
            .players
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, p)| (i + 1, p))
            .filter(|(_, p)| n.is_empty() || p.name.to_lowercase().contains(&n))
            .collect();
        match self.player_sort {
            PlayerSort::Index => {}
            PlayerSort::Frags => p.sort_by_key(|(_, x)| std::cmp::Reverse(x.score)),
            PlayerSort::Name => p.sort_by_key(|(_, x)| x.name.to_lowercase()),
            PlayerSort::Time => p.sort_by(|(_, a), (_, b)| b.duration.total_cmp(&a.duration)),
        }
        if self.player_reverse {
            p.reverse();
        }
        p
    }

    pub fn shown_rules<'a>(&self, s: &'a Server) -> Vec<&'a Rule> {
        let n = self.rule_filter.to_lowercase();
        s.rules
            .iter()
            .filter(|r| {
                n.is_empty()
                    || r.key.to_lowercase().contains(&n)
                    || r.value.to_lowercase().contains(&n)
            })
            .collect()
    }

    pub fn shown_maps<'a>(&self, s: &'a Server) -> Vec<&'a String> {
        s.rcon
            .maps
            .iter()
            .filter(|m| rcon_text::map_matches(&self.map_search.text, m))
            .collect()
    }

    // ----------------------------------------------------------------------
    // Background work
    // ----------------------------------------------------------------------

    /// Queries every idle server, with players and rules, when the refresh time has come
    /// (or `force`), so every server stays current; the selected one also right after it got
    /// selected.
    pub fn tick(&mut self, force: bool) {
        while let Ok(event) = self.rx.try_recv() {
            self.handle_event(event);
        }
        let due = self.last_refresh.is_none_or(|t| t.elapsed() >= self.delay) || force;
        if due {
            self.last_refresh = Some(Instant::now());
        }
        let selected = self.selected_id;
        // Wait a moment after the selection moved, so scrolling through the list does not
        // ask every server on the way.
        let settled = self.selected_at.elapsed() >= Duration::from_millis(300);
        let mut jobs = Vec::new();
        for s in self.servers.iter_mut().filter(|s| !s.busy) {
            let is_selected = selected == Some(s.id);
            let details_missing =
                is_selected && settled && s.details_at.is_none_or(|t| t < self.selected_at);
            if !(due || details_missing) {
                continue;
            }
            s.busy = true;
            s.details_at = Some(Instant::now());
            jobs.push(QueryJob {
                id: s.id,
                address: s.address.clone(),
                protocol: s.protocol(),
                game: s.game.map_or("", |g| g.key),
                details: true,
                challenge: s.challenge,
            });
        }
        for job in jobs {
            net::spawn_query(&self.tx, job);
        }
    }

    /// Seconds until the next automatic refresh.
    pub fn next_refresh_in(&self) -> u64 {
        self.last_refresh
            .map_or(0, |t| self.delay.saturating_sub(t.elapsed()).as_secs())
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Query { id, result } => {
                let Some(s) = self.server_mut(id) else { return };
                s.busy = false;
                match result.map(|b| *b) {
                    Ok(snap) => {
                        s.state = State::Online;
                        s.ping_ms = Some(snap.ping_ms);
                        s.pings.push_back(snap.ping_ms);
                        if s.pings.len() > PING_HISTORY {
                            s.pings.pop_front();
                        }
                        s.last_seen = Some(Instant::now());
                        s.challenge = snap.challenge;
                        if snap.detected.is_some() {
                            s.detected = snap.detected;
                        }
                        let mut info = snap.info;
                        info.name = clean(&info.name);
                        info.map = clean(&info.map);
                        info.game = clean(&info.game);
                        info.version = clean(&info.version);
                        s.info = Some(info);
                        if let Some(p) = snap.players {
                            s.players = p
                                .into_iter()
                                .map(|mut p| {
                                    p.name = clean(&p.name);
                                    p
                                })
                                .collect();
                        }
                        if let Some(r) = snap.rules {
                            s.rules = r
                                .into_iter()
                                .map(|r| Rule {
                                    key: clean(&r.key),
                                    value: clean(&r.value),
                                })
                                .collect();
                        }
                    }
                    Err(e) => {
                        s.state = State::Offline(clean(&e));
                        s.ping_ms = None;
                    }
                }
            }
            Event::Rcon { id, tag, result } => {
                let Some(s) = self.server_mut(id) else { return };
                let r = &mut s.rcon;
                r.pending = r.pending.saturating_sub(1);
                match (tag, result) {
                    (Tag::Silent, _) => {}
                    (_, Err(e)) => {
                        push_lines(&mut r.console, &mut r.console_scroll, &e, LineKind::Error)
                    }
                    (Tag::Console, Ok(text)) => {
                        if !text.trim().is_empty() {
                            push_lines(
                                &mut r.console,
                                &mut r.console_scroll,
                                &text,
                                LineKind::Output,
                            );
                        }
                    }
                    (Tag::BanIds | Tag::BanIps, Ok(text)) => {
                        let is_ip = tag == Tag::BanIps;
                        r.bans.retain(|b| b.is_ip != is_ip);
                        r.bans
                            .extend(rcon_text::parse_ban_list(&text, is_ip).into_iter().map(
                                |mut b| {
                                    b.ident = clean(&b.ident);
                                    b.duration = clean(&b.duration);
                                    b
                                },
                            ));
                    }
                    (Tag::Maps, Ok(text)) => {
                        let mut maps: Vec<String> = rcon_text::parse_map_list(&text)
                            .into_iter()
                            .map(|m| clean(&m))
                            .collect();
                        if let Some(i) = &s.info {
                            maps.extend(
                                rcon_text::stock_maps(i.real_app_id(), &i.folder)
                                    .iter()
                                    .map(|m| m.to_string()),
                            );
                        }
                        maps.sort_by_key(|m| m.to_lowercase());
                        maps.dedup();
                        s.rcon.maps = maps;
                    }
                }
            }
            Event::RconState { id, state } => {
                let Some(s) = self.server_mut(id) else { return };
                if state == RconState::BadPassword {
                    // The session has ended; a new password has to be typed and tested.
                    s.rcon.session = None;
                    s.rcon.pending = 0;
                    push_lines(
                        &mut s.rcon.console,
                        &mut s.rcon.console_scroll,
                        "RCON: Bad RCON password",
                        LineKind::Error,
                    );
                }
                s.rcon.state = state;
            }
        }
    }

    // ----------------------------------------------------------------------
    // RCON
    // ----------------------------------------------------------------------

    /// The RCON session of the selected server, opened with its password. Without a
    /// password the Rcon Config tab is shown with the password field.
    fn session(&mut self) -> Option<Sender<Request>> {
        let s = self.current()?;
        if let Some(tx) = &s.rcon.session {
            return Some(tx.clone());
        }
        let Some(password) = s.password.clone() else {
            self.tab = Tab::RconConfig;
            self.focus = Pane::Tabs;
            self.field = Field::Password;
            self.say("Enter the RCON password first (Rcon Config), then Test");
            return None;
        };
        let (id, address, goldsrc) = (
            s.id,
            s.address.clone(),
            s.info.as_ref().is_some_and(|i| i.goldsrc),
        );
        let tx = net::spawn_rcon(&self.tx, id, address, password, goldsrc);
        if let Some(s) = self.server_mut(id) {
            s.rcon.session = Some(tx.clone());
        }
        Some(tx)
    }

    fn rcon_send(&mut self, tag: Tag, command: &str) {
        if let Some(tx) = self.session() {
            if let Some(s) = self.current_mut() {
                s.rcon.pending += 1;
            }
            let _ = tx.send(Request::Exec(tag, command.to_string()));
        }
    }

    /// Takes the typed password and logs in with it (Test, or Enter in the field).
    fn rcon_test(&mut self) {
        let typed = self.password_input.text.clone();
        let Some(s) = self.current_mut() else { return };
        s.password = (!typed.is_empty()).then_some(typed);
        s.rcon.session = None;
        s.rcon.state = RconState::Unknown;
        s.rcon.maps_asked = false;
        s.rcon.bans_asked = false;
        if s.save_password {
            self.save_rcon();
        }
        if let Some(tx) = self.session() {
            let _ = tx.send(Request::Test);
        }
    }

    fn save_rcon(&mut self) {
        let Some(s) = self.current() else { return };
        if s.temporary {
            return;
        }
        let (address, pw, hide) = (
            s.address_text.clone(),
            s.password.clone().filter(|_| s.save_password),
            s.hide_password,
        );
        if let Err(e) = self.store.set_rcon(&address, pw.as_deref(), hide) {
            self.say(format!("Not saved: {e}"));
        }
    }

    /// Fetches what a tab shows the first time it is opened for a server.
    fn tab_opened(&mut self) {
        let Some(s) = self.current() else { return };
        if s.password.is_none() {
            return;
        }
        match self.tab {
            Tab::Bans if !s.rcon.bans_asked => self.fetch_bans(),
            Tab::ChangeLevel if !s.rcon.maps_asked => self.fetch_maps(),
            _ => {}
        }
    }

    fn fetch_bans(&mut self) {
        if let Some(s) = self.current_mut() {
            s.rcon.bans_asked = true;
        }
        self.rcon_send(Tag::BanIds, "listid");
        self.rcon_send(Tag::BanIps, "listip");
    }

    fn fetch_maps(&mut self) {
        if let Some(s) = self.current_mut() {
            s.rcon.maps_asked = true;
        }
        self.rcon_send(Tag::Maps, "maps *");
    }

    // ----------------------------------------------------------------------
    // Keys
    // ----------------------------------------------------------------------

    pub fn key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Help | Mode::About => {} // any key closes them
            Mode::Confirm(c) => {
                if matches!(
                    key.code,
                    KeyCode::Char('y' | 'Y' | 'j' | 'J') | KeyCode::Enter
                ) {
                    self.confirmed(c);
                }
            }
            Mode::Sort(i) => self.sort_key(key, i),
            Mode::Prompt { kind, text } => self.prompt_key(key, kind, text),
            Mode::Menu { menu, item } => self.menu_key(key, menu, item),
            Mode::Normal => self.normal_key(key),
        }
        self.selection_changed();
    }

    fn normal_key(&mut self, key: KeyEvent) {
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        // Keys that work everywhere.
        match key.code {
            KeyCode::F(10) => return self.quit = true,
            KeyCode::F(1) => return self.mode = Mode::Help,
            KeyCode::F(9) => return self.mode = Mode::Menu { menu: 0, item: 0 },
            KeyCode::F(2) => return self.open_tab(Tab::Console),
            KeyCode::F(5) => return self.refresh(),
            KeyCode::F(7) => return self.run(Action::Add),
            KeyCode::Tab => return self.cycle_focus(1),
            KeyCode::BackTab => return self.cycle_focus(-1),
            KeyCode::Char(c @ '1'..='5') if alt => {
                return self.open_tab(TABS[c as usize - '1' as usize].0);
            }
            KeyCode::Esc if self.focus != Pane::Servers => return self.focus = Pane::Servers,
            _ => {}
        }
        match self.focus {
            Pane::Servers => self.servers_key(key),
            Pane::Players => self.players_key(key),
            Pane::Rules => self.rules_key(key),
            Pane::Tabs => self.tabs_key(key),
        }
    }

    fn servers_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?' | 'h') => self.mode = Mode::Help,
            KeyCode::F(3) | KeyCode::Char('/') => self.run(Action::Search),
            KeyCode::Char('n') => self.find_next(1),
            KeyCode::Char('N') => self.find_next(-1),
            KeyCode::F(4) | KeyCode::Char('\\') => self.run(Action::Filter),
            KeyCode::F(6) => self.run(Action::Sort),
            KeyCode::F(8) | KeyCode::Delete | KeyCode::Char('d') => self.run(Action::Remove),
            KeyCode::Insert | KeyCode::Char('a') => self.run(Action::Add),
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('<' | ',') => self.cycle_sort(-1),
            KeyCode::Char('>' | '.') => self.cycle_sort(1),
            KeyCode::Char('I') => self.run(Action::Invert),
            KeyCode::Char('P') => self.set_sort(SortKey::Players),
            KeyCode::Char('M') => self.set_sort(SortKey::Map),
            KeyCode::Char('T') => self.set_sort(SortKey::Ping),
            KeyCode::Char('e') => self.run(Action::HideEmpty),
            KeyCode::Char('y') => self.run(Action::CopyAddress),
            KeyCode::Char('c') => self.open_tab(Tab::Console),
            KeyCode::Char(c @ '1'..='5') => self.open_tab(TABS[c as usize - '1' as usize].0),
            KeyCode::Enter => {
                self.focus = Pane::Tabs;
                self.tab_opened();
            }
            KeyCode::Esc if !self.filter.is_empty() => {
                self.filter.clear();
                self.say("Filter cleared");
            }
            _ => {
                let n = self.visible().len();
                let page = usize::from(self.areas.servers.height.saturating_sub(3)).max(1);
                self.selected = move_in(self.selected, n, key.code, page);
            }
        }
    }

    fn players_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::F(4) | KeyCode::Char('\\' | '/') => {
                self.mode = Mode::Prompt {
                    kind: PromptKind::Filter(Pane::Players),
                    text: self.player_filter.clone(),
                }
            }
            KeyCode::F(6) => self.run(Action::Sort),
            KeyCode::Char('<' | ',') => self.cycle_sort(-1),
            KeyCode::Char('>' | '.') => self.cycle_sort(1),
            KeyCode::Char('I') => self.run(Action::Invert),
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc => self.player_filter.clear(),
            _ => {
                let n = self.current().map_or(0, |s| self.shown_players(s).len());
                let page = usize::from(self.areas.players.height.saturating_sub(3)).max(1);
                self.player_sel = move_in(self.player_sel, n, key.code, page);
            }
        }
    }

    fn rules_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::F(4) | KeyCode::Char('\\' | '/') => {
                self.mode = Mode::Prompt {
                    kind: PromptKind::Filter(Pane::Rules),
                    text: self.rule_filter.clone(),
                }
            }
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc => self.rule_filter.clear(),
            _ => {
                let n = self.current().map_or(0, |s| self.shown_rules(s).len());
                let page = usize::from(self.areas.rules.height.saturating_sub(3)).max(1);
                self.rule_sel = move_in(self.rule_sel, n, key.code, page);
            }
        }
    }

    fn tabs_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        // Ctrl+PgUp / Ctrl+PgDn or Shift+← / Shift+→ switch tabs, like in many terminal programs.
        if (ctrl && matches!(key.code, KeyCode::PageUp | KeyCode::PageDown))
            || (shift && matches!(key.code, KeyCode::Left | KeyCode::Right))
        {
            let i = TABS.iter().position(|(t, _)| *t == self.tab).unwrap_or(0) as isize;
            let step = if matches!(key.code, KeyCode::PageUp | KeyCode::Left) {
                -1
            } else {
                1
            };
            return self.open_tab(TABS[(i + step).rem_euclid(TABS.len() as isize) as usize].0);
        }
        if self.current().is_none() {
            return;
        }
        match self.tab {
            Tab::RconConfig => self.rcon_config_key(key),
            Tab::Console => self.console_key(key),
            Tab::Chat => self.chat_key(key),
            Tab::Bans => self.bans_key(key),
            Tab::ChangeLevel => self.maps_key(key),
        }
    }

    fn rcon_config_key(&mut self, key: KeyEvent) {
        let i = FIELDS.iter().position(|f| *f == self.field).unwrap_or(0);
        match key.code {
            KeyCode::Up => self.field = FIELDS[i.saturating_sub(1)],
            KeyCode::Down => self.field = FIELDS[(i + 1).min(FIELDS.len() - 1)],
            KeyCode::Enter | KeyCode::Char(' ') if self.field != Field::Password => {
                match self.field {
                    Field::Test => self.rcon_test(),
                    Field::Disconnect => {
                        if let Some(s) = self.current_mut()
                            && let Some(tx) = s.rcon.session.take()
                        {
                            let _ = tx.send(Request::Disconnect);
                            s.rcon.state = RconState::Disconnected;
                        }
                    }
                    Field::Hide => {
                        if let Some(s) = self.current_mut() {
                            s.hide_password = !s.hide_password;
                        }
                        self.save_rcon();
                    }
                    Field::Save => {
                        if let Some(s) = self.current_mut() {
                            s.save_password = !s.save_password;
                            if s.save_password {
                                self.say("The password is stored in plain text in the config file");
                            }
                        }
                        self.save_rcon();
                    }
                    Field::Password => {}
                }
            }
            KeyCode::Enter => self.rcon_test(),
            _ if self.field == Field::Password => {
                // Typing only edits the password: the server is asked with it on Test or
                // Enter, not on every key (failed RCON logins get the address banned).
                self.password_input.key(key);
            }
            KeyCode::Char('q') => self.quit = true,
            _ => {}
        }
    }

    fn console_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Enter => {
                let command = self.console_input.take().trim().to_string();
                self.history_pos = None;
                if command.is_empty() {
                    return;
                }
                if self.session().is_none() {
                    self.console_input.set(&command);
                    return;
                }
                if let Some(s) = self.current_mut() {
                    s.rcon.console_scroll = 0;
                    push_lines(
                        &mut s.rcon.console,
                        &mut s.rcon.console_scroll,
                        &format!("] {command}"),
                        LineKind::Command,
                    );
                }
                self.rcon_send(Tag::Console, &command);
                if self.history.last() != Some(&command) {
                    self.history.push(command);
                    let excess = self
                        .history
                        .len()
                        .saturating_sub(openhlsw::history::CONSOLE_HISTORY_MAX);
                    self.history.drain(..excess);
                }
            }
            KeyCode::Up | KeyCode::Down if !self.history.is_empty() => {
                let n = self.history.len();
                let pos = match (key.code, self.history_pos) {
                    (KeyCode::Up, None) => Some(n - 1),
                    (KeyCode::Up, Some(p)) => Some(p.saturating_sub(1)),
                    (_, Some(p)) if p + 1 < n => Some(p + 1),
                    _ => None,
                };
                self.history_pos = pos;
                let text = pos.map(|p| self.history[p].clone()).unwrap_or_default();
                self.console_input.set(&text);
            }
            KeyCode::Char('l') if ctrl => {
                if let Some(s) = self.current_mut() {
                    s.rcon.console.clear();
                }
            }
            KeyCode::PageUp | KeyCode::PageDown => {
                if let Some(s) = self.current_mut() {
                    let r = &mut s.rcon;
                    r.console_scroll = if key.code == KeyCode::PageUp {
                        (r.console_scroll + 10).min(r.console.len())
                    } else {
                        r.console_scroll.saturating_sub(10)
                    };
                }
            }
            _ => {
                self.console_input.key(key);
            }
        }
    }

    fn chat_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                let text = self.chat_input.take().trim().to_string();
                if text.is_empty() {
                    return;
                }
                if self.session().is_none() {
                    self.chat_input.set(&text);
                    return;
                }
                self.rcon_send(Tag::Silent, &format!("say {text}"));
                if let Some(s) = self.current_mut() {
                    s.rcon.chat_scroll = 0;
                    s.rcon.chat.push_back(ChatLine {
                        time: chrono::Local::now().format("%H:%M:%S").to_string(),
                        name: "Console".into(),
                        text: clean(&text),
                    });
                    if s.rcon.chat.len() > CONSOLE_LINES {
                        s.rcon.chat.pop_front();
                    }
                }
            }
            KeyCode::PageUp | KeyCode::PageDown => {
                if let Some(s) = self.current_mut() {
                    let r = &mut s.rcon;
                    r.chat_scroll = if key.code == KeyCode::PageUp {
                        (r.chat_scroll + 10).min(r.chat.len())
                    } else {
                        r.chat_scroll.saturating_sub(10)
                    };
                }
            }
            _ => {
                self.chat_input.key(key);
            }
        }
    }

    fn bans_key(&mut self, key: KeyEvent) {
        let n = self.current().map_or(0, |s| s.rcon.bans.len());
        match key.code {
            KeyCode::Char('r') | KeyCode::F(3) => self.fetch_bans(),
            KeyCode::Char('w') => {
                self.rcon_send(Tag::Silent, "writeid");
                self.rcon_send(Tag::Silent, "writeip");
                self.say("Ban lists saved on the server (writeid / writeip)");
            }
            KeyCode::Char('b') | KeyCode::Insert => {
                self.mode = Mode::Prompt {
                    kind: PromptKind::Ban,
                    text: String::new(),
                }
            }
            KeyCode::Char('u') | KeyCode::Delete | KeyCode::F(8) => {
                if let Some(s) = self.current()
                    && let Some(b) = s.rcon.bans.get(self.ban_sel)
                {
                    self.mode = Mode::Confirm(Confirm::Unban(s.id, b.clone()));
                }
            }
            KeyCode::Char('q') => self.quit = true,
            _ => self.ban_sel = move_in(self.ban_sel, n, key.code, 5),
        }
    }

    fn maps_key(&mut self, key: KeyEvent) {
        let n = self.current().map_or(0, |s| self.shown_maps(s).len());
        let cols = self.areas.map_columns.max(1);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('r') if ctrl => self.fetch_maps(),
            KeyCode::Enter => {
                if let Some(s) = self.current()
                    && let Some(map) = self.shown_maps(s).get(self.map_sel)
                {
                    self.mode = Mode::Confirm(Confirm::ChangeLevel(s.id, map.to_string()));
                }
            }
            KeyCode::Left => self.map_sel = self.map_sel.saturating_sub(1),
            KeyCode::Right => self.map_sel = (self.map_sel + 1).min(n.saturating_sub(1)),
            KeyCode::Up => self.map_sel = self.map_sel.saturating_sub(cols),
            KeyCode::Down => {
                if self.map_sel + cols < n {
                    self.map_sel += cols;
                }
            }
            KeyCode::PageUp => self.map_sel = self.map_sel.saturating_sub(cols * 4),
            KeyCode::PageDown => self.map_sel = (self.map_sel + cols * 4).min(n.saturating_sub(1)),
            _ => {
                // Everything else types into the search field (`*` and `?` work).
                if self.map_search.key(key) {
                    self.map_sel = 0;
                }
            }
        }
    }

    fn cycle_focus(&mut self, step: isize) {
        let i = PANES.iter().position(|p| *p == self.focus).unwrap_or(0) as isize;
        self.focus = PANES[(i + step).rem_euclid(PANES.len() as isize) as usize];
        if self.focus == Pane::Tabs {
            self.tab_opened();
        }
    }

    pub fn open_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.focus = Pane::Tabs;
        if tab == Tab::RconConfig && self.current().is_some_and(|s| s.password.is_none()) {
            self.field = Field::Password;
        }
        self.tab_opened();
    }

    fn refresh(&mut self) {
        if self.focus == Pane::Tabs && self.tab == Tab::Bans {
            return self.fetch_bans();
        }
        if self.focus == Pane::Tabs && self.tab == Tab::ChangeLevel {
            return self.fetch_maps();
        }
        if let Some(s) = self.current_mut() {
            s.details_at = None;
        }
        self.tick(true);
        self.say("Refreshing…");
    }

    pub fn run(&mut self, action: Action) {
        match action {
            Action::Quit => self.quit = true,
            Action::Add => {
                self.mode = Mode::Prompt {
                    kind: PromptKind::Add,
                    text: String::new(),
                }
            }
            Action::Remove => {
                if let Some(s) = self.current() {
                    self.mode = Mode::Confirm(Confirm::Remove(s.id));
                }
            }
            Action::Refresh => self.refresh(),
            Action::Search => {
                self.focus = Pane::Servers;
                self.mode = Mode::Prompt {
                    kind: PromptKind::Search,
                    text: String::new(),
                }
            }
            Action::Filter => {
                self.focus = Pane::Servers;
                self.mode = Mode::Prompt {
                    kind: PromptKind::Filter(Pane::Servers),
                    text: self.filter.clone(),
                }
            }
            Action::HideEmpty => {
                self.hide_empty = !self.hide_empty;
                self.say(if self.hide_empty {
                    "Empty servers hidden"
                } else {
                    "Empty servers shown"
                });
            }
            Action::Sort => {
                let at = if self.focus == Pane::Players {
                    PLAYER_SORT.iter().position(|(k, _)| *k == self.player_sort)
                } else {
                    self.focus = Pane::Servers;
                    SERVER_SORT.iter().position(|(k, _)| *k == self.sort)
                };
                self.mode = Mode::Sort(at.unwrap_or(0));
            }
            Action::Invert => {
                if self.focus == Pane::Players {
                    self.player_reverse = !self.player_reverse;
                } else {
                    self.reverse = !self.reverse;
                }
            }
            Action::CopyAddress => {
                if let Some(s) = self.current() {
                    let text = s.address.game_address();
                    self.say(format!("Copied {text}"));
                    self.clipboard = Some(text);
                }
            }
            Action::Tab(t) => self.open_tab(t),
            Action::Help => self.mode = Mode::Help,
            Action::About => self.mode = Mode::About,
        }
    }

    fn menu_key(&mut self, key: KeyEvent, menu: usize, item: usize) {
        let items = MENUS[menu].1.len();
        let next = match key.code {
            KeyCode::Left => Some(((menu + MENUS.len() - 1) % MENUS.len(), 0)),
            KeyCode::Right => Some(((menu + 1) % MENUS.len(), 0)),
            KeyCode::Up => Some((menu, item.saturating_sub(1))),
            KeyCode::Down => Some((menu, (item + 1).min(items - 1))),
            KeyCode::Enter => {
                return self.run(MENUS[menu].1[item].2);
            }
            KeyCode::Esc | KeyCode::F(9) | KeyCode::F(10) => None,
            // The first letter of a menu opens it (f, s, v …).
            KeyCode::Char(c) => MENUS
                .iter()
                .position(|(name, _)| name.to_lowercase().starts_with(c.to_ascii_lowercase()))
                .map(|m| (m, 0))
                .or(Some((menu, item))),
            _ => Some((menu, item)),
        };
        if let Some((menu, item)) = next {
            self.mode = Mode::Menu { menu, item };
        }
    }

    fn sorting_players(&self) -> bool {
        self.focus == Pane::Players
    }

    fn set_sort(&mut self, key: SortKey) {
        let id = self.selected_id;
        if self.sort == key {
            self.reverse = !self.reverse;
        } else {
            self.sort = key;
            self.reverse = false;
        }
        if let Some(id) = id {
            self.select_id(id);
        }
    }

    fn cycle_sort(&mut self, step: isize) {
        if self.sorting_players() {
            let i = PLAYER_SORT
                .iter()
                .position(|(k, _)| *k == self.player_sort)
                .unwrap_or(0);
            let n = PLAYER_SORT.len() as isize;
            self.player_sort = PLAYER_SORT[((i as isize + step).rem_euclid(n)) as usize].0;
        } else {
            let id = self.selected_id;
            let i = SERVER_SORT
                .iter()
                .position(|(k, _)| *k == self.sort)
                .unwrap_or(0);
            let n = SERVER_SORT.len() as isize;
            self.sort = SERVER_SORT[((i as isize + step).rem_euclid(n)) as usize].0;
            if let Some(id) = id {
                self.select_id(id);
            }
        }
    }

    fn sort_key(&mut self, key: KeyEvent, i: usize) {
        let players = self.sorting_players();
        let n = if players {
            PLAYER_SORT.len()
        } else {
            SERVER_SORT.len()
        };
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.mode = Mode::Sort(i.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => self.mode = Mode::Sort((i + 1).min(n - 1)),
            KeyCode::Enter => {
                if players {
                    self.player_sort = PLAYER_SORT[i].0;
                } else {
                    let id = self.selected_id;
                    self.sort = SERVER_SORT[i].0;
                    if let Some(id) = id {
                        self.select_id(id);
                    }
                }
            }
            KeyCode::Esc | KeyCode::F(6) | KeyCode::F(10) | KeyCode::Char('q') => {}
            _ => self.mode = Mode::Sort(i),
        }
    }

    fn prompt_key(&mut self, key: KeyEvent, kind: PromptKind, mut text: String) {
        match key.code {
            KeyCode::Esc => {
                match kind {
                    PromptKind::Filter(Pane::Players) => self.player_filter.clear(),
                    PromptKind::Filter(Pane::Rules) => self.rule_filter.clear(),
                    PromptKind::Filter(_) => self.filter.clear(),
                    _ => {}
                }
                return;
            }
            KeyCode::Enter => return self.finish_prompt(kind, text),
            KeyCode::F(3) if kind == PromptKind::Search => {
                self.search = text.clone();
                self.find_next(1);
            }
            KeyCode::Backspace => {
                text.pop();
            }
            KeyCode::Char(c) => text.push(c),
            _ => {}
        }
        // Incremental, like htop: the list follows while typing.
        match kind {
            PromptKind::Filter(Pane::Players) => {
                self.player_filter = text.clone();
                self.player_sel = 0;
            }
            PromptKind::Filter(Pane::Rules) => {
                self.rule_filter = text.clone();
                self.rule_sel = 0;
            }
            PromptKind::Filter(_) => {
                self.filter = text.clone();
                self.selected = 0;
            }
            PromptKind::Search if !text.is_empty() => {
                self.search = text.clone();
                self.find_from(self.selected, 1);
            }
            _ => {}
        }
        self.mode = Mode::Prompt { kind, text };
    }

    fn finish_prompt(&mut self, kind: PromptKind, text: String) {
        match kind {
            PromptKind::Search => self.search = text,
            PromptKind::Filter(Pane::Players) => self.player_filter = text,
            PromptKind::Filter(Pane::Rules) => self.rule_filter = text,
            PromptKind::Filter(_) => self.filter = text,
            PromptKind::Add => self.add(text.trim()),
            PromptKind::Ban => {
                let ident = text.trim().to_string();
                if !ident.is_empty() && !ident.contains(char::is_whitespace) {
                    self.mode = Mode::Prompt {
                        kind: PromptKind::BanMinutes(ident),
                        text: "0".into(),
                    };
                }
            }
            PromptKind::BanMinutes(ident) => {
                let Ok(minutes) = text.trim().parse::<u32>() else {
                    return self.say(format!("Not a number: {text}"));
                };
                let ip = ident.parse::<std::net::IpAddr>().is_ok();
                if ip {
                    self.rcon_send(Tag::Console, &format!("addip {minutes} {ident}"));
                    self.rcon_send(Tag::Silent, "writeip");
                } else {
                    self.rcon_send(Tag::Console, &format!("banid {minutes} {ident}"));
                    self.rcon_send(Tag::Silent, "writeid");
                }
                self.fetch_bans();
            }
        }
    }

    fn confirmed(&mut self, c: Confirm) {
        match c {
            Confirm::Remove(id) => self.delete(id),
            Confirm::ChangeLevel(id, map) => {
                if self.selected_id == Some(id) {
                    self.rcon_send(Tag::Console, &format!("changelevel {map}"));
                    self.say(format!("changelevel {map}"));
                }
            }
            Confirm::Unban(id, ban) => {
                if self.selected_id == Some(id) {
                    let (remove, write) = if ban.is_ip {
                        ("removeip", "writeip")
                    } else {
                        ("removeid", "writeid")
                    };
                    self.rcon_send(Tag::Console, &format!("{remove} {}", ban.ident));
                    self.rcon_send(Tag::Silent, write);
                    self.fetch_bans();
                }
            }
        }
    }

    fn find_from(&mut self, start: usize, step: isize) {
        if self.search.is_empty() {
            return;
        }
        let list = self.visible();
        let n = list.len() as isize;
        let found = (0..n)
            .map(|k| (start as isize + step * k).rem_euclid(n.max(1)) as usize)
            .find(|&i| list[i].matches(&self.search));
        match found {
            Some(i) => self.selected = i,
            None => self.say(format!("\"{}\" not found", self.search)),
        }
    }

    fn find_next(&mut self, step: isize) {
        let n = self.visible().len().max(1) as isize;
        let start = (self.selected as isize + step).rem_euclid(n) as usize;
        self.find_from(start, step);
    }

    fn add(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let Some(address) = ServerAddress::parse(text) else {
            self.say(format!("Not a server address: {text}"));
            return;
        };
        let address_text = address.to_string();
        if self.servers.iter().any(|s| s.address == address) {
            self.say(format!("{address_text} is already in the list"));
            return;
        }
        let saved = self.store.add_server(&address_text);
        let id = self.push_server(address, address_text.clone(), None, None, false);
        match saved {
            Ok(()) => self.say(format!("Added {address_text}")),
            Err(e) => {
                if let Some(s) = self.server_mut(id) {
                    s.temporary = true;
                }
                self.say(format!("Added {address_text}, but not saved: {e}"));
            }
        }
        self.filter.clear();
        self.focus = Pane::Servers;
        self.select_id(id);
        self.tick(true);
    }

    fn delete(&mut self, id: Id) {
        let Some(i) = self.servers.iter().position(|s| s.id == id) else {
            return;
        };
        let s = self.servers.remove(i);
        if !s.temporary
            && let Err(e) = self.store.remove_server(&s.address_text)
        {
            self.say(format!("Removed, but not saved: {e}"));
            return;
        }
        self.say(format!("Removed {}", s.address_text));
    }

    pub fn mouse(&mut self, m: MouseEvent) {
        let (x, y) = (m.column, m.row);
        if let Mode::Menu { .. } = self.mode {
            if let MouseEventKind::Down(MouseButton::Left) = m.kind {
                self.mode = Mode::Normal;
            }
            return;
        }
        if !matches!(self.mode, Mode::Normal) {
            return;
        }
        let a = &self.areas;
        let pane = if inside(a.servers, x, y) {
            Some(Pane::Servers)
        } else if inside(a.players, x, y) {
            Some(Pane::Players)
        } else if inside(a.rules, x, y) {
            Some(Pane::Rules)
        } else if inside(a.tabs, x, y) {
            Some(Pane::Tabs)
        } else {
            None
        };
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(i) = a.menu_labels.iter().position(|r| inside(*r, x, y)) {
                    self.mode = Mode::Menu { menu: i, item: 0 };
                    return;
                }
                if let Some((_, t)) = a.tab_labels.iter().find(|(r, _)| inside(*r, x, y)) {
                    let t = *t;
                    return self.open_tab(t);
                }
                let Some(pane) = pane else { return };
                self.focus = pane;
                // Rows start below the border and the column header.
                let row = |r: Rect, offset: usize| {
                    (y >= r.y + 2 && y + 1 < r.y + r.height)
                        .then(|| offset + usize::from(y - r.y - 2))
                };
                match pane {
                    Pane::Servers => {
                        if let Some(r) = row(a.servers, a.servers_offset)
                            && r < self.visible().len()
                        {
                            self.selected = r;
                        }
                    }
                    Pane::Players => {
                        if let Some(r) = row(a.players, a.players_offset) {
                            self.player_sel = r;
                        }
                    }
                    Pane::Rules => {
                        if let Some(r) = row(a.rules, a.rules_offset) {
                            self.rule_sel = r;
                        }
                    }
                    Pane::Tabs => self.tab_opened(),
                }
                self.selection_changed();
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let code = if m.kind == MouseEventKind::ScrollUp {
                    KeyCode::Up
                } else {
                    KeyCode::Down
                };
                let key = KeyEvent::new(code, KeyModifiers::NONE);
                match pane {
                    Some(Pane::Servers) => {
                        self.focus = Pane::Servers;
                        self.servers_key(key);
                    }
                    Some(Pane::Players) => self.players_key(key),
                    Some(Pane::Rules) => self.rules_key(key),
                    Some(Pane::Tabs) if self.tab == Tab::Bans => self.bans_key(key),
                    Some(Pane::Tabs) => {
                        let page = KeyEvent::new(
                            if code == KeyCode::Up {
                                KeyCode::PageUp
                            } else {
                                KeyCode::PageDown
                            },
                            KeyModifiers::NONE,
                        );
                        match self.tab {
                            Tab::Console => self.console_key(page),
                            Tab::Chat => self.chat_key(page),
                            Tab::ChangeLevel => self.maps_key(key),
                            _ => {}
                        }
                    }
                    None => {}
                }
                self.selection_changed();
            }
            _ => {}
        }
    }

    /// Writes the console history back to the config (at exit).
    pub fn save(&mut self) {
        let _ = self.store.save_history(&self.history);
    }
}

/// Moves a selection in a list of `n` rows with the usual keys.
fn move_in(sel: usize, n: usize, code: KeyCode, page: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let last = n - 1;
    match code {
        KeyCode::Up | KeyCode::Char('k') => sel.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => (sel + 1).min(last),
        KeyCode::PageUp => sel.saturating_sub(page),
        KeyCode::PageDown => (sel + page).min(last),
        KeyCode::Home | KeyCode::Char('g') => 0,
        KeyCode::End | KeyCode::Char('G') => last,
        _ => sel.min(last),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_text_cannot_reach_the_terminal_as_escape_sequence() {
        assert_eq!(clean("a\x1b[2Jb\tc\u{7}"), "a [2Jb    c ");
    }

    #[test]
    fn input_edits_at_the_cursor() {
        let mut i = Input::default();
        for c in "stats".chars() {
            i.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        i.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        i.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        i.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE));
        assert_eq!(i.text, "stauts");
        i.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!((i.text.as_str(), i.cursor), ("stats", 3));
    }

    #[test]
    fn moving_stays_in_the_list() {
        assert_eq!(move_in(0, 5, KeyCode::Up, 3), 0);
        assert_eq!(move_in(4, 5, KeyCode::Down, 3), 4);
        assert_eq!(move_in(1, 5, KeyCode::PageDown, 3), 4);
        assert_eq!(move_in(3, 0, KeyCode::Down, 3), 0);
    }
}
