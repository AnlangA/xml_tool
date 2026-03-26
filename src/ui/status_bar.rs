use egui::{Frame, Margin, RichText};

use crate::utils::{compression_ratio, format_size};

use super::theme::Theme;

/// Status bar data for display.
#[derive(Default)]
pub struct StatusBarData {
    pub message: String,
    pub file_name: Option<String>,
    pub is_dirty: bool,
    pub original_size: usize,
    pub compressed_size: usize,
}

impl StatusBarData {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_message(&mut self, msg: impl Into<String>) {
        self.message = msg.into();
    }
}

/// Render the status bar.
pub fn show_status_bar(ctx: &egui::Context, data: &StatusBarData) {
    egui::TopBottomPanel::bottom("status_bar")
        .frame(
            Frame::default()
                .inner_margin(Margin::symmetric(8, 4))
                .fill(ctx.style().visuals.extreme_bg_color),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&data.message)
                        .small()
                        .color(Theme::TEXT_SECONDARY),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if data.is_dirty {
                        ui.label(RichText::new("● Unsaved").small().color(Theme::WARNING));
                        ui.separator();
                    }

                    if let Some(name) = &data.file_name {
                        ui.label(
                            RichText::new(format!("📄 {name}"))
                                .small()
                                .color(Theme::INFO),
                        );
                        ui.separator();
                    }

                    if data.original_size > 0 {
                        ui.label(
                            RichText::new(format_size(data.original_size))
                                .small()
                                .color(Theme::SUCCESS),
                        );

                        if data.compressed_size > 0 {
                            let pct = compression_ratio(data.original_size, data.compressed_size);
                            ui.label(
                                RichText::new(format!(
                                    "→ {} ({pct:.1}% saved)",
                                    format_size(data.compressed_size)
                                ))
                                .small()
                                .color(Theme::WARNING),
                            );
                        }
                    }
                });
            });
        });
}
