//! Toolbar icon buttons like HLSW's (16×16 icons on flat buttons). The icons are drawn for
//! openHLSW (assets/icon-sources/toolbar/*.svg) and rendered into two strips by
//! assets/icon-sources/toolbar/render.py: 16 px and 32 px (high-DPI screens), each with
//! the normal icons in the first row and the greyed-out ones in the second.

use eframe::egui::{
    self, Color32, CornerRadius, Rect, Response, Sense, Stroke, StrokeKind, TextureHandle, Ui,
    emath::GuiRounding,
};

const STRIP_16: &[u8] = include_bytes!("../assets/toolbar/toolbar-16.png");
const STRIP_32: &[u8] = include_bytes!("../assets/toolbar/toolbar-32.png");

/// Icons in the order of the strips (file names 01_… to 10_…).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolIcon {
    Connect,
    Rcon,
    ServerList,
    Rules,
    Delete,
    Refresh,
    Update,
    ListProperties,
    ServerProperties,
    About,
}

const COUNT: usize = 10;

struct Strip {
    texture: TextureHandle,
    size: usize,
}

pub struct ToolbarIcons {
    small: Strip,
    large: Option<Strip>,
}

fn load_strip(ctx: &egui::Context, name: &str, png_bytes: &[u8]) -> Option<Strip> {
    let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let size = [info.width as usize, info.height as usize];
    let image = egui::ColorImage::from_rgba_unmultiplied(size, &buf[..info.buffer_size()]);
    // Drawn 1:1 on whole pixels (sharp); LINEAR only matters for odd scale factors.
    let texture = ctx.load_texture(name, image, egui::TextureOptions::LINEAR);
    Some(Strip {
        texture,
        size: size[1] / 2,
    })
}

impl ToolbarIcons {
    pub fn load(ctx: &egui::Context) -> Option<Self> {
        Some(Self {
            small: load_strip(ctx, "toolbar-16", STRIP_16)?,
            large: load_strip(ctx, "toolbar-32", STRIP_32),
        })
    }

    /// Just the icon (16×16 screen pixels, 32 on high-DPI screens), e.g. the key of
    /// password-protected servers in the server list.
    pub fn icon(&self, ui: &mut Ui, icon: ToolIcon) -> Response {
        let ppp = ui.pixels_per_point();
        let strip = match &self.large {
            Some(large) if ppp >= 1.5 => large,
            _ => &self.small,
        };
        let side = strip.size as f32 / ppp;
        let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), Sense::hover());
        if ui.is_rect_visible(rect) {
            let target = Rect::from_min_size(rect.min.round_to_pixels(ppp), rect.size());
            let (w, h) = ((strip.size * COUNT) as f32, (strip.size * 2) as f32);
            let s = strip.size as f32;
            let uv = Rect::from_min_size(
                egui::pos2(icon as usize as f32 * s / w, 0.0),
                egui::vec2(s / w, s / h),
            );
            ui.painter()
                .image(strip.texture.id(), target, uv, Color32::WHITE);
        }
        response
    }

    /// Flat 22×22 px button: a frame under the mouse, a pressed frame for toggles that are on
    /// (`checked`), a greyed-out icon when disabled.
    pub fn button(
        &self,
        ui: &mut Ui,
        icon: ToolIcon,
        enabled: bool,
        checked: bool,
        tooltip: &str,
    ) -> Response {
        // 22×22 screen pixels like HLSW (44 on high-DPI screens), at every text size.
        let ppp = ui.pixels_per_point();
        let px = if ppp >= 1.5 && self.large.is_some() { 44.0 } else { 22.0 };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(px, px) / ppp, Sense::click());
        let response = if enabled {
            response
        } else {
            // Disabled buttons do not react, but still show their tooltip.
            ui.interact(rect, response.id.with("disabled"), Sense::hover())
        };
        if ui.is_rect_visible(rect) {
            let dark = ui.visuals().dark_mode;
            let hovered = enabled && response.hovered();
            let pressed = enabled && response.is_pointer_button_down_on();
            let frame = rect.shrink(0.5).round_to_pixels(ppp);
            let (fill, border) = match (dark, pressed || checked, hovered) {
                (false, true, _) => (
                    Color32::from_rgb(0xCC, 0xE8, 0xFF),
                    Color32::from_rgb(0x99, 0xD1, 0xFF),
                ),
                (false, false, true) => (
                    Color32::from_rgb(0xE5, 0xF3, 0xFF),
                    Color32::from_rgb(0xCC, 0xE8, 0xFF),
                ),
                (true, true, _) => (
                    Color32::from_rgb(0x2E, 0x4A, 0x66),
                    Color32::from_rgb(0x4A, 0x78, 0xA6),
                ),
                (true, false, true) => (
                    Color32::from_rgb(0x3A, 0x3F, 0x46),
                    Color32::from_rgb(0x55, 0x5C, 0x66),
                ),
                _ => (Color32::TRANSPARENT, Color32::TRANSPARENT),
            };
            if border != Color32::TRANSPARENT {
                ui.painter().rect(
                    frame,
                    CornerRadius::ZERO,
                    fill,
                    Stroke::new(1.0, border),
                    StrokeKind::Inside,
                );
            }
            // Always 1:1 on whole screen pixels (16 px, 32 px from 1.5× scaling), so the icons
            // stay sharp at every text size instead of being scaled by a fraction.
            let strip = match &self.large {
                Some(large) if ppp >= 1.5 => large,
                _ => &self.small,
            };
            let side = strip.size as f32 / ppp;
            let mut min = (rect.center() - egui::vec2(side, side) / 2.0).round_to_pixels(ppp);
            if pressed {
                min += egui::vec2(1.0, 1.0) / ppp;
            }
            let target = Rect::from_min_size(min, egui::vec2(side, side));
            let (col, row) = (icon as usize, usize::from(!enabled));
            let (w, h) = ((strip.size * COUNT) as f32, (strip.size * 2) as f32);
            let s = strip.size as f32;
            let uv = Rect::from_min_size(
                egui::pos2(col as f32 * s / w, row as f32 * s / h),
                egui::vec2(s / w, s / h),
            );
            ui.painter()
                .image(strip.texture.id(), target, uv, Color32::WHITE);
        }
        response.on_hover_text(tooltip)
    }
}
