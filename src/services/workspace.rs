//! Workspace state: document sessions, tab identity, and naming rules.
//!
//! One [`DocumentSession`] per open tab: the parsed document, its undo
//! history, dirty tracking against the save cursor, path, mode, and the
//! lightweight UI anchors (cursor offset, selected node) that crash
//! recovery restores. [`WorkspaceState`] owns the session list and enforces
//! the tab rules: re-opening the same path focuses the existing tab, new
//! files are named `Untitled-N.xml` with a monotonically increasing N, and
/// closing a tab keeps every other session's state independent.
use std::path::{Path, PathBuf};

use crate::core::document::{NodeId, XmlDocument};
use crate::core::history::History;
use crate::services::document_io::OpenMode;
use crate::services::task_manager::SessionId;

/// File type of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Xml,
    Exi,
}

/// Whether a session allows editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentMode {
    Editable,
    LargeReadOnly,
}

impl From<OpenMode> for DocumentMode {
    fn from(mode: OpenMode) -> DocumentMode {
        match mode {
            OpenMode::Editable => DocumentMode::Editable,
            OpenMode::LargeReadOnly => DocumentMode::LargeReadOnly,
        }
    }
}

/// One open document tab.
pub struct DocumentSession {
    pub id: SessionId,
    /// `None` for unsaved new documents.
    pub path: Option<PathBuf>,
    pub file_type: FileType,
    pub mode: DocumentMode,
    pub document: XmlDocument,
    pub history: History,
    /// Byte offset of the source-editor cursor.
    pub cursor: usize,
    /// Selected tree node, when the tree is the active pane.
    pub selection: Option<NodeId>,
    /// Present when the source editor holds an unapplied draft (step 6).
    pub source_draft: Option<String>,
}

impl DocumentSession {
    /// Whether the tab would prompt on close.
    pub fn is_dirty(&self) -> bool {
        self.history.is_dirty() || self.source_draft.is_some()
    }

    /// Marks the current state saved.
    pub fn mark_saved(&mut self) {
        self.history.mark_saved();
    }

    /// Display name for the tab: file stem or `Untitled-N`.
    pub fn display_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|path| path.file_stem())
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("Untitled-{}", self.id.0))
    }
}

/// The tab strip plus its naming rules.
pub struct WorkspaceState {
    sessions: Vec<DocumentSession>,
    active_index: Option<usize>,
    next_session: u64,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceState {
    pub fn new() -> WorkspaceState {
        WorkspaceState {
            sessions: Vec::new(),
            active_index: None,
            next_session: 1,
        }
    }

    /// All sessions in tab order.
    pub fn sessions(&self) -> &[DocumentSession] {
        &self.sessions
    }

    /// The active tab, if any.
    pub fn active(&self) -> Option<&DocumentSession> {
        self.active_index.and_then(|index| self.sessions.get(index))
    }

    /// Mutable access to the active tab.
    pub fn active_mut(&mut self) -> Option<&mut DocumentSession> {
        let index = self.active_index?;
        self.sessions.get_mut(index)
    }

    pub fn active_id(&self) -> Option<SessionId> {
        self.active().map(|session| session.id)
    }

    /// Selects the tab at `index`.
    pub fn select(&mut self, index: usize) {
        if index < self.sessions.len() {
            self.active_index = Some(index);
        }
    }

    /// Focuses the session for `path` if one is already open, returning its
    /// index. Opening the same file twice must not create a second session.
    pub fn focus_existing(&mut self, path: &Path) -> Option<usize> {
        let index = self.sessions.iter().position(|session| {
            session
                .path
                .as_deref()
                .is_some_and(|existing| existing == path)
        })?;
        self.active_index = Some(index);
        Some(index)
    }

    /// Adds a session for an opened document and makes it active. The
    /// caller supplies the already-parsed document (parsing happens in a
    /// background task).
    pub fn add_opened(
        &mut self,
        path: PathBuf,
        file_type: FileType,
        mode: DocumentMode,
        document: XmlDocument,
    ) -> SessionId {
        let id = SessionId(self.next_session);
        self.next_session += 1;
        self.sessions.push(DocumentSession {
            id,
            path: Some(path),
            file_type,
            mode,
            document,
            history: History::new(),
            cursor: 0,
            selection: None,
            source_draft: None,
        });
        self.active_index = Some(self.sessions.len() - 1);
        id
    }

    /// Adds a session for a brand-new empty document named `Untitled-N.xml`.
    pub fn add_untitled(&mut self) -> SessionId {
        let id = SessionId(self.next_session);
        self.next_session += 1;
        let document =
            XmlDocument::parse(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<root/>\n".as_slice())
                .expect("seed document parses");
        self.sessions.push(DocumentSession {
            id,
            path: None,
            file_type: FileType::Xml,
            mode: DocumentMode::Editable,
            document,
            history: History::new(),
            cursor: 0,
            selection: None,
            source_draft: None,
        });
        self.active_index = Some(self.sessions.len() - 1);
        id
    }

    /// Closes the tab at `index`. Every other session keeps its state.
    /// Returns the closed session's id.
    pub fn close(&mut self, index: usize) -> Option<SessionId> {
        if index >= self.sessions.len() {
            return None;
        }
        let session = self.sessions.remove(index);
        self.active_index = match self.sessions.len() {
            0 => None,
            _ => Some(index.min(self.sessions.len() - 1)),
        };
        Some(session.id)
    }

    /// Sessions whose tabs need a save prompt, in tab order.
    pub fn dirty_sessions(&self) -> Vec<SessionId> {
        self.sessions
            .iter()
            .filter(|session| session.is_dirty())
            .map(|session| session.id)
            .collect()
    }

    /// First save of an untitled session assigns its path (Save As).
    pub fn assign_path(&mut self, id: SessionId, path: PathBuf) {
        if let Some(session) = self.sessions.iter_mut().find(|s| s.id == id) {
            session.path = Some(path);
        }
    }
}
