//! Look & feel. The light theme mimics HLSW on Windows (white list
//! views, grey chrome, square controls); the dark theme uses the same layout with
//! dark colors.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke,
    TextStyle, Visuals,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Light,
    Dark,
}

/// Text size (Settings → Misc): the size of the normal text in pixels (at 100 % display
/// scaling). Scales the whole UI so lists and columns stay aligned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextSize {
    #[serde(rename = "px12", alias = "normal")]
    Px12,
    #[default]
    #[serde(rename = "px13")]
    Px13,
    #[serde(rename = "px14", alias = "large")]
    Px14,
    #[serde(rename = "px15")]
    Px15,
    #[serde(rename = "px16", alias = "extralarge")]
    Px16,
}

impl TextSize {
    pub const ALL: [TextSize; 5] = [
        TextSize::Px12,
        TextSize::Px13,
        TextSize::Px14,
        TextSize::Px15,
        TextSize::Px16,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TextSize::Px12 => "12 px",
            TextSize::Px13 => "13 px",
            TextSize::Px14 => "14 px",
            TextSize::Px15 => "15 px",
            TextSize::Px16 => "16 px",
        }
    }

    fn pixels(self) -> f32 {
        match self {
            TextSize::Px12 => 12.0,
            TextSize::Px13 => 13.0,
            TextSize::Px14 => 14.0,
            TextSize::Px15 => 15.0,
            TextSize::Px16 => 16.0,
        }
    }

    /// UI zoom that makes the body text (11 pt) exactly this many pixels tall.
    pub fn zoom(self) -> f32 {
        self.pixels() / BODY_SIZE
    }
}

/// Colors used by the custom-painted parts of the UI (lists, headers, tabs, graph).
#[derive(Clone, Copy)]
pub struct Palette {
    pub window: Color32,
    pub list_bg: Color32,
    pub list_border: Color32,
    pub text: Color32,
    pub weak_text: Color32,
    pub disabled_text: Color32,
    pub header_bg: Color32,
    pub header_border: Color32,
    pub selection_bg: Color32,
    pub selection_text: Color32,
    pub hover_bg: Color32,
    pub offline_bg: Color32,
    pub red: Color32,
    pub green: Color32,
    pub link: Color32,
    pub graph_bg: Color32,
    pub graph_hatch: Color32,
    pub graph_line: Color32,
    pub button_bg: Color32,
    pub button_border: Color32,
    pub button_hover_bg: Color32,
    pub button_hover_border: Color32,
    pub button_active_bg: Color32,
    pub group_border: Color32,
    /// Console: sent commands (dark) and server answers (light), like HLSW.
    pub console_cmd_bg: Color32,
    pub console_cmd_fg: Color32,
    pub console_resp_bg: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

impl Theme {
    pub fn palette(self) -> Palette {
        match self {
            // Colors picked from HLSW 1.4 on Windows 10/11.
            Theme::Light => Palette {
                window: rgb(240, 240, 240),
                list_bg: rgb(255, 255, 255),
                list_border: rgb(130, 135, 144),
                text: rgb(0, 0, 0),
                weak_text: rgb(96, 96, 96),
                disabled_text: rgb(160, 160, 160),
                header_bg: rgb(255, 255, 255),
                header_border: rgb(229, 229, 229),
                selection_bg: rgb(90, 90, 90),
                selection_text: rgb(255, 255, 255),
                hover_bg: rgb(229, 243, 255),
                offline_bg: rgb(255, 222, 222),
                red: rgb(224, 0, 0),
                green: rgb(0, 160, 0),
                link: rgb(0, 0, 238),
                graph_bg: rgb(236, 238, 240),
                graph_hatch: rgb(200, 204, 210),
                graph_line: rgb(0, 102, 204),
                button_bg: rgb(225, 225, 225),
                button_border: rgb(173, 173, 173),
                button_hover_bg: rgb(229, 241, 251),
                button_hover_border: rgb(0, 120, 215),
                button_active_bg: rgb(204, 228, 247),
                group_border: rgb(220, 220, 220),
                console_cmd_bg: rgb(128, 128, 0),
                console_cmd_fg: rgb(255, 255, 208),
                console_resp_bg: rgb(255, 255, 200),
            },
            Theme::Dark => Palette {
                window: rgb(43, 43, 43),
                list_bg: rgb(30, 30, 30),
                list_border: rgb(70, 70, 70),
                text: rgb(222, 222, 222),
                weak_text: rgb(150, 150, 150),
                disabled_text: rgb(100, 100, 100),
                header_bg: rgb(38, 38, 38),
                header_border: rgb(60, 60, 60),
                selection_bg: rgb(64, 96, 140),
                selection_text: rgb(255, 255, 255),
                hover_bg: rgb(48, 56, 66),
                offline_bg: rgb(78, 36, 40),
                red: rgb(255, 110, 110),
                green: rgb(110, 214, 110),
                link: rgb(120, 170, 255),
                graph_bg: rgb(36, 40, 46),
                graph_hatch: rgb(52, 58, 66),
                graph_line: rgb(110, 170, 255),
                button_bg: rgb(58, 58, 58),
                button_border: rgb(90, 90, 90),
                button_hover_bg: rgb(64, 74, 86),
                button_hover_border: rgb(100, 150, 220),
                button_active_bg: rgb(54, 80, 112),
                group_border: rgb(70, 70, 70),
                console_cmd_bg: rgb(92, 92, 16),
                console_cmd_fg: rgb(255, 246, 192),
                console_resp_bg: rgb(48, 48, 28),
            },
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Theme::Light => tr!("White (HLSW classic)"),
            Theme::Dark => tr!("Black"),
        }
    }
}

/// Applies colors, square widgets and the compact Windows-like spacing.
pub fn apply(ctx: &egui::Context, theme: Theme) {
    let p = theme.palette();
    let mut v = if theme == Theme::Dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };

    v.override_text_color = Some(p.text);
    v.weak_text_color = Some(p.weak_text);
    // Crisper text: no sub-pixel glyph positions (they blur small UI fonts) and, in the
    // light skin, slightly stronger anti-aliasing so dark text does not look washed out.
    v.text_options.subpixel_binning = false;
    v.text_options.font_hinting = true;
    // Fuller, darker strokes for dark text on light ground (thin grey text looks blurry).
    if theme == Theme::Light {
        v.text_options.color_transfer_function =
            egui::epaint::FontColorTransferFunction::Gamma(0.55);
    }
    v.panel_fill = p.window;
    v.window_fill = p.window;
    v.window_stroke = Stroke::new(1.0, p.list_border);
    v.window_corner_radius = CornerRadius::ZERO;
    v.window_shadow = Shadow {
        offset: [2, 2],
        blur: 6,
        spread: 0,
        color: Color32::from_black_alpha(60),
    };
    v.popup_shadow = Shadow {
        offset: [2, 2],
        blur: 4,
        spread: 0,
        color: Color32::from_black_alpha(50),
    };
    v.menu_corner_radius = CornerRadius::ZERO;
    v.extreme_bg_color = p.list_bg;
    v.text_edit_bg_color = Some(p.list_bg);
    v.faint_bg_color = p.window;
    v.code_bg_color = p.list_bg;
    v.hyperlink_color = p.link;
    v.striped = false;
    v.selection.bg_fill = p.selection_bg;
    v.selection.stroke = Stroke::new(1.0, p.selection_text);

    let w = &mut v.widgets;
    for wv in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        wv.corner_radius = CornerRadius::ZERO;
        wv.expansion = 0.0;
        wv.fg_stroke = Stroke::new(1.0, p.text);
    }
    w.noninteractive.bg_fill = p.window;
    w.noninteractive.weak_bg_fill = p.window;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.header_border);
    w.inactive.bg_fill = p.list_bg; // checkbox / text edit inner fill
    w.inactive.weak_bg_fill = p.button_bg; // buttons
    w.inactive.bg_stroke = Stroke::new(1.0, p.button_border);
    w.hovered.bg_fill = p.hover_bg;
    w.hovered.weak_bg_fill = p.button_hover_bg;
    w.hovered.bg_stroke = Stroke::new(1.0, p.button_hover_border);
    w.active.bg_fill = p.button_active_bg;
    w.active.weak_bg_fill = p.button_active_bg;
    w.active.bg_stroke = Stroke::new(1.0, p.button_hover_border);
    w.open.bg_fill = p.button_active_bg;
    w.open.weak_bg_fill = p.button_active_bg;
    w.open.bg_stroke = Stroke::new(1.0, p.button_hover_border);

    ctx.set_theme(if theme == Theme::Dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    ctx.set_visuals(v);

    ctx.global_style_mut(|style| {
        style.text_styles = [
            (
                TextStyle::Small,
                FontId::new(10.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(11.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(11.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Heading,
                FontId::new(13.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(11.0, FontFamily::Monospace),
            ),
        ]
        .into();
        let s = &mut style.spacing;
        s.item_spacing = egui::vec2(6.0, 4.0);
        s.button_padding = egui::vec2(6.0, 2.0);
        s.interact_size = egui::vec2(20.0, 18.0);
        s.icon_width = 13.0;
        s.icon_width_inner = 7.0;
        s.icon_spacing = 4.0;
        s.menu_margin = egui::Margin::same(3);
        s.window_margin = egui::Margin::same(8);
        s.combo_width = 120.0;
    });
}

/// Font sizes of the interface (points before scaling).
/// Size of the normal text before the zoom of [`TextSize`].
const BODY_SIZE: f32 = 11.0;

const TEXT_SIZES: [(TextStyle, f32); 5] = [
    (TextStyle::Small, 10.0),
    (TextStyle::Body, BODY_SIZE),
    (TextStyle::Button, BODY_SIZE),
    (TextStyle::Heading, 13.0),
    (TextStyle::Monospace, 11.0),
];

/// Snaps the font sizes to whole screen pixels for the current scale (text size × display
/// scaling). A 10 pt font at 14 px would otherwise be 12.7 px tall: every glyph is then
/// rendered between two pixel rows and looks blurry. Cheap; called every frame.
pub fn snap_text_sizes(ctx: &egui::Context) {
    let ppp = ctx.pixels_per_point();
    let snapped = |size: f32| (size * ppp).round().max(1.0) / ppp;
    let current = ctx.global_style();
    let needed = TEXT_SIZES.iter().any(|(style, size)| {
        current
            .text_styles
            .get(style)
            .is_none_or(|f| (f.size - snapped(*size)).abs() > 0.001)
    });
    if needed {
        ctx.global_style_mut(|style| {
            for (text_style, size) in &TEXT_SIZES {
                let family = if *text_style == TextStyle::Monospace {
                    FontFamily::Monospace
                } else {
                    FontFamily::Proportional
                };
                style
                    .text_styles
                    .insert(text_style.clone(), FontId::new(snapped(*size), family));
            }
        });
    }
}

/// Light hinting (like FreeType's "slight" and ClearType's vertical-only grid fitting):
/// horizontal strokes and the baseline snap to whole pixels, so small UI text is sharp,
/// while glyph widths stay as designed. Full hinting also squeezes glyphs horizontally,
/// which no longer matches their advance widths and leaves uneven gaps ("Rem ove").
/// No sub-pixel binning: every glyph starts on a whole pixel instead of being blurred
/// across two.
fn crisp(mut data: FontData) -> FontData {
    use egui::epaint::text::{HintingTarget, SmoothHinting};
    data.tweak.hinting = Some(true);
    data.tweak.subpixel_binning = Some(false);
    data.tweak.hinting_target = HintingTarget::Smooth(SmoothHinting {
        light: true,
        symmetric_rendering: false,
        preserve_linear_metrics: false,
    });
    data
}

/// Font of the user interface (Settings → Misc → Font): the family name of an installed
/// font, or "System" for the system's user interface font. Old configs stored "System",
/// "Arial", "Tahoma", "LiberationSans" or "Inconsolata"; all are still understood.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UiFont(pub String);

impl Default for UiFont {
    /// The system's user interface font (Segoe UI on Windows).
    fn default() -> Self {
        UiFont::system()
    }
}

/// "System": Segoe UI on Windows, San Francisco / Helvetica on macOS, the desktop's
/// sans-serif font on Linux (fontconfig).
pub const SYSTEM_FONT: &str = "System";

/// Fonts that come with openHLSW (used when they are not installed).
const BUNDLED: [&str; 4] = ["Arial", "Tahoma", "Liberation Sans", "Inconsolata"];

impl UiFont {
    pub fn system() -> Self {
        UiFont(SYSTEM_FONT.into())
    }

    /// Family name (old config spellings fixed).
    pub fn family(&self) -> &str {
        match self.0.as_str() {
            "LiberationSans" => "Liberation Sans",
            name => name,
        }
    }

    pub fn label(&self) -> &str {
        if self.is_system() {
            tr!("System font")
        } else {
            self.family()
        }
    }

    pub fn is_system(&self) -> bool {
        self.0 == SYSTEM_FONT
    }

    /// Well-known files of the system font / Arial / Tahoma, tried before scanning all fonts.
    fn known_paths(&self) -> &'static [&'static str] {
        match self.family() {
            SYSTEM_FONT if cfg!(windows) => &[
                "C:\\Windows\\Fonts\\segoeui.ttf",
                "C:\\Windows\\Fonts\\tahoma.ttf",
            ],
            SYSTEM_FONT if cfg!(target_os = "macos") => &[
                "/System/Library/Fonts/SFNS.ttf",
                "/System/Library/Fonts/Helvetica.ttc",
                "/System/Library/Fonts/Supplemental/Arial.ttf",
            ],
            SYSTEM_FONT => &[
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/TTF/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf",
                "/usr/local/share/fonts/dejavu/DejaVuSans.ttf",
            ],
            "Arial" if cfg!(windows) => &["C:\\Windows\\Fonts\\arial.ttf"],
            "Arial" if cfg!(target_os = "macos") => &[
                "/System/Library/Fonts/Supplemental/Arial.ttf",
                "/Library/Fonts/Arial.ttf",
            ],
            "Tahoma" if cfg!(windows) => &["C:\\Windows\\Fonts\\tahoma.ttf"],
            "Tahoma" if cfg!(target_os = "macos") => &[
                "/System/Library/Fonts/Supplemental/Tahoma.ttf",
                "/Library/Fonts/Tahoma.ttf",
            ],
            _ => &[],
        }
    }

    /// The font file to use: an installed one, otherwise the bundled replacement.
    fn data(&self) -> FontData {
        let family = self.family();
        // Linux / BSD: ask fontconfig for the desktop's sans-serif font first.
        if self.is_system()
            && cfg!(all(unix, not(target_os = "macos")))
            && let Some(bytes) = fontconfig_sans()
        {
            return FontData::from_owned(bytes);
        }
        if let Some(bytes) = self
            .known_paths()
            .iter()
            .find_map(|p| std::fs::read(p).ok())
        {
            return FontData::from_owned(bytes);
        }
        // Any other installed font: look it up among all fonts of the system.
        if !self.is_system()
            && let Some(font) = scanned_fonts()
                .iter()
                .find(|f| f.family.eq_ignore_ascii_case(family))
            && let Ok(bytes) = std::fs::read(&font.path)
        {
            let mut data = FontData::from_owned(bytes);
            data.index = font.index;
            return data;
        }
        FontData::from_static(match family {
            "Tahoma" => BUNDLED_TAHOMA,
            "Liberation Sans" => LIBERATION_SANS,
            "Inconsolata" => INCONSOLATA,
            // Arial, the system font and fonts that are gone: Arial's look-alike.
            _ => BUNDLED_ARIAL,
        })
    }
}

/// An installed font family (its regular face).
pub struct SystemFont {
    pub family: String,
    path: std::path::PathBuf,
    index: u32,
}

static SYSTEM_FONTS: std::sync::OnceLock<Vec<SystemFont>> = std::sync::OnceLock::new();

/// Starts looking up the installed fonts in the background (takes a moment on systems with
/// many fonts), so the font list in the settings is ready when it is opened.
pub fn scan_fonts_in_background() {
    std::thread::spawn(|| {
        scanned_fonts();
    });
}

/// The installed fonts, `None` while they are still being looked up.
pub fn system_fonts() -> Option<&'static [SystemFont]> {
    SYSTEM_FONTS.get().map(Vec::as_slice)
}

/// The installed fonts (waits for the lookup).
fn scanned_fonts() -> &'static [SystemFont] {
    SYSTEM_FONTS.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        // Per family the face closest to regular (weight 400, upright, normal width).
        let mut best: std::collections::BTreeMap<String, (u32, SystemFont)> = Default::default();
        for face in db.faces() {
            let fontdb::Source::File(path) = &face.source else {
                continue;
            };
            let Some((family, _)) = face.families.first() else {
                continue;
            };
            // Names starting with "." are the system's private fonts (macOS).
            if family.is_empty() || family.starts_with('.') {
                continue;
            }
            let score = u32::from(face.weight.0.abs_diff(400))
                + if face.style == fontdb::Style::Normal { 0 } else { 1000 }
                + (face.stretch.to_number() as i32 - 5).unsigned_abs() * 100;
            let key = family.to_lowercase();
            if best.get(&key).is_none_or(|(s, _)| score < *s) {
                best.insert(
                    key,
                    (
                        score,
                        SystemFont {
                            family: family.clone(),
                            path: path.clone(),
                            index: face.index,
                        },
                    ),
                );
            }
        }
        best.into_values().map(|(_, f)| f).collect()
    })
}

/// All fonts the settings offer: the system font, the installed fonts and the bundled ones
/// that are not installed. `None` while the installed fonts are still being looked up.
pub fn font_choices() -> Option<Vec<UiFont>> {
    let installed = system_fonts()?;
    let mut out = vec![UiFont::system()];
    let mut names: Vec<&str> = installed.iter().map(|f| f.family.as_str()).collect();
    for b in BUNDLED {
        if !names.iter().any(|n| n.eq_ignore_ascii_case(b)) {
            names.push(b);
        }
    }
    names.sort_by_key(|n| n.to_lowercase());
    out.extend(names.into_iter().map(|n| UiFont(n.to_string())));
    Some(out)
}

/// Font list previews: the name of a font drawn in that font.
struct Preview {
    texture: egui::TextureHandle,
    /// Size in points.
    size: egui::Vec2,
}

type PreviewCache = std::collections::HashMap<(String, u32), Option<Preview>>;
static PREVIEWS: std::sync::Mutex<Option<PreviewCache>> = std::sync::Mutex::new(None);

/// `text` drawn in `font` at `size` points, for the font list in the settings. Fonts are
/// only read when their entry is shown, at most `budget` per frame (it is counted down);
/// `Err(true)` = not drawn yet (ask again next frame), `Err(false)` = this font cannot draw
/// the text (show it in the interface font instead).
pub fn font_preview(
    ctx: &egui::Context,
    font: &UiFont,
    text: &str,
    size: f32,
    budget: &mut u32,
) -> Result<(egui::TextureId, egui::Vec2), bool> {
    let ppp = ctx.pixels_per_point();
    let key = (font.0.clone(), (ppp * 100.0).round() as u32);
    let mut cache = PREVIEWS.lock().unwrap_or_else(|e| e.into_inner());
    let cache = cache.get_or_insert_with(Default::default);
    if let Some(entry) = cache.get(&key) {
        return entry.as_ref().map(|p| (p.texture.id(), p.size)).ok_or(false);
    }
    if *budget == 0 {
        return Err(true);
    }
    *budget -= 1;
    let preview = rasterize(&font.data(), text, size * ppp).map(|image| {
        let size = egui::vec2(image.size[0] as f32, image.size[1] as f32) / ppp;
        let texture = ctx.load_texture(
            format!("font-preview-{}", font.0),
            image,
            egui::TextureOptions::LINEAR,
        );
        Preview { texture, size }
    });
    let result = preview.as_ref().map(|p| (p.texture.id(), p.size)).ok_or(false);
    cache.insert(key, preview);
    result
}

/// White text with the coverage as alpha (tinted when drawn); `None` if the font has none of
/// the letters.
fn rasterize(data: &FontData, text: &str, em_px: f32) -> Option<egui::ColorImage> {
    use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
    let font = FontRef::try_from_slice_and_index((*data.font).as_ref(), data.index).ok()?;
    let units_per_em = font.units_per_em()?;
    let scale = PxScale::from(em_px * font.height_unscaled() / units_per_em);
    let scaled = font.as_scaled(scale);
    if text.chars().all(|c| font.glyph_id(c).0 == 0) {
        return None;
    }
    let (ascent, descent) = (scaled.ascent(), scaled.descent());
    let mut glyphs = Vec::new();
    let mut x = 1.0;
    let mut previous = None;
    for c in text.chars() {
        let id = font.glyph_id(c);
        if let Some(p) = previous {
            x += scaled.kern(p, id);
        }
        glyphs.push(id.with_scale_and_position(scale, point(x, ascent.ceil() + 1.0)));
        x += scaled.h_advance(id);
        previous = Some(id);
    }
    let (w, h) = (x.ceil() as usize + 2, (ascent.ceil() - descent.floor()) as usize + 2);
    if w > 4000 || h > 400 {
        return None;
    }
    let mut alpha = vec![0u8; w * h];
    for glyph in glyphs {
        let Some(outline) = font.outline_glyph(glyph) else {
            continue;
        };
        let bounds = outline.px_bounds();
        outline.draw(|gx, gy, coverage| {
            let px = bounds.min.x as i64 + i64::from(gx);
            let py = bounds.min.y as i64 + i64::from(gy);
            if (0..w as i64).contains(&px) && (0..h as i64).contains(&py) {
                let a = &mut alpha[py as usize * w + px as usize];
                *a = (*a).max((coverage.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
        });
    }
    let pixels = alpha
        .into_iter()
        .map(egui::Color32::from_white_alpha)
        .collect();
    Some(egui::ColorImage::new([w, h], pixels))
}

/// The file of fontconfig's "sans-serif" (a .ttf / .otf, not a collection).
fn fontconfig_sans() -> Option<Vec<u8>> {
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", "sans-serif:style=Regular"])
        .output()
        .ok()?;
    let path = String::from_utf8(out.stdout).ok()?;
    let path = path.trim();
    let lower = path.to_ascii_lowercase();
    if !(lower.ends_with(".ttf") || lower.ends_with(".otf")) {
        return None;
    }
    std::fs::read(path).ok()
}

/// Bundled fonts, see `assets/fonts/NOTICE.txt`: Inconsolata and Liberation Sans (SIL Open
/// Font License 1.1, `OFL.txt`), Wine's Tahoma (LGPL-2.1-or-later, `LGPL-2.1.txt`).
/// Windows always has Arial and Tahoma, so their replacements are only bundled elsewhere.
static INCONSOLATA: &[u8] = include_bytes!("../assets/fonts/Inconsolata-Regular.ttf");
static LIBERATION_SANS: &[u8] = include_bytes!("../assets/fonts/LiberationSans-Regular.ttf");
#[cfg(not(windows))]
static BUNDLED_ARIAL: &[u8] = LIBERATION_SANS;
#[cfg(not(windows))]
static BUNDLED_TAHOMA: &[u8] = include_bytes!("../assets/fonts/tahoma.ttf");
#[cfg(windows)]
static BUNDLED_ARIAL: &[u8] = INCONSOLATA;
#[cfg(windows)]
static BUNDLED_TAHOMA: &[u8] = INCONSOLATA;

/// Installs the chosen interface font (Inconsolata for monospace text) with fallbacks for
/// all other scripts.
pub fn install_fonts(ctx: &egui::Context, font: &UiFont) {
    // Fallbacks for characters the interface font does not have (symbols, CJK, Cyrillic variants,
    // emoji, ...) - player and server names use all of Unicode.
    let fallbacks: &[&str] = if cfg!(windows) {
        &[
            "C:\\Windows\\Fonts\\segoeui.ttf",
            "C:\\Windows\\Fonts\\seguisym.ttf",
            "C:\\Windows\\Fonts\\seguiemj.ttf",
            "C:\\Windows\\Fonts\\arialuni.ttf",
            "C:\\Windows\\Fonts\\msyh.ttc",
            "C:\\Windows\\Fonts\\msgothic.ttc",
            "C:\\Windows\\Fonts\\malgun.ttf",
            "C:\\Windows\\Fonts\\Nirmala.ttf",
            "C:\\Windows\\Fonts\\ebrima.ttf",
            "C:\\Windows\\Fonts\\taile.ttf",
            "C:\\Windows\\Fonts\\himalaya.ttf",
            "C:\\Windows\\Fonts\\mmrtext.ttf",
            "C:\\Windows\\Fonts\\phagspa.ttf",
            "C:\\Windows\\Fonts\\gadugi.ttf",
        ]
    } else if cfg!(target_os = "macos") {
        &[
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
            "/Library/Fonts/Arial Unicode.ttf",
            "/System/Library/Fonts/Apple Symbols.ttf",
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/AppleSDGothicNeo.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
        ]
    } else {
        &[
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansSymbols-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "/usr/share/fonts/truetype/unifont/unifont.ttf",
        ]
    };

    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("ui".to_owned(), crisp(font.data()).into());
    fonts.font_data.insert(
        "inconsolata".to_owned(),
        crisp(FontData::from_static(INCONSOLATA)).into(),
    );
    let first = |fonts: &mut FontDefinitions, family: FontFamily, name: &str| {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, name.to_owned());
    };
    first(&mut fonts, FontFamily::Proportional, "ui");
    first(&mut fonts, FontFamily::Monospace, "inconsolata");

    // Fallbacks go right after the main font (before egui's bundled fonts) in both families.
    let mut position = 1;
    for (i, path) in fallbacks.iter().enumerate() {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let name = format!("fallback-{i}");
        fonts
            .font_data
            .insert(name.clone(), crisp(FontData::from_owned(bytes)).into());
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            let list = fonts.families.entry(family).or_default();
            let at = position.min(list.len());
            list.insert(at, name.clone());
        }
        position += 1;
    }
    ctx.set_fonts(fonts);
}
