//! Modal dialogs for the shell.
//!
//! A dialog is taken out of the shell for the frame, rendered as an
//! anchored window, and put back unless its body consumed it — so modal
//! state persists across frames without borrow gymnastics.

use egui::{Context, RichText, Ui, Window};

use crate::core::Command;
use crate::fluent_args;
use crate::ui::icons::Icons;
use crate::ui::panels::save_all_dirty;
use crate::ui::shell::{AppShell, Dialog};
use crate::ui::theme::Theme;

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
        Dialog::ConfirmDelete { .. } => "dialog-delete-title",
        Dialog::UnsavedExit => "dialog-unsaved-title",
        Dialog::ReloadBanner { .. } => "dialog-reload-title",
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
                        .color(Theme::ERROR),
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
                    save_all_dirty(shell);
                    *keep = false;
                }
                if ui
                    .button(shell.localization.msg("dialog-unsaved-discard-selected"))
                    .clicked()
                {
                    for index in 0..shell.workspace.sessions().len() {
                        shell.workspace.select(index);
                        if let Some(session) = shell.workspace.active_mut() {
                            session.history.clear();
                        }
                    }
                    *keep = false;
                }
                if ui
                    .button(shell.localization.msg("dialog-unsaved-cancel"))
                    .clicked()
                {
                    *keep = false;
                }
            });
        }
        Dialog::ReloadBanner { path, dirty } => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let key = if *dirty {
                "dialog-reload-dirty-body"
            } else {
                "dialog-reload-clean-body"
            };
            ui.label(
                shell
                    .localization
                    .msg_with(key, Some(&fluent_args!("name" => name.as_str()))),
            );
            ui.horizontal(|ui| {
                if ui
                    .button(shell.localization.msg("dialog-reload-reload"))
                    .clicked()
                {
                    let path = path.clone();
                    shell.open_path(path);
                    *keep = false;
                }
                if *dirty {
                    if ui
                        .button(shell.localization.msg("dialog-reload-keep"))
                        .clicked()
                    {
                        *keep = false;
                    }
                } else if ui
                    .button(shell.localization.msg("action-exit-cancel"))
                    .clicked()
                {
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
                    for (_, snapshot) in snapshots.clone() {
                        if let Ok(document) =
                            crate::core::document::XmlDocument::parse(snapshot.source.as_bytes())
                        {
                            shell.workspace.add_opened(
                                snapshot.path.clone().unwrap_or_default(),
                                crate::services::workspace::FileType::Xml,
                                crate::services::workspace::DocumentMode::Editable,
                                document,
                            );
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
            ] {
                ui.label(shell.localization.msg(key));
            }
            if ui.button("OK").clicked() {
                *keep = false;
            }
        }
    }
}
