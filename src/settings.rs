//! Program settings and the HLSW-style "Settings" dialog
//! (My Games, Startup, Connecting, Rcon, Connections, Plugins, Sounds, Misc).

use std::collections::BTreeMap;

use eframe::egui::{
    self, Button, Checkbox, DragValue, Label, RichText, Sense, TextEdit, Ui,
};
use egui_extras::Column;
use serde::{Deserialize, Serialize};

use crate::app::{EditMenu, fit, group_box, list_frame, plain_header, styled_table, tab_strip};
use crate::theme::{Palette, TextSize, Theme, UiFont};
use crate::typeahead::TypeAhead;

/// A game from HLSW's "My Games" list. Games with a Steam `app_id` are Source or GoldSrc
/// (Half-Life 1) games whose servers openHLSW can query today; the others are listed for their
/// executable path and get server support later.
pub struct GameDef {
    pub name: &'static str,
    pub app_id: Option<u32>,
}

impl GameDef {
    pub fn supported(&self) -> bool {
        self.app_id.is_some() || GAMESPY2_GAMES.contains(&self.name)
    }
}

/// Games queried with GameSpy 2 (entered as `ip:port:queryport`).
pub const GAMESPY2_GAMES: &[&str] = &["Deer Hunter 2005"];

pub const GAMES: &[GameDef] = &[
    GameDef {
        name: "Alien Swarm",
        app_id: Some(630),
    },
    GameDef {
        name: "Alien vs. Predator 2",
        app_id: None,
    },
    GameDef {
        name: "America's Army: Operations",
        app_id: None,
    },
    GameDef {
        name: "ARMA 2",
        app_id: None,
    },
    GameDef {
        name: "ARMA 2: Operation Arrowhead",
        app_id: None,
    },
    GameDef {
        name: "Armed Assault",
        app_id: None,
    },
    GameDef {
        name: "Battlefield 1942",
        app_id: None,
    },
    GameDef {
        name: "Battlefield 2",
        app_id: None,
    },
    GameDef {
        name: "Battlefield 2142",
        app_id: None,
    },
    GameDef {
        name: "Battlefield Vietnam",
        app_id: None,
    },
    GameDef {
        name: "Black Mesa",
        app_id: Some(362890),
    },
    GameDef {
        name: "Brink",
        app_id: None,
    },
    GameDef {
        name: "C&C Renegade",
        app_id: None,
    },
    GameDef {
        name: "Call of Duty",
        app_id: None,
    },
    GameDef {
        name: "Call of Duty 2",
        app_id: None,
    },
    GameDef {
        name: "Call of Duty 4",
        app_id: None,
    },
    GameDef {
        name: "Call of Duty: United Offensive",
        app_id: None,
    },
    GameDef {
        name: "Call of Duty: World at War",
        app_id: None,
    },
    GameDef {
        name: "Challenge Quake 3",
        app_id: None,
    },
    GameDef {
        name: "Counter-Strike 2",
        app_id: Some(730),
    },
    GameDef {
        name: "Counter-Strike: Source",
        app_id: Some(240),
    },
    GameDef {
        name: "Crysis",
        app_id: None,
    },
    GameDef {
        name: "Crysis Wars",
        app_id: None,
    },
    GameDef {
        name: "Day of Defeat: Source",
        app_id: Some(300),
    },
    GameDef {
        name: "Day of Infamy",
        app_id: Some(447820),
    },
    GameDef {
        name: "Deer Hunter 2005",
        app_id: None,
    },
    GameDef {
        name: "Devastation",
        app_id: None,
    },
    GameDef {
        name: "Doom 3",
        app_id: None,
    },
    GameDef {
        name: "Elite Force",
        app_id: None,
    },
    GameDef {
        name: "Elite Force II",
        app_id: None,
    },
    GameDef {
        name: "Enemy Territory: Quake Wars",
        app_id: None,
    },
    GameDef {
        name: "F.E.A.R.",
        app_id: None,
    },
    GameDef {
        name: "F.E.A.R. Perseus Mandate",
        app_id: None,
    },
    GameDef {
        name: "Far Cry",
        app_id: None,
    },
    GameDef {
        name: "Fistful of Frags",
        app_id: Some(265630),
    },
    GameDef {
        name: "Frontlines-Fuel of War",
        app_id: None,
    },
    GameDef {
        name: "Garry's Mod",
        app_id: Some(4000),
    },
    GameDef {
        name: "Half-Life",
        app_id: Some(70), // GoldSrc: CS 1.6, CZ, DoD, TFC, NS, TS, ... (all HL1 mods)
    },
    GameDef {
        name: "Half-Life (WON)",
        app_id: None,
    },
    GameDef {
        name: "Half-Life 2",
        app_id: Some(220),
    },
    GameDef {
        name: "Half-Life 2: Deathmatch",
        app_id: Some(320),
    },
    GameDef {
        name: "HALO",
        app_id: None,
    },
    GameDef {
        name: "Insurgency",
        app_id: Some(222880),
    },
    GameDef {
        name: "ioQuake3",
        app_id: None,
    },
    GameDef {
        name: "Jedi Knight 2",
        app_id: None,
    },
    GameDef {
        name: "Jedi Knight 3",
        app_id: None,
    },
    GameDef {
        name: "Killing Floor",
        app_id: None,
    },
    GameDef {
        name: "Land Of The Dead: Road to Fiddler's Green",
        app_id: None,
    },
    GameDef {
        name: "Left 4 Dead",
        app_id: Some(500),
    },
    GameDef {
        name: "Left 4 Dead 2",
        app_id: Some(550),
    },
    GameDef {
        name: "Medal of Honor Allied Assault",
        app_id: None,
    },
    GameDef {
        name: "Medal of Honor Pacific Assault",
        app_id: None,
    },
    GameDef {
        name: "Medal of Honor: Allied Assault Breakthrough",
        app_id: None,
    },
    GameDef {
        name: "Medal of Honor: Allied Assault Spearhead",
        app_id: None,
    },
    GameDef {
        name: "Neverwinter Nights",
        app_id: None,
    },
    GameDef {
        name: "Nexuiz",
        app_id: None,
    },
    GameDef {
        name: "No More Room in Hell",
        app_id: Some(224260),
    },
    GameDef {
        name: "Nuclear Dawn",
        app_id: Some(17710),
    },
    GameDef {
        name: "Operation Flashpoint",
        app_id: None,
    },
    GameDef {
        name: "Operation Flashpoint Resistance",
        app_id: None,
    },
    GameDef {
        name: "Painkiller",
        app_id: None,
    },
    GameDef {
        name: "Pariah",
        app_id: None,
    },
    GameDef {
        name: "Postal 2",
        app_id: None,
    },
    GameDef {
        name: "Prey",
        app_id: None,
    },
    GameDef {
        name: "Project IGI2: Covert Strike",
        app_id: None,
    },
    GameDef {
        name: "Quake 1",
        app_id: None,
    },
    GameDef {
        name: "Quake 2",
        app_id: None,
    },
    GameDef {
        name: "Quake 3 Arena",
        app_id: None,
    },
    GameDef {
        name: "Quake 4",
        app_id: None,
    },
    GameDef {
        name: "Red Orchestra 2: Heroes of Stalingrad",
        app_id: None,
    },
    GameDef {
        name: "Red Orchestra: Ostfront 41-45",
        app_id: None,
    },
    GameDef {
        name: "Return to Castle Wolfenstein",
        app_id: None,
    },
    GameDef {
        name: "Rune",
        app_id: None,
    },
    GameDef {
        name: "S.T.A.L.K.E.R. - Shadow of Chernobyl",
        app_id: None,
    },
    GameDef {
        name: "Savage: The Battle For Newerth",
        app_id: None,
    },
    GameDef {
        name: "Serious Sam: The First Encounter",
        app_id: None,
    },
    GameDef {
        name: "Serious Sam: The Second Encounter",
        app_id: None,
    },
    GameDef {
        name: "Shockvoice (v0.9.5+)",
        app_id: None,
    },
    GameDef {
        name: "Soldier of Fortune 2",
        app_id: None,
    },
    GameDef {
        name: "Star Wars: Battlefront",
        app_id: None,
    },
    GameDef {
        name: "SWAT 4",
        app_id: None,
    },
    GameDef {
        name: "SWAT 4: The Stetchkov Syndicate",
        app_id: None,
    },
    GameDef {
        name: "Synergy",
        app_id: Some(17520),
    },
    GameDef {
        name: "Team Fortress 2",
        app_id: Some(440),
    },
    GameDef {
        name: "TeamSpeak 2",
        app_id: None,
    },
    GameDef {
        name: "TeamSpeak 3",
        app_id: None,
    },
    GameDef {
        name: "Tremulous",
        app_id: None,
    },
    GameDef {
        name: "Tribes 2",
        app_id: None,
    },
    GameDef {
        name: "Tribes Vengeance",
        app_id: None,
    },
    GameDef {
        name: "Unreal Tournament",
        app_id: None,
    },
    GameDef {
        name: "Unreal Tournament 2003",
        app_id: None,
    },
    GameDef {
        name: "Unreal Tournament 2004",
        app_id: None,
    },
    GameDef {
        name: "Unreal Tournament 3",
        app_id: None,
    },
    GameDef {
        name: "Urban Terror",
        app_id: None,
    },
    GameDef {
        name: "Ventrilo (v2.3.0+)",
        app_id: None,
    },
    GameDef {
        name: "Warsow",
        app_id: None,
    },
    GameDef {
        name: "Wolfenstein - Enemy Territory",
        app_id: None,
    },
    GameDef {
        name: "Wolfenstein 2009",
        app_id: None,
    },
    GameDef {
        name: "Zombie Panic! Source",
        app_id: Some(17500),
    },
];

pub fn game_by_name(name: &str) -> Option<&'static GameDef> {
    GAMES.iter().find(|g| g.name == name)
}

pub fn game_by_app_id(app_id: u32) -> Option<&'static GameDef> {
    GAMES.iter().find(|g| g.app_id == Some(app_id))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupSelection {
    #[default]
    None,
    LastUsed,
    This,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnConnect {
    #[default]
    Nothing,
    Minimize,
    /// Minimize and pause all server queries while minimized.
    MinimizeNoTraffic,
    /// Close openHLSW after starting the game.
    Quit,
}

impl OnConnect {
    fn label(self) -> &'static str {
        match self {
            OnConnect::Nothing => tr!("Do nothing"),
            OnConnect::Minimize => tr!("Minimize openHLSW"),
            OnConnect::MinimizeNoTraffic => tr!("Minimize openHLSW (no traffic)"),
            OnConnect::Quit => tr!("Quit openHLSW"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecApp {
    pub enabled: bool,
    /// Command line; `%addr%`, `%ip%`, `%port%` are replaced with the server address.
    pub command: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundEvent {
    StartUp,
    IncomingMessage,
    SendingMessage,
    PlayerJoin,
    Notification,
}

impl SoundEvent {
    pub const ALL: [SoundEvent; 5] = [
        SoundEvent::StartUp,
        SoundEvent::IncomingMessage,
        SoundEvent::SendingMessage,
        SoundEvent::PlayerJoin,
        SoundEvent::Notification,
    ];
    fn label(self) -> &'static str {
        match self {
            SoundEvent::StartUp => tr!("StartUp"),
            SoundEvent::IncomingMessage => tr!("Incoming Message"),
            SoundEvent::SendingMessage => tr!("Sending Message"),
            SoundEvent::PlayerJoin => tr!("Player Join"),
            SoundEvent::Notification => tr!("Notification"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sound {
    pub event: SoundEvent,
    pub enabled: bool,
    /// Path to a .wav file.
    pub file: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // My Games: game name -> game executable
    pub game_paths: BTreeMap<String, String>,
    /// My Games: game name -> extra start parameters (e.g. "-novid -console").
    pub game_params: BTreeMap<String, String>,
    /// Player name passed to the game on Connect (`+name`), View → Player Name.
    pub player_name: String,
    /// Player names used before (the Player name dropdown, newest first).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub player_names: Vec<String>,
    /// Server list filter (View → Filter, "Edit filter...").
    pub filter: crate::filter::ServerFilter,
    /// Chat messages starting with "@" play the notification sound and are shown in the
    /// status bar (Rcon Config → Enable Admin Notification).
    pub admin_notification: bool,
    /// Players with a Steam ID are added to the Global Player ID Database with their names.
    pub iddb_auto_add: bool,
    // Startup
    pub startup: StartupSelection,
    pub startup_address: String,
    // Connecting
    pub on_connect: OnConnect,
    pub connect_apps: Vec<ExecApp>,
    // Rcon
    pub console_cache: usize,
    pub save_server_log: bool,
    pub save_chat_log: bool,
    pub autocomplete: bool,
    pub log_on_map_change: bool,
    pub kick_all_on_map_change: bool,
    // Connections
    pub log_port: u16,
    pub public_ip: String,
    // Sounds
    pub sounds_disabled: bool,
    pub sounds: Vec<Sound>,
    // Misc
    pub auto_refresh: bool,
    /// Seconds between refreshes of the server list.
    pub refresh_interval: f32,
    /// Query budget per server (srcds answers ~3 per second per IP, see sv_max_queries_sec).
    pub max_queries_per_sec: f32,
    pub keep_order_on_hover: bool,
    /// Look up country / provider of server IPs at ip-api.com.
    pub geo_lookup: bool,
    pub theme: Theme,
    pub text_size: TextSize,
    /// Font of the user interface.
    pub font: UiFont,
    /// Sub-pixel ("ClearType") or gray anti-aliasing of text.
    pub text_smoothing: crate::theme::TextSmoothing,
    /// User interface language.
    pub language: crate::i18n::Language,
    /// Look settings already updated to the current defaults (missing in old configs = 0:
    /// they get the default look once, see [`APPEARANCE`]).
    #[serde(default)]
    pub appearance: u32,
    /// Bumped when a default changes, so old configs are migrated once (1 = refresh 1 s).
    /// Missing in old configs = 0 (not the `Default` value), so they get migrated.
    #[serde(default)]
    pub defaults_revision: u32,
    pub connect_on_double_click: bool,
    /// View menu toggles.
    pub view: ViewOptions,
    /// Switches on the left of the console (C * F D E A).
    pub console: ConsoleOptions,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConsoleOptions {
    /// C: show log lines in readable form.
    pub translate_log: bool,
    /// *: mask the rcon password in the console.
    pub hide_rcon_password: bool,
    /// F: hide the server's answers to commands.
    pub hide_feedback: bool,
    /// D: show the date next to the time.
    pub show_date: bool,
    /// E: show empty lines.
    pub show_empty: bool,
    /// A: hide commands sent automatically (Auto Update Status).
    pub hide_automatic: bool,
}

impl Default for ConsoleOptions {
    fn default() -> Self {
        Self {
            translate_log: false,
            hide_rcon_password: true,
            hide_feedback: false,
            show_date: false,
            show_empty: false,
            hide_automatic: true,
        }
    }
}

/// What the View menu shows or hides.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewOptions {
    pub rcon_section: bool,
    pub server_list: bool,
    pub rules: bool,
    pub password: bool,
    /// Player name field in the toolbar.
    pub player_name: bool,
    pub filter: bool,
    pub comment: bool,
    pub toolbar: bool,
    pub status_bar: bool,
    pub ping_graph: bool,
    /// Columns switched off in the header's right-click menu (column titles).
    pub hidden_server_columns: Vec<String>,
    /// Visible columns of the players list (titles, in any order).
    pub player_columns: Vec<String>,
    /// Bumped by "Reset column width"; part of the table id, so widths start fresh.
    pub server_columns_reset: u32,
    pub player_columns_reset: u32,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            rcon_section: true,
            server_list: true,
            rules: true,
            password: true,
            player_name: false,
            filter: true,
            comment: true,
            toolbar: true,
            status_bar: true,
            ping_graph: true,
            hidden_server_columns: Vec::new(),
            player_columns: [
                "Country",
                "#",
                "Name",
                "Frags",
                "Time",
                "Ping",
                "Loss",
                "Global ID",
                "Addr",
            ]
            .iter()
            .map(|c| c.to_string())
            .collect(),
            server_columns_reset: 0,
            player_columns_reset: 0,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            game_paths: BTreeMap::new(),
            game_params: BTreeMap::new(),
            player_name: String::new(),
            player_names: Vec::new(),
            filter: crate::filter::ServerFilter::default(),
            admin_notification: false,
            iddb_auto_add: true,
            startup: StartupSelection::LastUsed,
            startup_address: String::new(),
            on_connect: OnConnect::Nothing,
            connect_apps: Vec::new(),
            console_cache: 1000,
            save_server_log: false,
            save_chat_log: false,
            autocomplete: true,
            log_on_map_change: true,
            kick_all_on_map_change: false,
            log_port: 27500,
            public_ip: String::new(),
            sounds_disabled: true,
            sounds: SoundEvent::ALL
                .iter()
                .map(|&event| Sound {
                    event,
                    enabled: true,
                    file: String::new(),
                })
                .collect(),
            auto_refresh: true,
            refresh_interval: 1.0,
            max_queries_per_sec: 2.5,
            keep_order_on_hover: false,
            geo_lookup: true,
            theme: Theme::Light,
            text_size: TextSize::default(),
            font: UiFont::default(),
            text_smoothing: Default::default(),
            language: crate::i18n::Language::System,
            appearance: APPEARANCE,
            defaults_revision: 1,
            connect_on_double_click: true,
            view: ViewOptions::default(),
            console: ConsoleOptions::default(),
        }
    }
}

impl Settings {
    pub fn sound_file(&self, event: SoundEvent) -> Option<&str> {
        if self.sounds_disabled {
            return None;
        }
        self.sounds
            .iter()
            .find(|s| s.event == event && s.enabled && !s.file.trim().is_empty())
            .map(|s| s.file.trim())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    MyGames,
    Startup,
    Connecting,
    Rcon,
    Connections,
    Plugins,
    Sounds,
    Misc,
}

const TABS: [(&str, Option<SettingsTab>); 8] = [
    ("My Games", Some(SettingsTab::MyGames)),
    ("Startup", Some(SettingsTab::Startup)),
    ("Connecting", Some(SettingsTab::Connecting)),
    ("Rcon", Some(SettingsTab::Rcon)),
    ("Connections", Some(SettingsTab::Connections)),
    ("Plugins", Some(SettingsTab::Plugins)),
    ("Sounds", Some(SettingsTab::Sounds)),
    ("Misc", Some(SettingsTab::Misc)),
];

/// Start parameter presets of HLSW's "Steam" parameter list (My Games → right click).
/// Placeholders like `<RCONPASSWORD>` are filled in on Connect.
const PARAMETER_PRESETS: &[(&str, &str, &str)] = &[
    ("-console", "-console", "Enables the console"),
    ("-novid", "-novid", "Disable intro video"),
    ("-windowed", "-windowed", "Start the game in a window"),
    ("-fullscreen", "-fullscreen", "Start the game in fullscreen"),
    ("-d3d", "-d3d", "Direct3D"),
    ("-gl", "-gl", "OpenGL"),
    (
        "-noforcemaccel",
        "-noforcemaccel",
        "Disable the Windows mouse acceleration",
    ),
    (
        "-noforcemparms",
        "-noforcemparms",
        "Disable the Windows mouse acceleration",
    ),
    (
        "-noforcemspd",
        "-noforcemspd",
        "Disable the Windows mouse acceleration",
    ),
    ("-preload", "-preload", "Files will be cached at startup"),
    ("-noaafonts", "-noaafonts", "Fonts without antialiasing"),
    ("Set rcon_password", "+rcon_password <RCONPASSWORD>", ""),
    ("Set rcon_address", "+rcon_address <SERVERIP>", ""),
    ("Set rcon_port", "+rcon_port <SERVERPORT>", ""),
    (
        "Set rate to max",
        "+rate <RULE(sv_maxrate)>",
        "Set rate to sv_maxrate",
    ),
];

/// Additional presets of HLSW's "HL" (GoldSrc) parameter list.
const GOLDSRC_PRESETS: &[(&str, &str, &str)] = &[
    ("-zone 4096", "-zone 4096", ""),
    ("-nojoy", "-nojoy", ""),
    ("-noipx", "-noipx", "Deactivates IPX protocol"),
    ("-freq 100", "-freq 100", ""),
];

/// Current revision of the look defaults, see [`Settings::appearance`].
/// 1: Tahoma 12 px, 2: Arial, 3: system font at 13 px.
pub const APPEARANCE: u32 = 3;

/// State of an open settings dialog: a draft that is applied on OK / Apply.
pub struct SettingsDialog {
    pub draft: Settings,
    pub tab: SettingsTab,
    /// Game whose file (false) or parameters (true) are being edited.
    editing_game: Option<(&'static str, bool)>,
    selected_app: Option<usize>,
    port_test: Option<String>,
}

pub enum DialogResult {
    Open,
    Apply(Settings),
    Ok(Settings),
    Cancel,
}

impl SettingsDialog {
    pub fn new(settings: &Settings, tab: SettingsTab) -> Self {
        Self {
            draft: settings.clone(),
            tab,
            editing_game: None,
            selected_app: None,
            port_test: None,
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        p: &Palette,
        current: &Settings,
        server_list: &[String],
        icons: Option<&crate::game_icons::GameIcons>,
    ) -> DialogResult {
        let mut result = DialogResult::Open;
        let mut open = true;
        crate::native_window::NativeWindow::new("settings", tr!("Settings"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size([640.0, 500.0])
            .show(ctx, |ui| {
                if let Some(tab) = tab_strip(ui, p, &TABS, self.tab) {
                    self.tab = tab;
                }
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .id_salt("settings_scroll")
                    .max_height(430.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.tab {
                        SettingsTab::MyGames => self.my_games(ui, p, icons),
                        SettingsTab::Startup => self.startup(ui, p, server_list),
                        SettingsTab::Connecting => self.connecting(ui, p),
                        SettingsTab::Rcon => self.rcon(ui, p),
                        SettingsTab::Connections => self.connections(ui, p),
                        SettingsTab::Plugins => plugins(ui, p),
                        SettingsTab::Sounds => self.sounds(ui, p),
                        SettingsTab::Misc => self.misc(ui, p),
                    });
                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let changed = self.draft != *current;
                    if ui
                        .add_enabled(
                            changed,
                            Button::new(tr!("Apply")).min_size(egui::vec2(75.0, 22.0)),
                        )
                        .clicked()
                    {
                        result = DialogResult::Apply(self.draft.clone());
                    }
                    if ui
                        .add(Button::new(tr!("Cancel")).min_size(egui::vec2(75.0, 22.0)))
                        .clicked()
                    {
                        result = DialogResult::Cancel;
                    }
                    if ui
                        .add(
                            Button::new(RichText::new(tr!("OK")).strong())
                                .min_size(egui::vec2(75.0, 22.0)),
                        )
                        .clicked()
                    {
                        result = DialogResult::Ok(self.draft.clone());
                    }
                });
            });
        if !open {
            result = DialogResult::Cancel;
        }
        result
    }

    fn my_games(&mut self, ui: &mut Ui, p: &Palette, icons: Option<&crate::game_icons::GameIcons>) {
        let paths = &mut self.draft.game_paths;
        let params = &mut self.draft.game_params;
        let editing = &mut self.editing_game;
        ui.allocate_ui(egui::vec2(ui.available_width(), 380.0), |ui| {
            list_frame(ui, p, |ui| {
                styled_table(ui, "my_games")
                    .sense(Sense::click())
                    .column(Column::initial(230.0).at_least(120.0).clip(true))
                    .column(Column::initial(80.0))
                    .column(Column::initial(180.0).at_least(60.0).clip(true))
                    .column(Column::remainder().clip(true))
                    .header(18.0, |mut h| {
                        for title in [
                            tr!("Game"),
                            tr!("Server query"),
                            tr!("File"),
                            tr!("Parameters"),
                        ] {
                            h.col(|ui| {
                                plain_header(ui, p, title);
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(17.0, GAMES.len(), |mut row| {
                            let game = &GAMES[row.index()];
                            row.col(|ui| {
                                if let Some(icons) = icons {
                                    icons.show(
                                        ui,
                                        crate::game_icons::icon_for_game(game.name),
                                        14.0,
                                    );
                                }
                                ui.add(Label::new(game.name).selectable(false));
                            });
                            row.col(|ui| {
                                let t = if game.supported() {
                                    RichText::new(tr!("Yes")).color(p.green)
                                } else {
                                    RichText::new(tr!("not yet")).color(p.weak_text)
                                };
                                ui.add(Label::new(t).selectable(false));
                            });
                            let mut double_clicked = None;
                            for (is_params, map, hint) in [
                                (
                                    false,
                                    &mut *paths,
                                    tr!("path to the game .exe, empty = Steam"),
                                ),
                                (true, &mut *params, tr!("e.g. -novid -console")),
                            ] {
                                let (_, resp) = row.col(|ui| {
                                    if *editing == Some((game.name, is_params)) {
                                        let value = map.entry(game.name.to_string()).or_default();
                                        let r = ui
                                            .add(
                                                TextEdit::singleline(value)
                                                    .hint_text(hint)
                                                    .desired_width(f32::INFINITY),
                                            )
                                            .with_edit_menu();
                                        r.request_focus();
                                        if r.lost_focus() {
                                            *editing = None;
                                        }
                                    } else {
                                        let value = map.get(game.name).cloned().unwrap_or_default();
                                        ui.add(
                                            Label::new(value).selectable(false).wrap_mode(fit(ui)),
                                        );
                                    }
                                });
                                if resp.double_clicked() {
                                    double_clicked = Some(is_params);
                                }
                            }
                            row.response().context_menu(|ui| {
                                ui.menu_button(tr!("Add parameter"), |ui| {
                                    let goldsrc =
                                        game.app_id.is_some_and(openhlsw::a2s::is_goldsrc_app);
                                    let extra: &[_] = if goldsrc { GOLDSRC_PRESETS } else { &[] };
                                    for (label, value, desc) in
                                        PARAMETER_PRESETS.iter().chain(extra)
                                    {
                                        let r = ui.button(*label);
                                        let r = if desc.is_empty() {
                                            r
                                        } else {
                                            r.on_hover_text(crate::i18n::t(desc))
                                        };
                                        if r.clicked() {
                                            let line =
                                                params.entry(game.name.to_string()).or_default();
                                            if !line.split_whitespace().any(|w| {
                                                w == value.split(' ').next().unwrap_or_default()
                                            }) {
                                                if !line.is_empty() {
                                                    line.push(' ');
                                                }
                                                line.push_str(value);
                                            }
                                            ui.close();
                                        }
                                    }
                                });
                                if ui.button(tr!("Clear parameters")).clicked() {
                                    params.remove(game.name);
                                    ui.close();
                                }
                            });
                            if let Some(is_params) = double_clicked {
                                *editing = Some((game.name, is_params));
                            } else if row.response().double_clicked() {
                                *editing = Some((game.name, false));
                            }
                        });
                    });
            });
        });
        paths.retain(|_, v| !v.trim().is_empty());
        params.retain(|_, v| !v.trim().is_empty());
        ui.vertical_centered(|ui| {
            ui.label(tr!(
                "(Double click to set the game executable or start parameters. Empty file = connect via Steam.)"
            ));
        });
    }

    fn startup(&mut self, ui: &mut Ui, p: &Palette, server_list: &[String]) {
        let d = &mut self.draft;
        group_box(
            ui,
            p,
            tr!("Selected server/server list on startup"),
            ui.available_width() - 20.0,
            |ui| {
                ui.radio_value(&mut d.startup, StartupSelection::None, tr!("None"));
                ui.radio_value(
                    &mut d.startup,
                    StartupSelection::LastUsed,
                    tr!("Last used server/server list"),
                );
                ui.radio_value(
                    &mut d.startup,
                    StartupSelection::This,
                    tr!("This server/server list:"),
                );
                ui.add_enabled_ui(d.startup == StartupSelection::This, |ui| {
                    egui::Grid::new("startup_grid")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label(tr!("Server list:"));
                            egui::ComboBox::from_id_salt("startup_list")
                                .selected_text(tr!("Custom server list"))
                                .width(260.0)
                                .show_ui(ui, |ui| {
                                    let mut find = TypeAhead::begin(ui);
                                    let r = ui.selectable_label(true, tr!("Custom server list"));
                                    find.item(&r, tr!("Custom server list"));
                                    find.finish(ui);
                                });
                            ui.end_row();
                            ui.label(tr!("IP/Port:"));
                            egui::ComboBox::from_id_salt("startup_addr")
                                .selected_text(if d.startup_address.is_empty() {
                                    "-"
                                } else {
                                    d.startup_address.as_str()
                                })
                                .width(260.0)
                                .height(popup_height(ui))
                                .show_ui(ui, |ui| {
                                    let mut find = TypeAhead::begin(ui);
                                    for addr in server_list {
                                        let r = ui.selectable_value(
                                            &mut d.startup_address,
                                            addr.clone(),
                                            addr,
                                        );
                                        find.item(&r, addr);
                                    }
                                    find.finish(ui);
                                });
                            ui.end_row();
                        });
                });
            },
        );
    }

    fn connecting(&mut self, ui: &mut Ui, p: &Palette) {
        let d = &mut self.draft;
        ui.horizontal(|ui| {
            ui.label(tr!("On Connection:"));
            egui::ComboBox::from_id_salt("on_connect")
                .selected_text(d.on_connect.label())
                .width(250.0)
                .show_ui(ui, |ui| {
                    let mut find = TypeAhead::begin(ui);
                    for v in [
                        OnConnect::Nothing,
                        OnConnect::Minimize,
                        OnConnect::MinimizeNoTraffic,
                        OnConnect::Quit,
                    ] {
                        let r = ui.selectable_value(&mut d.on_connect, v, v.label());
                        find.item(&r, v.label());
                    }
                    find.finish(ui);
                });
        });
        ui.add_space(8.0);
        let selected = &mut self.selected_app;
        group_box(
            ui,
            p,
            tr!("Execute applications"),
            ui.available_width() - 20.0,
            |ui| {
                ui.allocate_ui(egui::vec2(ui.available_width(), 180.0), |ui| {
                    list_frame(ui, p, |ui| {
                        styled_table(ui, "exec_apps")
                            .sense(Sense::click())
                            .column(Column::exact(40.0))
                            .column(Column::remainder().clip(true))
                            .header(18.0, |mut h| {
                                h.col(|ui| {
                                    plain_header(ui, p, tr!("Flags"));
                                });
                                h.col(|ui| {
                                    plain_header(ui, p, tr!("File + Parameter"));
                                });
                            })
                            .body(|body| {
                                body.rows(20.0, d.connect_apps.len(), |mut row| {
                                    let i = row.index();
                                    row.set_selected(*selected == Some(i));
                                    let app = &mut d.connect_apps[i];
                                    row.col(|ui| {
                                        ui.checkbox(&mut app.enabled, "");
                                    });
                                    row.col(|ui| {
                                        let r = ui
                                            .add(
                                                TextEdit::singleline(&mut app.command)
                                                    .desired_width(f32::INFINITY),
                                            )
                                            .with_edit_menu();
                                        if r.gained_focus() {
                                            *selected = Some(i);
                                        }
                                    });
                                    if row.response().clicked() {
                                        *selected = Some(i);
                                    }
                                });
                            });
                    });
                });
                ui.horizontal(|ui| {
                    if ui
                        .add(Button::new(tr!("Add...")).min_size(egui::vec2(75.0, 20.0)))
                        .clicked()
                    {
                        d.connect_apps.push(ExecApp {
                            enabled: true,
                            command: String::new(),
                        });
                        *selected = Some(d.connect_apps.len() - 1);
                    }
                    if ui
                        .add_enabled(
                            selected.is_some(),
                            Button::new(tr!("Delete")).min_size(egui::vec2(75.0, 20.0)),
                        )
                        .clicked()
                        && let Some(i) = selected.take()
                        && i < d.connect_apps.len()
                    {
                        d.connect_apps.remove(i);
                    }
                });
                ui.label(RichText::new(tr!("Started when you connect to a server. %addr%, %ip% and %port% are replaced.")).small());
            },
        );
    }

    fn rcon(&mut self, ui: &mut Ui, p: &Palette) {
        let d = &mut self.draft;
        group_box(ui, p, tr!("Rcon"), ui.available_width() - 20.0, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(180.0);
                ui.label(tr!("Cache size:"));
                ui.add(DragValue::new(&mut d.console_cache).range(100..=100_000));
                ui.label(tr!("entries."));
            });
            ui.add_space(6.0);
            let logs = crate::config::path()
                .and_then(|p| p.parent().map(|d| d.join("logs").display().to_string()))
                .unwrap_or_else(|| "logs".into());
            ui.checkbox(
                &mut d.save_server_log,
                tr!(
                    "Save Server Log ({path})",
                    path = format!(
                        "{logs}{}<SERVERADDR>{}*.log",
                        std::path::MAIN_SEPARATOR,
                        std::path::MAIN_SEPARATOR
                    )
                ),
            );
            ui.checkbox(&mut d.save_chat_log, tr!("Save Server Chat (*.chat.log)"));
            ui.checkbox(
                &mut d.autocomplete,
                tr!("Autocomplete at rcon console (Tab)"),
            );
            ui.checkbox(&mut d.log_on_map_change, tr!("Retrieve log on map change"));
        });
    }

    fn connections(&mut self, ui: &mut Ui, p: &Palette) {
        let d = &mut self.draft;
        let port_test = &mut self.port_test;
        group_box(
            ui,
            p,
            tr!("Connection Settings"),
            ui.available_width() - 20.0,
            |ui| {
                ui.horizontal(|ui| {
                ui.label(tr!("UDP Port:"));
                ui.add(DragValue::new(&mut d.log_port).range(1024..=65535));
                if ui.add(Button::new(tr!("Port test")).min_size(egui::vec2(110.0, 20.0))).clicked() {
                    *port_test = Some(match std::net::UdpSocket::bind(("0.0.0.0", d.log_port)) {
                        Ok(_) => tr!("UDP port {log_port} is free on this computer.", log_port = d.log_port),
                        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                            tr!("UDP port {log_port} is in use (openHLSW may already be listening on it).", log_port = d.log_port)
                        }
                        Err(e) => tr!("UDP port {log_port}: {e}", log_port = d.log_port, e = e),
                    });
                }
            });
                ui.label(tr!("Port the server log is received on (Get Log). Must be forwarded for Get Log (Internet)."));
                if let Some(msg) = port_test.as_ref() {
                    ui.label(RichText::new(msg).color(p.link));
                }
            },
        );
        group_box(ui, p, tr!("Public IP"), ui.available_width() - 20.0, |ui| {
            ui.horizontal(|ui| {
                ui.label(tr!("Public IP:"));
                ui.add(
                    TextEdit::singleline(&mut d.public_ip)
                        .hint_text(tr!("auto-detect"))
                        .desired_width(160.0),
                )
                .with_edit_menu();
            });
            ui.label(tr!(
                "Used by Get Log (Internet). Empty = ask api.ipify.org."
            ));
        });
    }

    fn sounds(&mut self, ui: &mut Ui, p: &Palette) {
        let d = &mut self.draft;
        group_box(ui, p, tr!("Sounds"), ui.available_width() - 20.0, |ui| {
            ui.checkbox(&mut d.sounds_disabled, tr!("Disable sounds"));
            ui.add_space(4.0);
            ui.add_enabled_ui(!d.sounds_disabled, |ui| {
                ui.allocate_ui(egui::vec2(ui.available_width(), 220.0), |ui| {
                    list_frame(ui, p, |ui| {
                        styled_table(ui, "sounds")
                            .column(Column::initial(160.0))
                            .column(Column::remainder().clip(true))
                            .header(18.0, |mut h| {
                                h.col(|ui| {
                                    plain_header(ui, p, tr!("Event"));
                                });
                                h.col(|ui| {
                                    plain_header(ui, p, tr!("Sound Filename (.wav)"));
                                });
                            })
                            .body(|body| {
                                body.rows(20.0, d.sounds.len(), |mut row| {
                                    let s = &mut d.sounds[row.index()];
                                    row.col(|ui| {
                                        ui.add(Checkbox::new(&mut s.enabled, s.event.label()));
                                    });
                                    row.col(|ui| {
                                        ui.add(
                                            TextEdit::singleline(&mut s.file)
                                                .hint_text(tr!("path to a .wav file"))
                                                .desired_width(f32::INFINITY),
                                        )
                                        .with_edit_menu();
                                    });
                                });
                            });
                    });
                });
            });
        });
    }

    /// Misc, in HLSW's order: Server list, Language, Skin, then openHLSW's own settings.
    fn misc(&mut self, ui: &mut Ui, p: &Palette) {
        let d = &mut self.draft;
        let width = ui.available_width() - 20.0;
        let indent = 22.0;
        group_box(ui, p, tr!("Server list"), width, |ui| {
            ui.checkbox(
                &mut d.auto_refresh,
                tr!("Auto update all servers in server list"),
            );
            ui.add_enabled_ui(d.auto_refresh, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(indent);
                    ui.label(tr!("Update interval"));
                    ui.add(
                        DragValue::new(&mut d.refresh_interval)
                            .range(
                                (1.0 / d.max_queries_per_sec.max(0.1)).clamp(0.1, 10.0)..=3600.0,
                            )
                            .speed(0.05)
                            .max_decimals(2),
                    );
                    ui.label(tr!("seconds."));
                });
            });
            ui.horizontal(|ui| {
                ui.add_space(indent);
                ui.label(tr!("Query limit"));
                ui.add(
                    DragValue::new(&mut d.max_queries_per_sec)
                        .range(0.5..=50.0)
                        .speed(0.05)
                        .max_decimals(1),
                );
                ui.label(tr!("queries per second and server."));
            });
            ui.horizontal(|ui| {
                ui.add_space(indent);
                ui.label(
                    RichText::new(tr!(
                        "srcds answers only sv_max_queries_sec (default 3) queries per second from one IP."
                    ))
                    .color(p.weak_text),
                );
            });
            ui.add_space(2.0);
            ui.checkbox(
                &mut d.keep_order_on_hover,
                tr!("Only change server list order if mouse cursor is out of server list"),
            );
            ui.checkbox(&mut d.connect_on_double_click, tr!("Connect on double click"))
                .on_hover_text(tr!("Double click on a server in the server list joins it"));
            ui.checkbox(
                &mut d.geo_lookup,
                tr!("Show server location and provider (looks up server IPs at ip-api.com)"),
            );
        });

        group_box(ui, p, tr!("Language"), width, |ui| {
            let label = |l: crate::i18n::Language| {
                if l == crate::i18n::Language::System {
                    format!(
                        "{} ({})",
                        tr!("Same as the system"),
                        l.resolve().native_name()
                    )
                } else {
                    l.native_name().to_string()
                }
            };
            egui::ComboBox::from_id_salt("language")
                .selected_text(label(d.language))
                .width(260.0)
                .height(popup_height(ui))
                .show_ui(ui, |ui| {
                    let mut find = TypeAhead::begin(ui);
                    for l in crate::i18n::Language::ALL {
                        let text = label(l);
                        let r = ui.selectable_value(&mut d.language, l, &text);
                        find.item(&r, &text);
                    }
                    find.finish(ui);
                });
        });

        group_box(ui, p, tr!("Skin"), width, |ui| {
            egui::ComboBox::from_id_salt("skin")
                .selected_text(d.theme.label())
                .width(260.0)
                .height(popup_height(ui))
                .show_ui(ui, |ui| {
                    let mut find = TypeAhead::begin(ui);
                    for t in [Theme::Light, Theme::Dark] {
                        let r = ui.selectable_value(&mut d.theme, t, t.label());
                        find.item(&r, t.label());
                    }
                    find.finish(ui);
                });
        });

        group_box(ui, p, tr!("Font"), width, |ui| {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("ui_font")
                    .selected_text(d.font.label().to_owned())
                    .width(260.0)
                    .height(popup_height(ui))
                    .show_ui(ui, |ui| match crate::theme::font_choices() {
                        Some(fonts) => font_list(ui, &mut d.font, fonts),
                        None => {
                            ui.label(RichText::new(tr!("Loading fonts…")).color(p.weak_text));
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
                        }
                    });
                ui.add_space(12.0);
                ui.label(tr!("Text size:"));
                egui::ComboBox::from_id_salt("text_size")
                    .selected_text(d.text_size.label())
                    .width(70.0)
                    .show_ui(ui, |ui| {
                        let mut find = TypeAhead::begin(ui);
                        for t in TextSize::ALL {
                            let r = ui.selectable_value(&mut d.text_size, t, t.label());
                            find.item(&r, t.label());
                        }
                        find.finish(ui);
                    });
            });
            ui.label(
                RichText::new(tr!(
                    "All fonts installed on this computer; without the chosen font the system font is used."
                ))
                .color(p.weak_text),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(tr!("Text smoothing:"));
                egui::ComboBox::from_id_salt("text_smoothing")
                    .selected_text(d.text_smoothing.label())
                    .width(160.0)
                    .show_ui(ui, |ui| {
                        let mut find = TypeAhead::begin(ui);
                        for t in crate::theme::TextSmoothing::ALL {
                            let r = ui.selectable_value(&mut d.text_smoothing, t, t.label());
                            find.item(&r, t.label());
                        }
                        find.finish(ui);
                    })
                    .response
                    .on_hover_text(tr!(
                        "ClearType uses the red, green and blue stripes of each screen pixel for sharper text. Automatic: on for normal screens, off for high-resolution screens (150 % and up)."
                    ));
                if !crate::theme::subpixel_supported() {
                    ui.label(
                        RichText::new(tr!("(needs OpenGL 3.3 – gray text on this computer)"))
                            .color(p.weak_text),
                    );
                }
            });
        });

        group_box(ui, p, tr!("Global ID Database"), width, |ui| {
            ui.checkbox(
                &mut d.iddb_auto_add,
                tr!("Automatically add player name with known Global ID to Global ID Database"),
            );
        });
    }
}

/// The fonts of Settings → Misc, each name shown in its own font (drawn when its entry
/// becomes visible, a few per frame), so the list shows what a font looks like.
fn font_list(ui: &mut Ui, current: &mut UiFont, fonts: Vec<UiFont>) {
    let mut find = TypeAhead::begin(ui);
    let mut budget = 3;
    let mut waiting = false;
    let row = egui::vec2(ui.available_width().max(250.0), 24.0);
    for f in fonts {
        let label = f.label().to_owned();
        let selected = *current == f;
        let (rect, response) = ui.allocate_exact_size(row, Sense::click());
        if ui.is_rect_visible(rect) {
            let visuals = ui.style().interact_selectable(&response, selected);
            if selected || response.hovered() || response.has_focus() {
                ui.painter().rect_filled(rect, 2.0, visuals.weak_bg_fill);
            }
            let left = rect.left_center() + egui::vec2(6.0, 0.0);
            match crate::theme::font_preview(ui.ctx(), &f, &label, 15.0, &mut budget) {
                Ok((texture, size)) => {
                    // Fonts with very tall letters are scaled down to the row.
                    let size = size * (rect.height() / size.y).min(1.0);
                    let image = egui::Rect::from_min_size(
                        egui::pos2(left.x, left.y - size.y / 2.0),
                        size,
                    );
                    ui.painter().with_clip_rect(rect).image(
                        texture,
                        image,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        visuals.text_color(),
                    );
                }
                Err(not_yet) => {
                    waiting |= not_yet;
                    ui.painter().text(
                        left,
                        egui::Align2::LEFT_CENTER,
                        &label,
                        egui::TextStyle::Body.resolve(ui.style()),
                        visuals.text_color(),
                    );
                }
            }
        }
        let response = response.on_hover_text(&label);
        if response.clicked() {
            *current = f;
            ui.close();
        }
        find.item(&response, &label);
    }
    find.finish(ui);
    if waiting {
        ui.ctx().request_repaint();
    }
}

/// Height for a dropdown list so it opens downwards inside the dialog (instead of beside
/// the dropdown when the list is taller than the room below it); longer lists scroll.
fn popup_height(ui: &Ui) -> f32 {
    (ui.ctx().content_rect().bottom() - ui.cursor().top() - 36.0).clamp(90.0, 420.0)
}

fn plugins(ui: &mut Ui, p: &Palette) {
    group_box(
        ui,
        p,
        tr!("Available Plugins"),
        ui.available_width() - 20.0,
        |ui| {
            ui.allocate_ui(egui::vec2(ui.available_width(), 150.0), |ui| {
                list_frame(ui, p, |ui| {
                    styled_table(ui, "plugins")
                        .column(Column::initial(170.0))
                        .column(Column::initial(60.0))
                        .column(Column::initial(100.0))
                        .column(Column::initial(55.0))
                        .column(Column::remainder())
                        .header(18.0, |mut h| {
                            for t in [
                                tr!("Title"),
                                tr!("Version"),
                                tr!("Author"),
                                tr!("Priority"),
                                tr!("Type"),
                            ] {
                                h.col(|ui| {
                                    plain_header(ui, p, t);
                                });
                            }
                        })
                        .body(|_| {});
                });
            });
        },
    );
    group_box(
        ui,
        p,
        tr!("Description"),
        ui.available_width() - 20.0,
        |ui| {
            ui.horizontal(|ui| {
                ui.label(tr!("Website:"));
                ui.add_space(60.0);
                ui.label(RichText::new(tr!("None")).color(p.link));
            });
            ui.allocate_ui(egui::vec2(ui.available_width(), 80.0), |ui| {
                list_frame(ui, p, |ui| {
                    ui.label(RichText::new(tr!("No plugins installed.")).color(p.weak_text));
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                ui.add_enabled(
                    false,
                    Button::new(tr!("Settings")).min_size(egui::vec2(75.0, 20.0)),
                );
            });
        },
    );
}
