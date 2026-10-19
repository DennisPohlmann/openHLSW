//! Country flags (public domain, github.com/hampusborgos/country-flags) packed into
//! one embedded texture atlas: 16 flags per row in the order of `flags.txt`, each cell
//! 3:2. The atlas may have any resolution (e.g. 36x24 or 96x64 per flag), RGB or RGBA.

use std::collections::HashMap;

use eframe::egui::{self, Color32, Rect, Sense, Stroke, StrokeKind, TextureHandle, Ui};

const ATLAS_PNG: &[u8] = include_bytes!("../assets/flags.png");
const ATLAS_CODES: &str = include_str!("../assets/flags.txt");
const COLUMNS: usize = 16;

pub struct Flags {
    texture: TextureHandle,
    index: HashMap<&'static str, usize>,
    atlas_size: [usize; 2],
    /// Cell size in pixels, derived from the atlas size and the number of flags.
    cell: [usize; 2],
}

impl Flags {
    pub fn load(ctx: &egui::Context) -> Option<Self> {
        let decoder = png::Decoder::new(std::io::Cursor::new(ATLAS_PNG));
        let mut reader = decoder.read_info().ok()?;
        let mut buf = vec![0; reader.output_buffer_size()?];
        let info = reader.next_frame(&mut buf).ok()?;
        if info.bit_depth != png::BitDepth::Eight {
            return None;
        }
        let size = [info.width as usize, info.height as usize];
        let data = &buf[..info.buffer_size()];
        let image = match info.color_type {
            png::ColorType::Rgba => egui::ColorImage::from_rgba_unmultiplied(size, data),
            png::ColorType::Rgb => egui::ColorImage::from_rgb(size, data),
            _ => return None,
        };
        let texture = ctx.load_texture("country-flags", image, egui::TextureOptions::LINEAR);
        let index: HashMap<&'static str, usize> = ATLAS_CODES
            .trim()
            .split(',')
            .enumerate()
            .map(|(i, c)| (c, i))
            .collect();
        let rows = index.len().div_ceil(COLUMNS).max(1);
        let cell = [size[0] / COLUMNS, size[1] / rows];
        Some(Self {
            texture,
            index,
            atlas_size: size,
            cell,
        })
    }

    pub fn has(&self, code: &str) -> bool {
        self.index.contains_key(code)
    }

    /// Draws the flag for an ISO country code ("de") at the given height (3:2).
    pub fn show(&self, ui: &mut Ui, code: &str, height: f32) -> egui::Response {
        let size = egui::vec2(height * 1.5, height);
        let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
        if let Some(&i) = self.index.get(code) {
            let (col, row) = (i % COLUMNS, i / COLUMNS);
            let (aw, ah) = (self.atlas_size[0] as f32, self.atlas_size[1] as f32);
            let [cw, ch] = self.cell;
            let uv = Rect::from_min_max(
                egui::pos2((col * cw) as f32 / aw, (row * ch) as f32 / ah),
                egui::pos2(((col + 1) * cw) as f32 / aw, ((row + 1) * ch) as f32 / ah),
            );
            ui.painter()
                .image(self.texture.id(), rect, uv, Color32::WHITE);
            ui.painter().rect_stroke(
                rect,
                0.0,
                Stroke::new(1.0, Color32::from_black_alpha(50)),
                StrokeKind::Inside,
            );
        }
        response
    }
}
