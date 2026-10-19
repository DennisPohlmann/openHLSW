//! Program settings and the HLSW-style "Settings" dialog
//! (My Games, Startup, Connecting, Rcon, Connections, Plugins, Sounds, Misc).

use std::collections::BTreeMap;

use eframe::egui::{
    self, Button, Checkbox, DragValue, Frame, Label, Margin, RichText, Sense, TextEdit, Ui,
};
use egui_extras::Column;
use serde::{Deserialize, Serialize};

use crate::app::{EditMenu, fit, group_box, list_frame, plain_header, styled_table, tab_strip};
use crate::theme::{Palette, TextSize, Theme};

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
}

impl OnConnect {
    fn label(self) -> &'static str {
        match self {
            OnConnect::Nothing => "Do nothing",
            OnConnect::Minimize => "Minimize openHLSW",
            OnConnect::MinimizeNoTraffic => "Minimize openHLSW (no traffic)",
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
            SoundEvent::StartUp => "StartUp",
            SoundEvent::IncomingMessage => "Incoming Message",
            SoundEvent::SendingMessage => "Sending Message",
            SoundEvent::PlayerJoin => "Player Join",
            SoundEvent::Notification => "Notification",
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
            text_size: TextSize::Normal,
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

/// State of an open settings dialog: a draft that is applied on OK / Apply.
pub struct SettingsDialog {
    pub draft: Settings,
    pub tab: SettingsTab,
    editing_game: Option<&'static str>,
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
        egui::Window::new("Settings")
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
                            Button::new("Apply").min_size(egui::vec2(75.0, 22.0)),
                        )
                        .clicked()
                    {
                        result = DialogResult::Apply(self.draft.clone());
                    }
                    if ui.add_sized([75.0, 22.0], Button::new("Cancel")).clicked() {
                        result = DialogResult::Cancel;
                    }
                    if ui
                        .add_sized([75.0, 22.0], Button::new(RichText::new("OK").strong()))
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
        let editing = &mut self.editing_game;
        ui.allocate_ui(egui::vec2(ui.available_width(), 380.0), |ui| {
            list_frame(ui, p, |ui| {
                styled_table(ui, "my_games")
                    .sense(Sense::click())
                    .column(Column::initial(260.0).at_least(120.0).clip(true))
                    .column(Column::initial(85.0))
                    .column(Column::remainder().clip(true))
                    .header(18.0, |mut h| {
                        h.col(|ui| {
                            plain_header(ui, p, "Game");
                        });
                        h.col(|ui| {
                            plain_header(ui, p, "Server query");
                        });
                        h.col(|ui| {
                            plain_header(ui, p, "File");
                        });
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
                                    RichText::new("Yes").color(p.green)
                                } else {
                                    RichText::new("not yet").color(p.weak_text)
                                };
                                ui.add(Label::new(t).selectable(false));
                            });
                            row.col(|ui| {
                                if *editing == Some(game.name) {
                                    let path = paths.entry(game.name.to_string()).or_default();
                                    let r = ui
                                        .add(
                                            TextEdit::singleline(path)
                                                .hint_text("path to the game .exe, empty = Steam")
                                                .desired_width(f32::INFINITY),
                                        )
                                        .with_edit_menu();
                                    r.request_focus();
                                    if r.lost_focus() {
                                        *editing = None;
                                    }
                                } else {
                                    let path = paths.get(game.name).cloned().unwrap_or_default();
                                    ui.add(Label::new(path).selectable(false).wrap_mode(fit(ui)));
                                }
                            });
                            if row.response().double_clicked() {
                                *editing = Some(game.name);
                            }
                        });
                    });
            });
        });
        paths.retain(|_, v| !v.trim().is_empty());
        ui.vertical_centered(|ui| {
            ui.label("(Double click to set the game executable. Empty = connect via Steam.)");
        });
    }

    fn startup(&mut self, ui: &mut Ui, p: &Palette, server_list: &[String]) {
        let d = &mut self.draft;
        group_box(
            ui,
            p,
            "Selected server/server list on startup",
            ui.available_width() - 20.0,
            |ui| {
                ui.radio_value(&mut d.startup, StartupSelection::None, "None");
                ui.radio_value(
                    &mut d.startup,
                    StartupSelection::LastUsed,
                    "Last used server/server list",
                );
                ui.radio_value(
                    &mut d.startup,
                    StartupSelection::This,
                    "This server/server list:",
                );
                ui.add_enabled_ui(d.startup == StartupSelection::This, |ui| {
                    egui::Grid::new("startup_grid")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Server list:");
                            egui::ComboBox::from_id_salt("startup_list")
                                .selected_text("Custom server list")
                                .width(260.0)
                                .show_ui(ui, |ui| {
                                    let _ = ui.selectable_label(true, "Custom server list");
                                });
                            ui.end_row();
                            ui.label("IP/Port:");
                            egui::ComboBox::from_id_salt("startup_addr")
                                .selected_text(if d.startup_address.is_empty() {
                                    "-"
                                } else {
                                    d.startup_address.as_str()
                                })
                                .width(260.0)
                                .show_ui(ui, |ui| {
                                    for addr in server_list {
                                        ui.selectable_value(
                                            &mut d.startup_address,
                                            addr.clone(),
                                            addr,
                                        );
                                    }
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
            ui.label("On Connection:");
            egui::ComboBox::from_id_salt("on_connect")
                .selected_text(d.on_connect.label())
                .width(250.0)
                .show_ui(ui, |ui| {
                    for v in [
                        OnConnect::Nothing,
                        OnConnect::Minimize,
                        OnConnect::MinimizeNoTraffic,
                    ] {
                        ui.selectable_value(&mut d.on_connect, v, v.label());
                    }
                });
        });
        ui.add_space(8.0);
        let selected = &mut self.selected_app;
        group_box(
            ui,
            p,
            "Execute applications",
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
                                    plain_header(ui, p, "Flags");
                                });
                                h.col(|ui| {
                                    plain_header(ui, p, "File + Parameter");
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
                    if ui.add_sized([75.0, 20.0], Button::new("Add...")).clicked() {
                        d.connect_apps.push(ExecApp {
                            enabled: true,
                            command: String::new(),
                        });
                        *selected = Some(d.connect_apps.len() - 1);
                    }
                    if ui
                        .add_enabled(
                            selected.is_some(),
                            Button::new("Delete").min_size(egui::vec2(75.0, 20.0)),
                        )
                        .clicked()
                        && let Some(i) = selected.take()
                        && i < d.connect_apps.len()
                    {
                        d.connect_apps.remove(i);
                    }
                });
                ui.label(RichText::new("Started when you connect to a server. %addr%, %ip% and %port% are replaced.").small());
            },
        );
    }

    fn rcon(&mut self, ui: &mut Ui, p: &Palette) {
        let d = &mut self.draft;
        group_box(ui, p, "Rcon", ui.available_width() - 20.0, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(180.0);
                ui.label("Cache size:");
                ui.add(DragValue::new(&mut d.console_cache).range(100..=100_000));
                ui.label("entries.");
            });
            ui.add_space(6.0);
            let logs = crate::config::path()
                .and_then(|p| p.parent().map(|d| d.join("logs").display().to_string()))
                .unwrap_or_else(|| "logs".into());
            ui.checkbox(
                &mut d.save_server_log,
                format!(
                    "Save Server Log ({logs}{}<SERVERADDR>{}*.log)",
                    std::path::MAIN_SEPARATOR,
                    std::path::MAIN_SEPARATOR
                ),
            );
            ui.checkbox(&mut d.save_chat_log, "Save Server Chat (*.chat.log)");
            ui.checkbox(&mut d.autocomplete, "Autocomplete at rcon console (Tab)");
            ui.checkbox(&mut d.log_on_map_change, "Retrieve log on map change");
        });
    }

    fn connections(&mut self, ui: &mut Ui, p: &Palette) {
        let d = &mut self.draft;
        let port_test = &mut self.port_test;
        group_box(
            ui,
            p,
            "Connection Settings",
            ui.available_width() - 20.0,
            |ui| {
                ui.horizontal(|ui| {
                ui.label("UDP Port:");
                ui.add(DragValue::new(&mut d.log_port).range(1024..=65535));
                if ui.add_sized([110.0, 20.0], Button::new("Port test")).clicked() {
                    *port_test = Some(match std::net::UdpSocket::bind(("0.0.0.0", d.log_port)) {
                        Ok(_) => format!("UDP port {} is free on this computer.", d.log_port),
                        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                            format!("UDP port {} is in use (openHLSW may already be listening on it).", d.log_port)
                        }
                        Err(e) => format!("UDP port {}: {e}", d.log_port),
                    });
                }
            });
                ui.label("Port the server log is received on (Get Log). Must be forwarded for Get Log (Internet).");
                if let Some(msg) = port_test.as_ref() {
                    ui.label(RichText::new(msg).color(p.link));
                }
            },
        );
        group_box(ui, p, "Public IP", ui.available_width() - 20.0, |ui| {
            ui.horizontal(|ui| {
                ui.label("Public IP:");
                ui.add(
                    TextEdit::singleline(&mut d.public_ip)
                        .hint_text("auto-detect")
                        .desired_width(160.0),
                )
                .with_edit_menu();
            });
            ui.label("Used by Get Log (Internet). Empty = ask api.ipify.org.");
        });
    }

    fn sounds(&mut self, ui: &mut Ui, p: &Palette) {
        let d = &mut self.draft;
        group_box(ui, p, "Sounds", ui.available_width() - 20.0, |ui| {
            ui.checkbox(&mut d.sounds_disabled, "Disable sounds");
            ui.add_space(4.0);
            ui.add_enabled_ui(!d.sounds_disabled, |ui| {
                ui.allocate_ui(egui::vec2(ui.available_width(), 220.0), |ui| {
                    list_frame(ui, p, |ui| {
                        styled_table(ui, "sounds")
                            .column(Column::initial(160.0))
                            .column(Column::remainder().clip(true))
                            .header(18.0, |mut h| {
                                h.col(|ui| {
                                    plain_header(ui, p, "Event");
                                });
                                h.col(|ui| {
                                    plain_header(ui, p, "Sound Filename (.wav)");
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
                                                .hint_text("path to a .wav file")
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

    fn misc(&mut self, ui: &mut Ui, p: &Palette) {
        let d = &mut self.draft;
        group_box(ui, p, "Server list", ui.available_width() - 20.0, |ui| {
            ui.checkbox(
                &mut d.auto_refresh,
                "Auto update all servers in server list",
            );
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                ui.label("Update interval");
                ui.add(
                    DragValue::new(&mut d.refresh_interval)
                        .range((1.0 / d.max_queries_per_sec.max(0.1)).clamp(0.1, 10.0)..=3600.0)
                        .speed(0.05)
                        .max_decimals(2),
                );
                ui.label("seconds.");
            });
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                ui.label("Query limit");
                ui.add(
                    DragValue::new(&mut d.max_queries_per_sec)
                        .range(0.5..=50.0)
                        .speed(0.05)
                        .max_decimals(1),
                );
                ui.label("queries per second and server.");
            });
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                ui.label(
                    RichText::new("srcds answers only sv_max_queries_sec (default 3) queries per second from one IP.")
                        .color(p.weak_text),
                );
            });
            ui.checkbox(
                &mut d.keep_order_on_hover,
                "Only change server list order if mouse cursor is out of server list",
            );
            ui.checkbox(
                &mut d.geo_lookup,
                "Show server location and provider (looks up server IPs at ip-api.com)",
            );
        });
        group_box(ui, p, "Skin", ui.available_width() - 20.0, |ui| {
            egui::ComboBox::from_id_salt("skin")
                .selected_text(d.theme.label())
                .width(180.0)
                .show_ui(ui, |ui| {
                    for t in [Theme::Light, Theme::Dark] {
                        ui.selectable_value(&mut d.theme, t, t.label());
                    }
                });
        });
        group_box(ui, p, "Text size", ui.available_width() - 20.0, |ui| {
            ui.horizontal(|ui| {
                for t in TextSize::ALL {
                    ui.radio_value(&mut d.text_size, t, t.label());
                }
            });
        });
        Frame::new()
            .inner_margin(Margin::symmetric(6, 4))
            .show(ui, |ui| {
                ui.checkbox(&mut d.connect_on_double_click, "Connect on double click");
            });
    }
}

fn plugins(ui: &mut Ui, p: &Palette) {
    group_box(
        ui,
        p,
        "Available Plugins",
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
                            for t in ["Title", "Version", "Author", "Priority", "Type"] {
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
    group_box(ui, p, "Description", ui.available_width() - 20.0, |ui| {
        ui.horizontal(|ui| {
            ui.label("Website:");
            ui.add_space(60.0);
            ui.label(RichText::new("None").color(p.link));
        });
        ui.allocate_ui(egui::vec2(ui.available_width(), 80.0), |ui| {
            list_frame(ui, p, |ui| {
                ui.label(RichText::new("No plugins installed.").color(p.weak_text));
            });
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            ui.add_enabled(
                false,
                Button::new("Settings").min_size(egui::vec2(75.0, 20.0)),
            );
        });
    });
}
