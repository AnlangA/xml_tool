//! UI panels for the shell: top bar, document tabs, outline, inspector,
//! central source view, problems, status bar, and modal dialogs.
//!
//! Every visible string goes through the shell's `Localization`. The
//! outline renders through the flattened-row cache with a fixed row height,
//! so only viewport rows are laid out regardless of document size.

use egui::{
    CentralPanel, Context, RichText, ScrollArea, SidePanel, TextEdit, TopBottomPanel, Ui, Window,
};

use crate::core::Command;
use crate::core::document::{NodeId, XmlNodeKind};
use crate::fluent_args;
use crate::services::outline::FlatTree;
use crate::services::search::{SearchHit, SearchIndex};
use crate::services::workspace::DocumentMode;
use crate::ui::icons::Icons;
pub use crate::ui::inspector::{inspector_contents, inspector_panel};
use crate::ui::localization::Language;
use crate::ui::shell::{AppShell, Dialog, FocusPane};
use crate::ui::theme::Theme;
use crate::ui::theme_prefs::ThemeMode;

pub const OUTLINE_MIN_WIDTH: f32 = 260.0;
pub const OUTLINE_MAX_WIDTH: f32 = 420.0;
pub const INSPECTOR_WIDTH: f32 = 320.0;
const OUTLINE_ROW_HEIGHT: f32 = 20.0;

/// Cached flattened outline plus element count for the status bar.
pub struct OutlineCache {
    pub tree: FlatTree,
    pub elements: usize,
}

/// Search box state for the active session.
#[derive(Default)]
pub struct SearchState {
    pub open: bool,
    pub needle: String,
    pub case_sensitive: bool,
    pub index: Option<(u64, u64, SearchIndex)>, // (session, revision, index)
    pub hits: Vec<SearchHit>,
    pub current: usize,
}

impl SearchState {
    pub(crate) fn jump_next(&mut self) {
        if !self.hits.is_empty() {
            self.current = (self.current + 1) % self.hits.len();
        }
    }

    pub(crate) fn jump_previous(&mut self) {
        if !self.hits.is_empty() {
            self.current = if self.current == 0 {
                self.hits.len() - 1
            } else {
                self.current - 1
            };
        }
    }

    pub(crate) fn selected_node(&self) -> Option<NodeId> {
        self.hits.get(self.current).map(|hit| hit.node)
    }
}

// ---------------------------------------------------------------------------
// Top: menu bar, toolbar, document tabs
// ---------------------------------------------------------------------------

pub fn top_bar(ctx: &Context, shell: &mut AppShell) {
    TopBottomPanel::top("top-bar").show(ctx, |ui| {
        menu_bar(ui, shell);
        toolbar(ui, shell);
    });
}

fn menu_bar(ui: &mut Ui, shell: &mut AppShell) {
    ui.horizontal_wrapped(|ui| {
        ui.menu_button(shell.localization.msg("menu-file"), |ui| {
            if ui.button(shell.localization.msg("action-new")).clicked() {
                shell.new_document();
            }
            if ui.button(shell.localization.msg("action-open")).clicked() {
                shell.pick_and_open();
            }
            if ui.button(shell.localization.msg("action-save")).clicked() {
                shell.save_active(None);
            }
            if ui
                .button(shell.localization.msg("action-save-as"))
                .clicked()
            {
                shell.pick_and_save_as();
            }
            if ui
                .button(shell.localization.msg("action-save-all"))
                .clicked()
            {
                save_all_dirty(shell);
            }
            ui.separator();
            if ui
                .button(shell.localization.msg("action-close-tab"))
                .clicked()
            {
                shell.close_active_tab();
            }
            ui.separator();
            if ui.button(shell.localization.msg("action-exit")).clicked() {
                if shell.workspace.dirty_sessions().is_empty() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                } else {
                    shell.dialog = Some(Dialog::UnsavedExit);
                }
            }
        });
        ui.menu_button(shell.localization.msg("menu-edit"), |ui| {
            let undo_enabled = shell
                .workspace
                .active()
                .is_some_and(|session| session.history.undo_depth() > 0);
            if ui
                .add_enabled(
                    undo_enabled,
                    egui::Button::new(shell.localization.msg("action-undo")),
                )
                .clicked()
            {
                shell.undo();
            }
            let redo_enabled = shell
                .workspace
                .active()
                .is_some_and(|session| session.history.redo_depth() > 0);
            if ui
                .add_enabled(
                    redo_enabled,
                    egui::Button::new(shell.localization.msg("action-redo")),
                )
                .clicked()
            {
                shell.redo();
            }
        });
        ui.menu_button(shell.localization.msg("menu-search"), |ui| {
            if ui.button(shell.localization.msg("action-find")).clicked() {
                shell.search.open = true;
                shell.focus = FocusPane::Outline;
            }
        });
        ui.menu_button(shell.localization.msg("menu-xml"), |ui| {
            if ui.button(shell.localization.msg("action-format")).clicked() {
                shell.commit(Command::FormatDocument {
                    indent: "  ".to_string(),
                });
            }
        });
        ui.menu_button(shell.localization.msg("menu-exi"), |ui| {
            if ui
                .button(shell.localization.msg("exi-open-workbench"))
                .clicked()
            {
                shell.push_problem(
                    crate::core::Severity::Info,
                    "exi-pending",
                    shell.localization.msg("exi-not-ready"),
                );
            }
        });
        ui.menu_button(shell.localization.msg("menu-view"), |ui| {
            ui.label(shell.localization.msg("view-theme"));
            for (mode, key) in [
                (ThemeMode::System, "theme-system"),
                (ThemeMode::Light, "theme-light"),
                (ThemeMode::Dark, "theme-dark"),
            ] {
                if ui
                    .radio(shell.theme_mode == mode, shell.localization.msg(key))
                    .clicked()
                {
                    shell.theme_mode = mode;
                }
            }
            ui.separator();
            ui.label(shell.localization.msg_with(
                "font-scale",
                Some(&fluent_args!("percent" => i32::from(shell.font_scale.0))),
            ));
            if ui.button("−").clicked() {
                shell.font_scale = shell.font_scale.step_down();
            }
            if ui.button("+").clicked() {
                shell.font_scale = shell.font_scale.step_up();
            }
            ui.separator();
            ui.label(shell.localization.msg("view-language"));
            for (language, label) in [
                (Language::English, "English"),
                (Language::Chinese, "简体中文"),
            ] {
                if ui
                    .radio(shell.localization.language() == language, label)
                    .clicked()
                {
                    shell.localization.set_language(language);
                }
            }
        });
        ui.menu_button(shell.localization.msg("menu-help"), |ui| {
            if ui.button(shell.localization.msg("shortcut-help")).clicked() {
                shell.dialog = Some(Dialog::Shortcuts);
            }
            if ui.button(shell.localization.msg("action-about")).clicked() {
                shell.dialog = Some(Dialog::About);
            }
        });
    });
}

pub fn save_all_dirty(shell: &mut AppShell) {
    for index in 0..shell.workspace.sessions().len() {
        shell.workspace.select(index);
        if shell
            .workspace
            .active()
            .is_some_and(|session| session.is_dirty())
        {
            shell.save_active(None);
        }
    }
}

fn toolbar(ui: &mut Ui, shell: &mut AppShell) {
    ui.horizontal_wrapped(|ui| {
        if ui
            .add(egui::Button::new(Icons::FILE_PLUS))
            .on_hover_text(shell.localization.msg("toolbar-new-file"))
            .clicked()
        {
            shell.new_document();
        }
        if ui
            .add(egui::Button::new(Icons::FOLDER_OPEN))
            .on_hover_text(shell.localization.msg("toolbar-open-file"))
            .clicked()
        {
            shell.pick_and_open();
        }
        if ui
            .add(egui::Button::new(Icons::FLOPPY_DISK))
            .on_hover_text(shell.localization.msg("toolbar-save-file"))
            .clicked()
        {
            shell.save_active(None);
        }
        if ui
            .add(egui::Button::new(Icons::MAGIC_WAND))
            .on_hover_text(shell.localization.msg("toolbar-format"))
            .clicked()
        {
            shell.commit(Command::FormatDocument {
                indent: "  ".to_string(),
            });
        }
        if shell.open_pending > 0 {
            ui.spinner();
        }
    });
}

pub fn document_tabs(ctx: &Context, shell: &mut AppShell) {
    TopBottomPanel::top("document-tabs").show(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            let titles: Vec<(String, bool)> = shell
                .workspace
                .sessions()
                .iter()
                .map(|session| (session.display_name(), session.is_dirty()))
                .collect();
            let active = shell
                .workspace
                .sessions()
                .iter()
                .position(|session| Some(session.id) == shell.workspace.active_id());
            let mut select = None;
            let mut close = None;
            for (index, (title, dirty)) in titles.iter().enumerate() {
                let label = if *dirty {
                    format!("{title} ●")
                } else {
                    title.clone()
                };
                let is_active = Some(index) == active;
                let mut text = RichText::new(&label);
                if is_active {
                    text = text.strong();
                }
                if ui.selectable_label(is_active, text).clicked() {
                    select = Some(index);
                }
                if ui.small_button("×").clicked() {
                    close = Some(index);
                }
                ui.separator();
            }
            if let Some(index) = select {
                shell.workspace.select(index);
                shell.outline_cache = None;
                shell.search.index = None;
            }
            if let Some(index) = close {
                shell.workspace.select(index);
                shell.close_active_tab();
            }
        });
    });
}

// ---------------------------------------------------------------------------
// Left: outline + search
// ---------------------------------------------------------------------------

pub fn outline_panel(ctx: &Context, shell: &mut AppShell, drawer: bool) {
    let panel = if drawer {
        SidePanel::left("outline-drawer")
            .resizable(false)
            .exact_width(OUTLINE_MIN_WIDTH)
    } else {
        SidePanel::left("outline")
            .resizable(true)
            .min_width(OUTLINE_MIN_WIDTH)
            .max_width(OUTLINE_MAX_WIDTH)
    };
    panel.show(ctx, |ui| {
        outline_contents(ui, shell);
    });
}

fn outline_contents(ui: &mut Ui, shell: &mut AppShell) {
    ui.heading(shell.localization.msg("panel-outline"));
    ui.horizontal(|ui| {
        if ui
            .small_button(shell.localization.msg("outline-expand-all"))
            .clicked()
            && let Some(session) = shell.workspace.active_id()
        {
            shell.expand_all(session);
        }
        if ui
            .small_button(shell.localization.msg("outline-collapse-all"))
            .clicked()
            && let Some(session) = shell.workspace.active_id()
        {
            shell.collapse_all(session);
        }
    });
    search_box(ui, shell);

    let Some(session) = shell.workspace.active() else {
        ui.label(shell.localization.msg("panel-empty"));
        return;
    };
    if session.mode == DocumentMode::LargeReadOnly {
        ui.label(
            RichText::new(shell.localization.msg("readonly-reason"))
                .color(Theme::WARNING)
                .small(),
        );
    }

    let session_id = session.id;
    let rows = shell.outline_cache().tree.rows.clone();
    let mut toggle = None;
    let mut select = None;
    let search_node = shell.search.selected_node();
    let selection = shell.workspace.active().and_then(|s| s.selection);

    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, OUTLINE_ROW_HEIGHT, rows.len(), |ui, range| {
            for row in &rows[range] {
                let indent = (row.depth as f32) * 14.0;
                ui.horizontal(|ui| {
                    ui.add_space(indent);
                    let arrow = if row.expandable {
                        if row.expanded { "▾" } else { "▸" }
                    } else {
                        ""
                    };
                    if ui
                        .add(
                            egui::Label::new(RichText::new(arrow).color(Theme::TEXT_SECONDARY))
                                .selectable(false),
                        )
                        .interact(egui::Sense::click())
                        .clicked()
                    {
                        toggle = Some(row.node);
                    }
                    let kind = shell
                        .workspace
                        .active()
                        .and_then(|s| s.document.kind(row.node));
                    let label_text = shell
                        .workspace
                        .active()
                        .map(|s| row_label(&s.document, row.node))
                        .unwrap_or_default();
                    let name_color = match kind {
                        Some(XmlNodeKind::Comment) => Theme::COMMENT,
                        Some(XmlNodeKind::Text) | Some(XmlNodeKind::CData) => Theme::TEXT_SECONDARY,
                        Some(XmlNodeKind::ProcessingInstruction) => Theme::SYNTAX_KEYWORD,
                        _ => Theme::ELEMENT_NAME,
                    };
                    let is_selected = selection == Some(row.node) || search_node == Some(row.node);
                    let mut text = RichText::new(label_text).color(if is_selected {
                        Theme::TEXT_HIGHLIGHT
                    } else {
                        name_color
                    });
                    if is_selected {
                        text = text.strong();
                    }
                    let response = ui.add(egui::Label::new(text).selectable(false));
                    if response.clicked() {
                        select = Some(row.node);
                    }
                    if response.interact(egui::Sense::click()).double_clicked() {
                        toggle = Some(row.node);
                    }
                });
            }
        });

    if let Some(node) = toggle {
        shell.toggle_expanded(session_id, node);
    }
    if let Some(node) = select {
        if let Some(session) = shell.workspace.active_mut() {
            session.selection = Some(node);
        }
        shell.focus = FocusPane::Inspector;
    }
}

/// Overlay inspector drawer for narrow windows.
pub fn inspector_drawer(ctx: &Context, shell: &mut AppShell) {
    let height = ctx.content_rect().height();
    Window::new("inspector-drawer")
        .title_bar(false)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::RIGHT_TOP, [0.0, 0.0])
        .fixed_size([INSPECTOR_WIDTH, height])
        .show(ctx, |ui| {
            inspector_contents(ui, shell);
        });
}

fn row_label(document: &crate::core::document::XmlDocument, node: NodeId) -> String {
    match document.kind(node) {
        Some(XmlNodeKind::Element) => {
            let mut text = document.qname(node).map(|q| q.render()).unwrap_or_default();
            for (name, value) in document.attributes(node) {
                let shown: String = value.chars().take(24).collect();
                text.push_str(&format!(" {name}=\"{shown}\""));
            }
            text
        }
        Some(XmlNodeKind::Text) | Some(XmlNodeKind::CData) => document
            .node_text(node)
            .unwrap_or_default()
            .trim()
            .chars()
            .take(40)
            .collect(),
        Some(XmlNodeKind::Comment) => {
            let shown: String = document
                .comment_text(node)
                .unwrap_or_default()
                .chars()
                .take(40)
                .collect();
            format!("<!-- {shown} -->")
        }
        Some(XmlNodeKind::ProcessingInstruction) => match document.pi(node) {
            Some((target, data)) => match data {
                Some(data) => format!("<?{target} {data}?>"),
                None => format!("<?{target}?>"),
            },
            None => String::new(),
        },
        _ => String::new(),
    }
}

fn search_box(ui: &mut Ui, shell: &mut AppShell) {
    if !shell.search.open {
        return;
    }
    ui.horizontal(|ui| {
        let mut needle = shell.search.needle.clone();
        let response = ui.add(
            TextEdit::singleline(&mut needle)
                .hint_text(shell.localization.msg("search-placeholder"))
                .desired_width(160.0),
        );
        if response.changed() {
            shell.search.needle = needle;
            shell.refresh_search();
        }
        let case = shell.search.case_sensitive;
        if ui
            .selectable_label(case, "Aa")
            .on_hover_text(shell.localization.msg("search-case-sensitive"))
            .clicked()
        {
            shell.search.case_sensitive = !case;
            shell.refresh_search();
        }
        let hits = shell.search.hits.len();
        let summary = if shell.search.needle.is_empty() {
            String::new()
        } else if hits == 0 {
            shell.localization.msg("search-no-hits")
        } else {
            shell
                .localization
                .msg_with("search-hits", Some(&fluent_args!("count" => hits as i32)))
        };
        ui.label(RichText::new(summary).small().color(Theme::TEXT_MUTED));
    });
}

/// Overlay outline drawer for narrow windows: an anchored frameless
/// window drawn above the central panel.
pub fn outline_drawer(ctx: &Context, shell: &mut AppShell) {
    let height = ctx.content_rect().height();
    Window::new("outline-drawer")
        .title_bar(false)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::LEFT_TOP, [0.0, 0.0])
        .fixed_size([OUTLINE_MIN_WIDTH, height])
        .show(ctx, |ui| {
            outline_contents(ui, shell);
        });
}

// ---------------------------------------------------------------------------
// Center: source view
// ---------------------------------------------------------------------------

pub fn central_panel(ctx: &Context, shell: &mut AppShell) {
    CentralPanel::default().show(ctx, |ui| {
        let Some(session) = shell.workspace.active() else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new(shell.localization.msg("panel-empty"))
                        .size(20.0)
                        .color(Theme::TEXT_MUTED),
                );
            });
            return;
        };
        if session.mode == DocumentMode::LargeReadOnly {
            ui.label(
                RichText::new(shell.localization.msg("status-read-only"))
                    .color(Theme::WARNING)
                    .small(),
            );
        }

        let editable = session.mode == DocumentMode::Editable;
        let draft_active = session.source_draft.is_some();
        let mut text = session
            .source_draft
            .as_ref()
            .map(|draft| draft.buffer.text())
            .unwrap_or_else(|| session.document.source().to_string());
        let revision = session.document.revision();

        if draft_active {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(shell.localization.msg("source-draft-active"))
                        .color(Theme::WARNING)
                        .small(),
                );
                if ui.button(shell.localization.msg("source-apply")).clicked() {
                    shell.apply_source();
                }
                if ui
                    .button(shell.localization.msg("source-discard-draft"))
                    .clicked()
                {
                    shell.discard_draft();
                }
            });
        }

        let mut changed = false;
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let editor = TextEdit::multiline(&mut text)
                    .font(egui::TextStyle::Monospace)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .lock_focus(true)
                    .id(egui::Id::new(("source", revision.0, draft_active)));
                let response = ui.add_enabled(editable, editor);
                changed = response.changed();
            });
        if changed {
            shell.update_source_text(text);
        }
    });
}

// ---------------------------------------------------------------------------
// Bottom: problems + tasks
// ---------------------------------------------------------------------------

pub fn bottom_panel(ctx: &Context, shell: &mut AppShell) {
    TopBottomPanel::bottom("problems")
        .resizable(true)
        .default_height(110.0)
        .show(ctx, |ui| {
            ui.heading(shell.localization.msg("panel-problems"));
            if shell.problems.is_empty() {
                ui.label(
                    RichText::new(shell.localization.msg("problems-empty"))
                        .color(Theme::TEXT_MUTED),
                );
                return;
            }
            ScrollArea::vertical().show(ui, |ui| {
                for problem in &shell.problems {
                    let color = match problem.severity {
                        crate::core::Severity::Error => Theme::ERROR,
                        crate::core::Severity::Warning => Theme::WARNING,
                        crate::core::Severity::Info => Theme::INFO,
                    };
                    ui.label(
                        RichText::new(format!("{} {}", problem.code, problem.message_key))
                            .color(color),
                    );
                }
            });
        });
}

// ---------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------

pub fn status_bar(ctx: &Context, shell: &mut AppShell) {
    TopBottomPanel::bottom("status-bar").show(ctx, |ui| {
        let elements = if shell.workspace.active().is_some() {
            shell.outline_cache().elements
        } else {
            0
        };
        let status_texts = {
            let localization = &shell.localization;
            match shell.workspace.active() {
                None => vec![localization.msg("status-ready")],
                Some(session) => {
                    let mut parts = vec![
                        if session.is_dirty() {
                            localization.msg("status-edited")
                        } else {
                            localization.msg("status-clean")
                        },
                        localization.msg_with(
                            "status-encoding",
                            Some(&fluent_args!(
                                "name" => session.document.encoding().declaration_name()
                            )),
                        ),
                        localization.msg_with(
                            "status-elements",
                            Some(&fluent_args!("count" => elements as i32)),
                        ),
                    ];
                    if session.mode == DocumentMode::LargeReadOnly {
                        parts.push(localization.msg("status-read-only"));
                    }
                    parts
                }
            }
        };
        ui.horizontal(|ui| {
            for (index, text) in status_texts.iter().enumerate() {
                if index > 0 {
                    ui.separator();
                }
                let mut rich = RichText::new(text);
                if shell
                    .workspace
                    .active()
                    .is_some_and(|s| s.mode == DocumentMode::LargeReadOnly)
                    && index == status_texts.len() - 1
                {
                    rich = rich.color(Theme::WARNING);
                }
                ui.label(rich);
            }
        });
    });
}

#[allow(dead_code)]
fn unused_status_body(_ui: &mut Ui, _shell: &mut AppShell) {}
