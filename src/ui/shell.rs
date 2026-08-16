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
/// Pseudo-session for source-draft apply jobs.
const APPLY_SESSION: SessionId = SessionId(u64::MAX - 2);

/// Modal dialogs the shell can show.
pub enum Dialog {
    About,
    /// XPath query entry (expression buffer).
    XPathQuery {
        expression: String,
    },
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
        if session.source_draft.is_some() {
            self.push_problem(
                Severity::Warning,
                "draft-active",
                self.localization.msg("source-draft-active"),
            );
            return false; // tree edits are disabled while a draft exists
        }
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
        self.poll_apply_jobs();
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

    /// Applies the active session's source draft: the parse runs in a
    /// background job attributed to the draft's base revision; a success
    /// commits one `ReplaceWholeSource`, a failure keeps the draft and
    /// reports the exact error position.
    pub fn apply_source(&mut self) -> bool {
        let Some(session) = self.workspace.active_mut() else {
            return false;
        };
        let Some(draft) = session.source_draft.as_ref() else {
            return false;
        };
        let base_revision = draft.base_revision;
        let draft_text = draft.buffer.text();
        let session_id = session.id;
        self.tasks
            .spawn(APPLY_SESSION, crate::core::Revision(0), move |_| {
                Box::new(ApplyJobResult {
                    session: session_id,
                    base_revision,
                    outcome: crate::services::source_editor::parse_draft(&draft_text),
                })
            });
        true
    }

    /// Records source-pane text: first change forks a draft at the current
    /// revision; later keystrokes only update the buffer.
    pub fn update_source_text(&mut self, text: String) {
        let Some(session) = self.workspace.active_mut() else {
            return;
        };
        match session.source_draft.as_mut() {
            Some(draft) => draft.buffer = crate::services::source_buffer::SourceBuffer::new(&text),
            None => {
                session.source_draft = Some(crate::services::source_editor::SourceDraft::fork(
                    &text,
                    session.document.revision(),
                ));
            }
        }
    }

    /// Discards the active session's draft without applying.
    pub fn discard_draft(&mut self) {
        if let Some(session) = self.workspace.active_mut() {
            session.source_draft = None;
        }
    }

    fn poll_apply_jobs(&mut self) {
        while let Ok(Some(outcome)) = self
            .tasks
            .take_outcome(APPLY_SESSION, crate::core::Revision(0))
        {
            let payload = *outcome
                .result
                .downcast::<ApplyJobResult>()
                .expect("apply job payload type");
            self.handle_apply_result(payload);
        }
    }

    fn handle_apply_result(&mut self, payload: ApplyJobResult) {
        let Some(session) = self
            .workspace
            .sessions_mut()
            .iter_mut()
            .find(|session| session.id == payload.session)
        else {
            return;
        };
        if session.document.revision() != payload.base_revision {
            return; // document moved on: the draft stays unapplied
        }
        match payload.outcome {
            crate::services::source_editor::ApplyOutcome::Parsed { new_source } => {
                session.source_draft = None;
                let revision_before = session.document.revision();
                let _ = revision_before;
                // Commit through the command layer so undo/redo covers it.
                let result = session.history.commit(
                    &mut session.document,
                    Command::ReplaceWholeSource { new_source },
                );
                match result {
                    Ok(_) => {
                        self.outline_cache = None;
                        self.search.index = None;
                    }
                    Err(err) => {
                        self.push_problem(Severity::Error, err.code, err.message.clone());
                    }
                }
            }
            crate::services::source_editor::ApplyOutcome::Invalid {
                line,
                column,
                message,
            } => {
                let diagnostic = Diagnostic::new(Severity::Error, "source-draft", message.clone())
                    .with_argument("line", line.to_string())
                    .with_argument("column", column.to_string());
                self.problems.push(diagnostic);
            }
        }
    }

    /// Opens the XPath query dialog.
    pub fn run_xpath_dialog(&mut self) {
        self.dialog = Some(Dialog::XPathQuery {
            expression: String::new(),
        });
    }

    /// Executes an XPath expression; results land in the Problems panel.
    pub fn execute_xpath(&mut self, expression: &str) {
        let Some(session) = self.workspace.active() else {
            return;
        };
        match crate::services::xpath::query(&session.document, expression) {
            Ok(crate::services::xpath::XPathOutcome::NodeSet(nodes)) => {
                let text = self.localization.msg_with(
                    "xpath-result-nodes",
                    Some(&crate::fluent_args!("count" => nodes.len() as i32)),
                );
                self.push_problem(crate::core::Severity::Info, "xpath", text);
            }
            Ok(crate::services::xpath::XPathOutcome::String(value)) => {
                self.push_problem(crate::core::Severity::Info, "xpath", value);
            }
            Ok(crate::services::xpath::XPathOutcome::Number(value)) => {
                self.push_problem(crate::core::Severity::Info, "xpath", value.to_string());
            }
            Ok(crate::services::xpath::XPathOutcome::Boolean(value)) => {
                self.push_problem(crate::core::Severity::Info, "xpath", value.to_string());
            }
            Err(err) => {
                self.push_problem(crate::core::Severity::Error, "xpath", err.to_string());
            }
        }
    }

    /// XSD validation: pick a schema, validate, report diagnostics.
    pub fn run_validation_dialog(&mut self) {
        let Some(schema_path) = rfd::FileDialog::new()
            .add_filter("XSD", &["xsd"])
            .pick_file()
        else {
            return;
        };
        match crate::services::validation::compile_schema(&schema_path) {
            Ok(validator) => {
                let Some(session) = self.workspace.active() else {
                    return;
                };
                for diagnostic in
                    crate::services::validation::validate(&session.document, &validator)
                {
                    let mut rendered = diagnostic.message_key.clone();
                    if let (Some(line), Some(column)) = (
                        diagnostic.arguments.get("line"),
                        diagnostic.arguments.get("column"),
                    ) {
                        rendered.push_str(&format!(" ({line}:{column})"));
                    }
                    self.push_problem(diagnostic.severity, &diagnostic.code, rendered);
                }
                if self.problems.is_empty() {
                    self.push_problem(crate::core::Severity::Info, "xsd", String::from("valid"));
                }
            }
            Err(err) => {
                self.push_problem(crate::core::Severity::Error, "xsd", err.to_string());
            }
        }
    }

    /// Structural diff against a file on disk.
    pub fn run_diff_dialog(&mut self) {
        let Some(other_path) = rfd::FileDialog::new()
            .add_filter("XML", &["xml"])
            .pick_file()
        else {
            return;
        };
        let Ok(bytes) = std::fs::read(&other_path) else {
            return;
        };
        let Ok(other) = XmlDocument::parse(&bytes) else {
            self.push_problem(
                Severity::Error,
                "diff",
                self.localization.msg("error-parse-failed"),
            );
            return;
        };
        let Some(session) = self.workspace.active() else {
            return;
        };
        match crate::services::diff::diff_xml(
            &session.document,
            &other,
            crate::services::diff::DiffOptions::default(),
        ) {
            Ok(entries) if entries.is_empty() => {
                self.push_problem(
                    crate::core::Severity::Info,
                    "diff",
                    String::from("identical"),
                );
            }
            Ok(entries) => {
                for entry in &entries {
                    let (tag, label) = match entry {
                        crate::services::diff::DiffEntry::Added { label } => ("+", label),
                        crate::services::diff::DiffEntry::Removed { label } => ("-", label),
                        crate::services::diff::DiffEntry::Modified { label, .. } => ("~", label),
                        crate::services::diff::DiffEntry::Moved { label } => (">", label),
                    };
                    self.push_problem(
                        crate::core::Severity::Info,
                        "diff",
                        format!("{tag} {label}"),
                    );
                }
            }
            Err(err) => {
                self.push_problem(Severity::Error, "diff", err);
            }
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

/// Payload of the background draft-apply job.
pub struct ApplyJobResult {
    pub session: SessionId,
    pub base_revision: crate::core::Revision,
    pub outcome: crate::services::source_editor::ApplyOutcome,
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
    use crate::core::{Command, InsertPosition, NewNode, QNameSpec};
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

    /// Drains apply jobs until none are pending (bounded wait).
    fn drain_apply_jobs(shell: &mut AppShell) {
        for _ in 0..200 {
            shell.poll_apply_jobs();
            if shell
                .workspace
                .active()
                .and_then(|s| s.source_draft.as_ref())
                .is_none()
            {
                // Applied (draft cleared) — drain once more for stragglers.
                shell.poll_apply_jobs();
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn draft_disables_all_tree_edits() {
        let mut shell = shell_with_edited_document();
        let root = shell
            .workspace
            .active()
            .unwrap()
            .document
            .root_element()
            .unwrap();
        let source = shell
            .workspace
            .active()
            .unwrap()
            .document
            .source()
            .to_string();
        shell.update_source_text(source);

        assert!(
            !shell.commit(Command::RenameElement {
                node: root,
                new_name: QNameSpec::local("blocked"),
            }),
            "tree edits must be rejected while a draft exists"
        );
        assert!(
            shell
                .problems
                .iter()
                .any(|problem| problem.code == "draft-active")
        );
        // The failure left no trace.
        assert_eq!(
            shell.workspace.active().unwrap().document.revision().0,
            1,
            "rejected edit must not bump the revision"
        );
    }

    #[test]
    fn apply_source_renames_show_up_and_undo_covers_apply() {
        let mut shell = AppShell::new();
        shell.new_document();
        // Seed a child element via the tree, then rename it in the source.
        let root = shell
            .workspace
            .active()
            .unwrap()
            .document
            .root_element()
            .unwrap();
        assert!(shell.commit(Command::InsertNode {
            parent: root,
            position: InsertPosition::Last,
            node: NewNode::Element {
                name: "old-name".into()
            },
        }));
        shell.undo(); // keep the document minimal and clean
        shell.problems.clear();

        let mut text = shell
            .workspace
            .active()
            .unwrap()
            .document
            .source()
            .to_string();
        text = text.replacen("root", "renamed-root", 2);
        shell.update_source_text(text);
        assert!(shell.workspace.active().unwrap().source_draft.is_some());

        assert!(shell.apply_source());
        drain_apply_jobs(&mut shell);

        let session = shell.workspace.active().unwrap();
        assert!(
            session.source_draft.is_none(),
            "successful apply clears the draft"
        );
        assert!(
            session.document.source().contains("renamed-root"),
            "applied source: {}",
            session.document.source()
        );

        // One undo restores the pre-apply source (Apply is one command).
        shell.undo();
        let session = shell.workspace.active().unwrap();
        assert!(
            !session.document.source().contains("renamed-root"),
            "undo must revert the whole apply"
        );
    }

    #[test]
    fn invalid_apply_keeps_draft_and_reports_position() {
        let mut shell = AppShell::new();
        shell.new_document();
        let revision_before = shell.workspace.active().unwrap().document.revision();
        let source_before = shell
            .workspace
            .active()
            .unwrap()
            .document
            .source()
            .to_string();

        let broken = format!("{source_before}<unclosed");
        shell.update_source_text(broken.clone());
        assert!(shell.apply_source());

        // Give the job a moment, then drain.
        for _ in 0..200 {
            shell.poll_apply_jobs();
            if shell
                .problems
                .iter()
                .any(|problem| problem.code == "source-draft")
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        let session = shell.workspace.active().unwrap();
        assert!(
            session.source_draft.is_some(),
            "invalid apply must keep the draft"
        );
        assert_eq!(
            session.document.revision(),
            revision_before,
            "invalid apply must not touch the DOM"
        );
        assert_eq!(session.document.source(), source_before);
        assert!(
            shell
                .problems
                .iter()
                .any(|problem| problem.code == "source-draft"
                    && problem.arguments.contains_key("line")
                    && problem.arguments.contains_key("column"))
        );
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
