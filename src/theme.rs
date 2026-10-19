//! Look & feel. The light theme mimics HLSW on Windows (Tahoma 8pt, white list
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

/// Text size (Settings → Misc). Scales the whole UI so lists and columns stay aligned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextSize {
    #[default]
    Normal,
    Large,
    ExtraLarge,
}

impl TextSize {
    pub const ALL: [TextSize; 3] = [TextSize::Normal, TextSize::Large, TextSize::ExtraLarge];

    pub fn label(self) -> &'static str {
        match self {
            TextSize::Normal => "Normal (100 %)",
            TextSize::Large => "Large (115 %)",
            TextSize::ExtraLarge => "Extra large (130 %)",
        }
    }

    pub fn zoom(self) -> f32 {
        match self {
            TextSize::Normal => 1.0,
            TextSize::Large => 1.15,
            TextSize::ExtraLarge => 1.3,
        }
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
            Theme::Light => "White (HLSW classic)",
            Theme::Dark => "Black",
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
    if theme == Theme::Light {
        v.text_options.color_transfer_function =
            egui::epaint::FontColorTransferFunction::Gamma(0.8);
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
        s.item_spacing = egui::vec2(6.0, 3.0);
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

/// Uses Tahoma (HLSW's font) if the system has it, otherwise a close substitute.
/// The bundled egui fonts stay as fallbacks for missing glyphs.
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

pub fn install_fonts(ctx: &egui::Context) {
    let candidates: &[&str] = if cfg!(windows) {
        &[
            "C:\\Windows\\Fonts\\tahoma.ttf",
            "C:\\Windows\\Fonts\\segoeui.ttf",
        ]
    } else if cfg!(target_os = "macos") {
        &[
            "/System/Library/Fonts/Supplemental/Tahoma.ttf",
            "/Library/Fonts/Tahoma.ttf",
            "/System/Library/Fonts/Supplemental/Verdana.ttf",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
        ]
    } else {
        &[
            "/usr/share/fonts/truetype/msttcorefonts/tahoma.ttf",
            "/usr/share/fonts/truetype/msttcorefonts/Tahoma.ttf",
            "/usr/share/fonts/TTF/tahoma.ttf",
            "/usr/share/wine/fonts/tahoma.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        ]
    };
    let mono_candidates: &[&str] = if cfg!(windows) {
        &[
            "C:\\Windows\\Fonts\\consola.ttf",
            "C:\\Windows\\Fonts\\cour.ttf",
        ]
    } else if cfg!(target_os = "macos") {
        &[
            "/System/Library/Fonts/Supplemental/Andale Mono.ttf",
            "/System/Library/Fonts/Supplemental/Courier New.ttf",
        ]
    } else {
        &[
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
        ]
    };

    // Fallbacks for characters Tahoma does not have (symbols, CJK, Cyrillic variants,
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
    let add_first =
        |fonts: &mut FontDefinitions, name: &str, paths: &[&str], family: FontFamily| {
            if let Some(bytes) = paths.iter().find_map(|p| std::fs::read(p).ok()) {
                fonts
                    .font_data
                    .insert(name.to_owned(), crisp(FontData::from_owned(bytes)).into());
                fonts
                    .families
                    .entry(family)
                    .or_default()
                    .insert(0, name.to_owned());
            }
        };
    add_first(
        &mut fonts,
        "system-ui",
        candidates,
        FontFamily::Proportional,
    );
    add_first(
        &mut fonts,
        "system-mono",
        mono_candidates,
        FontFamily::Monospace,
    );

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
