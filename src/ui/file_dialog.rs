use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

/// Result delivered from the background file-dialog thread.
pub enum FileDialogResult {
    OpenXml(Option<PathBuf>),
    OpenExi(Option<PathBuf>),
    SaveXml(Option<PathBuf>),
    SaveExi(Option<PathBuf>),
    SaveJson(Option<PathBuf>),
}

/// Which file-dialog action to open.
#[derive(Debug, Clone, Copy)]
pub enum FileDialogAction {
    OpenXml,
    OpenExi,
    SaveXml,
    SaveExi,
    SaveJson,
}

/// Manages async file dialogs.
pub struct FileDialogManager {
    rx: Option<Receiver<FileDialogResult>>,
}

impl Default for FileDialogManager {
    fn default() -> Self {
        Self::new()
    }
}

impl FileDialogManager {
    pub fn new() -> Self {
        Self { rx: None }
    }

    /// Check if a dialog is currently pending.
    pub fn is_pending(&self) -> bool {
        self.rx.is_some()
    }

    /// Poll for a completed dialog result.
    pub fn poll(&mut self) -> Option<FileDialogResult> {
        let result = self.rx.as_ref().and_then(|rx| rx.try_recv().ok());
        if result.is_some() {
            self.rx = None;
        }
        result
    }

    /// Open a file dialog of the specified type.
    pub fn open(&mut self, action: FileDialogAction) {
        if self.rx.is_some() {
            return;
        }

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);

        std::thread::spawn(move || {
            let result = match action {
                FileDialogAction::OpenXml => FileDialogResult::OpenXml(
                    rfd::FileDialog::new()
                        .add_filter("XML Files", &["xml"])
                        .add_filter("All Files", &["*"])
                        .pick_file(),
                ),
                FileDialogAction::OpenExi => FileDialogResult::OpenExi(
                    rfd::FileDialog::new()
                        .add_filter("EXI Files", &["exi"])
                        .add_filter("All Files", &["*"])
                        .pick_file(),
                ),
                FileDialogAction::SaveXml => FileDialogResult::SaveXml(
                    rfd::FileDialog::new()
                        .add_filter("XML Files", &["xml"])
                        .set_file_name("output.xml")
                        .save_file(),
                ),
                FileDialogAction::SaveExi => FileDialogResult::SaveExi(
                    rfd::FileDialog::new()
                        .add_filter("EXI Files", &["exi"])
                        .set_file_name("output.exi")
                        .save_file(),
                ),
                FileDialogAction::SaveJson => FileDialogResult::SaveJson(
                    rfd::FileDialog::new()
                        .add_filter("JSON Files", &["json"])
                        .set_file_name("output.json")
                        .save_file(),
                ),
            };
            let _ = tx.send(result);
        });
    }
}
