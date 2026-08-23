//! Outline panel: virtualized XML tree with aligned hierarchy guides.

use egui::{
    Align2, Color32, Context, CornerRadius, FontId, RichText, ScrollArea, Sense, SidePanel, Stroke,
    Ui, Window, vec2,
};

use crate::core::Command;
use crate::core::document::{NodeId, XmlDocument, XmlNodeKind};
use crate::services::outline::{Row, node_path};
use crate::services::workspace::DocumentMode;
use crate::ui::icons::Icons;
use crate::ui::inspector::count_descendants;
use crate::ui::panels::{panel_heading, search_box, toolbar_button};
use crate::ui::shell::{AppShell, Dialog, FocusPane};
use crate::ui::theme::{Palette, Spacing, Typography};

pub const OUTLINE_MIN_WIDTH: f32 = 260.0;
pub const OUTLINE_MAX_WIDTH: f32 = 420.0;

/// Fixed layout metrics for tree rows (caret column + indent guides).
#[derive(Clone, Copy)]
struct OutlineLayout {
    row_height: f32,
    indent: f32,
    caret_width: f32,
}

impl OutlineLayout {
    fn resolve() -> Self {
        Self {
            row_height: Spacing::MD + 8.0,
            indent: 12.0,
            caret_width: 20.0,
        }
    }

    fn label_x(&self, depth: usize) -> f32 {
        depth as f32 * self.indent + self.caret_width
    }

    fn caret_center_x(&self, depth: usize) -> f32 {
        depth as f32 * self.indent + self.caret_width * 0.5
    }
}

#[derive(Clone, Copy)]
enum LabelKind {
    ElementName,
    AttributeKey,
    AttributeValue,
    Text,
    Comment,
    Pi,
}

struct LabelSegment {
    text: String,
    kind: LabelKind,
}

pub fn outline_panel(ctx: &Context, shell: &mut AppShell) {
    SidePanel::left("outline")
        .resizable(true)
        .min_width(OUTLINE_MIN_WIDTH)
        .max_width(OUTLINE_MAX_WIDTH)
        .show(ctx, |ui| {
            outline_contents(ui, shell, true);
        });
}

pub fn outline_drawer(ctx: &Context, shell: &mut AppShell) {
    let screen = ctx.content_rect();
    let backdrop_id = egui::Id::new("outline-drawer-backdrop");
    egui::Area::new(backdrop_id)
        .fixed_pos(screen.min)
        .interactable(true)
        .show(ctx, |ui| {
            let (_, response) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(
                screen,
                CornerRadius::ZERO,
                Color32::from_rgba_premultiplied(0, 0, 0, 60),
            );
            if response.clicked() {
                shell.show_outline_drawer = false;
            }
        });

    let height = screen.height();
    Window::new("outline-drawer")
        .title_bar(false)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::LEFT_TOP, [0.0, 0.0])
        .fixed_size([OUTLINE_MIN_WIDTH, height])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(shell.localization.msg("panel-outline"))
                        .strong()
                        .color(Palette::resolve(ui.ctx()).accent),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(Icons::X).clicked() {
                        shell.show_outline_drawer = false;
                    }
                });
            });
            ui.separator();
            outline_contents(ui, shell, false);
        });
}

pub fn outline_contents(ui: &mut Ui, shell: &mut AppShell, show_heading: bool) {
    let layout = OutlineLayout::resolve();
    if show_heading {
        panel_heading(
            ui,
            Icons::TREE_STRUCTURE,
            shell.localization.msg("panel-outline"),
        );
    }
    outline_toolbar(ui, shell);
    outline_search_section(ui, shell);

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
    let (tree, _) = shell.outline_snapshot();
    let rows = &tree.rows;
    if rows.is_empty() {
        ui.label(RichText::new(shell.localization.msg("outline-empty")).color(pal.text_muted));
        return;
    }

    let mut toggle = None;
    let mut select = None;
    let search_node = shell.search.selected_node();
    let selection = shell.workspace.active().and_then(|s| s.selection);
    let body_font = egui::TextStyle::Body.resolve(ui.style());
    let icon_font = FontId::proportional(Typography::SMALL);

    let mut area = ScrollArea::vertical().auto_shrink([false, false]);
    if let Some(target) = shell.outline_scroll_to.take()
        && let Some(index) = rows.iter().position(|row| row.node == target)
    {
        area = area
            .vertical_scroll_offset((index as f32 * layout.row_height - 60.0).max(0.0));
    }

    area.show_rows(ui, layout.row_height, rows.len(), |ui, range| {
        for row in &rows[range] {
            let (rect, response) = ui.allocate_exact_size(
                vec2(ui.available_width().max(1.0), layout.row_height),
                Sense::click(),
            );
            let is_selected = selection == Some(row.node) || search_node == Some(row.node);
            let is_search_hit = search_node == Some(row.node);
            let painter = ui.painter_at(rect);

            if is_selected {
                painter.rect_filled(rect, CornerRadius::same(4), pal.selection);
                painter.rect_filled(
                    egui::Rect::from_min_size(rect.min, vec2(2.0, rect.height())),
                    CornerRadius::ZERO,
                    pal.accent,
                );
            } else if is_search_hit {
                painter.rect_filled(rect, CornerRadius::same(4), pal.info_bg);
            } else if response.hovered() {
                painter.rect_filled(rect, CornerRadius::same(4), pal.hover_bg);
            }

            draw_depth_guides(&painter, rect, row.depth, layout, pal.border);
            draw_caret(
                &painter,
                rect,
                row,
                layout,
                &icon_font,
                pal,
                response.hovered() || is_selected,
            );

            let document = shell
                .workspace
                .active()
                .map(|s| &s.document)
                .expect("session exists");
            let segments = row_label_segments(document, row.node);
            draw_label_segments(
                &painter,
                rect,
                layout.label_x(row.depth),
                &segments,
                pal,
                is_selected,
                &body_font,
            );

            if response.double_clicked() {
                toggle = Some(row.node);
            } else if response.clicked() {
                let caret_end = rect.min.x + layout.label_x(row.depth);
                let on_caret = row.expandable
                    && response
                        .interact_pointer_pos()
                        .is_some_and(|pos| pos.x < caret_end);
                if on_caret {
                    toggle = Some(row.node);
                } else {
                    select = Some(row.node);
                }
            }
            response.context_menu(|ui| {
                outline_context_menu(ui, shell, row.node);
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

fn outline_toolbar(ui: &mut Ui, shell: &mut AppShell) {
    ui.horizontal(|ui| {
        if toolbar_button(
            ui,
            Icons::CARET_DOWN,
            shell.localization.msg("outline-expand-all"),
        )
        .clicked()
            && let Some(session) = shell.workspace.active_id()
        {
            shell.expand_all(session);
        }
        if toolbar_button(
            ui,
            Icons::CARET_RIGHT,
            shell.localization.msg("outline-collapse-all"),
        )
        .clicked()
            && let Some(session) = shell.workspace.active_id()
        {
            shell.collapse_all(session);
        }
    });
}

fn outline_search_section(ui: &mut Ui, shell: &mut AppShell) {
    let pal = Palette::resolve(ui.ctx());
    ui.horizontal(|ui| {
        let collapsed = shell.outline_search_collapsed;
        let toggle_icon = if collapsed {
            Icons::CARET_RIGHT
        } else {
            Icons::CARET_DOWN
        };
        if ui
            .small_button(toggle_icon)
            .on_hover_text(shell.localization.msg("panel-search"))
            .clicked()
        {
            shell.outline_search_collapsed = !collapsed;
        }
        ui.label(
            RichText::new(shell.localization.msg("panel-search"))
                .small()
                .color(pal.text_muted),
        );
    });
    if !shell.outline_search_collapsed {
        search_box(ui, shell);
    }
}

fn draw_depth_guides(
    painter: &egui::Painter,
    rect: egui::Rect,
    depth: usize,
    layout: OutlineLayout,
    border: Color32,
) {
    if depth == 0 {
        return;
    }
    let guide_color = Color32::from_rgba_premultiplied(
        border.r(),
        border.g(),
        border.b(),
        border.a().min(80),
    );
    for level in 0..depth {
        let x = rect.min.x + level as f32 * layout.indent + layout.indent * 0.5;
        painter.line_segment(
            [egui::pos2(x, rect.min.y + 2.0), egui::pos2(x, rect.max.y - 2.0)],
            Stroke::new(1.0, guide_color),
        );
    }
}

fn draw_caret(
    painter: &egui::Painter,
    rect: egui::Rect,
    row: &Row,
    layout: OutlineLayout,
    font: &FontId,
    pal: Palette,
    active: bool,
) {
    let center = egui::pos2(layout.caret_center_x(row.depth) + rect.min.x, rect.center().y);
    if row.expandable {
        let icon = if row.expanded {
            Icons::CARET_DOWN
        } else {
            Icons::CARET_RIGHT
        };
        let color = if active {
            pal.text_highlight
        } else {
            pal.text_secondary
        };
        painter.text(center, Align2::CENTER_CENTER, icon, font.clone(), color);
    } else {
        painter.circle_filled(center, 1.5, pal.text_muted);
    }
}

fn draw_label_segments(
    painter: &egui::Painter,
    rect: egui::Rect,
    start_x: f32,
    segments: &[LabelSegment],
    pal: Palette,
    selected: bool,
    font: &FontId,
) {
    let mut x = rect.min.x + start_x;
    let y = rect.center().y;
    for segment in segments {
        let color = if selected {
            pal.text_highlight
        } else {
            match segment.kind {
                LabelKind::ElementName => pal.element_name,
                LabelKind::AttributeKey => pal.attribute_key,
                LabelKind::AttributeValue => pal.attribute_value,
                LabelKind::Text => pal.text_secondary,
                LabelKind::Comment => pal.comment,
                LabelKind::Pi => pal.syntax_keyword,
            }
        };
        let galley = painter.layout_no_wrap(segment.text.clone(), font.clone(), color);
        painter.galley(egui::pos2(x, y - galley.size().y * 0.5), galley, color);
        x += painter
            .layout_no_wrap(segment.text.clone(), font.clone(), color)
            .size()
            .x;
    }
}

fn row_label_segments(document: &XmlDocument, node: NodeId) -> Vec<LabelSegment> {
    match document.kind(node) {
        Some(XmlNodeKind::Element) => {
            let mut segments = Vec::new();
            segments.push(LabelSegment {
                text: document.qname(node).map(|q| q.render()).unwrap_or_default(),
                kind: LabelKind::ElementName,
            });
            for (name, value) in document.attributes(node) {
                let shown: String = value.chars().take(24).collect();
                segments.push(LabelSegment {
                    text: format!(" {name}=\""),
                    kind: LabelKind::AttributeKey,
                });
                segments.push(LabelSegment {
                    text: shown,
                    kind: LabelKind::AttributeValue,
                });
                segments.push(LabelSegment {
                    text: "\"".to_string(),
                    kind: LabelKind::AttributeKey,
                });
            }
            segments
        }
        Some(XmlNodeKind::Text) | Some(XmlNodeKind::CData) => {
            let text: String = document
                .node_text(node)
                .unwrap_or_default()
                .trim()
                .chars()
                .take(40)
                .collect();
            vec![LabelSegment {
                text,
                kind: LabelKind::Text,
            }]
        }
        Some(XmlNodeKind::Comment) => {
            let shown: String = document
                .comment_text(node)
                .unwrap_or_default()
                .chars()
                .take(40)
                .collect();
            vec![LabelSegment {
                text: format!("<!-- {shown} -->"),
                kind: LabelKind::Comment,
            }]
        }
        Some(XmlNodeKind::ProcessingInstruction) => {
            let text = match document.pi(node) {
                Some((target, data)) => match data {
                    Some(data) => format!("<?{target} {data}?>"),
                    None => format!("<?{target}?>"),
                },
                None => String::new(),
            };
            vec![LabelSegment {
                text,
                kind: LabelKind::Pi,
            }]
        }
        _ => Vec::new(),
    }
}

fn outline_context_menu(ui: &mut Ui, shell: &mut AppShell, node: NodeId) {
    let edits = shell.tree_edits_enabled();
    let delete_meta = shell.workspace.active().map(|session| {
        let document = &session.document;
        (
            document
                .qname(node)
                .map(|q| q.render())
                .unwrap_or_default(),
            count_descendants(document, node),
        )
    });
    let xpath_path = shell
        .workspace
        .active()
        .and_then(|session| node_path(&session.document, node));
    let xml_snippet = shell.workspace.active().and_then(|session| {
        session
            .document
            .source_range(node)
            .and_then(|range| {
                session
                    .document
                    .source()
                    .get(range.start_byte..range.end_byte)
                    .map(|s| s.to_string())
            })
    });

    if ui
        .button(shell.localization.msg("outline-copy-xpath"))
        .clicked()
        && let Some(path) = xpath_path
    {
        ui.ctx().copy_text(path);
        ui.close_kind(egui::UiKind::Menu);
    }
    if ui
        .button(shell.localization.msg("outline-copy-xml"))
        .clicked()
        && let Some(snippet) = xml_snippet
    {
        ui.ctx().copy_text(snippet);
        ui.close_kind(egui::UiKind::Menu);
    }
    ui.separator();
    if ui
        .add_enabled(edits, egui::Button::new(shell.localization.msg("outline-duplicate")))
        .clicked()
    {
        shell.commit(Command::DuplicateSubtree { node });
        ui.close_kind(egui::UiKind::Menu);
    }
    if ui
        .add_enabled(edits, egui::Button::new(shell.localization.msg("outline-delete")))
        .clicked()
        && let Some((name, descendants)) = delete_meta
    {
        shell.dialog = Some(Dialog::ConfirmDelete {
            node,
            name,
            descendants,
        });
        ui.close_kind(egui::UiKind::Menu);
    }
}

/// Tree keyboard navigation when the outline pane is focused.
pub fn handle_keyboard(ctx: &Context, shell: &mut AppShell) {
    let Some(session_id) = shell.workspace.active_id() else {
        return;
    };
    let input = ctx.input(|i| i.clone());
    let (tree, _) = shell.outline_snapshot();
    let rows = &tree.rows;
    if rows.is_empty() {
        return;
    }

    let current_node = shell.workspace.active().and_then(|s| s.selection);
    let current_index = current_node
        .and_then(|node| rows.iter().position(|row| row.node == node))
        .unwrap_or(0);

    let mut new_index = None;
    let mut toggle = None;
    let mut expand = false;
    let mut collapse = false;

    if input.key_pressed(egui::Key::ArrowDown) {
        new_index = Some((current_index + 1).min(rows.len() - 1));
    } else if input.key_pressed(egui::Key::ArrowUp) {
        new_index = Some(current_index.saturating_sub(1));
    } else if input.key_pressed(egui::Key::Home) {
        new_index = Some(0);
    } else if input.key_pressed(egui::Key::End) {
        new_index = Some(rows.len() - 1);
    } else if input.key_pressed(egui::Key::Enter)
        || input.key_pressed(egui::Key::ArrowRight)
    {
        let row = &rows[current_index];
        if row.expandable && !row.expanded {
            expand = true;
            toggle = Some(row.node);
        }
    } else if input.key_pressed(egui::Key::ArrowLeft) {
        let row = &rows[current_index];
        if row.expandable && row.expanded {
            collapse = true;
            toggle = Some(row.node);
        } else if let Some(session) = shell.workspace.active()
            && let Some(parent) = session.document.parent(row.node)
            && parent != NodeId::DOCUMENT
        {
            new_index = rows.iter().position(|r| r.node == parent);
        }
    } else if input.key_pressed(egui::Key::Space) {
        let row = &rows[current_index];
        if row.expandable {
            toggle = Some(row.node);
        }
    }

    if let Some(node) = toggle {
        shell.toggle_expanded(session_id, node);
        if expand || collapse {
            let (tree, _) = shell.outline_snapshot();
            if let Some(idx) = tree.rows.iter().position(|r| r.node == node) {
                shell.outline_scroll_to = Some(tree.rows[idx].node);
            }
        }
    }

    if let Some(index) = new_index {
        let node = rows[index].node;
        if let Some(session) = shell.workspace.active_mut() {
            session.selection = Some(node);
        }
        shell.outline_scroll_to = Some(node);
    }
}
