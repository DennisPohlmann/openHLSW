//! Typing in an open dropdown list jumps to the matching entry, like in Windows dropdowns:
//! "de" selects "Deutsch". Letters typed within a second add up; after a pause the search
//! starts over; Backspace removes the last letter. The entry gets the keyboard focus, so
//! Enter picks it and the arrow keys move on from there.
//!
//! ```ignore
//! combo.show_ui(ui, |ui| {
//!     let mut find = TypeAhead::begin(ui);
//!     for item in items {
//!         let r = ui.selectable_value(&mut value, item, item.label());
//!         find.item(&r, item.label());
//!     }
//!     find.finish(ui);
//! });
//! ```

use eframe::egui::{Align, Event, Id, Key, Response, Ui};

/// Pause after which typing starts a new search.
const RESET_SECONDS: f64 = 1.0;

#[derive(Clone, Default)]
struct State {
    /// Typed text, lower case.
    typed: String,
    last: f64,
    /// The matching entry still has to get the focus.
    pending: bool,
    /// No entry starts with the typed text: match the start of any word instead.
    words: bool,
}

pub struct TypeAhead {
    id: Id,
    state: State,
    matched: bool,
    /// An entry was picked this frame.
    picked: bool,
}

impl TypeAhead {
    /// Call first inside the dropdown list; reads the keys typed this frame.
    pub fn begin(ui: &Ui) -> Self {
        let id = ui.scope_id().with("typeahead");
        let mut state: State = ui.data(|d| d.get_temp(id)).unwrap_or_default();
        // Typing into a text field (e.g. an editable dropdown) is not a search.
        if !ui.ctx().text_edit_focused() {
            let (typed, backspace, now) = ui.input(|i| {
                let typed: String = i
                    .events
                    .iter()
                    .filter_map(|e| match e {
                        Event::Text(t) => Some(t.as_str()),
                        _ => None,
                    })
                    .collect();
                (typed, i.key_pressed(Key::Backspace), i.time)
            });
            if !typed.is_empty() {
                if now - state.last > RESET_SECONDS {
                    state.typed.clear();
                }
                state.typed.push_str(&typed.to_lowercase());
                state.last = now;
                state.pending = true;
                state.words = false;
            } else if backspace && !state.typed.is_empty() {
                state.typed.pop();
                state.last = now;
                state.pending = !state.typed.is_empty();
                state.words = false;
            }
        }
        Self {
            id,
            state,
            matched: false,
            picked: false,
        }
    }

    /// Checks one entry of the list (its response and the text it shows).
    pub fn item(&mut self, response: &Response, label: &str) {
        self.picked |= response.clicked();
        if self.matched || !self.state.pending || self.state.typed.is_empty() {
            return;
        }
        if matches(label, &self.state.typed, self.state.words) {
            self.matched = true;
            response.request_focus();
            response.scroll_to_me(Some(Align::Center));
        }
    }

    /// Call after the last entry.
    pub fn finish(mut self, ui: &Ui) {
        // Picked with Enter / Space: close the list like a mouse click does.
        if self.picked && ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Space)) {
            ui.close();
        }
        if self.matched || self.state.words {
            self.state.pending = false;
        } else if self.state.pending {
            // Nothing starts with the text: try word starts in the next frame.
            self.state.words = true;
            ui.ctx().request_repaint();
        }
        ui.data_mut(|d| d.insert_temp(self.id, self.state));
    }
}

/// `typed` (lower case) at the start of the label, or with `words` at the start of any word
/// ("united" finds "English (United States)").
fn matches(label: &str, typed: &str, words: bool) -> bool {
    let label = label.to_lowercase();
    if label.starts_with(typed) {
        return true;
    }
    words
        && label
            .match_indices(|c: char| !c.is_alphanumeric())
            .any(|(i, sep)| label[i + sep.len()..].starts_with(typed))
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn matching() {
        assert!(matches("Deutsch (Deutschland)", "de", false));
        assert!(!matches("English (United States)", "united", false));
        assert!(matches("English (United States)", "united", true));
        assert!(matches("Español (Argentina)", "esp", false));
        assert!(matches("Русский", "ру", false));
        assert!(!matches("Arial", "x", true));
    }
}
