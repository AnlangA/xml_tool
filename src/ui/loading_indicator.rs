use egui::{Context, RichText, Window};

use super::theme::Theme;

/// Loading indicator with progress
pub struct LoadingIndicator {
    pub visible: bool,
    pub message: String,
    pub progress: Option<f32>, // 0.0 - 1.0, None for indeterminate
    rotation: f32,
}

impl Default for LoadingIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadingIndicator {
    pub fn new() -> Self {
        Self {
            visible: false,
            message: "Loading...".to_string(),
            progress: None,
            rotation: 0.0,
        }
    }
    
    pub fn show_loading(&mut self, message: impl Into<String>) {
        self.visible = true;
        self.message = message.into();
        self.progress = None;
    }
    
    pub fn show_progress(&mut self, message: impl Into<String>, progress: f32) {
        self.visible = true;
        self.message = message.into();
        self.progress = Some(progress.clamp(0.0, 1.0));
    }
    
    pub fn hide(&mut self) {
        self.visible = false;
    }
    
    pub fn show(&mut self, ctx: &Context) {
        if !self.visible {
            return;
        }
        
        // Animate rotation for spinner
        self.rotation += 0.05;
        if self.rotation > std::f32::consts::TAU {
            self.rotation = 0.0;
        }
        ctx.request_repaint();
        
        Window::new("##loading")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_min_width(250.0);
                
                ui.vertical_centered(|ui| {
                    ui.add_space(10.0);
                    
                    // Spinner or progress bar
                    if let Some(progress) = self.progress {
                        // Progress bar
                        let progress_bar = egui::ProgressBar::new(progress)
                            .text(format!("{:.0}%", progress * 100.0))
                            .animate(true);
                        ui.add(progress_bar);
                    } else {
                        // Spinning indicator
                        ui.spinner();
                    }
                    
                    ui.add_space(10.0);
                    
                    // Message
                    ui.label(
                        RichText::new(&self.message)
                            .color(Theme::TEXT_PRIMARY)
                    );
                    
                    ui.add_space(10.0);
                });
            });
    }
}
