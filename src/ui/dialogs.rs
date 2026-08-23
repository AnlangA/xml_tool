//! Modal dialogs for the shell.
//!
//! A dialog is taken out of the shell for the frame, rendered as an
//! anchored window, and put back unless its body consumed it — so modal
//! state persists across frames without borrow gymnastics.

use egui::{Context, RichText, Ui, Window};

use crate::core::Command;
use crate::fluent_args;
use crate::ui::icons::Icons;
use crate::ui::shell::{AppShell, Dialog};
use crate::ui::theme::Palette;

// ---------------------------------------------------------------------------
// Dialogs (modal windows)
// ---------------------------------------------------------------------------

pub fn dialogs(ctx: &Context, shell: &mut AppShell) {
    let Some(mut dialog) = shell.dialog.take() else {
        return;
    };
    let title = title_key(&dialog);
    let mut keep = true;

    Window::new(shell.localization.msg(title))
        .collapsible(false)
        .resizable(false)
        .default_width(340.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            dialog_body(ui, shell, &mut dialog, &mut keep);
        });

    if keep && shell.dialog.is_none() {
        shell.dialog = Some(dialog);
    }
}

fn title_key(dialog: &Dialog) -> &'static str {
    match dialog {
        Dialog::About => "dialog-about-title",
        Dialog::XPathQuery { .. } => "action-xpath",
        Dialog::ExiWorkbench { .. } => "exi-dialog-title",
        Dialog::ConfirmDelete { .. } => "dialog-delete-title",
        Dialog::UnsavedExit => "dialog-unsaved-title",
        Dialog::Recovery { .. } => "dialog-recovery-title",
        Dialog::Shortcuts => "shortcut-help",
    }
}

fn dialog_body(ui: &mut Ui, shell: &mut AppShell, dialog: &mut Dialog, keep: &mut bool) {
    match dialog {
        Dialog::About => {
            let args = fluent_args!(
                "name" => shell.localization.msg("app-name"),
                "version" => env!("CARGO_PKG_VERSION"),
            );
            ui.label(
                shell
                    .localization
                    .msg_with("dialog-about-version", Some(&args)),
            );
            let backends = fluent_args!("xml" => "uppsala 0.9.0", "exi" => "erxi");
            ui.label(
                shell
                    .localization
                    .msg_with("dialog-about-backends", Some(&backends)),
            );
            let license = fluent_args!("license" => "MIT");
            ui.label(
                shell
                    .localization
                    .msg_with("dialog-about-license", Some(&license)),
            );
            if ui.button("OK").clicked() {
                *keep = false;
            }
        }
        Dialog::ExiWorkbench { preset, report } => {
            use crate::services::exi_workbench::ExiPreset;
            for (candidate, key) in [
                (ExiPreset::FidelityBitPacked, "exi-preset-fidelity"),
                (ExiPreset::ByteAligned, "exi-preset-byte"),
                (ExiPreset::PreCompression, "exi-preset-precompression"),
                (ExiPreset::MaximumCompression, "exi-preset-max"),
            ] {
                if ui
                    .radio(*preset == candidate, shell.localization.msg(key))
                    .clicked()
                {
                    *preset = candidate;
                }
            }
            if ui.button(shell.localization.msg("exi-encode")).clicked() {
                let preset = *preset;
                shell.exi_encode_current(preset);
                return;
            }
            if let Some(report) = report {
                ui.monospace(report);
            }
            if ui.button("OK").clicked() {
                *keep = false;
            }
        }
        Dialog::XPathQuery {
            expression,
            results,
        } => {
            let mut buffer = expression.clone();
            let field_id = ui.id().with("xpath-field");
            let response = ui.add(
                egui::TextEdit::singleline(&mut buffer)
                    .id(field_id)
                    .hint_text("//element[@attr='value']")
                    .desired_width(f32::INFINITY),
            );
            if ui.ctx().memory(|memory| memory.focused().is_none()) {
                ui.ctx().memory_mut(|memory| memory.request_focus(field_id));
            }
            ui.add_space(4.0);
            let mut run =
                response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                if ui.button(shell.localization.msg("dialog-run")).clicked() {
                    run = true;
                }
                if ui.button(shell.localization.msg("dialog-cancel")).clicked() {
                    *keep = false;
                }
            });
            if run {
                shell.execute_xpath(&buffer);
                *expression = buffer;
                return;
            }
            if let Some(nodes) = results {
                ui.separator();
                ui.label(shell.localization.msg_with(
                    "xpath-result-nodes",
                    Some(&fluent_args!("count" => nodes.len() as i32)),
                ));
                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for hit in nodes {
                            if ui.selectable_label(false, &hit.label).clicked() {
                                shell.reveal_outline_node(hit.node);
                                *keep = false;
                            }
                        }
                    });
            }
            *expression = buffer;
        }
        Dialog::ConfirmDelete {
            node,
            name,
            descendants,
        } => {
            let body = fluent_args!(
                "name" => name.as_str(),
                "descendants" => *descendants as i32
            );
            ui.label(
                shell
                    .localization
                    .msg_with("dialog-delete-body", Some(&body)),
            );
            ui.horizontal(|ui| {
                if ui
                    .button(
                        RichText::new(format!(
                            "{} {}",
                            Icons::TRASH,
                            shell.localization.msg("dialog-confirm")
                        ))
                        .color(Palette::resolve(ui.ctx()).error),
                    )
                    .clicked()
                {
                    let node = *node;
                    shell.commit(Command::DeleteNode { node });
                    if let Some(session) = shell.workspace.active_mut() {
                        session.selection = None;
                    }
                    *keep = false;
                }
                if ui.button(shell.localization.msg("dialog-cancel")).clicked() {
                    *keep = false;
                }
            });
        }
        Dialog::UnsavedExit => {
            let count = shell.workspace.dirty_sessions().len();
            ui.label(shell.localization.msg_with(
                "dialog-unsaved-body",
                Some(&fluent_args!("count" => count as i32)),
            ));
            ui.horizontal(|ui| {
                if ui
                    .button(shell.localization.msg("dialog-unsaved-save-selected"))
                    .clicked()
                {
                    // Synchronous: background jobs would not finish before
                    // the window closes.
                    let failures = shell.save_dirty_sessions_sync();
                    if failures == 0 {
                        match shell.after_unsaved.take() {
                            Some(crate::ui::shell::AfterUnsaved::Exit) => {
                                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                            _ => shell.close_active_tab(),
                        }
                    } else {
                        // Saving failed: stay open, drop the pending action.
                        shell.after_unsaved = None;
                    }
                    *keep = false;
                }
                if ui
                    .button(shell.localization.msg("dialog-unsaved-discard-selected"))
                    .clicked()
                {
                    match shell.after_unsaved.take() {
                        Some(crate::ui::shell::AfterUnsaved::CloseTab) => {
                            // Discard only the tab being closed (including
                            // any un-applied source draft).
                            if let Some(session) = shell.workspace.active_mut() {
                                session.history.clear();
                                session.source_draft = None;
                            }
                            shell.close_active_tab();
                        }
                        _ => {
                            for index in 0..shell.workspace.sessions().len() {
                                shell.workspace.select(index);
                                if let Some(session) = shell.workspace.active_mut() {
                                    session.history.clear();
                                    session.source_draft = None;
                                }
                            }
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                    *keep = false;
                }
                if ui
                    .button(shell.localization.msg("dialog-unsaved-cancel"))
                    .clicked()
                {
                    shell.after_unsaved = None;
                    *keep = false;
                }
            });
        }
        Dialog::Recovery { snapshots } => {
            ui.label(shell.localization.msg_with(
                "dialog-recovery-body",
                Some(&fluent_args!("count" => snapshots.len() as i32)),
            ));
            ui.horizontal(|ui| {
                if ui
                    .button(shell.localization.msg("dialog-recovery-open"))
                    .clicked()
                {
                    for (_session_id, snapshot) in snapshots.clone() {
                        if let Ok(document) =
                            crate::core::document::XmlDocument::parse(snapshot.source.as_bytes())
                        {
                            // Empty paths (untitled snapshots) stay untitled;
                            // the restored session is dirty by construction.
                            let path = snapshot
                                .path
                                .clone()
                                .filter(|path| !path.as_os_str().is_empty());
                            let session_id = shell.workspace.add_restored(path, document);
                            shell.expand_root_default(session_id);
                            if let Some(path) = snapshot.selection_path.as_deref()
                                && let Some(session) = shell.workspace.active()
                                && let Some(node) = crate::services::outline::node_from_path(
                                    &session.document,
                                    path,
                                )
                            {
                                shell.reveal_outline_node(node);
                            }
                            // Restored: the snapshot must not prompt again on
                            // the next launch.
                            shell.recovery.remove(session_id.0);
                        }
                    }
                    *keep = false;
                }
                if ui
                    .button(shell.localization.msg("dialog-recovery-discard"))
                    .clicked()
                {
                    for (session, _) in snapshots.clone() {
                        shell.recovery.remove(session);
                    }
                    *keep = false;
                }
            });
        }
        Dialog::Shortcuts => {
            for key in [
                "shortcut-new",
                "shortcut-open",
                "shortcut-save",
                "shortcut-save-as",
                "shortcut-close",
                "shortcut-undo",
                "shortcut-redo",
                "shortcut-find",
                "shortcut-replace",
                "shortcut-next-match",
                "shortcut-prev-match",
                "shortcut-cycle-focus",
                "shortcut-help-key",
                "shortcut-tree-nav",
            ] {
                ui.label(shell.localization.msg(key));
            }
            if ui.button("OK").clicked() {
                *keep = false;
            }
        }
    }
}
