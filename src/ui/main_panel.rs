use std::path::PathBuf;

use egui::{Color32, Context, FontId, Frame, Margin, RichText};

use crate::exi::{decode_exi_to_xml, encode_xml_to_exi};
use crate::xml::{parse_xml_file, serialize_xml, XmlDocument};

use super::file_dialog::{FileDialogAction, FileDialogManager, FileDialogResult};
use super::status_bar::{show_status_bar, StatusBarData};
use super::xml_tree::XmlTreeView;

/// Whether the current document originated from XML or EXI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FileType {
    Xml,
    Exi,
}

// ---------------------------------------------------------------------------
// MainPanel
// ---------------------------------------------------------------------------

pub struct MainPanel {
    // Document state
    current_document: Option<XmlDocument>,
    current_file_path: Option<PathBuf>,
    current_file_type: FileType,
    exi_data: Option<Vec<u8>>,

    // UI state
    xml_tree_view: XmlTreeView,
    raw_xml_cache: Option<String>,
    show_raw_xml: bool,

    // Status bar
    status_data: StatusBarData,

    // Async file dialog
    file_dialog: FileDialogManager,
}

impl Default for MainPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl MainPanel {
    pub fn new() -> Self {
        let mut status_data = StatusBarData::new();
        status_data.set_message("Ready — open an XML or EXI file to get started.");

        Self {
            current_document: None,
            current_file_path: None,
            current_file_type: FileType::Xml,
            exi_data: None,

            xml_tree_view: XmlTreeView::new(),
            raw_xml_cache: None,
            show_raw_xml: false,

            status_data,

            file_dialog: FileDialogManager::new(),
        }
    }

    pub fn show(&mut self, ctx: &Context) {
        self.poll_file_dialog();
        self.show_menu_bar(ctx);
        self.show_toolbar(ctx);
        show_status_bar(ctx, &self.status_data);
        self.show_body(ctx);

        if self.file_dialog.is_pending() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    // -----------------------------------------------------------------------
    // File dialog
    // -----------------------------------------------------------------------

    fn poll_file_dialog(&mut self) {
        if let Some(result) = self.file_dialog.poll() {
            match result {
                FileDialogResult::OpenXml(Some(p)) => self.load_xml(&p),
                FileDialogResult::OpenExi(Some(p)) => self.load_exi(&p),
                FileDialogResult::SaveXml(Some(p)) => self.save_xml(&p),
                FileDialogResult::SaveExi(Some(p)) => self.save_exi(&p),
                _ => {}
            }
        }
    }

    fn open_file_dialog(&mut self, action: FileDialogAction) {
        self.file_dialog.open(action);
    }

    // -----------------------------------------------------------------------
    // Document operations
    // -----------------------------------------------------------------------

    fn load_xml(&mut self, path: &PathBuf) {
        match parse_xml_file(path) {
            Ok(doc) => {
                self.status_data.original_size =
                    std::fs::metadata(path).map(|m| m.len() as usize).unwrap_or(0);
                self.status_data.compressed_size = 0;
                self.exi_data = None;
                self.current_document = Some(doc);
                self.current_file_path = Some(path.clone());
                self.current_file_type = FileType::Xml;
                self.xml_tree_view.clear_selection();
                self.invalidate_raw_xml_cache();
                self.update_file_name(path);
                self.set_status(format!("Opened: {}", path.display()));
            }
            Err(e) => self.set_status(format!("Error opening XML: {e}")),
        }
    }

    fn load_exi(&mut self, path: &PathBuf) {
        match std::fs::read(path) {
            Ok(data) => match decode_exi_to_xml(&data) {
                Ok(doc) => {
                    self.status_data.original_size = data.len();
                    self.status_data.compressed_size = 0;
                    self.exi_data = Some(data);
                    self.current_document = Some(doc);
                    self.current_file_path = Some(path.clone());
                    self.current_file_type = FileType::Exi;
                    self.xml_tree_view.clear_selection();
                    self.invalidate_raw_xml_cache();
                    self.update_file_name(path);
                    self.set_status(format!("Opened EXI: {}", path.display()));
                }
                Err(e) => self.set_status(format!("EXI decode error: {e}")),
            },
            Err(e) => self.set_status(format!("Cannot read file: {e}")),
        }
    }

    fn compress_to_exi(&mut self) {
        let Some(doc) = &self.current_document else {
            self.set_status("No document loaded.");
            return;
        };

        match serialize_xml(doc) {
            Ok(xml_str) => {
                self.status_data.original_size = xml_str.len();
                match encode_xml_to_exi(&xml_str) {
                    Ok(exi) => {
                        self.status_data.compressed_size = exi.len();
                        let pct = compression_ratio(
                            self.status_data.original_size,
                            self.status_data.compressed_size,
                        );
                        self.exi_data = Some(exi);
                        self.set_status(format!("Compressed — {pct:.1}% size reduction."));
                    }
                    Err(e) => self.set_status(format!("Compression error: {e}")),
                }
            }
            Err(e) => self.set_status(format!("Serialisation error: {e}")),
        }
    }

    fn decompress_from_exi(&mut self) {
        let Some(exi) = &self.exi_data else {
            self.set_status("No EXI data — compress a document first.");
            return;
        };

        match decode_exi_to_xml(exi) {
            Ok(doc) => {
                self.current_document = Some(doc);
                self.xml_tree_view.clear_selection();
                self.invalidate_raw_xml_cache();
                self.set_status("Decompressed from EXI.");
            }
            Err(e) => self.set_status(format!("Decompression error: {e}")),
        }
    }

    fn save_xml(&mut self, path: &PathBuf) {
        let Some(doc) = &self.current_document else {
            return;
        };
        match serialize_xml(doc) {
            Ok(xml_str) => match std::fs::write(path, &xml_str) {
                Ok(_) => self.set_status(format!("Saved XML: {}", path.display())),
                Err(e) => self.set_status(format!("Write error: {e}")),
            },
            Err(e) => self.set_status(format!("Serialisation error: {e}")),
        }
    }

    fn save_exi(&mut self, path: &PathBuf) {
        let Some(exi) = &self.exi_data else {
            return;
        };
        match std::fs::write(path, exi) {
            Ok(_) => self.set_status(format!("Saved EXI: {}", path.display())),
            Err(e) => self.set_status(format!("Write error: {e}")),
        }
    }

    // -----------------------------------------------------------------------
    // UI panels
    // -----------------------------------------------------------------------

    fn show_menu_bar(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("menu_bar")
            .frame(
                Frame::default()
                    .inner_margin(Margin::symmetric(8, 4))
                    .fill(ctx.style().visuals.panel_fill),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.menu_button("File", |ui| {
                        if ui.button("Open XML…").clicked() {
                            self.open_file_dialog(FileDialogAction::OpenXml);
                            ui.close();
                        }
                        if ui.button("Open EXI…").clicked() {
                            self.open_file_dialog(FileDialogAction::OpenExi);
                            ui.close();
                        }
                        ui.separator();
                        ui.add_enabled_ui(self.current_document.is_some(), |ui| {
                            if ui.button("Save XML As…").clicked() {
                                self.open_file_dialog(FileDialogAction::SaveXml);
                                ui.close();
                            }
                        });
                        ui.add_enabled_ui(self.exi_data.is_some(), |ui| {
                            if ui.button("Save EXI As…").clicked() {
                                self.open_file_dialog(FileDialogAction::SaveExi);
                                ui.close();
                            }
                        });
                        ui.separator();
                        if ui.button("Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });

                    ui.menu_button("Tools", |ui| {
                        ui.add_enabled_ui(self.current_document.is_some(), |ui| {
                            if ui.button("Compress to EXI").clicked() {
                                self.compress_to_exi();
                                ui.close();
                            }
                        });
                        ui.add_enabled_ui(self.exi_data.is_some(), |ui| {
                            if ui.button("Decompress from EXI").clicked() {
                                self.decompress_from_exi();
                                ui.close();
                            }
                        });
                    });

                    ui.menu_button("View", |ui| {
                        ui.checkbox(&mut self.show_raw_xml, "Show Raw XML");
                    });
                });
            });
    }

    fn show_toolbar(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("toolbar")
            .frame(
                Frame::default()
                    .inner_margin(Margin::symmetric(8, 4))
                    .fill(ctx.style().visuals.extreme_bg_color),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("📂 Open XML").clicked() {
                        self.open_file_dialog(FileDialogAction::OpenXml);
                    }
                    if ui.button("📂 Open EXI").clicked() {
                        self.open_file_dialog(FileDialogAction::OpenExi);
                    }

                    ui.separator();

                    ui.add_enabled_ui(self.current_document.is_some(), |ui| {
                        if ui.button("💾 Save XML").clicked() {
                            self.open_file_dialog(FileDialogAction::SaveXml);
                        }
                    });
                    ui.add_enabled_ui(self.exi_data.is_some(), |ui| {
                        if ui.button("💾 Save EXI").clicked() {
                            self.open_file_dialog(FileDialogAction::SaveExi);
                        }
                    });

                    ui.separator();

                    ui.add_enabled_ui(self.current_document.is_some(), |ui| {
                        if ui.button("▶ Compress").clicked() {
                            self.compress_to_exi();
                        }
                    });
                    ui.add_enabled_ui(self.exi_data.is_some(), |ui| {
                        if ui.button("◀ Decompress").clicked() {
                            self.decompress_from_exi();
                        }
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.checkbox(&mut self.show_raw_xml, "Raw XML");
                    });
                });
            });
    }

    fn show_body(&mut self, ctx: &Context) {
        egui::SidePanel::left("tree_panel")
            .default_width(350.0)
            .min_width(200.0)
            .max_width(600.0)
            .resizable(true)
            .frame(
                Frame::default()
                    .inner_margin(0.0)
                    .fill(ctx.style().visuals.panel_fill),
            )
            .show(ctx, |ui| {
                self.show_tree_panel(ui);
            });

        egui::CentralPanel::default()
            .frame(
                Frame::default()
                    .inner_margin(0.0)
                    .fill(ctx.style().visuals.panel_fill),
            )
            .show(ctx, |ui| {
                self.show_detail_panel(ui);
            });
    }

    fn show_tree_panel(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.heading(
                    RichText::new("XML Structure")
                        .font(FontId::proportional(14.0))
                        .strong(),
                );
            });
            ui.separator();
        });

        if let Some(doc) = &self.current_document {
            let root = doc.root.clone();
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    self.xml_tree_view.show(ui, &root);
                });
        } else {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.label(
                    RichText::new("No file loaded")
                        .color(Color32::GRAY)
                        .italics(),
                );
                ui.add_space(10.0);
                ui.label(
                    RichText::new("Open an XML or EXI file to view its structure")
                        .small()
                        .color(Color32::DARK_GRAY),
                );
                ui.add_space(20.0);
                if ui.button("📂 Open XML File").clicked() {
                    self.open_file_dialog(FileDialogAction::OpenXml);
                }
            });
        }
    }

    fn show_detail_panel(&mut self, ui: &mut egui::Ui) {
        egui::TopBottomPanel::top("detail_tabs")
            .frame(Frame::default().inner_margin(0.0))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.show_raw_xml, false, "Details");
                    ui.selectable_value(&mut self.show_raw_xml, true, "Raw XML");
                });
                ui.separator();
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if self.show_raw_xml {
                self.show_raw_xml_tab(ui);
            } else {
                self.show_details_tab(ui);
            }
        });
    }

    fn show_details_tab(&mut self, ui: &mut egui::Ui) {
        let selected = self.xml_tree_view.get_selected_info().cloned();

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                ui.add_space(10.0);

                if let Some(info) = selected {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Element:")
                                .strong()
                                .color(Color32::from_rgb(180, 190, 254)),
                        );
                        ui.label(
                            RichText::new(&info.name)
                                .font(FontId::monospace(16.0))
                                .color(Color32::from_rgb(166, 227, 161)),
                        );
                    });

                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);

                    if !info.attributes.is_empty() {
                        ui.label(
                            RichText::new("Attributes")
                                .strong()
                                .color(Color32::from_rgb(137, 180, 250)),
                        );
                        ui.add_space(5.0);

                        egui::Grid::new("attr_grid")
                            .num_columns(2)
                            .spacing([10.0, 4.0])
                            .show(ui, |ui| {
                                for (key, value) in &info.attributes {
                                    ui.label(
                                        RichText::new(key)
                                            .font(FontId::monospace(12.0))
                                            .color(Color32::from_rgb(249, 226, 175)),
                                    );
                                    ui.label(
                                        RichText::new(value)
                                            .font(FontId::monospace(12.0))
                                            .color(Color32::from_rgb(205, 214, 244)),
                                    );
                                    ui.end_row();
                                }
                            });

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);
                    }

                    if let Some(text) = &info.text {
                        let t = text.trim();
                        if !t.is_empty() {
                            ui.label(
                                RichText::new("Text Content")
                                    .strong()
                                    .color(Color32::from_rgb(137, 180, 250)),
                            );
                            ui.add_space(5.0);
                            Frame::new()
                                .fill(Color32::from_rgb(30, 30, 46))
                                .inner_margin(8.0)
                                .corner_radius(4.0)
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(t)
                                            .font(FontId::monospace(12.0))
                                            .color(Color32::from_rgb(205, 214, 244)),
                                    );
                                });
                            ui.add_space(10.0);
                        }
                    }

                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Children:")
                                .strong()
                                .color(Color32::from_rgb(137, 180, 250)),
                        );
                        ui.label(
                            RichText::new(info.child_count.to_string())
                                .font(FontId::monospace(14.0)),
                        );
                    });
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.label(
                            RichText::new("Select a node in the tree")
                                .color(Color32::GRAY)
                                .italics(),
                        );
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new("to view its details here")
                                .small()
                                .color(Color32::DARK_GRAY),
                        );
                    });
                }
            });
    }

    fn show_raw_xml_tab(&mut self, ui: &mut egui::Ui) {
        if self.raw_xml_cache.is_none()
            && let Some(doc) = &self.current_document
        {
            self.raw_xml_cache = Some(match serialize_xml(doc) {
                Ok(xml) => xml,
                Err(e) => format!("<!-- serialisation error: {e} -->"),
            });
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                ui.add_space(10.0);

                if let Some(xml) = &self.raw_xml_cache {
                    Frame::new()
                        .fill(Color32::from_rgb(30, 30, 46))
                        .inner_margin(10.0)
                        .corner_radius(4.0)
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut xml.as_str())
                                    .font(FontId::monospace(12.0))
                                    .desired_rows(20)
                                    .desired_width(f32::INFINITY),
                            );
                        });
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.label(RichText::new("No XML loaded").color(Color32::GRAY).italics());
                    });
                }
            });
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status_data.set_message(msg);
    }

    fn invalidate_raw_xml_cache(&mut self) {
        self.raw_xml_cache = None;
    }

    fn update_file_name(&mut self, path: &std::path::Path) {
        self.status_data.file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(String::from);
    }
}

fn compression_ratio(original: usize, compressed: usize) -> f64 {
    if original == 0 {
        return 0.0;
    }
    100.0 - (compressed as f64 / original as f64 * 100.0)
}
