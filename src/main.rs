// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

/// Translated text: `tr!("Refresh")` (a `&str`), or with named values
/// `tr!("Added {n} servers", n = count)` (a `String`). See `i18n`.
macro_rules! tr {
    ($s:literal) => {
        $crate::i18n::t($s)
    };
    ($s:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::fill(
            $crate::i18n::t($s),
            &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),+],
        )
    };
}

mod app;
mod config;
mod discover;
mod filter;
mod flags;
mod game_icons;
mod games;
mod geo;
mod i18n;
mod iddb;
mod native_window;
mod server_io;
mod settings;
mod theme;
mod toolbar_icons;
mod trace;
mod typeahead;
#[cfg(windows)]
mod win_icon;
mod worker;

use eframe::egui;

/// The program icon (title bars of the main window and the dialogs).
pub fn app_icon() -> std::sync::Arc<egui::IconData> {
    std::sync::Arc::new(egui::IconData {
        rgba: game_icons::APP_ICON_RGBA.to_vec(),
        width: 64,
        height: 64,
    })
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(concat!("openHLSW - v", env!("CARGO_PKG_VERSION")))
            .with_app_id("openhlsw")
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([800.0, 500.0])
            .with_maximized(true)
            .with_icon(app_icon()),
        // Remember size, position and maximized state of the window (the first start is
        // maximized); panel sizes and column widths are stored with the egui memory.
        persist_window: true,
        ..Default::default()
    };
    eframe::run_native(
        "openHLSW",
        options,
        Box::new(|cc| {
            // Taskbar / Alt+Tab icon from the icon embedded in the .exe.
            #[cfg(windows)]
            win_icon::apply(cc);
            Ok(Box::new(app::OpenHlswApp::new(cc)))
        }),
    )
}
