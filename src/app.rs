//! eframe application hosting the shell.

use crate::ui::shell::{AppShell, Dialog};

pub struct XmlToolApp {
    shell: AppShell,
}

impl XmlToolApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut shell = AppShell::new();
        shell.theme_mode.apply(&cc.egui_ctx);
        // Crash-recovery snapshots from a previous session surface as a
        // modal choice — never an automatic overwrite of disk files.
        let snapshots = shell.recovery.load_all();
        if !snapshots.is_empty() {
            shell.dialog = Some(Dialog::Recovery { snapshots });
        }
        Self { shell }
    }
}

impl eframe::App for XmlToolApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.shell.update(ctx);
    }
}
