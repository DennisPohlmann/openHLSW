//! Dialogs as real windows of the operating system (like HLSW's): native title bar, moved
//! and closed like any other window, instead of egui windows drawn inside the main window.
//! Same builder calls as `egui::Window`, so the dialogs only swap the type.
//!
//! Without multi-window support (some Linux setups) the dialog falls back to an egui window.

use std::hash::Hash;

use eframe::egui::{self, Frame, Margin, Ui, ViewportBuilder, ViewportClass, ViewportId};

pub struct NativeWindow<'a> {
    id: ViewportId,
    title: String,
    open: Option<&'a mut bool>,
    resizable: bool,
    size: Option<egui::Vec2>,
    margin: i8,
}

impl<'a> NativeWindow<'a> {
    pub fn new(id: impl Hash + std::fmt::Debug, title: impl Into<String>) -> Self {
        Self {
            id: ViewportId::from_hash_of(("dialog", id)),
            title: title.into(),
            open: None,
            resizable: false,
            size: None,
            margin: 8,
        }
    }

    /// Closed with the title bar's ×, Escape or by setting it to false.
    pub fn open(mut self, open: &'a mut bool) -> Self {
        self.open = Some(open);
        self
    }

    /// Kept for the same calls as `egui::Window`; native windows do not collapse.
    pub fn collapsible(self, _collapsible: bool) -> Self {
        self
    }

    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Start size (resizable windows); others fit their content.
    pub fn default_size(mut self, size: impl Into<egui::Vec2>) -> Self {
        self.size = Some(size.into());
        self
    }

    pub fn fixed_size(self, size: impl Into<egui::Vec2>) -> Self {
        self.default_size(size)
    }

    /// Start width; the height fits the content.
    pub fn default_width(mut self, width: f32) -> Self {
        self.size = Some(egui::vec2(width, 0.0));
        self
    }

    /// Inner margin of the window content (default 8).
    pub fn margin(mut self, margin: i8) -> Self {
        self.margin = margin;
        self
    }

    pub fn show<R>(self, ctx: &egui::Context, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        let NativeWindow {
            id,
            title,
            open,
            resizable,
            size,
            margin,
        } = self;
        if open.as_deref() == Some(&false) {
            return None;
        }
        // Content size measured in the last frame: windows that are not resizable fit it.
        let memory = egui::Id::unique(("dialog_size", id));
        let measured: Option<egui::Vec2> = ctx.data(|d| d.get_temp(memory));
        let inner = match (size, measured) {
            (_, Some(m)) if !resizable => m,
            (Some(s), _) if s.y > 0.0 => s,
            (Some(s), _) => egui::vec2(s.x, 200.0),
            (None, _) => egui::vec2(360.0, 160.0),
        };
        // Centred over the main window when it opens.
        let parent = ctx.input(|i| i.viewport().outer_rect);
        let mut builder = ViewportBuilder::default()
            .with_title(title.clone())
            .with_inner_size(inner)
            .with_resizable(resizable)
            .with_minimize_button(false)
            .with_maximize_button(resizable)
            .with_icon(crate::app_icon());
        if let Some(parent) = parent {
            builder = builder.with_position(parent.center() - inner / 2.0);
        }

        let fill = ctx.global_style().visuals.panel_fill;
        let mut add = Some(add);
        let mut result = None;
        let mut close = false;
        let mut new_size = None;
        ctx.show_viewport_immediate(id, builder, |ui, class| {
            let mut content = |ui: &mut Ui| {
                if let Some(add) = add.take() {
                    result = Some(add(ui));
                }
            };
            if class == ViewportClass::EmbeddedWindow {
                // egui already put us into an egui window inside the main window.
                content(ui);
                return;
            }
            if ui.input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
            egui::CentralPanel::default()
                .frame(Frame::new().fill(fill).inner_margin(Margin::same(margin)))
                .show(ui, |ui| {
                    // Measured in its own scope: the panel itself always spans the window.
                    let used = ui.scope(|ui| content(ui)).response.rect.size();
                    if !resizable {
                        let m = 2.0 * margin as f32;
                        let want = used + egui::vec2(m, m);
                        let want = egui::vec2(want.x.ceil(), want.y.ceil());
                        if measured.is_none_or(|old| (old - want).length() > 1.0) {
                            new_size = Some(want);
                        }
                    }
                });
        });
        if let Some(size) = new_size {
            ctx.data_mut(|d| d.insert_temp(memory, size));
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::InnerSize(size));
            ctx.request_repaint();
        }
        if close && let Some(open) = open {
            *open = false;
        }
        result
    }
}
