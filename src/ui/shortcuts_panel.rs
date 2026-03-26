use egui::{Context, RichText, Window};

use super::theme::{Theme, Typography};

/// Keyboard shortcuts help panel
pub struct ShortcutsPanel {
    pub visible: bool,
}

impl Default for ShortcutsPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl ShortcutsPanel {
    pub fn new() -> Self {
        Self { visible: false }
    }
    
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }
    
    pub fn show(&mut self, ctx: &Context) {
        if !self.visible {
            return;
        }
        
        let visible = &mut self.visible;
        
        Window::new("⌨ Keyboard Shortcuts")
            .open(visible)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.heading(RichText::new("File Operations").size(Typography::HEADING_3));
                ui.add_space(8.0);
                
                egui::Grid::new("file_shortcuts")
                    .num_columns(2)
                    .spacing([40.0, 8.0])
                    .striped(true)
                    .show(ui, |ui| {
                        Self::shortcut_row_static(ui, "Ctrl+O", "Open XML file");
                        Self::shortcut_row_static(ui, "Ctrl+E", "Open EXI file");
                        Self::shortcut_row_static(ui, "Ctrl+S", "Save XML file");
                    });
                
                ui.add_space(16.0);
                ui.heading(RichText::new("Navigation").size(Typography::HEADING_3));
                ui.add_space(8.0);
                
                egui::Grid::new("nav_shortcuts")
                    .num_columns(2)
                    .spacing([40.0, 8.0])
                    .striped(true)
                    .show(ui, |ui| {
                        Self::shortcut_row_static(ui, "Ctrl+F", "Focus search");
                        Self::shortcut_row_static(ui, "Esc", "Clear search");
                    });
                
                ui.add_space(16.0);
                ui.heading(RichText::new("Tools").size(Typography::HEADING_3));
                ui.add_space(8.0);
                
                egui::Grid::new("tool_shortcuts")
                    .num_columns(2)
                    .spacing([40.0, 8.0])
                    .striped(true)
                    .show(ui, |ui| {
                        Self::shortcut_row_static(ui, "F1", "Show this help");
                        Self::shortcut_row_static(ui, "Ctrl+Q", "Quit application");
                    });
                
                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);
                
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("💡 Tip:")
                            .color(Theme::INFO)
                            .strong()
                    );
                    ui.label(
                        RichText::new("Press F1 anytime to show this panel")
                            .color(Theme::TEXT_SECONDARY)
                            .italics()
                    );
                });
            });
    }
    
    fn shortcut_row_static(ui: &mut egui::Ui, key: &str, description: &str) {
        ui.label(
            RichText::new(key)
                .color(Theme::ACCENT)
                .monospace()
                .strong()
        );
        ui.label(
            RichText::new(description)
                .color(Theme::TEXT_PRIMARY)
        );
        ui.end_row();
    }
}
