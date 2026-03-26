use std::path::PathBuf;

use egui::text::LayoutJob;
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
use super::xml_tree::{SelectedNodeInfo, XmlTreeView};

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
    pending_json_export: Option<String>,

    // UI state
    xml_tree_view: XmlTreeView,
    search_bar: SearchBar,
    shortcuts_panel: ShortcutsPanel,
    raw_xml_cache: Option<(u64, String)>,
    raw_xml_highlight_cache: Option<(u64, Vec<LayoutJob>)>,
    show_raw_xml: bool,
    syntax_highlighter: SyntaxHighlighter,
    detail_editor: DetailEditorState,

    // Status bar
    status_data: StatusBarData,

    // Async file dialog
    file_dialog: FileDialogManager,
}

#[derive(Default)]
struct DetailEditorState {
    selected_id: Option<u64>,
    element_name: String,
    attribute_values: Vec<String>,
    text_content: String,
    new_attribute_name: String,
    new_attribute_value: String,
    new_child_name: String,
}

impl DetailEditorState {
    fn sync_with_selection(&mut self, selected: Option<&SelectedNodeInfo>) {
        match selected {
            Some(info)
                if self.selected_id != Some(info.id)
                    || self.attribute_values.len() != info.attributes.len() =>
            {
                self.load_from_info(info);
            }
            Some(_) => {}
            None => self.clear(),
        }
    }

    fn load_from_info(&mut self, info: &SelectedNodeInfo) {
        self.selected_id = Some(info.id);
        self.element_name = info.name.clone();
        self.attribute_values = info
            .attributes
            .iter()
            .map(|(_, value)| value.clone())
            .collect();
        self.text_content = info.text.clone().unwrap_or_default();
        self.new_attribute_name.clear();
        self.new_attribute_value.clear();
        self.new_child_name.clear();
    }

    fn has_changes(&self, info: &SelectedNodeInfo) -> bool {
        if self.element_name != info.name {
            return true;
        }

        if self.attribute_values.len() != info.attributes.len() {
            return true;
        }

        if info
            .attributes
            .iter()
            .zip(&self.attribute_values)
            .any(|((_, current), draft)| current != draft)
        {
            return true;
        }

        info.child_count == 0 && self.text_content != info.text.clone().unwrap_or_default()
    }

    fn clear(&mut self) {
        *self = Self::default();
    }
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
            pending_json_export: None,

            xml_tree_view: XmlTreeView::new(),
            search_bar: SearchBar::new(),
            shortcuts_panel: ShortcutsPanel::new(),
            raw_xml_cache: None,
            raw_xml_highlight_cache: None,
            show_raw_xml: false,
            syntax_highlighter: SyntaxHighlighter::new(),
            detail_editor: DetailEditorState::default(),

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
                FileDialogResult::SaveJson(Some(p)) => self.save_json(&p),
                FileDialogResult::SaveJson(None) => {
                    self.pending_json_export = None;
                    self.set_status("JSON export cancelled.");
                }
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
                self.pending_json_export = None;
                self.current_document = Some(doc);
                self.current_file_path = Some(path.clone());
                self.current_file_type = FileType::Xml;
                self.xml_tree_view.clear_search_cache();
                self.xml_tree_view.clear_selection();
                self.detail_editor.clear();
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
                    self.pending_json_export = None;
                    self.current_document = Some(doc);
                    self.current_file_path = Some(path.clone());
                    self.current_file_type = FileType::Exi;
                    self.xml_tree_view.clear_search_cache();
                    self.xml_tree_view.clear_selection();
                    self.detail_editor.clear();
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
                self.xml_tree_view.clear_search_cache();
                self.xml_tree_view.clear_selection();
                self.detail_editor.clear();
                self.pending_json_export = None;
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

    fn save_json(&mut self, path: &PathBuf) {
        let Some(json_str) = self.pending_json_export.take() else {
            self.set_status("No JSON export pending.");
            return;
        };

        match std::fs::write(path, json_str) {
            Ok(_) => self.set_status(format!("Exported to JSON: {}", path.display())),
            Err(e) => self.set_status(format!("Write error: {e}")),
        }
    }

    fn export_to_json(&mut self) {
        if self.file_dialog.is_pending() {
            self.set_status("Another file dialog is already open.");
            return;
        }

        let Some(doc) = &self.current_document else {
            self.set_status("No document loaded.");
            return;
        };

        match export_to_json(doc) {
            Ok(json_str) => {
                self.pending_json_export = Some(json_str);
                self.open_file_dialog(FileDialogAction::SaveJson);
                self.set_status("Choose where to save the JSON export.");
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
                    self.xml_tree_view.show_with_search(
                        ui,
                        root,
                        doc.version(),
                        query,
                        case_sensitive,
                    );
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
        self.detail_editor.sync_with_selection(selected.as_ref());

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                ui.add_space(10.0);

                if let Some(info) = selected {
                    let is_root_selected = self
                        .current_document
                        .as_ref()
                        .and_then(|doc| doc.root.as_element().map(|root| root.id.0))
                        == Some(info.id);

                    ui.label(RichText::new("Element Name").strong().color(Theme::ACCENT));
                    ui.add_space(5.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.detail_editor.element_name)
                            .desired_width(f32::INFINITY),
                    );

                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);

                    if !info.attributes.is_empty() {
                        ui.label(
                            RichText::new("Attribute Values")
                                .strong()
                                .color(Theme::INFO),
                        );
                        ui.add_space(5.0);

                        egui::Grid::new("attr_grid")
                            .num_columns(3)
                            .spacing([10.0, 4.0])
                            .show(ui, |ui| {
                                let mut attribute_to_remove: Option<String> = None;
                                for ((key, _), value) in info
                                    .attributes
                                    .iter()
                                    .zip(self.detail_editor.attribute_values.iter_mut())
                                {
                                    ui.label(
                                        RichText::new(key)
                                            .font(FontId::monospace(12.0))
                                            .color(Theme::ATTRIBUTE_KEY),
                                    );
                                    ui.add(
                                        egui::TextEdit::singleline(value)
                                            .desired_width(f32::INFINITY),
                                    );
                                    if ui.small_button("Remove").clicked() {
                                        attribute_to_remove = Some(key.clone());
                                    }
                                    ui.end_row();
                                }

                                if let Some(attribute_name) = attribute_to_remove {
                                    self.remove_selected_attribute(&attribute_name);
                                }
                            });

                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.detail_editor.new_attribute_name)
                                    .hint_text("attribute name")
                                    .desired_width(160.0),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut self.detail_editor.new_attribute_value)
                                    .hint_text("attribute value")
                                    .desired_width(180.0),
                            );
                            let can_add_attribute =
                                !self.detail_editor.new_attribute_name.trim().is_empty();
                            if ui
                                .add_enabled(can_add_attribute, egui::Button::new("Add Attribute"))
                                .clicked()
                            {
                                self.add_attribute_to_selected();
                            }
                        });

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);
                    } else {
                        ui.label(RichText::new("Attributes").strong().color(Theme::INFO));
                        ui.add_space(5.0);
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.detail_editor.new_attribute_name)
                                    .hint_text("attribute name")
                                    .desired_width(160.0),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut self.detail_editor.new_attribute_value)
                                    .hint_text("attribute value")
                                    .desired_width(180.0),
                            );
                            let can_add_attribute =
                                !self.detail_editor.new_attribute_name.trim().is_empty();
                            if ui
                                .add_enabled(can_add_attribute, egui::Button::new("Add Attribute"))
                                .clicked()
                            {
                                self.add_attribute_to_selected();
                            }
                        });
                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);
                    }

                    ui.label(RichText::new("Text Content").strong().color(Theme::INFO));
                    ui.add_space(5.0);
                    if info.child_count == 0 {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.detail_editor.text_content)
                                .desired_width(f32::INFINITY)
                                .desired_rows(5),
                        );
                    } else {
                        let preview = info.text.as_deref().unwrap_or("");
                        Frame::new()
                            .fill(Theme::CARD_BG)
                            .inner_margin(8.0)
                            .corner_radius(4.0)
                            .show(ui, |ui| {
                                if preview.is_empty() {
                                    ui.label(
                                        RichText::new(
                                            "Text editing for nodes with children will land in a later phase.",
                                        )
                                        .small()
                                        .color(Theme::TEXT_MUTED),
                                    );
                                } else {
                                    ui.label(
                                        RichText::new(preview)
                                            .font(FontId::monospace(12.0))
                                            .color(Theme::TEXT_CONTENT),
                                    );
                                }
                            });
                    }
                    ui.add_space(10.0);

                    let has_changes = self.detail_editor.has_changes(&info);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(has_changes, egui::Button::new("Apply Changes"))
                            .clicked()
                        {
                            self.apply_selected_element_edits();
                        }

                        if ui
                            .add_enabled(has_changes, egui::Button::new("Reset"))
                            .clicked()
                        {
                            self.detail_editor.load_from_info(&info);
                        }
                    });

                    if !has_changes {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("No unapplied changes")
                                .small()
                                .color(Theme::TEXT_MUTED),
                        );
                    }

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Children:").strong().color(Theme::INFO));
                        ui.label(
                            RichText::new(info.child_count.to_string())
                                .font(FontId::monospace(14.0))
                                .color(Theme::TEXT_PRIMARY),
                        );
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.detail_editor.new_child_name)
                                .hint_text("new child element name")
                                .desired_width(220.0),
                        );
                        let can_add_child = !self.detail_editor.new_child_name.trim().is_empty();
                        if ui
                            .add_enabled(can_add_child, egui::Button::new("Add Child Element"))
                            .clicked()
                        {
                            self.add_child_to_selected();
                        }
                    });
                    ui.add_space(8.0);
                    ui.add_enabled_ui(!is_root_selected, |ui| {
                        if ui.button("Delete Selected Element").clicked() {
                            self.remove_selected_element();
                        }
                    });
                    if is_root_selected {
                        ui.label(
                            RichText::new("The document root cannot be deleted.")
                                .small()
                                .color(Theme::TEXT_MUTED),
                        );
                    }
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
        let current_version = self.current_document.as_ref().map(XmlDocument::version);

        if let (Some(doc), Some(version)) = (&self.current_document, current_version)
            && self
                .raw_xml_cache
                .as_ref()
                .map(|(cached_version, _)| *cached_version)
                != Some(version)
        {
            let xml = match serialize_xml(doc) {
                Ok(xml) => xml,
                Err(e) => format!("<!-- serialisation error: {e} -->"),
            };
            self.raw_xml_cache = Some((version, xml));
        }

        if let Some(version) = current_version
            && self
                .raw_xml_highlight_cache
                .as_ref()
                .map(|(cached_version, _)| *cached_version)
                != Some(version)
            && let Some((_, xml)) = &self.raw_xml_cache
        {
            self.raw_xml_highlight_cache =
                Some((version, self.syntax_highlighter.highlight_xml_lines(xml)));
        }

        ui.add_space(10.0);

        if let (Some((_, xml)), Some((_, line_jobs))) =
            (&self.raw_xml_cache, &self.raw_xml_highlight_cache)
        {
            let line_number_width = line_jobs.len().max(1).to_string().len();
            let row_height = ui.text_style_height(&egui::TextStyle::Monospace);

            Frame::new()
                .fill(Theme::CARD_BG)
                .inner_margin(10.0)
                .corner_radius(4.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{} lines", line_jobs.len()))
                                .small()
                                .color(Theme::INFO),
                        );
                        ui.separator();
                        ui.label(
                            RichText::new(format!("{} bytes", xml.len()))
                                .small()
                                .color(Theme::TEXT_MUTED),
                        );
                    });
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    egui::ScrollArea::both()
                        .auto_shrink([false, false])
                        .show_rows(ui, row_height, line_jobs.len(), |ui, row_range| {
                            for index in row_range {
                                let job = &line_jobs[index];
                                ui.horizontal(|ui| {
                                    ui.add_sized(
                                        [20.0 + line_number_width as f32 * 8.0, 0.0],
                                        egui::Label::new(
                                            RichText::new(format!(
                                                "{:>width$}",
                                                index + 1,
                                                width = line_number_width
                                            ))
                                            .font(FontId::monospace(12.0))
                                            .color(Theme::TEXT_MUTED),
                                        ),
                                    );
                                    ui.add(
                                        egui::Label::new(job.clone())
                                            .selectable(true)
                                            .extend()
                                            .show_tooltip_when_elided(false),
                                    );
                                });
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
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status_data.set_message(msg);
    }

    fn apply_selected_element_edits(&mut self) {
        let Some(selected_id) = self.xml_tree_view.selected_id() else {
            self.set_status("No element selected.");
            return;
        };
        let Some(selected) = self.xml_tree_view.get_selected_info().cloned() else {
            self.set_status("No element details available.");
            return;
        };

        let draft_name = self.detail_editor.element_name.clone();
        let draft_attribute_values = self.detail_editor.attribute_values.clone();
        let draft_text = self.detail_editor.text_content.clone();

        let mut name_changed = false;
        let mut attributes_changed = false;
        let mut text_changed = false;

        let update_result = (|| {
            let Some(doc) = self.current_document.as_mut() else {
                return Ok(());
            };

            name_changed = doc.rename_element(selected_id, draft_name)?;

            for ((attribute_name, current_value), draft_value) in
                selected.attributes.iter().zip(&draft_attribute_values)
            {
                if current_value != draft_value
                    && doc.set_attribute_value(selected_id, attribute_name, draft_value.clone())
                {
                    attributes_changed = true;
                }
            }

            if selected.child_count == 0 && selected.text.clone().unwrap_or_default() != draft_text
            {
                text_changed = doc.set_text_content(
                    selected_id,
                    Some(draft_text.clone()).filter(|text| !text.is_empty()),
                )?;
            }

            Ok::<(), anyhow::Error>(())
        })();

        if let Err(err) = update_result {
            self.set_status(format!("Edit rejected: {err}"));
            return;
        }

        if !name_changed && !attributes_changed && !text_changed {
            self.set_status("No element changes to apply.");
            return;
        }

        let cleared_exi = self.invalidate_after_document_edit();
        self.sync_selection_from_document();

        let mut changes = Vec::new();
        if name_changed {
            changes.push("name");
        }
        if attributes_changed {
            changes.push("attributes");
        }
        if text_changed {
            changes.push("text");
        }

        let message = format!("Updated selected element ({})", changes.join(", "));
        self.set_status(self.with_exi_notice(message, cleared_exi));
    }

    fn add_attribute_to_selected(&mut self) {
        let Some(selected_id) = self.xml_tree_view.selected_id() else {
            self.set_status("No element selected.");
            return;
        };

        let attribute_name = self.detail_editor.new_attribute_name.trim().to_string();
        let attribute_value = self.detail_editor.new_attribute_value.clone();

        let add_result = match self.current_document.as_mut() {
            Some(doc) => doc.add_attribute(selected_id, attribute_name.clone(), attribute_value),
            None => {
                self.set_status("No document loaded.");
                return;
            }
        };

        match add_result {
            Ok(true) => {
                self.detail_editor.new_attribute_name.clear();
                self.detail_editor.new_attribute_value.clear();
                let cleared_exi = self.invalidate_after_document_edit();
                self.sync_selection_from_document();
                self.set_status(
                    self.with_exi_notice(
                        format!("Added attribute '{attribute_name}'"),
                        cleared_exi,
                    ),
                );
            }
            Ok(false) => self.set_status("Attribute was not added."),
            Err(err) => self.set_status(format!("Cannot add attribute: {err}")),
        }
    }

    fn remove_selected_attribute(&mut self, attribute_name: &str) {
        let Some(selected_id) = self.xml_tree_view.selected_id() else {
            self.set_status("No element selected.");
            return;
        };

        let removed = match self.current_document.as_mut() {
            Some(doc) => doc.remove_attribute(selected_id, attribute_name),
            None => {
                self.set_status("No document loaded.");
                return;
            }
        };

        if !removed {
            self.set_status(format!("Attribute '{attribute_name}' was not removed."));
            return;
        }

        let cleared_exi = self.invalidate_after_document_edit();
        self.sync_selection_from_document();
        self.set_status(
            self.with_exi_notice(format!("Removed attribute '{attribute_name}'"), cleared_exi),
        );
    }

    fn add_child_to_selected(&mut self) {
        let Some(selected_id) = self.xml_tree_view.selected_id() else {
            self.set_status("No element selected.");
            return;
        };

        let child_name = self.detail_editor.new_child_name.trim().to_string();
        let add_result = match self.current_document.as_mut() {
            Some(doc) => doc.append_child_element(selected_id, child_name.clone()),
            None => {
                self.set_status("No document loaded.");
                return;
            }
        };

        match add_result {
            Ok(Some(child_id)) => {
                self.detail_editor.new_child_name.clear();
                let cleared_exi = self.invalidate_after_document_edit();
                self.select_document_node(Some(child_id));
                self.set_status(
                    self.with_exi_notice(
                        format!("Added child element '{child_name}'"),
                        cleared_exi,
                    ),
                );
            }
            Ok(None) => self.set_status("Selected element is no longer available."),
            Err(err) => self.set_status(format!("Cannot add child element: {err}")),
        }
    }

    fn remove_selected_element(&mut self) {
        let Some(selected_id) = self.xml_tree_view.selected_id() else {
            self.set_status("No element selected.");
            return;
        };

        let remove_result = match self.current_document.as_mut() {
            Some(doc) => doc.remove_element(selected_id),
            None => {
                self.set_status("No document loaded.");
                return;
            }
        };

        match remove_result {
            Ok(Some(parent_id)) => {
                let cleared_exi = self.invalidate_after_document_edit();
                self.select_document_node(Some(parent_id));
                self.set_status(
                    self.with_exi_notice("Removed selected element.".to_string(), cleared_exi),
                );
            }
            Ok(None) => self.set_status("Selected element was not removed."),
            Err(err) => self.set_status(format!("Cannot remove selected element: {err}")),
        }
    }

    fn invalidate_raw_xml_cache(&mut self) {
        self.raw_xml_cache = None;
        self.raw_xml_highlight_cache = None;
    }

    fn invalidate_after_document_edit(&mut self) -> bool {
        let cleared_exi = self.exi_data.take().is_some();
        self.pending_json_export = None;
        self.status_data.compressed_size = 0;
        cleared_exi
    }

    fn sync_selection_from_document(&mut self) {
        if let Some(doc) = &self.current_document {
            self.xml_tree_view.sync_selected_info(doc.root.as_ref());
            if let Some(updated) = self.xml_tree_view.get_selected_info().cloned() {
                self.detail_editor.load_from_info(&updated);
            } else {
                self.detail_editor.clear();
            }
        } else {
            self.xml_tree_view.clear_selection();
            self.detail_editor.clear();
        }
    }

    fn select_document_node(&mut self, id: Option<u64>) {
        match (&self.current_document, id) {
            (Some(doc), Some(id)) => {
                self.xml_tree_view.select_id(doc.root.as_ref(), id);
                if let Some(updated) = self.xml_tree_view.get_selected_info().cloned() {
                    self.detail_editor.load_from_info(&updated);
                } else {
                    self.detail_editor.clear();
                }
            }
            _ => {
                self.xml_tree_view.clear_selection();
                self.detail_editor.clear();
            }
        }
    }

    fn with_exi_notice(&self, message: String, cleared_exi: bool) -> String {
        if cleared_exi {
            format!("{message} — EXI cache cleared, recompress to save EXI again.")
        } else {
            message
        }
    }

    fn update_file_name(&mut self, path: &std::path::Path) {
        self.status_data.file_name = path.file_name().and_then(|n| n.to_str()).map(String::from);
    }
}
