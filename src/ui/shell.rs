//! The application shell: menus, toolbar, document tabs, panel layout,
//! dialogs, and keyboard shortcuts.
//!
//! The shell owns the workspace (`WorkspaceState`), the background task
//! manager, file watching, and recovery — all services from step 3 — and
//! never performs file I/O, parsing, or codec work on the UI thread. Heavy
//! operations are submitted as jobs and their results are applied only
//! while the `SessionId + Revision` triple still matches.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use egui::{Context, Key, KeyboardShortcut, Modifiers};

use crate::core::document::NodeId;
use crate::core::document::XmlDocument;
use crate::core::{Command, Diagnostic, Severity};
use crate::fluent_args;
use crate::services::document_io::{classify_bytes, save_bytes_atomically};
use crate::services::recovery::{RecoverySnapshot, RecoveryStore};
use crate::services::search::SearchIndex;
use crate::services::task_manager::{SessionId, TaskManager};
use crate::services::watcher::FileWatcher;
use crate::services::workspace::{DocumentMode, FileType, WorkspaceState};
use crate::ui::fonts;
use crate::ui::localization::Localization;
use crate::ui::panels;
use crate::ui::theme_prefs::{FontScale, ThemeMode, apply_accent_theme};

/// Pseudo-session used for workspace-level jobs (opening files).
const WORKSPACE_SESSION: SessionId = SessionId(u64::MAX);
/// Pseudo-session for save jobs (delivered regardless of document revision).
const SAVE_SESSION: SessionId = SessionId(u64::MAX - 1);

/// Modal dialogs the shell can show.
pub enum Dialog {
    About,
    ConfirmDelete {
        node: NodeId,
        name: String,
        descendants: usize,
    },
    UnsavedExit,
    ReloadBanner {
        path: PathBuf,
        dirty: bool,
    },
    Recovery {
        snapshots: Vec<(u64, RecoverySnapshot)>,
    },
    Shortcuts,
}

/// Which pane receives keyboard focus (F6 cycles).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Outline,
    Source,
    Inspector,
    Problems,
}

/// The desktop shell state.
pub struct AppShell {
    pub localization: Localization,
    pub theme_mode: ThemeMode,
    pub font_scale: FontScale,
    pub workspace: WorkspaceState,
    pub tasks: Arc<TaskManager>,
    pub watcher: Option<FileWatcher>,
    pub recovery: RecoveryStore,
    /// Expansion state per session (node ids are session-scoped).
    expanded: HashMap<SessionId, HashSet<NodeId>>,
    pub dialog: Option<Dialog>,
    pub focus: FocusPane,
    pub problems: Vec<Diagnostic>,
    /// Search state for the active session.
    pub search: panels::SearchState,
    pub(crate) pending_new_attr: bool,
    pub(crate) pending_remove_attr: Option<String>,
    pub(crate) open_pending: usize,
    pub(crate) outline_cache: Option<(SessionId, u64, panels::OutlineCache)>,
    /// Fonts are installed once per context, not per frame.
    fonts_installed: bool,
    /// Narrow-layout drawers (default open so panels stay reachable).
    pub show_outline_drawer: bool,
    pub show_inspector_drawer: bool,
}

impl AppShell {
    /// Builds the shell with system-detected language and theme.
    pub fn new() -> AppShell {
        AppShell {
            localization: Localization::new(),
            theme_mode: ThemeMode::System,
            font_scale: FontScale::default(),
            workspace: WorkspaceState::new(),
            tasks: Arc::new(TaskManager::new()),
            watcher: FileWatcher::new().ok(),
            recovery: RecoveryStore::new(RecoveryStore::default_root()),
            expanded: HashMap::new(),
            dialog: None,
            focus: FocusPane::Outline,
            problems: Vec::new(),
            outline_cache: None,
            search: panels::SearchState::default(),
            pending_new_attr: false,
            pending_remove_attr: None,
            open_pending: 0,
            fonts_installed: false,
            show_outline_drawer: true,
            show_inspector_drawer: true,
        }
    }

    /// Rebuilds the search index for the active session and re-runs the
    /// current query (called after every keystroke or edit).
    pub(crate) fn refresh_search(&mut self) {
        let Some(session) = self.workspace.active() else {
            self.search.hits.clear();
            return;
        };
        let revision = session.document.revision().0;
        let stale = !self
            .search
            .index
            .as_ref()
            .is_some_and(|(s, r, _)| *s == session.id.0 && *r == revision);
        if stale {
            let index = SearchIndex::build(&session.document);
            self.search.index = Some((session.id.0, revision, index));
        }
        if self.search.needle.is_empty() {
            self.search.hits.clear();
            return;
        }
        let order = session.document.document_order().to_vec();
        let needle = self.search.needle.clone();
        let case = self.search.case_sensitive;
        let index = &mut self.search.index.as_mut().expect("index was just built").2;
        self.search.hits = index.search(&needle, case, &order);
        self.search.current = 0;
    }

    /// Runs one frame.
    pub fn update(&mut self, ctx: &Context) {
        self.apply_preferences_once(ctx);
        self.poll_background_jobs();
        self.poll_file_watcher();
        self.handle_shortcuts(ctx);

        panels::top_bar(ctx, self);
        panels::document_tabs(ctx, self);
        panels::bottom_panel(ctx, self);
        self.layout_panels(ctx);
        panels::status_bar(ctx, self);
        crate::ui::dialogs::dialogs(ctx, self);
    }

    fn apply_preferences_once(&mut self, ctx: &Context) {
        self.theme_mode.apply(ctx);
        self.font_scale.apply(ctx);
        apply_accent_theme(ctx);
        if !self.fonts_installed {
            fonts::install_cjk_font(ctx);
            self.fonts_installed = true;
        }
    }

    // -----------------------------------------------------------------------
    // Commands
    // -----------------------------------------------------------------------

    /// Opens a document: read, classify, and parse all run in a background
    /// job; the UI thread only receives the finished result.
    pub fn open_path(&mut self, path: PathBuf) {
        if self.workspace.focus_existing(&path).is_some() {
            return; // same file: focus, never a second tab
        }
        self.open_pending += 1;
        self.tasks
            .spawn(WORKSPACE_SESSION, crate::core::Revision(0), move |_| {
                Box::new(open_document_job(path))
            });
    }

    /// Creates a new untitled document.
    pub fn new_document(&mut self) {
        self.workspace.add_untitled();
        self.refresh_outline_cache();
    }

    /// Saves the active document in a background job. The bytes saved are
    /// those of the revision at spawn time; if the user keeps editing, the
    /// tab stays dirty when the job finishes.
    pub fn save_active(&mut self, path: Option<PathBuf>) -> bool {
        let Some(session) = self.workspace.active_mut() else {
            return false;
        };
        if session.mode == DocumentMode::LargeReadOnly {
            return false; // read-only sessions save via explicit save-as only
        }
        let Some(path) = path.or_else(|| session.path.clone()) else {
            return false;
        };
        let session_id = session.id;
        let revision = session.document.revision();
        let source = session.document.source().to_string();
        let encoding = session.document.encoding();
        if session.path.is_none() {
            session.path = Some(path.clone()); // Save As assigns immediately
        }
        let job_path = path;
        self.tasks
            .spawn(SAVE_SESSION, crate::core::Revision(0), move |_| {
                let bytes = crate::xml::encoding::encode_xml_text(&source, encoding);
                let outcome =
                    save_bytes_atomically(&job_path, &bytes).map_err(|err| err.to_string());
                Box::new(SaveJobResult {
                    session: session_id,
                    revision,
                    outcome,
                })
            });
        true
    }

    /// Commits an editing command on the active session.
    pub fn commit(&mut self, command: Command) -> bool {
        let Some(session) = self.workspace.active_mut() else {
            return false;
        };
        if session.mode == DocumentMode::LargeReadOnly {
            self.push_problem(
                Severity::Warning,
                "readonly-edit",
                self.localization.msg("error-readonly-edit"),
            );
            return false;
        }
        let outcome = session.history.commit(&mut session.document, command);
        match outcome {
            Ok(_changed) => {
                // Invalidate caches for this session's new revision.
                self.outline_cache = None;
                self.search.index = None;
                true
            }
            Err(err) => {
                self.push_problem(Severity::Error, err.code, err.message.clone());
                false
            }
        }
    }

    /// Undo on the active session.
    pub fn undo(&mut self) {
        if let Some(session) = self.workspace.active_mut() {
            session.history.undo(&mut session.document);
            self.outline_cache = None;
            self.search.index = None;
        }
    }

    /// Redo on the active session.
    pub fn redo(&mut self) {
        if let Some(session) = self.workspace.active_mut() {
            session.history.redo(&mut session.document);
            self.outline_cache = None;
            self.search.index = None;
        }
    }

    /// Closes the active tab, prompting when dirty.
    pub fn close_active_tab(&mut self) {
        if self
            .workspace
            .active()
            .is_some_and(|session| session.is_dirty())
        {
            self.dialog = Some(Dialog::UnsavedExit);
            return;
        }
        self.close_tab_silent();
    }

    fn close_tab_silent(&mut self) {
        if let Some(session) = self.workspace.active() {
            self.recovery.remove(session.id.0);
            if let Some(path) = session.path.clone()
                && let Some(watcher) = self.watcher.as_mut()
            {
                watcher.unwatch(&path);
            }
        }
        let active = self
            .workspace
            .sessions()
            .iter()
            .position(|session| Some(session.id) == self.workspace.active_id());
        if let Some(index) = active {
            self.workspace.close(index);
        }
        self.outline_cache = None;
        self.search.index = None;
        self.refresh_outline_cache();
    }

    pub(crate) fn push_problem(&mut self, severity: Severity, code: &str, message: String) {
        self.problems.push(Diagnostic::new(severity, code, message));
    }

    // -----------------------------------------------------------------------
    // Background polling
    // -----------------------------------------------------------------------

    fn poll_background_jobs(&mut self) {
        self.poll_save_jobs();
        while let Ok(Some(outcome)) = self
            .tasks
            .take_outcome(WORKSPACE_SESSION, crate::core::Revision(0))
        {
            self.open_pending = self.open_pending.saturating_sub(1);
            let payload = outcome
                .result
                .downcast::<OpenJobResult>()
                .expect("open job payload type");
            match *payload {
                OpenJobResult::Opened {
                    path,
                    mode,
                    document,
                } => {
                    self.workspace
                        .add_opened(path.clone(), FileType::Xml, mode, document);
                    if let Some(watcher) = self.watcher.as_mut() {
                        let _ = watcher.watch(&path);
                    }
                    self.refresh_outline_cache();
                }
                OpenJobResult::Failed { code, message } => {
                    let key = match code.as_str() {
                        "too-large" => "error-too-large",
                        "parse" => "error-parse-failed",
                        _ => "error-io",
                    };
                    let text = self
                        .localization
                        .msg_with(key, Some(&fluent_args!("message" => message.as_str())));
                    self.push_problem(Severity::Error, &code, text);
                }
            }
        }
    }

    fn poll_save_jobs(&mut self) {
        while let Ok(Some(outcome)) = self
            .tasks
            .take_outcome(SAVE_SESSION, crate::core::Revision(0))
        {
            let payload = outcome
                .result
                .downcast::<SaveJobResult>()
                .expect("save job payload type");
            match payload.outcome {
                Ok(()) => {
                    let mut recovery_remove = None;
                    if let Some(session) = self
                        .workspace
                        .sessions_mut()
                        .iter_mut()
                        .find(|session| session.id == payload.session)
                        && session.document.revision() == payload.revision
                    {
                        session.mark_saved();
                        recovery_remove = Some(session.id.0);
                    }
                    if let Some(session) = recovery_remove {
                        self.recovery.remove(session);
                    }
                }
                Err(message) => {
                    self.push_problem(Severity::Error, "io", message);
                }
            }
        }
    }

    fn poll_file_watcher(&mut self) {
        let Some(watcher) = self.watcher.as_mut() else {
            return;
        };
        for change in watcher.poll_changes() {
            let path = change.path().to_path_buf();
            let dirty = self.workspace.sessions().iter().any(|session| {
                session.path.as_deref() == Some(change.path()) && session.is_dirty()
            });
            self.dialog = Some(Dialog::ReloadBanner { path, dirty });
        }
    }

    // -----------------------------------------------------------------------
    // Layout
    // -----------------------------------------------------------------------

    fn layout_panels(&mut self, ctx: &Context) {
        let width = ctx.content_rect().width();
        let inspector_inline = width >= 1100.0;
        let outline_inline = width >= 850.0;

        if outline_inline {
            panels::outline_panel(ctx, self, false);
        }
        if inspector_inline {
            panels::inspector_panel(ctx, self, false);
        }
        panels::central_panel(ctx, self);
        // Narrow windows: panels become overlay drawers (drawn after and
        // above the central panel).
        if !outline_inline && self.show_outline_drawer {
            panels::outline_drawer(ctx, self);
        }
        if !inspector_inline && self.show_inspector_drawer {
            panels::inspector_drawer(ctx, self);
        }
    }

    pub(crate) fn outline_cache(&mut self) -> &panels::OutlineCache {
        let active = self.workspace.active_id();
        let revision = self
            .workspace
            .active()
            .map(|session| session.document.revision().0);
        let cached = self
            .outline_cache
            .as_ref()
            .is_some_and(|(session, rev, _)| Some(*session) == active && Some(*rev) == revision);
        if !cached {
            self.refresh_outline_cache();
        }
        self.outline_cache
            .as_ref()
            .map(|(_, _, cache)| cache)
            .expect("cache was just refreshed")
    }

    fn refresh_outline_cache(&mut self) {
        let Some(session) = self.workspace.active() else {
            self.outline_cache = None;
            return;
        };
        let expanded = self.expanded.get(&session.id).cloned().unwrap_or_default();
        let tree = crate::services::outline::FlatTree::build(&session.document, &expanded);
        self.outline_cache = Some((
            session.id,
            session.document.revision().0,
            panels::OutlineCache {
                tree,
                elements: session.document.document_order().len().saturating_sub(1),
            },
        ));
    }

    pub(crate) fn toggle_expanded(&mut self, session: SessionId, node: NodeId) {
        let set = self.expanded.entry(session).or_default();
        if !set.insert(node) {
            set.remove(&node);
        }
    }

    pub(crate) fn expand_all(&mut self, session: SessionId) {
        let Some(current) = self.workspace.sessions().iter().find(|s| s.id == session) else {
            return;
        };
        let all: HashSet<NodeId> = current
            .document
            .document_order()
            .iter()
            .map(|&id| NodeId(id))
            .collect();
        self.expanded.insert(session, all);
        self.refresh_outline_cache();
    }

    pub(crate) fn collapse_all(&mut self, session: SessionId) {
        self.expanded.insert(session, HashSet::new());
        self.refresh_outline_cache();
    }

    // -----------------------------------------------------------------------
    // Shortcuts
    // -----------------------------------------------------------------------

    fn handle_shortcuts(&mut self, ctx: &Context) {
        const CTRL: Modifiers = Modifiers::CTRL;
        const CTRL_SHIFT: Modifiers = Modifiers::CTRL.plus(Modifiers::SHIFT);

        let new = KeyboardShortcut::new(CTRL, Key::N);
        let open = KeyboardShortcut::new(CTRL, Key::O);
        let save = KeyboardShortcut::new(CTRL, Key::S);
        let save_as = KeyboardShortcut::new(CTRL_SHIFT, Key::S);
        let close = KeyboardShortcut::new(CTRL, Key::W);
        let undo = KeyboardShortcut::new(CTRL, Key::Z);
        let redo = KeyboardShortcut::new(CTRL, Key::Y);
        let find = KeyboardShortcut::new(CTRL, Key::F);
        let replace = KeyboardShortcut::new(CTRL, Key::H);

        let wants =
            |shortcut: KeyboardShortcut| ctx.input_mut(|input| input.consume_shortcut(&shortcut));

        if wants(new) {
            self.new_document();
        }
        if wants(open) {
            self.pick_and_open();
        }
        if wants(save) {
            self.save_active(None);
        }
        if wants(save_as) {
            self.pick_and_save_as();
        }
        if wants(close) {
            self.close_active_tab();
        }
        if wants(undo) {
            self.undo();
        }
        if wants(redo) {
            self.redo();
        }
        if wants(find) {
            self.search.open = true;
            self.focus = FocusPane::Outline;
        }
        if wants(replace) {
            // Batch replace UI arrives with step 7; find is the entry point.
            self.search.open = true;
            self.focus = FocusPane::Outline;
        }
        if wants(KeyboardShortcut::new(Modifiers::NONE, Key::F3)) {
            self.search.jump_next();
        }
        if wants(KeyboardShortcut::new(Modifiers::SHIFT, Key::F3)) {
            self.search.jump_previous();
        }
        if wants(KeyboardShortcut::new(Modifiers::NONE, Key::F6)) {
            self.focus = match self.focus {
                FocusPane::Outline => FocusPane::Source,
                FocusPane::Source => FocusPane::Inspector,
                FocusPane::Inspector => FocusPane::Problems,
                FocusPane::Problems => FocusPane::Outline,
            };
        }
        if wants(KeyboardShortcut::new(Modifiers::NONE, Key::F1)) {
            self.dialog = Some(Dialog::Shortcuts);
        }
    }

    pub fn pick_and_open(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("XML", &["xml"])
            .add_filter("EXI", &["exi", "bin"])
            .pick_file()
        {
            self.open_path(path);
        }
    }

    pub fn pick_and_save_as(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("XML", &["xml"])
            .set_file_name("document.xml")
            .save_file()
        {
            self.save_active(Some(path));
        }
    }
}

impl Default for AppShell {
    fn default() -> Self {
        AppShell::new()
    }
}

/// Payload of the background save job.
pub struct SaveJobResult {
    pub session: SessionId,
    pub revision: crate::core::Revision,
    pub outcome: Result<(), String>,
}

/// Payload of the background open job.
#[allow(clippy::large_enum_variant)]
pub enum OpenJobResult {
    Opened {
        path: PathBuf,
        mode: DocumentMode,
        document: XmlDocument,
    },
    Failed {
        code: String,
        message: String,
    },
}

/// Runs entirely on the worker thread: read bytes, classify, parse.
fn open_document_job(path: PathBuf) -> OpenJobResult {
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            return OpenJobResult::Failed {
                code: String::from("io"),
                message: err.to_string(),
            };
        }
    };
    let outcome = match classify_bytes(&bytes) {
        Ok(outcome) => outcome,
        Err(err) => {
            return OpenJobResult::Failed {
                code: String::from("too-large"),
                message: err.to_string(),
            };
        }
    };
    match XmlDocument::parse(&bytes) {
        Ok(document) => OpenJobResult::Opened {
            path,
            mode: DocumentMode::from(outcome.mode),
            document,
        },
        Err(err) => OpenJobResult::Failed {
            code: String::from("parse"),
            message: err.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Command, InsertPosition, NewNode};
    use crate::services::workspace::DocumentMode;

    /// Equivalents of the four `main_panel` tests deleted with step 5,
    /// restated against the shell's workspace/history services.
    fn shell_with_edited_document() -> AppShell {
        let mut shell = AppShell::new();
        shell.new_document();
        let root = shell
            .workspace
            .active()
            .unwrap()
            .document
            .root_element()
            .unwrap();
        assert!(
            shell.commit(Command::InsertNode {
                parent: root,
                position: InsertPosition::Last,
                node: NewNode::Element {
                    name: "child".into(),
                },
            }),
            "insert applies on a fresh untitled document"
        );
        shell
    }

    fn first_child_id(shell: &AppShell) -> crate::core::NodeId {
        let session = shell.workspace.active().unwrap();
        let root = session.document.root_element().unwrap();
        session
            .document
            .children(root)
            .into_iter()
            .find(|id| session.document.kind(*id) == Some(crate::core::XmlNodeKind::Element))
            .expect("child element")
    }

    #[test]
    fn undo_and_redo_restore_dirty_state() {
        let mut shell = shell_with_edited_document();
        let node = first_child_id(&shell);
        assert!(shell.workspace.active().unwrap().is_dirty());

        shell.undo();
        assert!(
            !shell.workspace.active().unwrap().is_dirty(),
            "undo back to the save point must clean the tab"
        );

        shell.redo();
        assert!(shell.workspace.active().unwrap().is_dirty());
        let session = shell.workspace.active().unwrap();
        assert_eq!(
            session.document.qname(node).unwrap().render(),
            "child",
            "redo must restore the inserted node"
        );
    }

    #[test]
    fn request_unsaved_action_queues_prompt_when_dirty() {
        let mut shell = shell_with_edited_document();
        shell.close_active_tab();
        assert!(
            matches!(shell.dialog, Some(Dialog::UnsavedExit)),
            "closing a dirty tab must queue the unsaved-changes prompt"
        );
    }

    #[test]
    fn discard_quit_allows_one_close_without_saving() {
        let mut shell = shell_with_edited_document();
        shell.dialog = Some(Dialog::UnsavedExit);
        // Simulate the dialog's Discard button.
        for index in 0..shell.workspace.sessions().len() {
            shell.workspace.select(index);
            if let Some(session) = shell.workspace.active_mut() {
                session.history.clear();
            }
        }
        shell.dialog = None;
        shell.close_active_tab();
        assert!(
            shell.workspace.sessions().is_empty(),
            "tab closes after discard"
        );
    }

    #[test]
    fn save_xml_marks_document_clean() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("saved.xml");
        let mut shell = shell_with_edited_document();

        assert!(shell.save_active(Some(path.clone())));
        // The save runs in a background job; drain it synchronously.
        for _ in 0..200 {
            shell.poll_save_jobs();
            if !shell.workspace.active().unwrap().is_dirty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let session = shell.workspace.active().unwrap();
        assert!(!session.is_dirty(), "save must mark the tab clean");
        assert_eq!(session.path.as_deref(), Some(path.as_path()));
        assert!(path.exists());
    }

    #[test]
    fn readonly_sessions_refuse_edits_and_saves() {
        let mut shell = AppShell::new();
        let document = XmlDocument::parse(b"<r/>".as_slice()).unwrap();
        shell.workspace.add_opened(
            std::path::PathBuf::from("/tmp/x.xml"),
            FileType::Xml,
            DocumentMode::LargeReadOnly,
            document,
        );
        let root = shell
            .workspace
            .active()
            .unwrap()
            .document
            .root_element()
            .unwrap();
        assert!(!shell.commit(Command::FormatDocument {
            indent: "  ".into()
        }));
        assert!(!shell.save_active(None));
        assert!(shell.workspace.active().unwrap().mode == DocumentMode::LargeReadOnly);
        let _ = root;
    }
}
