//! Game icons (assets/games/*.ico, packed into an atlas by build.rs).

use std::collections::HashMap;

use eframe::egui::{self, Color32, Rect, Sense, TextureHandle, Ui};

const ATLAS_PNG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/game_icons.png"));
const ATLAS16_PNG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/game_icons16.png"));
const ATLAS_NAMES: &str = include_str!(concat!(env!("OUT_DIR"), "/game_icons.txt"));
const COLUMNS: usize = 16;

/// Window icon (64x64 RGBA) from assets/openhlsw.ico.
pub const APP_ICON_RGBA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/app_icon.rgba"));

pub struct GameIcons {
    /// 16 px cells (normal screens) and 32 px cells (high-DPI screens).
    small: TextureHandle,
    large: TextureHandle,
    index: HashMap<&'static str, usize>,
    rows: usize,
}

fn load_atlas(ctx: &egui::Context, name: &str, png_bytes: &[u8]) -> Option<TextureHandle> {
    let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let size = [info.width as usize, info.height as usize];
    let image = egui::ColorImage::from_rgba_unmultiplied(size, &buf[..info.buffer_size()]);
    Some(ctx.load_texture(name, image, egui::TextureOptions::LINEAR))
}

impl GameIcons {
    pub fn load(ctx: &egui::Context) -> Option<Self> {
        let small = load_atlas(ctx, "game-icons-16", ATLAS16_PNG)?;
        let large = load_atlas(ctx, "game-icons-32", ATLAS_PNG)?;
        let rows = small.size()[1] / 16;
        let index = ATLAS_NAMES
            .split(',')
            .filter(|n| !n.is_empty())
            .enumerate()
            .map(|(i, n)| (n, i))
            .collect();
        Some(Self {
            small,
            large,
            index,
            rows: rows.max(1),
        })
    }

    /// Draws the icon (or reserves the space if there is none): 16x16 screen pixels on
    /// whole pixels, so it is sharp and never cut off. `size` is the most it may take
    /// (in points); in a row lower than that the icon is scaled down to fit completely.
    pub fn show(&self, ui: &mut Ui, key: &str, size: f32) -> egui::Response {
        let ppp = ui.pixels_per_point();
        // 16 points (16 screen pixels at 100 %), at most `size` and the row height.
        let limit = size.min(ui.available_height().max(8.0)).min(16.0) * ppp;
        let px = limit.floor().max(1.0);
        let side = px / ppp;
        let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), Sense::hover());
        self.paint(ui, key, rect);
        response
    }

    /// Size of the space [`Self::paint`] needs for a 16 px icon (whole screen pixels).
    pub fn slot(ui: &Ui) -> egui::Vec2 {
        let ppp = ui.pixels_per_point();
        egui::Vec2::splat((16.0 * ppp).floor().max(1.0) / ppp)
    }

    /// Draws the icon into `rect` (e.g. a [`Self::slot`] inside a button), at its left edge
    /// and vertically centered, as large as the rect's height allows (at most 16 px).
    pub fn paint(&self, ui: &Ui, key: &str, rect: Rect) {
        let ppp = ui.pixels_per_point();
        let px = (rect.height().min(16.0) * ppp).round().max(1.0);
        let side = px / ppp;
        if let Some(&i) = self.index.get(key) {
            let (col, row) = (i % COLUMNS, i / COLUMNS);
            let texture = if px > 16.0 { &self.large } else { &self.small };
            let (cols, rows) = (COLUMNS as f32, self.rows as f32);
            let uv = Rect::from_min_size(
                egui::pos2(col as f32 / cols, row as f32 / rows),
                egui::vec2(1.0 / cols, 1.0 / rows),
            );
            // On whole screen pixels: 1:1 copies of the 16 px pictures.
            let min = egui::pos2(
                (rect.min.x * ppp).round() / ppp,
                (rect.center().y * ppp - px / 2.0).round() / ppp,
            );
            let target = Rect::from_min_size(min, egui::vec2(side, side));
            ui.painter().image(texture.id(), target, uv, Color32::WHITE);
        }
    }
}

/// Icon of a Source server, from its Steam AppID or game folder.
pub fn icon_for_server(app_id: u32, folder: &str) -> &'static str {
    if openhlsw::a2s::is_goldsrc_app(app_id) {
        return match folder.to_ascii_lowercase().as_str() {
            "cstrike" => "HL_CS",
            "czero" => "HL_CZ",
            "dod" => "HL_DoD",
            "tfc" => "HL_TFC",
            "dmc" => "HL_DMC",
            "ns" => "HL_NS",
            "ts" => "HL_TS",
            "firearms" => "HL_FA",
            "frontline" => "HL_FLF",
            "ios" => "HL_IOS",
            _ => "HL",
        };
    }
    match app_id {
        240 => "HL2_CSS",
        730 => "HL2_CS2",
        440 => "HL2_TF2",
        300 => "HL2_DoD",
        4000 => "HL2_GM",
        222880 | 447820 => "HL2_INS",
        17520 => "HL2_Synergy",
        17500 => "HL2_ZPS",
        500 => "L4D",
        550 => "L4D2",
        630 => "HL2_AS",
        _ => match folder.to_ascii_lowercase().as_str() {
            "ageofchivalry" => "HL2_AoC",
            "fortressforever" | "ff" => "HL2_FF",
            "sourceforts" => "HL2_SourceForts",
            "obsidian" => "HL2_OC",
            "diprip" => "HL2_DIPRIP",
            "cspromod" => "HL2_CSPromod",
            _ => "HL2",
        },
    }
}

/// Icon for a GameSpy 2 server by its `gamename`.
/// `folder` is the gamename, for Battlefield 2 / 2142 mods "gamename/mod".
pub fn icon_for_gamespy(folder: &str) -> &'static str {
    let folder = folder.to_ascii_lowercase();
    let (gamename, mod_dir) = folder.split_once('/').unwrap_or((folder.as_str(), ""));
    match gamename {
        "dh2005" | "deerhunter2005" | "deerhunt2005" => "DH2005",
        "battlefield2" if mod_dir == "xpack" => "BF2_XPACK",
        // Brown "2" like HLSW (the Special Forces "2" in brown).
        "battlefield2" => "BF2_Classic",
        "stella" => "BF2142",
        "bfvietnam" => "BFV",
        "armedass" => "ARMA",
        "arma2" | "arma2pc" => "ARMA2",
        "crysis" => "Crysis",
        "crysiswars" => "CrysisWars",
        _ => "GS2", // generic GameSpy 2 icon
    }
}

/// Icon for an entry of the "My Games" list.
pub fn icon_for_game(name: &str) -> &'static str {
    match name {
        "Alien vs. Predator 2" => "AvP2",
        "America's Army: Operations" => "AAO",
        "ARMA 2" => "ARMA2",
        "ARMA 2: Operation Arrowhead" => "ARMA2AO",
        "Armed Assault" => "ARMA",
        "Battlefield 1942" => "BF1942",
        "Battlefield 2" => "BF2_Classic",
        "Battlefield 2142" => "BF2142",
        "Battlefield Vietnam" => "BFV",
        "Brink" => "Brink",
        "C&C Renegade" => "CCR",
        "Call of Duty" => "CoD",
        "Call of Duty 2" => "CoD2",
        "Call of Duty 4" => "CoD4",
        "Call of Duty: United Offensive" => "CoDUO",
        "Call of Duty: World at War" => "CoD5",
        "Challenge Quake 3" => "CNQ3",
        "Crysis" => "Crysis",
        "Crysis Wars" => "CrysisWars",
        "Deer Hunter 2005" => "DH2005",
        "Devastation" => "DVS",
        "Doom 3" => "D3",
        "Elite Force" => "EF",
        "Elite Force II" => "EF2",
        "Enemy Territory: Quake Wars" => "ETQW",
        "F.E.A.R." => "FEAR",
        "F.E.A.R. Perseus Mandate" => "FEARXP2",
        "Far Cry" => "FarCry",
        "Frontlines-Fuel of War" => "FFOW",
        "Half-Life" | "Half-Life (WON)" => "HL",
        "HALO" => "HALO",
        "ioQuake3" => "IOQ3",
        "Jedi Knight 2" => "JK2",
        "Jedi Knight 3" => "JK3",
        "Killing Floor" => "KF",
        "Land Of The Dead: Road to Fiddler's Green" => "LOTD",
        "Medal of Honor Allied Assault" => "MoHAA",
        "Medal of Honor Pacific Assault" => "MoHPA",
        "Medal of Honor: Allied Assault Breakthrough" => "MoHAAB",
        "Medal of Honor: Allied Assault Spearhead" => "MoHAAS",
        "Neverwinter Nights" => "NWN",
        "Nexuiz" => "NEXUIZ",
        "Operation Flashpoint" | "Operation Flashpoint Resistance" => "OPFR",
        "Painkiller" => "PK",
        "Pariah" => "Pariah",
        "Postal 2" => "Postal2",
        "Prey" => "PREY",
        "Project IGI2: Covert Strike" => "IGI2",
        "Quake 1" => "Q1",
        "Quake 2" => "Q2",
        "Quake 3 Arena" => "Q3",
        "Quake 4" => "Q4",
        "Red Orchestra 2: Heroes of Stalingrad" => "RO2",
        "Red Orchestra: Ostfront 41-45" => "ROO",
        "Return to Castle Wolfenstein" => "RtCW",
        "Rune" => "Rune",
        "S.T.A.L.K.E.R. - Shadow of Chernobyl" => "STALKER",
        "Savage: The Battle For Newerth" => "Savage",
        "Serious Sam: The First Encounter" | "Serious Sam: The Second Encounter" => "SeriousSam",
        "Shockvoice (v0.9.5+)" => "Shockvoice",
        "Soldier of Fortune 2" => "SoF2",
        "Star Wars: Battlefront" => "SWBF",
        "SWAT 4" => "SWAT4",
        "SWAT 4: The Stetchkov Syndicate" => "SWAT4X",
        "TeamSpeak 2" => "TeamSpeak2",
        "TeamSpeak 3" => "TeamSpeak3",
        "Tremulous" => "TREMULOUS",
        "Tribes 2" => "Tribes2",
        "Tribes Vengeance" => "TribesV",
        "Unreal Tournament" => "UT",
        "Unreal Tournament 2003" => "UT2003",
        "Unreal Tournament 2004" => "UT2004",
        "Unreal Tournament 3" => "UT3",
        "Urban Terror" => "UrT",
        "Ventrilo (v2.3.0+)" => "Ventrilo",
        "Warsow" => "WSW",
        "Wolfenstein - Enemy Territory" => "ET",
        "Wolfenstein 2009" => "Wolf09",
        // Source engine games
        "Alien Swarm" => "HL2_AS",
        "Counter-Strike 2" | "Counter-Strike: Global Offensive" => "HL2_CS2",
        "Counter-Strike: Source" => "HL2_CSS",
        "Day of Defeat: Source" => "HL2_DoD",
        "Day of Infamy" | "Insurgency" => "HL2_INS",
        "Garry's Mod" => "HL2_GM",
        "Left 4 Dead" => "L4D",
        "Left 4 Dead 2" => "L4D2",
        "Synergy" => "HL2_Synergy",
        "Team Fortress 2" => "HL2_TF2",
        "Zombie Panic! Source" => "HL2_ZPS",
        _ => "HL2",
    }
}
