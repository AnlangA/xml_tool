use std::path::PathBuf;

use egui::{Context, FontId, Frame, Key, Margin, Modifiers, RichText};

use crate::exi::{decode_exi_to_xml, encode_xml_to_exi};
use crate::export::export_to_json;
use crate::utils::compression_ratio;
use crate::xml::{XmlDocument, parse_xml_file, serialize_xml};

use super::file_dialog::{FileDialogAction, FileDialogManager, FileDialogResult};
use super::search_bar::SearchBar;
use super::shortcuts_panel::ShortcutsPanel;
use super::status_bar::{StatusBarData, show_status_bar};
use super::syntax_highlighter::SyntaxHighlighter;
use super::theme::Theme;
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
    search_bar: SearchBar,
    shortcuts_panel: ShortcutsPanel,
    raw_xml_cache: Option<String>,
    show_raw_xml: bool,
    syntax_highlighter: SyntaxHighlighter,

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
            search_bar: SearchBar::new(),
            shortcuts_panel: ShortcutsPanel::new(),
            raw_xml_cache: None,
            show_raw_xml: false,
            syntax_highlighter: SyntaxHighlighter::new(),

            status_data,

            file_dialog: FileDialogManager::new(),
        }
    }

    pub fn show(&mut self, ctx: &Context) {
        self.handle_shortcuts(ctx);
        self.poll_file_dialog();
        self.show_menu_bar(ctx);
        show_status_bar(ctx, &self.status_data);
        self.show_body(ctx);
        
        // Show shortcuts panel if visible
        self.shortcuts_panel.show(ctx);

        if self.file_dialog.is_pending() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    // -----------------------------------------------------------------------
    // Keyboard shortcuts
    // -----------------------------------------------------------------------

    fn handle_shortcuts(&mut self, ctx: &Context) {
        let ctrl_o = egui::KeyboardShortcut::new(Modifiers::CTRL, Key::O);
        let ctrl_e = egui::KeyboardShortcut::new(Modifiers::CTRL, Key::E);
        let ctrl_s = egui::KeyboardShortcut::new(Modifiers::CTRL, Key::S);
        let ctrl_f = egui::KeyboardShortcut::new(Modifiers::CTRL, Key::F);
        let f1 = egui::KeyboardShortcut::new(Modifiers::NONE, Key::F1);

        // Ctrl+O: Open XML
        if ctx.input_mut(|i| i.consume_shortcut(&ctrl_o)) {
            self.open_file_dialog(FileDialogAction::OpenXml);
        }
        // Ctrl+E: Open EXI
        if ctx.input_mut(|i| i.consume_shortcut(&ctrl_e)) {
            self.open_file_dialog(FileDialogAction::OpenExi);
        }
        // Ctrl+S: Save XML
        if ctx.input_mut(|i| i.consume_shortcut(&ctrl_s)) && self.current_document.is_some() {
            self.open_file_dialog(FileDialogAction::SaveXml);
        }
        // Ctrl+F: Focus search
        if ctx.input_mut(|i| i.consume_shortcut(&ctrl_f)) {
            self.search_bar.focus();
        }
        // F1: Show shortcuts help
        if ctx.input_mut(|i| i.consume_shortcut(&f1)) {
            self.shortcuts_panel.toggle();
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
                self.status_data.original_size = std::fs::metadata(path)
                    .map(|m| m.len() as usize)
                    .unwrap_or(0);
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
    
    fn export_to_json(&mut self) {
        let Some(doc) = &self.current_document else {
            self.set_status("No document loaded.");
            return;
        };
        
        match export_to_json(doc) {
            Ok(json_str) => {
                // Open save dialog for JSON
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("JSON Files", &["json"])
                    .set_file_name("output.json")
                    .save_file()
                {
                    match std::fs::write(&path, json_str) {
                        Ok(_) => self.set_status(format!("Exported to JSON: {}", path.display())),
                        Err(e) => self.set_status(format!("Write error: {e}")),
                    }
                }
            }
            Err(e) => self.set_status(format!("JSON export error: {e}")),
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
                        if ui.button("Open XML… (Ctrl+O)").clicked() {
                            self.open_file_dialog(FileDialogAction::OpenXml);
                            ui.close();
                        }
                        if ui.button("Open EXI… (Ctrl+E)").clicked() {
                            self.open_file_dialog(FileDialogAction::OpenExi);
                            ui.close();
                        }
                        ui.separator();
                        ui.add_enabled_ui(self.current_document.is_some(), |ui| {
                            if ui.button("Save XML As… (Ctrl+S)").clicked() {
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
                        if ui.button("Quit (Ctrl+Q)").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });

                    ui.menu_button("Tools", |ui| {
                        ui.add_enabled_ui(self.current_document.is_some(), |ui| {
                            if ui.button("🗜 Compress to EXI").clicked() {
                                self.compress_to_exi();
                                ui.close();
                            }
                        });
                        ui.add_enabled_ui(self.exi_data.is_some(), |ui| {
                            if ui.button("📦 Decompress from EXI").clicked() {
                                self.decompress_from_exi();
                                ui.close();
                            }
                        });
                        ui.separator();
                        ui.add_enabled_ui(self.current_document.is_some(), |ui| {
                            if ui.button("📄 Export to JSON").clicked() {
                                self.export_to_json();
                                ui.close();
                            }
                        });
                    });

                    ui.menu_button("View", |ui| {
                        ui.checkbox(&mut self.show_raw_xml, "Show Raw XML");
                    });

                    ui.menu_button("Help", |ui| {
                        if ui.button("⌨ Keyboard Shortcuts (F1)").clicked() {
                            self.shortcuts_panel.toggle();
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("📖 About").clicked() {
                            // TODO: Show about dialog
                            ui.close();
                        }
                    });

                    // Quick action buttons in menu bar
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Compression buttons
                        let has_doc = self.current_document.is_some();
                        let has_exi = self.exi_data.is_some();

                        ui.add_enabled_ui(has_exi, |ui| {
                            if ui.button("◀ Decompress").clicked() {
                                self.decompress_from_exi();
                            }
                        });

                        ui.add_enabled_ui(has_doc, |ui| {
                            if ui.button("▶ Compress").clicked() {
                                self.compress_to_exi();
                            }
                        });
                    });
                });
            });
    }

    fn show_body(&mut self, ctx: &Context) {
        egui::SidePanel::left("tree_panel")
            .default_width(320.0)
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
                ui.label(
                    RichText::new("XML Structure")
                        .font(FontId::proportional(14.0))
                        .strong()
                        .color(Theme::TEXT_PRIMARY),
                );
            });

            // Search bar
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.add_space(4.0);
                let _ = self.search_bar.show(ui);
            });

            ui.separator();
        });

        if let Some(doc) = &self.current_document {
            // Use reference instead of cloning the entire tree
            let root = &doc.root;
            let query = &self.search_bar.query;
            let case_sensitive = self.search_bar.case_sensitive;

            // Use both() for horizontal and vertical scrolling
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    self.xml_tree_view
                        .show_with_search(ui, root, query, case_sensitive);
                });
        } else {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.label(
                    RichText::new("No file loaded")
                        .color(Theme::TEXT_MUTED)
                        .italics(),
                );
                ui.add_space(10.0);
                ui.label(
                    RichText::new("Open an XML or EXI file to view its structure")
                        .small()
                        .color(Theme::TEXT_SECONDARY),
                );
                ui.add_space(20.0);
                if ui.button("📂 Open XML File").clicked() {
                    self.open_file_dialog(FileDialogAction::OpenXml);
                }
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Ctrl+O: Open XML | Ctrl+E: Open EXI")
                        .small()
                        .color(Theme::TEXT_MUTED),
                );
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
                        ui.label(RichText::new("Element:").strong().color(Theme::ACCENT));
                        ui.label(
                            RichText::new(&info.name)
                                .font(FontId::monospace(16.0))
                                .color(Theme::ELEMENT_NAME),
                        );
                    });

                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);

                    if !info.attributes.is_empty() {
                        ui.label(RichText::new("Attributes").strong().color(Theme::INFO));
                        ui.add_space(5.0);

                        egui::Grid::new("attr_grid")
                            .num_columns(2)
                            .spacing([10.0, 4.0])
                            .show(ui, |ui| {
                                for (key, value) in &info.attributes {
                                    ui.label(
                                        RichText::new(key)
                                            .font(FontId::monospace(12.0))
                                            .color(Theme::ATTRIBUTE_KEY),
                                    );
                                    ui.label(
                                        RichText::new(value)
                                            .font(FontId::monospace(12.0))
                                            .color(Theme::ATTRIBUTE_VALUE),
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
                            ui.label(RichText::new("Text Content").strong().color(Theme::INFO));
                            ui.add_space(5.0);
                            Frame::new()
                                .fill(Theme::CARD_BG)
                                .inner_margin(8.0)
                                .corner_radius(4.0)
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(t)
                                            .font(FontId::monospace(12.0))
                                            .color(Theme::TEXT_CONTENT),
                                    );
                                });
                            ui.add_space(10.0);
                        }
                    }

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Children:").strong().color(Theme::INFO));
                        ui.label(
                            RichText::new(info.child_count.to_string())
                                .font(FontId::monospace(14.0))
                                .color(Theme::TEXT_PRIMARY),
                        );
                    });
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.label(
                            RichText::new("Select a node in the tree")
                                .color(Theme::TEXT_MUTED)
                                .italics(),
                        );
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new("to view its details here")
                                .small()
                                .color(Theme::TEXT_SECONDARY),
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
                        .fill(Theme::CARD_BG)
                        .inner_margin(10.0)
                        .corner_radius(4.0)
                        .show(ui, |ui| {
                            // Use syntax highlighting for better readability
                            let highlighted = self.syntax_highlighter.highlight_xml_custom(xml);
                            
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                for (color, text) in highlighted {
                                    ui.label(
                                        RichText::new(text)
                                            .color(color)
                                            .font(FontId::monospace(12.0))
                                    );
                                }
                            });
                        });
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.label(
                            RichText::new("No XML loaded")
                                .color(Theme::TEXT_MUTED)
                                .italics(),
                        );
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
        self.status_data.file_name = path.file_name().and_then(|n| n.to_str()).map(String::from);
    }
}
