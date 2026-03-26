use egui::{RichText, TextEdit, Ui};

use super::icons::Icons;
use super::theme::Theme;

/// Search bar for filtering XML tree nodes.
#[derive(Default)]
pub struct SearchBar {
    pub query: String,
    pub case_sensitive: bool,
    focused: bool,
}

impl SearchBar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the search bar UI.
    pub fn show(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;

        ui.horizontal(|ui| {
            ui.label(RichText::new(Icons::SEARCH).color(Theme::TEXT_SECONDARY));

            let response = ui.add(
                TextEdit::singleline(&mut self.query)
                    .hint_text("Search nodes...")
                    .desired_width(150.0)
                    .interactive(true),
            );

            if response.changed() {
                changed = true;
            }

            // Focus on first frame if requested
            if self.focused {
                response.request_focus();
                self.focused = false;
            }

            // Case sensitivity toggle
            let case_text = if self.case_sensitive { "Aa" } else { "aa" };
            let case_color = if self.case_sensitive {
                Theme::ACCENT
            } else {
                Theme::TEXT_MUTED
            };

            if ui
                .button(RichText::new(case_text).color(case_color).small())
                .clicked()
            {
                self.case_sensitive = !self.case_sensitive;
                changed = true;
            }

            // Clear button
            if !self.query.is_empty() && ui.button(Icons::CLOSE).clicked() {
                self.query.clear();
                changed = true;
            }
        });

        changed
    }

    /// Request focus on the search input.
    pub fn focus(&mut self) {
        self.focused = true;
    }

    pub fn clear(&mut self) -> bool {
        if self.query.is_empty() {
            false
        } else {
            self.query.clear();
            true
        }
    }
}
