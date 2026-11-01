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
mod firewall;
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
mod update;
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

/// Moving a maximized window un-maximizes it on Windows, and macOS needs the move too.
const MAXIMIZE_AFTER_PLACING: bool = cfg!(any(windows, target_os = "macos"));

/// Puts the new, still hidden main window exactly where it was closed: winit places it
/// with the scale factor of the monitor it starts on, which can differ from the one it
/// was saved on. If that place is on no monitor any more (unplugged), the window is
/// centered on the primary monitor. Then it is maximized if it was.
fn place_window(window: &winit::window::Window, view: &settings::ViewOptions) {
    use winit::dpi::PhysicalPosition;
    if let Some(([x, y], scale)) = view.main_window_pos() {
        let [w, h] = view.main_window_size();
        let size = [w * scale, h * scale];
        let monitors: Vec<[f32; 4]> = window
            .available_monitors()
            .map(|m| {
                let (p, s) = (m.position(), m.size());
                [p.x as f32, p.y as f32, s.width as f32, s.height as f32]
            })
            .collect();
        let target = if monitors.is_empty() || title_bar_visible([x, y], size, &monitors) {
            Some([x, y])
        } else {
            window
                .primary_monitor()
                .or_else(|| window.available_monitors().next())
                .map(|m| {
                    let (p, s) = (m.position(), m.size());
                    [
                        p.x as f32 + ((s.width as f32 - size[0]) / 2.0).max(0.0),
                        p.y as f32 + ((s.height as f32 - size[1]) / 2.0).max(0.0),
                    ]
                })
        };
        if let Some([x, y]) = target {
            window.set_outer_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
        }
    }
    if MAXIMIZE_AFTER_PLACING && view.window_maximized {
        window.set_maximized(true);
    }
}

/// Whether enough of the title bar of a window at `pos` with `size` (physical pixels)
/// lies on one of the `monitors` (`[x, y, width, height]`) to grab it.
fn title_bar_visible(pos: [f32; 2], size: [f32; 2], monitors: &[[f32; 4]]) -> bool {
    let [x, y] = pos;
    monitors.iter().any(|&[mx, my, mw, mh]| {
        let left = x.max(mx);
        let right = (x + size[0]).min(mx + mw);
        // The top 30 px of the frame: the title bar.
        let top = y.max(my);
        let bottom = (y + 30.0).min(my + mh);
        right - left >= 100.0 && bottom - top >= 10.0
    })
}

fn main() -> eframe::Result {
    // The window opens maximized and centered the first time, later exactly as it was
    // closed: position, normal size ("Restore" goes back to it) and maximized or not. The
    // app saves these (`OpenHlswApp::track_main_window`), not eframe: eframe also saves
    // the size of a minimized window, which then restored to a tiny window. Panel sizes
    // and column widths are stored with the egui memory.
    let settings = config::load().settings;
    let view = settings.view.clone();
    // eframe multiplies the start size and position by the text size zoom it remembers
    // from last time.
    let zoom = settings.text_size.zoom();
    let [w, h] = view.main_window_size();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(concat!("openHLSW - v", env!("CARGO_PKG_VERSION")))
        .with_app_id("openhlsw")
        .with_inner_size([w / zoom, h / zoom])
        .with_min_inner_size(settings::MIN_WINDOW_SIZE)
        // Windows and macOS: maximized in `place_window`, after it is moved to its place.
        .with_maximized(view.window_maximized && !MAXIMIZE_AFTER_PLACING)
        .with_icon(app_icon());
    let pos = view.main_window_pos();
    if let Some(([x, y], scale)) = pos {
        viewport = viewport.with_position([x / scale / zoom, y / scale / zoom]);
    }
    let options = eframe::NativeOptions {
        viewport,
        centered: pos.is_none(),
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "openHLSW",
        options,
        Box::new(|cc| {
            // Taskbar / Alt+Tab icon from the icon embedded in the .exe.
            #[cfg(windows)]
            win_icon::apply(cc);
            if let Some(window) = cc.winit_window() {
                place_window(window, &view);
            }
            Ok(Box::new(app::AppHandle::new(app::OpenHlswApp::new(cc))))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::title_bar_visible;

    #[test]
    fn window_on_an_unplugged_monitor_is_not_visible() {
        let monitors = [[0.0, 0.0, 2560.0, 1440.0]];
        let size = [1500.0, 900.0];
        assert!(title_bar_visible([200.0, 150.0], size, &monitors));
        // Mostly off the left edge, the title bar still reachable.
        assert!(title_bar_visible([-1300.0, 10.0], size, &monitors));
        // Where a second monitor used to be.
        assert!(!title_bar_visible([2600.0, 100.0], size, &monitors));
        assert!(!title_bar_visible([100.0, 1500.0], size, &monitors));
        // Title bar above the screen.
        assert!(!title_bar_visible([100.0, -200.0], size, &monitors));
    }
}
