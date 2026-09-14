use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// Result delivered from the background file-dialog thread.
pub enum FileDialogResult {
    OpenXml(Option<PathBuf>),
    OpenExi(Option<PathBuf>),
    SaveXml(Option<PathBuf>),
    SaveExi(Option<PathBuf>),
    SaveJson(Option<PathBuf>),
    OpenImage(Option<PathBuf>),
    SaveIconText(Option<PathBuf>),
}

/// Which file-dialog action to open.
#[derive(Debug, Clone, Copy)]
pub enum FileDialogAction {
    OpenXml,
    OpenExi,
    SaveXml,
    SaveExi,
    SaveJson,
    OpenImage,
    SaveIconText,
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
        match self.rx.as_ref()?.try_recv() {
            Ok(result) => {
                self.rx = None;
                Some(result)
            }
            Err(TryRecvError::Disconnected) => {
                self.rx = None;
                None
            }
            Err(TryRecvError::Empty) => None,
        }
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
                FileDialogAction::OpenImage => FileDialogResult::OpenImage(
                    rfd::FileDialog::new()
                        .add_filter(
                            "Images",
                            &["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico"],
                        )
                        .add_filter("All Files", &["*"])
                        .pick_file(),
                ),
                FileDialogAction::SaveIconText => FileDialogResult::SaveIconText(
                    rfd::FileDialog::new()
                        .add_filter("Encoded Text", &["txt"])
                        .set_file_name("icon.txt")
                        .save_file(),
                ),
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
