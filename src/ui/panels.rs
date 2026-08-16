//! UI panels for the shell: top bar, document tabs, outline, inspector,
//! central source view, problems, status bar, and modal dialogs.
//!
//! Every visible string goes through the shell's `Localization`. The
//! outline renders through the flattened-row cache with a fixed row height,
//! so only viewport rows are laid out regardless of document size. Colors
//! come from the per-frame [`Palette`], so they follow the active
//! light/dark theme.

use egui::{
    Align2, CentralPanel, Context, CornerRadius, FontId, Frame, Margin, RichText, ScrollArea,
    Sense, SidePanel, Stroke, TextEdit, TopBottomPanel, Ui, Window, vec2,
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
use crate::ui::syntax_highlighter::SyntaxHighlighter;
use crate::ui::theme::{Palette, Spacing, Typography};
use crate::ui::theme_prefs::ThemeMode;

pub const OUTLINE_MIN_WIDTH: f32 = 260.0;
pub const OUTLINE_MAX_WIDTH: f32 = 420.0;
pub const INSPECTOR_WIDTH: f32 = 320.0;
const OUTLINE_ROW_HEIGHT: f32 = 20.0;
const OUTLINE_INDENT: f32 = 14.0;
const OUTLINE_ARROW_WIDTH: f32 = 18.0;
/// Above this many characters the source view skips syntax highlighting so
/// huge documents stay responsive.
const HIGHLIGHT_CHAR_LIMIT: usize = 200_000;

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
    /// Session the hits belong to; switching tabs invalidates them.
    pub session: Option<crate::services::task_manager::SessionId>,
}

impl SearchState {
    pub(crate) fn jump_next(&mut self) {
        if !self.hits.is_empty() {
            self.current = (self.current + 1) % self.hits.len();
        }
    }

    pub(crate) fn jump_previous(&mut self) {
        if self.hits.is_empty() {
            return;
        }
        self.current = if self.current == 0 {
            self.hits.len() - 1
        } else {
            self.current - 1
        };
    }

    pub(crate) fn selected_node(&self) -> Option<NodeId> {
        self.hits.get(self.current).map(|hit| hit.node)
    }
}

// ---------------------------------------------------------------------------
// Shared widgets
// ---------------------------------------------------------------------------

/// Uniform panel header: accent icon + strong title over a thin rule.
pub(crate) fn panel_heading(ui: &mut Ui, icon: &str, title: String) {
    let pal = Palette::resolve(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).color(pal.accent));
        ui.label(RichText::new(title).strong());
    });
    ui.add_space(Spacing::XXS);
    ui.separator();
    ui.add_space(Spacing::XXS);
}

/// Muted caption used for inspector/dialog sections.
pub(crate) fn section_label(ui: &mut Ui, text: String) {
    let pal = Palette::resolve(ui.ctx());
    ui.label(RichText::new(text).small().strong().color(pal.text_muted));
}

/// Ghost toolbar button: flat at rest, framed on hover, fixed hit size.
fn toolbar_button(ui: &mut Ui, icon: &str, tooltip: String) -> egui::Response {
    toolbar_button_enabled(ui, icon, tooltip, true)
}

/// [`toolbar_button`] with an enabled flag (greyed out and inert when off).
fn toolbar_button_enabled(
    ui: &mut Ui,
    icon: &str,
    tooltip: String,
    enabled: bool,
) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(icon).size(Typography::HEADING_3))
            .min_size(vec2(28.0, 24.0))
            .frame_when_inactive(false),
    )
    .on_hover_text(tooltip)
}

/// Tooltip text with an optional keyboard shortcut suffix.
fn with_shortcut(tooltip: String, shortcut: &str) -> String {
    format!("{tooltip} ({shortcut})")
}

// ---------------------------------------------------------------------------
// Top: menu bar, toolbar, document tabs
// ---------------------------------------------------------------------------

pub fn top_bar(ctx: &Context, shell: &mut AppShell) {
    let pal = Palette::resolve(ctx);
    TopBottomPanel::top("top-bar")
        .frame(Frame::new().fill(pal.panel_bg))
        .show(ctx, |ui| {
            // One compact header row: menus, then the icon toolbar.
            ui.horizontal_wrapped(|ui| {
                menu_bar(ui, shell);
                ui.separator();
                toolbar(ui, shell);
            });
        });
}

fn menu_bar(ui: &mut Ui, shell: &mut AppShell) {
    {
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
                    shell.after_unsaved = Some(crate::ui::shell::AfterUnsaved::Exit);
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
            if ui.button(shell.localization.msg("action-xpath")).clicked() {
                shell.run_xpath_dialog();
            }
            if ui
                .button(shell.localization.msg("action-validate-with"))
                .clicked()
            {
                shell.run_validation_dialog();
            }
            if ui.button(shell.localization.msg("action-diff")).clicked() {
                shell.run_diff_dialog();
            }
        });
        ui.menu_button(shell.localization.msg("menu-exi"), |ui| {
            if ui
                .button(shell.localization.msg("exi-open-workbench"))
                .clicked()
            {
                shell.open_exi_workbench();
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
    }
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
    let pal = Palette::resolve(ui.ctx());
    {
        if toolbar_button(
            ui,
            Icons::FILE_PLUS,
            with_shortcut(shell.localization.msg("toolbar-new-file"), "Ctrl+N"),
        )
        .clicked()
        {
            shell.new_document();
        }
        if toolbar_button(
            ui,
            Icons::FOLDER_OPEN,
            with_shortcut(shell.localization.msg("toolbar-open-file"), "Ctrl+O"),
        )
        .clicked()
        {
            shell.pick_and_open();
        }
        let has_session = shell.workspace.active().is_some();
        if toolbar_button_enabled(
            ui,
            Icons::FLOPPY_DISK,
            with_shortcut(shell.localization.msg("toolbar-save-file"), "Ctrl+S"),
            has_session,
        )
        .clicked()
        {
            shell.save_active(None);
        }
        if toolbar_button_enabled(
            ui,
            Icons::MAGIC_WAND,
            shell.localization.msg("toolbar-format"),
            has_session,
        )
        .clicked()
        {
            shell.commit(Command::FormatDocument {
                indent: "  ".to_string(),
            });
        }
        ui.separator();
        let undo_enabled = shell
            .workspace
            .active()
            .is_some_and(|session| session.history.undo_depth() > 0);
        if ui
            .add_enabled(
                undo_enabled,
                egui::Button::new(
                    RichText::new(Icons::ARROW_COUNTER_CLOCKWISE).size(Typography::HEADING_3),
                )
                .min_size(vec2(28.0, 24.0))
                .frame_when_inactive(false),
            )
            .on_hover_text(with_shortcut(
                shell.localization.msg("toolbar-undo"),
                "Ctrl+Z",
            ))
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
                egui::Button::new(
                    RichText::new(Icons::ARROW_CLOCKWISE).size(Typography::HEADING_3),
                )
                .min_size(vec2(28.0, 24.0))
                .frame_when_inactive(false),
            )
            .on_hover_text(with_shortcut(
                shell.localization.msg("toolbar-redo"),
                "Ctrl+Y",
            ))
            .clicked()
        {
            shell.redo();
        }
        let search_button =
            egui::Button::new(RichText::new(Icons::MAGNIFYING_GLASS).size(Typography::HEADING_3))
                .min_size(vec2(28.0, 24.0))
                .selected(shell.search.open);
        if ui
            .add_enabled(has_session, search_button)
            .on_hover_text(with_shortcut(
                shell.localization.msg("action-find"),
                "Ctrl+F",
            ))
            .clicked()
        {
            shell.search.open = !shell.search.open;
        }
        ui.separator();
        let (errors, warnings, infos) = shell.alerts.counts();
        let chip = if errors > 0 {
            format!("{} {errors}", Icons::WARNING)
        } else if warnings > 0 {
            format!("{} {warnings}", Icons::WARNING)
        } else if infos > 0 {
            format!("{} {infos}", Icons::INFO)
        } else {
            Icons::CHECK.to_string()
        };
        let chip_color = if errors > 0 {
            pal.error
        } else if warnings > 0 {
            pal.warning
        } else {
            pal.success
        };
        if ui
            .add(
                egui::Button::new(RichText::new(chip).color(chip_color))
                    .min_size(vec2(28.0, 24.0))
                    .frame_when_inactive(false)
                    .selected(shell.problems_panel_open),
            )
            .on_hover_text(shell.localization.msg("panel-problems"))
            .clicked()
        {
            shell.problems_panel_open = !shell.problems_panel_open;
        }
        if shell.open_pending > 0 {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(egui::Spinner::new().size(16.0));
            });
        }
    }
}

pub fn document_tabs(ctx: &Context, shell: &mut AppShell) {
    let pal = Palette::resolve(ctx);
    TopBottomPanel::top("document-tabs")
        .frame(Frame::new().fill(pal.panel_bg))
        .show(ctx, |ui| {
            // Horizontal scrolling keeps every tab reachable without wrapping
            // rows when a session list grows.
            egui::ScrollArea::horizontal()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = Spacing::XS;
                        let titles: Vec<(String, bool)> = shell
                            .workspace
                            .sessions()
                            .iter()
                            .map(|session| (session.display_name(), session.is_dirty()))
                            .collect();
                        let active =
                            shell.workspace.sessions().iter().position(|session| {
                                Some(session.id) == shell.workspace.active_id()
                            });
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
                                text = text.strong().color(pal.text_highlight);
                            }
                            if *dirty {
                                text = text.color(pal.warning);
                            }
                            let button = egui::Button::new(text).min_size(vec2(0.0, 24.0));
                            let button = if is_active {
                                button
                                    .fill(pal.card_bg)
                                    .stroke(Stroke::new(1.0_f32, pal.accent))
                            } else {
                                button.frame_when_inactive(false)
                            };
                            if ui.add(button).clicked() {
                                select = Some(index);
                            }
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(Icons::X)
                                            .size(Typography::TINY)
                                            .color(pal.text_muted),
                                    )
                                    .frame_when_inactive(false),
                                )
                                .on_hover_text(shell.localization.msg("tab-close"))
                                .clicked()
                            {
                                close = Some(index);
                            }
                        }
                        if let Some(index) = select {
                            shell.workspace.select(index);
                        }
                        if let Some(index) = close {
                            shell.workspace.select(index);
                            shell.close_active_tab();
                        }
                    });
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
    panel_heading(
        ui,
        Icons::TREE_STRUCTURE,
        shell.localization.msg("panel-outline"),
    );
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

    let pal = Palette::resolve(ui.ctx());
    let Some(session) = shell.workspace.active() else {
        ui.label(RichText::new(shell.localization.msg("panel-empty")).color(pal.text_muted));
        return;
    };
    if session.mode == DocumentMode::LargeReadOnly {
        ui.label(
            RichText::new(shell.localization.msg("readonly-reason"))
                .color(pal.warning)
                .small(),
        );
    }

    let session_id = session.id;
    let (rows, _) = shell.outline_snapshot();
    let rows = rows.rows;
    let mut toggle = None;
    let mut select = None;
    let search_node = shell.search.selected_node();
    let selection = shell.workspace.active().and_then(|s| s.selection);
    let body_font = egui::TextStyle::Body.resolve(ui.style());
    let arrow_font = FontId::proportional(body_font.size - 1.0);

    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, OUTLINE_ROW_HEIGHT, rows.len(), |ui, range| {
            for row in &rows[range] {
                let (rect, response) = ui.allocate_exact_size(
                    vec2(ui.available_width().max(1.0), OUTLINE_ROW_HEIGHT),
                    Sense::click(),
                );
                let is_selected = selection == Some(row.node) || search_node == Some(row.node);
                let painter = ui.painter_at(rect);
                if is_selected {
                    painter.rect_filled(rect, CornerRadius::same(4), pal.selection);
                    painter.rect_filled(
                        egui::Rect::from_min_size(rect.min, vec2(2.0, rect.height())),
                        CornerRadius::ZERO,
                        pal.accent,
                    );
                } else if response.hovered() {
                    painter.rect_filled(rect, CornerRadius::same(4), pal.hover_bg);
                }

                let indent = (row.depth as f32) * OUTLINE_INDENT;
                if row.expandable {
                    let arrow = if row.expanded { "▾" } else { "▸" };
                    painter.text(
                        egui::pos2(
                            rect.min.x + indent + OUTLINE_ARROW_WIDTH * 0.5,
                            rect.center().y,
                        ),
                        Align2::CENTER_CENTER,
                        arrow,
                        arrow_font.clone(),
                        pal.text_secondary,
                    );
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
                    Some(XmlNodeKind::Comment) => pal.comment,
                    Some(XmlNodeKind::Text) | Some(XmlNodeKind::CData) => pal.text_secondary,
                    Some(XmlNodeKind::ProcessingInstruction) => pal.syntax_keyword,
                    _ => pal.element_name,
                };
                let color = if is_selected {
                    pal.text_highlight
                } else {
                    name_color
                };
                painter.text(
                    egui::pos2(rect.min.x + indent + OUTLINE_ARROW_WIDTH, rect.center().y),
                    Align2::LEFT_CENTER,
                    label_text,
                    body_font.clone(),
                    color,
                );

                if response.double_clicked() {
                    toggle = Some(row.node);
                } else if response.clicked() {
                    let arrow_zone_end = rect.min.x + indent + OUTLINE_ARROW_WIDTH;
                    let on_arrow = row.expandable
                        && response
                            .interact_pointer_pos()
                            .is_some_and(|pos| pos.x <= arrow_zone_end);
                    if on_arrow {
                        toggle = Some(row.node);
                    } else {
                        select = Some(row.node);
                    }
                }
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
    let pal = Palette::resolve(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(RichText::new(Icons::MAGNIFYING_GLASS).color(pal.text_muted));
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
        ui.label(RichText::new(summary).small().color(pal.text_muted));
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
        let pal = Palette::resolve(ui.ctx());
        if shell.workspace.active().is_none() {
            welcome_view(ui, shell, &pal);
            return;
        }
        let session = shell.workspace.active().expect("checked above");
        if session.mode == DocumentMode::LargeReadOnly {
            ui.label(
                RichText::new(shell.localization.msg("status-read-only"))
                    .color(pal.warning)
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

        if let Some((line, column)) = shell.source_jump {
            let hint = shell.localization.msg_with(
                "source-jump-hint",
                Some(&crate::fluent_args!(
                    "line" => line as i32,
                    "column" => column as i32
                )),
            );
            Frame::new()
                .fill(pal.info_bg)
                .corner_radius(CornerRadius::same(4))
                .inner_margin(Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(Icons::INFO).color(pal.info));
                        ui.label(RichText::new(hint).color(pal.info));
                        if ui.small_button(Icons::X).clicked() {
                            shell.source_jump = None;
                        }
                    });
                });
        }
        if draft_active {
            Frame::new()
                .fill(pal.warning_bg)
                .corner_radius(CornerRadius::same(4))
                .inner_margin(Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(shell.localization.msg("source-draft-active"))
                                .color(pal.warning)
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
                });
        }

        let mut changed = false;
        let highlight = text.len() <= HIGHLIGHT_CHAR_LIMIT;
        let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
            let font_id = egui::TextStyle::Monospace.resolve(ui.style());
            let job = if highlight {
                SyntaxHighlighter::new().highlight_layout_job(
                    &pal,
                    buf.as_str(),
                    font_id,
                    wrap_width,
                )
            } else {
                egui::text::LayoutJob::single_section(
                    buf.as_str().to_owned(),
                    egui::text::TextFormat {
                        font_id,
                        color: pal.text_primary,
                        ..Default::default()
                    },
                )
            };
            ui.fonts_mut(|fonts| fonts.layout_job(job))
        };
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut editor = TextEdit::multiline(&mut text)
                    .font(egui::TextStyle::Monospace)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .lock_focus(true)
                    .id(egui::Id::new(("source", revision.0, draft_active)));
                if highlight {
                    editor = editor.layouter(&mut layouter);
                }
                let response = ui.add_enabled(editable, editor);
                changed = response.changed();
            });
        if changed {
            shell.update_source_text(text);
        }
    });
}

/// Landing view shown when no document is open.
fn welcome_view(ui: &mut Ui, shell: &mut AppShell, pal: &Palette) {
    let height = ui.available_height();
    ui.add_space(height * 0.2);
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new(Icons::FILE_PLUS)
                .size(44.0)
                .color(pal.text_muted),
        );
        ui.add_space(Spacing::SM);
        ui.label(
            RichText::new(shell.localization.msg("welcome-title"))
                .size(Typography::HEADING_2)
                .strong(),
        );
        ui.add_space(Spacing::XS);
        ui.label(RichText::new(shell.localization.msg("welcome-hint")).color(pal.text_muted));
    });
    ui.add_space(Spacing::MD);
    // Equal-width stacked buttons center reliably inside `vertical_centered`
    // (a horizontal strip would claim the full row width and left-align).
    ui.vertical_centered(|ui| {
        if ui
            .add_sized(
                [180.0, 28.0],
                egui::Button::new(format!(
                    "{} {}",
                    Icons::FILE_PLUS,
                    shell.localization.msg("action-new")
                )),
            )
            .clicked()
        {
            shell.new_document();
        }
        ui.add_space(Spacing::XS);
        if ui
            .add_sized(
                [180.0, 28.0],
                egui::Button::new(format!(
                    "{} {}",
                    Icons::FOLDER_OPEN,
                    shell.localization.msg("action-open")
                )),
            )
            .clicked()
        {
            shell.pick_and_open();
        }
        ui.add_space(Spacing::SM);
        ui.label(
            RichText::new("Ctrl+N · Ctrl+O")
                .small()
                .monospace()
                .color(pal.text_muted),
        );
    });
}

// ---------------------------------------------------------------------------
// Banner strip: non-blocking alerts under the toolbar
// ---------------------------------------------------------------------------

pub fn banner_strip(ctx: &Context, shell: &mut AppShell) {
    if shell.banner.is_none() {
        return;
    }
    let pal = Palette::resolve(ctx);
    let (path, dirty) = match &shell.banner {
        Some(crate::ui::shell::Banner::Reload { path, dirty }) => (path.clone(), *dirty),
        None => return,
    };
    let mut dismiss_banner = false;
    {
        {
            let path = &path;
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut action: Option<&str> = None;
            TopBottomPanel::top("banner-reload")
                .frame(
                    Frame::new()
                        .fill(pal.warning_bg)
                        .inner_margin(Margin::symmetric(8, 4)),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(Icons::WARNING).color(pal.warning));
                        let body_key = if dirty {
                            "dialog-reload-dirty-body"
                        } else {
                            "dialog-reload-clean-body"
                        };
                        ui.label(shell.localization.msg_with(
                            body_key,
                            Some(&crate::fluent_args!("name" => name.as_str())),
                        ));
                        if ui
                            .small_button(shell.localization.msg("dialog-reload-reload"))
                            .clicked()
                        {
                            action = Some("reload");
                        }
                        if dirty
                            && ui
                                .small_button(shell.localization.msg("dialog-reload-keep"))
                                .clicked()
                        {
                            action = Some("keep");
                        }
                        if ui
                            .small_button(shell.localization.msg("action-exit-cancel"))
                            .clicked()
                        {
                            action = Some("ignore");
                        }
                    });
                });
            match action {
                Some("reload") => {
                    let path = path.clone();
                    shell.open_path(path);
                    dismiss_banner = true;
                }
                Some("keep") | Some("ignore") => {
                    dismiss_banner = true; // dismissed
                }
                _ => {}
            }
        }
    }
    if dismiss_banner {
        shell.banner = None;
    }
}

// ---------------------------------------------------------------------------
// Bottom: problems + tasks
// ---------------------------------------------------------------------------

pub fn bottom_panel(ctx: &Context, shell: &mut AppShell) {
    if !shell.problems_panel_open {
        return;
    }
    let pal = Palette::resolve(ctx);
    let mut dismiss = None;
    let mut clear_all = false;
    let mut jump = None;
    let mut filter_change = None;
    let mut collapse_requested = false;

    TopBottomPanel::bottom("problems")
        .resizable(true)
        .default_height(130.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(Icons::WARNING).color(pal.accent));
                ui.label(RichText::new(shell.localization.msg("panel-problems")).strong());
                let (errors, warnings, infos) = shell.alerts.counts();
                let filter = shell.alerts.filter;
                for (shown, count, toggle_key) in [
                    (filter.errors, errors, "problems-filter-errors"),
                    (filter.warnings, warnings, "problems-filter-warnings"),
                    (filter.infos, infos, "problems-filter-infos"),
                ] {
                    let label = format!("{} {}", shell.localization.msg(toggle_key), count);
                    let mut text = RichText::new(label);
                    if !shown {
                        text = text.color(pal.text_muted);
                    }
                    if ui.selectable_label(shown, text).clicked() {
                        filter_change = Some(toggle_key);
                    }
                }
                if !shell.alerts.is_empty()
                    && ui
                        .small_button(shell.localization.msg("problems-clear"))
                        .clicked()
                {
                    clear_all = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let collapse = ui.small_button(Icons::CARET_DOWN);
                    if collapse.clicked() {
                        collapse_requested = true;
                    }
                });
            });

            if shell.alerts.is_empty() {
                ui.label(
                    RichText::new(shell.localization.msg("problems-empty")).color(pal.text_muted),
                );
                return;
            }
            let rows: Vec<crate::ui::alerts::Alert> = shell.alerts.visible().cloned().collect();
            if rows.is_empty() {
                ui.label(
                    RichText::new(shell.localization.msg("problems-filtered-empty"))
                        .color(pal.text_muted),
                );
                return;
            }
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for alert in &rows {
                        let (color, icon) = match alert.severity {
                            crate::core::Severity::Error => (pal.error, Icons::WARNING),
                            crate::core::Severity::Warning => (pal.warning, Icons::WARNING),
                            crate::core::Severity::Info => (pal.info, Icons::INFO),
                        };
                        let row_id = ui.id().with(("alert-row", alert.sequence));
                        let clickable = alert.position.is_some();
                        let hovered = clickable
                            && ui
                                .ctx()
                                .read_response(row_id)
                                .is_some_and(|response| response.hovered());
                        let mut frame = Frame::new()
                            .corner_radius(CornerRadius::same(4))
                            .inner_margin(Margin::symmetric(6, 2));
                        if hovered {
                            frame = frame.fill(pal.hover_bg);
                        }
                        let frame_response = frame.show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(icon).color(color));
                                let mut line = format!("{}  {}", alert.code, alert.message);
                                if alert.count > 1 {
                                    line.push_str(&format!("  ×{}", alert.count));
                                }
                                if let Some((line_no, column)) = alert.position {
                                    line.push_str(&format!(
                                        "  [{}]",
                                        shell.localization.msg_with(
                                            "problems-at-position",
                                            Some(&crate::fluent_args!(
                                                "line" => line_no as i32,
                                                "column" => column as i32
                                            )),
                                        )
                                    ));
                                }
                                ui.label(RichText::new(line).color(color));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| ui.small_button(Icons::X),
                                )
                                .inner
                            })
                            .inner
                        });
                        if frame_response.inner.clicked() {
                            dismiss = Some(alert.sequence);
                        }
                        if clickable {
                            // Clicks jump to the source; the dismiss button
                            // keeps its own zone on the right.
                            let click_rect = egui::Rect::from_min_max(
                                frame_response.response.rect.min,
                                egui::pos2(
                                    frame_response.inner.rect.min.x - Spacing::XS,
                                    frame_response.response.rect.max.y,
                                ),
                            );
                            let response = ui.interact(click_rect, row_id, Sense::click());
                            if response.clicked() {
                                jump = alert.position;
                            }
                            response.on_hover_text(shell.localization.msg("problems-jump"));
                        }
                    }
                });
        });

    if let Some(key) = filter_change {
        let filter = &mut shell.alerts.filter;
        match key {
            "problems-filter-errors" => filter.errors = !filter.errors,
            "problems-filter-warnings" => filter.warnings = !filter.warnings,
            _ => filter.infos = !filter.infos,
        }
    }
    if let Some(sequence) = dismiss {
        shell.alerts.dismiss(sequence);
    }
    if clear_all {
        shell.alerts.clear();
    }
    if let Some(position) = jump {
        shell.source_jump = Some(position);
        shell.focus = FocusPane::Source;
    }
    if collapse_requested {
        shell.problems_panel_open = false;
    }
}

// ---------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------

pub fn status_bar(ctx: &Context, shell: &mut AppShell) {
    let pal = Palette::resolve(ctx);
    TopBottomPanel::bottom("status-bar")
        .frame(Frame::new().fill(pal.panel_bg))
        .show(ctx, |ui| {
            let elements = if shell.workspace.active().is_some() {
                shell.outline_elements()
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
                    let mut rich = RichText::new(text).small();
                    if shell
                        .workspace
                        .active()
                        .is_some_and(|s| s.mode == DocumentMode::LargeReadOnly)
                        && index == status_texts.len() - 1
                    {
                        rich = rich.color(pal.warning);
                    } else {
                        rich = rich.color(pal.text_secondary);
                    }
                    ui.label(rich);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (errors, warnings, infos) = shell.alerts.counts();
                    if errors + warnings + infos > 0 {
                        let (color, icon) = if errors > 0 {
                            (pal.error, Icons::WARNING)
                        } else if warnings > 0 {
                            (pal.warning, Icons::WARNING)
                        } else {
                            (pal.info, Icons::INFO)
                        };
                        let chip = format!("{icon} {errors}/{warnings}/{infos}");
                        if ui
                            .selectable_label(
                                shell.problems_panel_open,
                                RichText::new(chip).color(color).small(),
                            )
                            .on_hover_text(shell.localization.msg("panel-problems"))
                            .clicked()
                        {
                            shell.problems_panel_open = !shell.problems_panel_open;
                        }
                    }
                });
            });
        });
}

#[allow(dead_code)]
fn unused_status_body(_ui: &mut Ui, _shell: &mut AppShell) {}
