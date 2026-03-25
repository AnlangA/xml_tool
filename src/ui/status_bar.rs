use egui::{Color32, Frame, Margin, RichText};

/// Status bar data for display.
pub struct StatusBarData {
    pub message: String,
    pub file_name: Option<String>,
    pub original_size: usize,
    pub compressed_size: usize,
}

impl Default for StatusBarData {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusBarData {
    pub fn new() -> Self {
        Self {
            message: String::new(),
            file_name: None,
            original_size: 0,
            compressed_size: 0,
        }
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
                ui.label(RichText::new(&data.message).small());

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(name) = &data.file_name {
                        ui.label(
                            RichText::new(format!("📄 {name}"))
                                .small()
                                .color(Color32::from_rgb(137, 180, 250)),
                        );
                        ui.separator();
                    }

                    if data.original_size > 0 {
                        ui.label(
                            RichText::new(format_size(data.original_size))
                                .small()
                                .color(Color32::from_rgb(166, 227, 161)),
                        );

                        if data.compressed_size > 0 {
                            let pct = compression_ratio(data.original_size, data.compressed_size);
                            ui.label(
                                RichText::new(format!(
                                    "→ {} ({pct:.1}% saved)",
                                    format_size(data.compressed_size)
                                ))
                                .small()
                                .color(Color32::from_rgb(249, 226, 175)),
                            );
                        }
                    }
                });
            });
        });
}

fn format_size(bytes: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = 1024 * KB;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn compression_ratio(original: usize, compressed: usize) -> f64 {
    if original == 0 {
        return 0.0;
    }
    100.0 - (compressed as f64 / original as f64 * 100.0)
}
