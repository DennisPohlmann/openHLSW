// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod config;
mod flags;
mod game_icons;
mod geo;
mod settings;
mod theme;
mod trace;
mod worker;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(concat!("openHLSW - v", env!("CARGO_PKG_VERSION")))
            .with_app_id("openhlsw")
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([800.0, 500.0])
            .with_maximized(true)
            .with_icon(egui::IconData {
                rgba: game_icons::APP_ICON_RGBA.to_vec(),
                width: 64,
                height: 64,
            }),
        // Remember size, position and maximized state of the window (the first start is
        // maximized); panel sizes and column widths are stored with the egui memory.
        persist_window: true,
        ..Default::default()
    };
    eframe::run_native(
        "openHLSW",
        options,
        Box::new(|cc| Ok(Box::new(app::OpenHlswApp::new(cc)))),
    )
}
