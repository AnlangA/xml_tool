//! The inspector panel: property editing for the selected node.

use egui::{Context, RichText, SidePanel, TextEdit, Ui};

use crate::core::document::{NodeId, XmlNodeKind};
use crate::core::{Command, NewNode, NodeContent};
use crate::core::{InsertPosition, QNameSpec};
use crate::services::workspace::DocumentMode;
use crate::ui::icons::Icons;
use crate::ui::panels::{INSPECTOR_WIDTH, panel_heading, section_label};
use crate::ui::shell::{AppShell, Dialog};
use crate::ui::theme::Palette;

// ---------------------------------------------------------------------------
// Right: inspector
// ---------------------------------------------------------------------------

pub fn inspector_panel(ctx: &Context, shell: &mut AppShell) {
    SidePanel::right("inspector")
        .resizable(false)
        .exact_width(INSPECTOR_WIDTH)
        .show(ctx, |ui| {
            inspector_contents(ui, shell);
        });
}

pub fn inspector_contents(ui: &mut Ui, shell: &mut AppShell) {
    panel_heading(
        ui,
        Icons::SLIDERS_HORIZONTAL,
        shell.localization.msg("panel-inspector"),
    );
    let pal = Palette::resolve(ui.ctx());
    let Some(session) = shell.workspace.active() else {
        ui.label(RichText::new(shell.localization.msg("panel-empty")).color(pal.text_muted));
        return;
    };
    if session.mode == DocumentMode::LargeReadOnly {
        ui.label(shell.localization.msg("inspector-read-only"));
    }
    let Some(node) = shell.workspace.active().and_then(|s| s.selection) else {
        ui.label(
            RichText::new(shell.localization.msg("inspector-no-selection")).color(pal.text_muted),
        );
        return;
    };
    inspector_body(ui, shell, node);
}

fn inspector_body(ui: &mut Ui, shell: &mut AppShell, node: NodeId) {
    let pal = Palette::resolve(ui.ctx());
    let editable = shell
        .workspace
        .active()
        .is_some_and(|session| session.mode == DocumentMode::Editable);

    // Pending edits are collected while borrowing the session immutably and
    // committed after, so the command layer runs outside the egui closure.
    let mut rename_to: Option<String> = None;
    let mut set_attr: Option<(String, String)> = None;
    let mut set_text: Option<String> = None;
    let mut add_node: Option<NewNode> = None;
    let mut request_delete = false;

    ui.add_enabled_ui(editable, |ui| {
        let Some(session) = shell.workspace.active() else {
            return;
        };
        let document = &session.document;
        match document.kind(node) {
            Some(XmlNodeKind::Element) => {
                section_label(ui, shell.localization.msg("inspector-qname"));
                let current = document.qname(node).map(|q| q.render()).unwrap_or_default();
                // Commits on Enter / focus loss, not per keystroke.
                if let Some(value) = inspector_text_edit(ui, node, "rename", &current, false) {
                    rename_to = Some(value);
                }

                if let Some(uri) = document
                    .qname(node)
                    .and_then(|q| q.namespace_uri().map(str::to_string))
                {
                    section_label(ui, shell.localization.msg("inspector-namespace"));
                    ui.monospace(&uri);
                }

                ui.separator();
                section_label(ui, shell.localization.msg("inspector-attributes"));
                for (name, attr_value) in document.attributes(node) {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&name).monospace().color(pal.attribute_key));
                        let field = format!("attr:{name}");
                        if let Some(value) =
                            inspector_text_edit(ui, node, &field, &attr_value, false)
                        {
                            set_attr = Some((name.clone(), value));
                        }
                        if ui.small_button("×").clicked() {
                            shell.pending_remove_attr = Some(name);
                        }
                    });
                }
                if ui
                    .small_button(shell.localization.msg("inspector-add-attribute"))
                    .clicked()
                {
                    shell.pending_new_attr = true;
                }

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.small_button("+ <e/>").clicked() {
                        add_node = Some(NewNode::Element {
                            name: "element".into(),
                        });
                    }
                    if ui.small_button("+ text").clicked() {
                        add_node = Some(NewNode::Text {
                            text: "text".into(),
                        });
                    }
                    if ui.small_button("+ CDATA").clicked() {
                        add_node = Some(NewNode::CData { text: "raw".into() });
                    }
                    if ui.small_button("+ <!--").clicked() {
                        add_node = Some(NewNode::Comment {
                            text: "note".into(),
                        });
                    }
                    if ui.small_button("+ <?pi").clicked() {
                        add_node = Some(NewNode::ProcessingInstruction {
                            target: "target".into(),
                            data: Some("data".into()),
                        });
                    }
                });
                if ui
                    .button(
                        RichText::new(format!(
                            "{} {}",
                            Icons::TRASH,
                            shell.localization.msg("outline-delete")
                        ))
                        .color(pal.error),
                    )
                    .clicked()
                {
                    request_delete = true;
                }
            }
            Some(XmlNodeKind::Text) | Some(XmlNodeKind::CData) => {
                section_label(
                    ui,
                    if document.kind(node) == Some(XmlNodeKind::Text) {
                        shell.localization.msg("inspector-text")
                    } else {
                        shell.localization.msg("inspector-cdata")
                    },
                );
                let current = document.node_text(node).unwrap_or_default().to_string();
                if let Some(value) = inspector_text_edit(ui, node, "content", &current, true) {
                    set_text = Some(value);
                }
            }
            Some(XmlNodeKind::Comment) => {
                section_label(ui, shell.localization.msg("inspector-comment"));
                let current = document.comment_text(node).unwrap_or_default().to_string();
                if let Some(value) = inspector_text_edit(ui, node, "comment", &current, true) {
                    set_text = Some(value);
                }
            }
            Some(XmlNodeKind::ProcessingInstruction) => {
                if let Some((target, data)) = document.pi(node) {
                    section_label(ui, shell.localization.msg("inspector-pi-target"));
                    ui.monospace(target);
                    section_label(ui, shell.localization.msg("inspector-pi-data"));
                    ui.monospace(data.unwrap_or(""));
                }
            }
            _ => {}
        }
    });

    if let Some(name) = rename_to {
        shell.commit(Command::RenameElement {
            node,
            new_name: QNameSpec {
                name,
                namespace_uri: None,
            },
        });
    }
    if let Some((name, value)) = set_attr {
        shell.commit(Command::SetAttributeValue {
            element: node,
            name,
            value,
        });
    }
    if let Some(text) = set_text {
        let cdata = shell.workspace.active().and_then(|s| s.document.kind(node))
            == Some(XmlNodeKind::CData);
        shell.commit(Command::SetNodeContent {
            node,
            content: if cdata {
                NodeContent::CData(text)
            } else {
                NodeContent::Text(text)
            },
        });
    }
    if let Some(new_node) = add_node {
        shell.commit(Command::InsertNode {
            parent: node,
            position: InsertPosition::Last,
            node: new_node,
        });
    }
    if shell.pending_new_attr {
        shell.pending_new_attr = false;
        let name = unique_attribute_name(shell, node);
        shell.commit(Command::AddAttribute {
            element: node,
            name,
            value: String::new(),
        });
    }
    if let Some(name) = shell.pending_remove_attr.take() {
        shell.commit(Command::RemoveAttribute {
            element: node,
            name,
        });
    }
    if request_delete && let Some(session) = shell.workspace.active() {
        let name = session
            .document
            .qname(node)
            .map(|q| q.render())
            .unwrap_or_default();
        let descendants = count_descendants(&session.document, node);
        shell.dialog = Some(Dialog::ConfirmDelete {
            node,
            name,
            descendants,
        });
    }
}

/// Counts the node's subtree descendants (shown in the delete dialog).
pub(crate) fn count_descendants(
    document: &crate::core::document::XmlDocument,
    node: NodeId,
) -> usize {
    document
        .children(node)
        .into_iter()
        .map(|child| 1 + count_descendants(document, child))
        .sum()
}

/// Inspector text field with a persistent buffer: the in-progress text
/// lives in egui temp memory keyed by (node, field), so typing survives
/// re-renders, and the value commits only on Enter / focus loss. Returns
/// the committed value, if any.
fn inspector_text_edit(
    ui: &mut Ui,
    node: NodeId,
    field: &str,
    current: &str,
    multiline: bool,
) -> Option<String> {
    let id = ui.id().with(("inspector-edit", field, node.0));
    let mut value = ui
        .data_mut(|data| data.get_temp::<String>(id))
        .unwrap_or_else(|| current.to_owned());
    let response = if multiline {
        ui.add(
            TextEdit::multiline(&mut value)
                .desired_rows(4)
                .desired_width(f32::INFINITY),
        )
    } else {
        ui.add(TextEdit::singleline(&mut value).desired_width(f32::INFINITY))
    };
    if response.lost_focus() && value != current {
        ui.data_mut(|data| data.remove::<String>(id));
        return Some(value);
    }
    ui.data_mut(|data| data.insert_temp(id, value));
    None
}

fn unique_attribute_name(shell: &AppShell, element: NodeId) -> String {
    let existing = shell
        .workspace
        .active()
        .map(|session| session.document.attributes(element))
        .unwrap_or_default();
    let mut index = 1;
    loop {
        let candidate = format!("attr{index}");
        if !existing.iter().any(|(name, _)| *name == candidate) {
            return candidate;
        }
        index += 1;
    }
}
