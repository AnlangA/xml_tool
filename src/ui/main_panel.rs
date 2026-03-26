use std::path::PathBuf;

use egui::text::LayoutJob;
use egui::{Context, FontId, Frame, Key, Margin, Modifiers, RichText, Window};

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

#[derive(Debug, Clone)]
struct DocumentSnapshot {
    document: XmlDocument,
    exi_data: Option<Vec<u8>>,
    pending_json_export: Option<String>,
    original_size: usize,
    compressed_size: usize,
}

#[derive(Default)]
struct DocumentHistory {
    undo_stack: Vec<DocumentSnapshot>,
    redo_stack: Vec<DocumentSnapshot>,
    saved_doc_version: Option<u64>,
}

impl DocumentHistory {
    fn reset_saved(&mut self, doc: Option<&XmlDocument>) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.saved_doc_version = doc.map(XmlDocument::version);
    }

    fn push_undo(&mut self, snapshot: DocumentSnapshot) {
        self.undo_stack.push(snapshot);
        self.redo_stack.clear();
    }

    fn push_redo(&mut self, snapshot: DocumentSnapshot) {
        self.redo_stack.push(snapshot);
    }

    fn pop_undo(&mut self) -> Option<DocumentSnapshot> {
        self.undo_stack.pop()
    }

    fn pop_redo(&mut self) -> Option<DocumentSnapshot> {
        self.redo_stack.pop()
    }

    fn mark_saved(&mut self, doc: Option<&XmlDocument>) {
        self.saved_doc_version = doc.map(XmlDocument::version);
    }

    fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

#[derive(Debug, Clone)]
enum PendingUnsavedAction {
    OpenDialog(FileDialogAction),
    LoadXml(PathBuf),
    LoadExi(PathBuf),
    DecompressFromExi,
    Quit,
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
    history: DocumentHistory,
    pending_delete_confirmation: Option<u64>,
    pending_unsaved_action: Option<PendingUnsavedAction>,
    allow_unsaved_close_once: bool,

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
    new_sibling_name: String,
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
        self.new_sibling_name.clear();
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
            history: DocumentHistory::default(),
            pending_delete_confirmation: None,
            pending_unsaved_action: None,
            allow_unsaved_close_once: false,

            status_data,

            file_dialog: FileDialogManager::new(),
        }
    }

    pub fn show(&mut self, ctx: &Context) {
        self.handle_close_request(ctx);
        self.handle_shortcuts(ctx);
        self.poll_file_dialog(ctx);
        if self.pending_unsaved_action.is_some()
            && !self.has_unsaved_changes()
            && !self.file_dialog.is_pending()
            && let Some(action) = self.pending_unsaved_action.take()
        {
            self.perform_pending_action(ctx, action);
        }
        self.status_data.is_dirty = self.has_unsaved_changes();
        self.show_menu_bar(ctx);
        show_status_bar(ctx, &self.status_data);
        self.show_body(ctx);
        self.show_unsaved_changes_dialog(ctx);

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
        let ctrl_z = egui::KeyboardShortcut::new(Modifiers::CTRL, Key::Z);
        let ctrl_shift_z = egui::KeyboardShortcut::new(
            Modifiers {
                ctrl: true,
                shift: true,
                ..Modifiers::NONE
            },
            Key::Z,
        );
        let ctrl_y = egui::KeyboardShortcut::new(Modifiers::CTRL, Key::Y);
        let ctrl_q = egui::KeyboardShortcut::new(Modifiers::CTRL, Key::Q);
        let f1 = egui::KeyboardShortcut::new(Modifiers::NONE, Key::F1);

        // Ctrl+O: Open XML
        if ctx.input_mut(|i| i.consume_shortcut(&ctrl_o)) {
            self.request_unsaved_action(
                ctx,
                PendingUnsavedAction::OpenDialog(FileDialogAction::OpenXml),
            );
        }
        // Ctrl+E: Open EXI
        if ctx.input_mut(|i| i.consume_shortcut(&ctrl_e)) {
            self.request_unsaved_action(
                ctx,
                PendingUnsavedAction::OpenDialog(FileDialogAction::OpenExi),
            );
        }
        // Ctrl+S: Save XML
        if ctx.input_mut(|i| i.consume_shortcut(&ctrl_s)) && self.current_document.is_some() {
            self.open_file_dialog(FileDialogAction::SaveXml);
        }
        // Ctrl+F: Focus search
        if ctx.input_mut(|i| i.consume_shortcut(&ctrl_f)) {
            self.search_bar.focus();
        }
        // Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y
        if ctx.input_mut(|i| i.consume_shortcut(&ctrl_z)) {
            self.undo_last_change();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&ctrl_shift_z))
            || ctx.input_mut(|i| i.consume_shortcut(&ctrl_y))
        {
            self.redo_last_change();
        }
        // Esc: Clear search
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) && self.search_bar.clear()
        {
            self.set_status("Search cleared.");
        }
        // Ctrl+Q: Quit application
        if ctx.input_mut(|i| i.consume_shortcut(&ctrl_q)) {
            self.request_unsaved_action(ctx, PendingUnsavedAction::Quit);
        }
        // F1: Show shortcuts help
        if ctx.input_mut(|i| i.consume_shortcut(&f1)) {
            self.shortcuts_panel.toggle();
        }
    }

    // -----------------------------------------------------------------------
    // File dialog
    // -----------------------------------------------------------------------

    fn poll_file_dialog(&mut self, ctx: &Context) {
        if let Some(result) = self.file_dialog.poll() {
            match result {
                FileDialogResult::OpenXml(Some(p)) => {
                    self.request_unsaved_action(ctx, PendingUnsavedAction::LoadXml(p))
                }
                FileDialogResult::OpenExi(Some(p)) => {
                    self.request_unsaved_action(ctx, PendingUnsavedAction::LoadExi(p))
                }
                FileDialogResult::SaveXml(Some(p)) => {
                    if self.save_xml(&p)
                        && let Some(action) = self.pending_unsaved_action.take()
                    {
                        self.perform_pending_action(ctx, action);
                    }
                }
                FileDialogResult::SaveExi(Some(p)) => {
                    let _ = self.save_exi(&p);
                }
                FileDialogResult::SaveXml(None) if self.pending_unsaved_action.is_some() => {
                    self.set_status("Save cancelled — choose save, discard, or cancel.");
                }
                FileDialogResult::SaveXml(None) => {}
                FileDialogResult::SaveExi(None) => self.set_status("EXI save cancelled."),
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
                self.clear_delete_confirmation();
                self.xml_tree_view.clear_search_cache();
                self.xml_tree_view.clear_selection();
                self.detail_editor.clear();
                self.invalidate_raw_xml_cache();
                self.history.reset_saved(self.current_document.as_ref());
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
                    self.clear_delete_confirmation();
                    self.xml_tree_view.clear_search_cache();
                    self.xml_tree_view.clear_selection();
                    self.detail_editor.clear();
                    self.invalidate_raw_xml_cache();
                    self.history.reset_saved(self.current_document.as_ref());
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
                self.clear_delete_confirmation();
                self.xml_tree_view.clear_search_cache();
                self.xml_tree_view.clear_selection();
                self.detail_editor.clear();
                self.pending_json_export = None;
                self.invalidate_raw_xml_cache();
                self.history.reset_saved(self.current_document.as_ref());
                self.set_status("Decompressed from EXI.");
            }
            Err(e) => self.set_status(format!("Decompression error: {e}")),
        }
    }

    fn save_xml(&mut self, path: &PathBuf) -> bool {
        let Some(doc) = &self.current_document else {
            return false;
        };
        match serialize_xml(doc) {
            Ok(xml_str) => match std::fs::write(path, &xml_str) {
                Ok(_) => {
                    self.current_file_path = Some(path.clone());
                    self.current_file_type = FileType::Xml;
                    self.history.mark_saved(self.current_document.as_ref());
                    self.update_file_name(path);
                    self.set_status(format!("Saved XML: {}", path.display()));
                    true
                }
                Err(e) => {
                    self.set_status(format!("Write error: {e}"));
                    false
                }
            },
            Err(e) => {
                self.set_status(format!("Serialisation error: {e}"));
                false
            }
        }
    }

    fn save_exi(&mut self, path: &PathBuf) -> bool {
        let Some(exi) = &self.exi_data else {
            return false;
        };
        match std::fs::write(path, exi) {
            Ok(_) => {
                self.current_file_path = Some(path.clone());
                self.current_file_type = FileType::Exi;
                self.history.mark_saved(self.current_document.as_ref());
                self.update_file_name(path);
                self.set_status(format!("Saved EXI: {}", path.display()));
                true
            }
            Err(e) => {
                self.set_status(format!("Write error: {e}"));
                false
            }
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
                            self.request_unsaved_action(
                                ctx,
                                PendingUnsavedAction::OpenDialog(FileDialogAction::OpenXml),
                            );
                            ui.close();
                        }
                        if ui.button("Open EXI… (Ctrl+E)").clicked() {
                            self.request_unsaved_action(
                                ctx,
                                PendingUnsavedAction::OpenDialog(FileDialogAction::OpenExi),
                            );
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
                            self.request_unsaved_action(ctx, PendingUnsavedAction::Quit);
                            ui.close();
                        }
                    });

                    ui.menu_button("Edit", |ui| {
                        ui.add_enabled_ui(self.history.can_undo(), |ui| {
                            if ui.button("Undo (Ctrl+Z)").clicked() {
                                self.undo_last_change();
                                ui.close();
                            }
                        });
                        ui.add_enabled_ui(self.history.can_redo(), |ui| {
                            if ui.button("Redo (Ctrl+Shift+Z)").clicked() {
                                self.redo_last_change();
                                ui.close();
                            }
                        });
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
                                self.request_unsaved_action(
                                    ctx,
                                    PendingUnsavedAction::DecompressFromExi,
                                );
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
                                self.request_unsaved_action(
                                    ctx,
                                    PendingUnsavedAction::DecompressFromExi,
                                );
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
        let search_context = self
            .current_document
            .as_ref()
            .map(|doc| (doc.version(), doc.root.clone()));

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
                if !self.search_bar.query.is_empty() {
                    let match_count = search_context.as_ref().map_or(0, |(doc_version, root)| {
                        self.xml_tree_view.search_match_count(
                            root.as_ref(),
                            *doc_version,
                            &self.search_bar.query,
                            self.search_bar.case_sensitive,
                        )
                    });
                    ui.separator();
                    ui.label(
                        RichText::new(format!("{match_count} matches"))
                            .small()
                            .color(Theme::TEXT_MUTED),
                    );
                    if ui
                        .add_enabled(match_count > 0, egui::Button::new("Prev"))
                        .clicked()
                    {
                        self.select_adjacent_search_match(true);
                    }
                    if ui
                        .add_enabled(match_count > 0, egui::Button::new("Next"))
                        .clicked()
                    {
                        self.select_adjacent_search_match(false);
                    }
                }
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
                    self.request_unsaved_action(
                        ui.ctx(),
                        PendingUnsavedAction::OpenDialog(FileDialogAction::OpenXml),
                    );
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
                    let selected_position = self
                        .current_document
                        .as_ref()
                        .and_then(|doc| doc.element_position(info.id));
                    let can_move_up =
                        selected_position.is_some_and(|position| position.can_move_up());
                    let can_move_down =
                        selected_position.is_some_and(|position| position.can_move_down());

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
                    if let Some(position) = selected_position.filter(|position| position.parent_id.is_some()) {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(format!(
                                "Sibling {} of {}",
                                position.element_index + 1,
                                position.element_count
                            ))
                            .small()
                            .color(Theme::TEXT_MUTED),
                        );
                    }
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
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.detail_editor.new_sibling_name)
                                .hint_text("new sibling element name")
                                .desired_width(220.0),
                        );
                        let can_add_sibling =
                            !is_root_selected && !self.detail_editor.new_sibling_name.trim().is_empty();
                        if ui
                            .add_enabled(can_add_sibling, egui::Button::new("Add Sibling After"))
                            .clicked()
                        {
                            self.add_sibling_to_selected();
                        }
                    });
                    if is_root_selected {
                        ui.label(
                            RichText::new("The document root cannot have sibling elements.")
                                .small()
                                .color(Theme::TEXT_MUTED),
                        );
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(can_move_up, egui::Button::new("Move Up"))
                            .clicked()
                        {
                            self.move_selected_element_up();
                        }
                        if ui
                            .add_enabled(can_move_down, egui::Button::new("Move Down"))
                            .clicked()
                        {
                            self.move_selected_element_down();
                        }
                    });
                    if let Some(position) = selected_position
                        && position.parent_id.is_some()
                        && !position.reorderable
                    {
                        ui.label(
                            RichText::new(
                                "Reordering is currently available only when the parent contains element children only.",
                            )
                            .small()
                            .color(Theme::TEXT_MUTED),
                        );
                    }
                    ui.add_space(8.0);
                    let delete_armed = self.pending_delete_confirmation == Some(info.id);
                    ui.add_enabled_ui(!is_root_selected, |ui| {
                        let delete_label = if delete_armed {
                            "Confirm Delete"
                        } else {
                            "Delete Selected Element"
                        };
                        if ui.button(delete_label).clicked() {
                            self.request_selected_element_removal();
                        }
                    });
                    if is_root_selected {
                        ui.label(
                            RichText::new("The document root cannot be deleted.")
                                .small()
                                .color(Theme::TEXT_MUTED),
                        );
                    }
                    if delete_armed {
                        ui.label(
                            RichText::new("Click delete again to confirm removing this element.")
                                .small()
                                .color(Theme::WARNING),
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

    fn handle_close_request(&mut self, ctx: &Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }

        if self.allow_unsaved_close_once {
            self.allow_unsaved_close_once = false;
            return;
        }

        if self.has_unsaved_changes() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if !matches!(
                self.pending_unsaved_action,
                Some(PendingUnsavedAction::Quit)
            ) {
                self.pending_unsaved_action = Some(PendingUnsavedAction::Quit);
                self.set_status("Unsaved changes — save or discard before closing.");
            }
        }
    }

    fn has_unsaved_changes(&self) -> bool {
        match (&self.current_document, self.history.saved_doc_version) {
            (Some(doc), Some(saved_version)) => doc.version() != saved_version,
            (Some(_), None) => true,
            _ => false,
        }
    }

    fn capture_document_snapshot(&self) -> Option<DocumentSnapshot> {
        Some(DocumentSnapshot {
            document: self.current_document.clone()?,
            exi_data: self.exi_data.clone(),
            pending_json_export: self.pending_json_export.clone(),
            original_size: self.status_data.original_size,
            compressed_size: self.status_data.compressed_size,
        })
    }

    fn restore_document_snapshot(&mut self, snapshot: DocumentSnapshot) {
        self.current_document = Some(snapshot.document);
        self.exi_data = snapshot.exi_data;
        self.pending_json_export = snapshot.pending_json_export;
        self.status_data.original_size = snapshot.original_size;
        self.status_data.compressed_size = snapshot.compressed_size;
        self.clear_delete_confirmation();
        self.xml_tree_view.clear_search_cache();
        self.invalidate_raw_xml_cache();
        self.sync_selection_from_document();
    }

    fn push_undo_snapshot(&mut self, snapshot: Option<DocumentSnapshot>) {
        if let Some(snapshot) = snapshot {
            self.history.push_undo(snapshot);
        }
    }

    fn request_unsaved_action(&mut self, ctx: &Context, action: PendingUnsavedAction) {
        if self.has_unsaved_changes() {
            if matches!(action, PendingUnsavedAction::Quit) {
                self.pending_unsaved_action = Some(PendingUnsavedAction::Quit);
                self.set_status("Unsaved changes — save or discard before closing.");
            } else if self.pending_unsaved_action.is_none() {
                self.pending_unsaved_action = Some(action);
                self.set_status("Unsaved changes — save or discard before continuing.");
            }
            if matches!(
                self.pending_unsaved_action,
                Some(PendingUnsavedAction::Quit)
            ) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
            return;
        }

        self.perform_pending_action(ctx, action);
    }

    fn discard_unsaved_changes_and_continue(
        &mut self,
        ctx: &Context,
        action: PendingUnsavedAction,
    ) {
        self.pending_unsaved_action = None;
        self.allow_unsaved_close_once = matches!(action, PendingUnsavedAction::Quit);
        self.perform_pending_action(ctx, action);
    }

    fn perform_pending_action(&mut self, ctx: &Context, action: PendingUnsavedAction) {
        match action {
            PendingUnsavedAction::OpenDialog(action) => self.open_file_dialog(action),
            PendingUnsavedAction::LoadXml(path) => self.load_xml(&path),
            PendingUnsavedAction::LoadExi(path) => self.load_exi(&path),
            PendingUnsavedAction::DecompressFromExi => self.decompress_from_exi(),
            PendingUnsavedAction::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    fn show_unsaved_changes_dialog(&mut self, ctx: &Context) {
        let Some(action) = self.pending_unsaved_action.clone() else {
            return;
        };

        Window::new("Unsaved changes")
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("You have unsaved XML edits.")
                        .strong()
                        .color(Theme::WARNING),
                );
                ui.add_space(8.0);
                ui.label("Save the current document, discard the changes, or cancel this action.");
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let save_label = if self.file_dialog.is_pending() {
                        "Saving…"
                    } else {
                        "Save XML As…"
                    };
                    if ui
                        .add_enabled(
                            self.current_document.is_some() && !self.file_dialog.is_pending(),
                            egui::Button::new(save_label),
                        )
                        .clicked()
                    {
                        self.open_file_dialog(FileDialogAction::SaveXml);
                    }

                    if ui.button("Discard Changes").clicked() {
                        self.discard_unsaved_changes_and_continue(ctx, action);
                    }

                    if ui.button("Cancel").clicked() {
                        self.pending_unsaved_action = None;
                        self.set_status("Kept the current document unchanged.");
                    }
                });
            });
    }

    fn undo_last_change(&mut self) {
        self.clear_delete_confirmation();
        let Some(previous) = self.history.pop_undo() else {
            self.set_status("Nothing to undo.");
            return;
        };
        let current = self.capture_document_snapshot();
        self.restore_document_snapshot(previous);
        if let Some(current) = current {
            self.history.push_redo(current);
        }
        self.set_status("Undid the last document change.");
    }

    fn redo_last_change(&mut self) {
        self.clear_delete_confirmation();
        let Some(next) = self.history.pop_redo() else {
            self.set_status("Nothing to redo.");
            return;
        };
        let current = self.capture_document_snapshot();
        self.restore_document_snapshot(next);
        if let Some(current) = current {
            self.history.push_undo(current);
        }
        self.set_status("Redid the last document change.");
    }

    fn select_adjacent_search_match(&mut self, backwards: bool) {
        let query = self.search_bar.query.clone();
        if query.is_empty() {
            self.set_status("Enter a search term first.");
            return;
        }

        let case_sensitive = self.search_bar.case_sensitive;
        let Some((doc_version, root)) = self
            .current_document
            .as_ref()
            .map(|doc| (doc.version(), doc.root.clone()))
        else {
            self.set_status("No document loaded.");
            return;
        };

        let Some(match_id) = self.xml_tree_view.adjacent_search_match_id(
            root.as_ref(),
            doc_version,
            &query,
            case_sensitive,
            backwards,
        ) else {
            self.set_status("No matching elements found for the current search.");
            return;
        };

        self.select_document_node(Some(match_id));
        self.set_status(if backwards {
            "Selected previous search match."
        } else {
            "Selected next search match."
        });
    }

    fn apply_selected_element_edits(&mut self) {
        self.clear_delete_confirmation();
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
        let previous_snapshot = self.capture_document_snapshot();

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

        self.push_undo_snapshot(previous_snapshot);
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
        self.clear_delete_confirmation();
        let Some(selected_id) = self.xml_tree_view.selected_id() else {
            self.set_status("No element selected.");
            return;
        };

        let attribute_name = self.detail_editor.new_attribute_name.trim().to_string();
        let attribute_value = self.detail_editor.new_attribute_value.clone();
        let previous_snapshot = self.capture_document_snapshot();

        let add_result = match self.current_document.as_mut() {
            Some(doc) => doc.add_attribute(selected_id, attribute_name.clone(), attribute_value),
            None => {
                self.set_status("No document loaded.");
                return;
            }
        };

        match add_result {
            Ok(true) => {
                self.push_undo_snapshot(previous_snapshot);
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
        self.clear_delete_confirmation();
        let Some(selected_id) = self.xml_tree_view.selected_id() else {
            self.set_status("No element selected.");
            return;
        };
        let previous_snapshot = self.capture_document_snapshot();

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

        self.push_undo_snapshot(previous_snapshot);
        let cleared_exi = self.invalidate_after_document_edit();
        self.sync_selection_from_document();
        self.set_status(
            self.with_exi_notice(format!("Removed attribute '{attribute_name}'"), cleared_exi),
        );
    }

    fn add_child_to_selected(&mut self) {
        self.clear_delete_confirmation();
        let Some(selected_id) = self.xml_tree_view.selected_id() else {
            self.set_status("No element selected.");
            return;
        };

        let child_name = self.detail_editor.new_child_name.trim().to_string();
        let previous_snapshot = self.capture_document_snapshot();
        let add_result = match self.current_document.as_mut() {
            Some(doc) => doc.append_child_element(selected_id, child_name.clone()),
            None => {
                self.set_status("No document loaded.");
                return;
            }
        };

        match add_result {
            Ok(Some(child_id)) => {
                self.push_undo_snapshot(previous_snapshot);
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

    fn add_sibling_to_selected(&mut self) {
        self.clear_delete_confirmation();
        let Some(selected_id) = self.xml_tree_view.selected_id() else {
            self.set_status("No element selected.");
            return;
        };

        let sibling_name = self.detail_editor.new_sibling_name.trim().to_string();
        let previous_snapshot = self.capture_document_snapshot();
        let add_result = match self.current_document.as_mut() {
            Some(doc) => doc.insert_sibling_element_after(selected_id, sibling_name.clone()),
            None => {
                self.set_status("No document loaded.");
                return;
            }
        };

        match add_result {
            Ok(Some(sibling_id)) => {
                self.push_undo_snapshot(previous_snapshot);
                self.detail_editor.new_sibling_name.clear();
                let cleared_exi = self.invalidate_after_document_edit();
                self.select_document_node(Some(sibling_id));
                self.set_status(self.with_exi_notice(
                    format!("Added sibling element '{sibling_name}'"),
                    cleared_exi,
                ));
            }
            Ok(None) => self.set_status("Selected element is no longer available."),
            Err(err) => self.set_status(format!("Cannot add sibling element: {err}")),
        }
    }

    fn move_selected_element_up(&mut self) {
        self.move_selected_element(true);
    }

    fn move_selected_element_down(&mut self) {
        self.move_selected_element(false);
    }

    fn move_selected_element(&mut self, move_up: bool) {
        self.clear_delete_confirmation();
        let Some(selected_id) = self.xml_tree_view.selected_id() else {
            self.set_status("No element selected.");
            return;
        };
        let previous_snapshot = self.capture_document_snapshot();

        let move_result = match self.current_document.as_mut() {
            Some(doc) => {
                if move_up {
                    doc.move_element_up(selected_id)
                } else {
                    doc.move_element_down(selected_id)
                }
            }
            None => {
                self.set_status("No document loaded.");
                return;
            }
        };

        match move_result {
            Ok(true) => {
                self.push_undo_snapshot(previous_snapshot);
                let cleared_exi = self.invalidate_after_document_edit();
                self.select_document_node(Some(selected_id));
                self.set_status(self.with_exi_notice(
                    if move_up {
                        "Moved selected element up.".to_string()
                    } else {
                        "Moved selected element down.".to_string()
                    },
                    cleared_exi,
                ));
            }
            Ok(false) => self.set_status(if move_up {
                "Selected element is already at the top of its sibling list."
            } else {
                "Selected element is already at the bottom of its sibling list."
            }),
            Err(err) => self.set_status(format!("Cannot reorder selected element: {err}")),
        }
    }

    fn request_selected_element_removal(&mut self) {
        let Some(selected_id) = self.xml_tree_view.selected_id() else {
            self.set_status("No element selected.");
            return;
        };

        if self.pending_delete_confirmation != Some(selected_id) {
            self.pending_delete_confirmation = Some(selected_id);
            self.set_status("Click delete again to confirm removing the selected element.");
            return;
        }

        self.remove_selected_element();
    }

    fn remove_selected_element(&mut self) {
        self.clear_delete_confirmation();
        let Some(selected_id) = self.xml_tree_view.selected_id() else {
            self.set_status("No element selected.");
            return;
        };
        let previous_snapshot = self.capture_document_snapshot();

        let remove_result = match self.current_document.as_mut() {
            Some(doc) => doc.remove_element(selected_id),
            None => {
                self.set_status("No document loaded.");
                return;
            }
        };

        match remove_result {
            Ok(Some(parent_id)) => {
                self.push_undo_snapshot(previous_snapshot);
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

    fn clear_delete_confirmation(&mut self) {
        self.pending_delete_confirmation = None;
    }

    fn sync_selection_from_document(&mut self) {
        self.clear_delete_confirmation();
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
        self.clear_delete_confirmation();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::parse_xml;
    use tempfile::NamedTempFile;

    fn child_id(panel: &MainPanel) -> u64 {
        panel
            .current_document
            .as_ref()
            .and_then(|doc| doc.root.as_element())
            .and_then(|root| root.children[0].as_element())
            .expect("child element")
            .id
            .0
    }

    fn panel_with_document(xml: &str) -> MainPanel {
        let mut panel = MainPanel::new();
        panel.current_document = Some(parse_xml(xml).expect("parse xml"));
        panel.history.reset_saved(panel.current_document.as_ref());
        panel
    }

    #[test]
    fn undo_and_redo_restore_dirty_state() {
        let mut panel = panel_with_document(r#"<root><child/></root>"#);
        let node_id = child_id(&panel);
        let previous = panel.capture_document_snapshot();

        panel
            .current_document
            .as_mut()
            .expect("document")
            .rename_element(node_id, "renamed".to_string())
            .expect("rename");
        panel.push_undo_snapshot(previous);

        assert!(panel.has_unsaved_changes());

        panel.undo_last_change();
        assert!(!panel.has_unsaved_changes());
        assert_eq!(
            panel
                .current_document
                .as_ref()
                .and_then(|doc| doc.find_element(node_id))
                .map(|e| e.name.as_str()),
            Some("child")
        );

        panel.redo_last_change();
        assert!(panel.has_unsaved_changes());
        assert_eq!(
            panel
                .current_document
                .as_ref()
                .and_then(|doc| doc.find_element(node_id))
                .map(|e| e.name.as_str()),
            Some("renamed")
        );
    }

    #[test]
    fn request_unsaved_action_queues_prompt_when_dirty() {
        let mut panel = panel_with_document(r#"<root><child/></root>"#);
        let node_id = child_id(&panel);
        panel
            .current_document
            .as_mut()
            .expect("document")
            .rename_element(node_id, "renamed".to_string())
            .expect("rename");

        let ctx = Context::default();
        panel.request_unsaved_action(&ctx, PendingUnsavedAction::Quit);

        assert!(matches!(
            panel.pending_unsaved_action,
            Some(PendingUnsavedAction::Quit)
        ));
    }

    #[test]
    fn discard_quit_allows_one_close_without_saving() {
        let mut panel = panel_with_document(r#"<root><child/></root>"#);
        let node_id = child_id(&panel);
        panel
            .current_document
            .as_mut()
            .expect("document")
            .rename_element(node_id, "renamed".to_string())
            .expect("rename");
        panel.pending_unsaved_action = Some(PendingUnsavedAction::Quit);

        let ctx = Context::default();
        panel.discard_unsaved_changes_and_continue(&ctx, PendingUnsavedAction::Quit);

        assert!(panel.pending_unsaved_action.is_none());
        assert!(panel.allow_unsaved_close_once);
        assert!(panel.has_unsaved_changes());
    }

    #[test]
    fn save_xml_marks_document_clean() {
        let mut panel = panel_with_document(r#"<root><child/></root>"#);
        let node_id = child_id(&panel);
        let previous = panel.capture_document_snapshot();

        panel
            .current_document
            .as_mut()
            .expect("document")
            .rename_element(node_id, "renamed".to_string())
            .expect("rename");
        panel.push_undo_snapshot(previous);

        let temp_file = NamedTempFile::new().expect("temp file");
        let path = temp_file.path().to_path_buf();

        assert!(panel.save_xml(&path));
        assert!(!panel.has_unsaved_changes());
        assert_eq!(panel.current_file_path.as_ref(), Some(&path));
        assert_eq!(panel.current_file_type, FileType::Xml);
    }
}
