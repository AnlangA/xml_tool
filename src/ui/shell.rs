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
/// Pseudo-session for native file-dialog picks (run off the UI thread).
const DIALOG_SESSION: SessionId = SessionId(u64::MAX - 3);
/// Pseudo-session for file-reload jobs (watcher banner "Reload").
const RELOAD_SESSION: SessionId = SessionId(u64::MAX - 4);
/// Pseudo-session for heavy tool jobs (XPath/XSD/diff/EXI encode).
const TOOLS_SESSION: SessionId = SessionId(u64::MAX - 5);

/// Modal dialogs the shell can show.
pub enum Dialog {
    About,
    /// XPath query entry (expression buffer + optional node-set results).
    XPathQuery {
        expression: String,
        results: Option<Vec<crate::services::xpath::XPathNode>>,
    },
    /// EXI workbench: chosen preset plus the last report line.
    ExiWorkbench {
        preset: crate::services::exi_workbench::ExiPreset,
        report: Option<String>,
    },
    ConfirmDelete {
        node: NodeId,
        name: String,
        descendants: usize,
    },
    UnsavedExit,
    Recovery {
        snapshots: Vec<(u64, RecoverySnapshot)>,
    },
    Shortcuts,
}

/// Non-blocking banners: transient conditions the user should see
/// without a modal stealing focus. Rendered as a strip under the toolbar.
pub enum Banner {
    /// The file behind a session changed on disk.
    Reload {
        session: SessionId,
        path: PathBuf,
        dirty: bool,
    },
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
    pub banner: Option<Banner>,
    pub focus: FocusPane,
    pub alerts: crate::ui::alerts::AlertCenter,
    /// Whether the problems panel is expanded.
    pub problems_panel_open: bool,
    /// Pending jump target (1-based line, column) for the source pane,
    /// bound to its session so it never shows on the wrong tab.
    pub source_jump: Option<(SessionId, usize, usize)>,
    /// One-shot scroll request paired with the jump (consumed by the
    /// source pane after it scrolls the target line into view).
    pub pending_scroll: Option<(SessionId, usize, usize)>,
    /// One-shot outline scroll target (search-hit reveal).
    pub outline_scroll_to: Option<NodeId>,
    /// Search state for the active session.
    pub search: panels::SearchState,
    pub(crate) pending_new_attr: bool,
    pub(crate) pending_remove_attr: Option<String>,
    pub(crate) open_pending: usize,
    /// Unified document caches (search index, outline) with hit stats.
    pub cache: crate::services::session_cache::DocumentSessionCache,
    /// Frame section timings.
    pub frames: crate::services::frame_observer::FrameObserver,
    /// Last applied (theme mode, font scale, effective theme) triple.
    prefs_applied: Option<(ThemeMode, FontScale, egui::Theme)>,
    /// Fonts are installed once per context, not per frame.
    fonts_installed: bool,
    /// Narrow-layout drawers (outline closed by default on narrow windows).
    pub show_outline_drawer: bool,
    pub show_inspector_drawer: bool,
    /// Collapse the search strip inside the outline panel.
    pub outline_search_collapsed: bool,
    /// What the unsaved-changes dialog continues with when confirmed.
    pub(crate) after_unsaved: Option<AfterUnsaved>,
    /// Paths we saved recently: watcher events for these are our own
    /// writes, not external modifications.
    pub(crate) recent_saves: HashMap<PathBuf, std::time::Instant>,
    /// Last time crash-recovery snapshots were written.
    pub(crate) last_recovery_write: std::time::Instant,
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
            banner: None,
            focus: FocusPane::Outline,
            alerts: crate::ui::alerts::AlertCenter::default(),
            problems_panel_open: false,
            source_jump: None,
            pending_scroll: None,
            outline_scroll_to: None,
            search: panels::SearchState::default(),
            pending_new_attr: false,
            pending_remove_attr: None,
            open_pending: 0,
            cache: crate::services::session_cache::DocumentSessionCache::default(),
            frames: crate::services::frame_observer::FrameObserver::default(),
            prefs_applied: None,
            fonts_installed: false,
            show_outline_drawer: false,
            show_inspector_drawer: true,
            outline_search_collapsed: false,
            after_unsaved: None,
            recent_saves: HashMap::new(),
            last_recovery_write: std::time::Instant::now(),
        }
    }

    /// Re-runs the current query through the unified cache (the index is
    /// built on miss and reused on hit).
    pub(crate) fn refresh_search(&mut self) {
        let Some(session) = self.workspace.active() else {
            self.search.hits.clear();
            return;
        };
        if self.search.needle.is_empty() {
            self.search.hits.clear();
            return;
        }
        let revision = session.document.revision().0;
        let order = session.document.document_order().to_vec();
        let needle = self.search.needle.clone();
        let case = self.search.case_sensitive;
        let session_id = session.id;
        let workspace = &self.workspace;
        let document = &workspace.active().expect("checked above").document;
        let index = self.cache.search_index(session_id, revision, document);
        self.search.hits = index.search(&needle, case, &order);
        self.search.current = 0;
    }

    /// Batch-replaces every search hit with the replacement text as one
    /// atomic `BatchReplace` command (a single undo step).
    pub(crate) fn replace_all(&mut self) {
        if self.search.needle.is_empty() {
            return;
        }
        let Some(session) = self.workspace.active() else {
            return;
        };
        let session_id = session.id;
        let revision = session.document.revision().0;
        let order = session.document.document_order().to_vec();
        let needle = self.search.needle.clone();
        let replacement = self.search.replacement.clone();
        let case = self.search.case_sensitive;
        let workspace = &self.workspace;
        let document = &workspace.active().expect("checked above").document;
        let index = self.cache.search_index(session_id, revision, document);
        let previews = crate::services::replace::build_replacements(
            document,
            index,
            &order,
            &needle,
            &replacement,
            crate::services::replace::ReplaceScope::All,
            case,
        );
        match previews {
            Ok(previews) if previews.is_empty() => {}
            Ok(previews) => {
                let ops = crate::services::replace::to_ops(&previews);
                self.commit(Command::BatchReplace { ops });
                self.refresh_search();
            }
            Err(problem) => {
                self.push_problem(Severity::Error, "replace", problem);
            }
        }
    }

    /// F3 / Shift+F3: move to the next/previous hit and make it visible —
    /// expand collapsed ancestors, scroll the outline to the row, and
    /// select the node so the inspector follows.
    pub(crate) fn jump_to_search_hit(&mut self, next: bool) {
        if next {
            self.search.jump_next();
        } else {
            self.search.jump_previous();
        }
        let Some(hit) = self.search.selected_node() else {
            return;
        };
        let Some(session_id) = self.workspace.active_id() else {
            return;
        };
        // Expand every ancestor so the hit row exists in the outline.
        if let Some(session) = self.workspace.active() {
            let mut ancestors = Vec::new();
            let mut node = hit;
            while let Some(parent) = session.document.parent(node) {
                if parent == NodeId::DOCUMENT {
                    break;
                }
                ancestors.push(parent);
                node = parent;
            }
            self.expanded
                .entry(session_id)
                .or_default()
                .extend(ancestors);
        }
        if let Some(session) = self.workspace.active_mut() {
            session.selection = Some(hit);
        }
        self.outline_scroll_to = Some(hit);
    }

    /// Keeps search hits and jump targets consistent with the active
    /// session: edits or tab switches rebuild the search, and jump
    /// hints/scrolls bound to another (or a closed) tab are dropped.
    pub(crate) fn invalidate_session_scoped_state(&mut self) {
        let active_id = self.workspace.active_id();
        let active_revision = self
            .workspace
            .active()
            .map(|session| session.document.revision().0);
        if self.search.session != active_id || self.search.revision != active_revision {
            self.search.session = active_id;
            self.search.revision = active_revision;
            self.refresh_search();
        }
        if let Some((session, _, _)) = self.source_jump
            && Some(session) != active_id
        {
            self.source_jump = None;
        }
        if let Some((session, _, _)) = self.pending_scroll
            && Some(session) != active_id
        {
            self.pending_scroll = None;
        }
    }

    /// Runs one frame.
    pub fn update(&mut self, ctx: &Context) {
        self.apply_preferences_once(ctx);
        let started = std::time::Instant::now();
        self.poll_background_jobs();
        self.poll_file_watcher();
        self.maybe_snapshot_recovery();
        self.frames.record("background-poll", started.elapsed());
        // Search hits are session- and revision-scoped; switching tabs or
        // editing the document rebuilds them.
        self.invalidate_session_scoped_state();
        // Alerts stamped from here on belong to this session (jump-to-source
        // switches back to the owning tab).
        self.alerts.session = self.workspace.active_id();
        self.handle_shortcuts(ctx);

        if self.alerts.panel_requested {
            self.alerts.panel_requested = false;
            self.problems_panel_open = true;
        }
        let started = std::time::Instant::now();
        panels::top_bar(ctx, self);
        self.frames.record("top-bar", started.elapsed());
        panels::banner_strip(ctx, self);
        if self.workspace.active().is_some() {
            panels::document_tabs(ctx, self);
        }
        panels::bottom_panel(ctx, self);
        let started = std::time::Instant::now();
        self.layout_panels(ctx);
        self.frames.record("panels", started.elapsed());
        let started = std::time::Instant::now();
        panels::status_bar(ctx, self);
        self.frames.record("status-bar", started.elapsed());
        crate::ui::dialogs::dialogs(ctx, self);
    }

    fn apply_preferences_once(&mut self, ctx: &Context) {
        // Re-apply only when the preference (or the OS theme, in System
        // mode) actually changed — per-frame `set_zoom_factor` would fight
        // egui's built-in zoom gestures.
        let effective = ctx.theme();
        let key = (self.theme_mode, self.font_scale, effective);
        if self.prefs_applied != Some(key) {
            self.prefs_applied = Some(key);
            self.theme_mode.apply(ctx);
            self.font_scale.apply(ctx);
            apply_accent_theme(ctx);
        }
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

    /// Reloads an already-open session from disk (watcher banner action).
    /// Runs in a background job; on success the session's document,
    /// history, and caches are replaced wholesale.
    pub fn reload_session(&mut self, session: SessionId, path: PathBuf) {
        self.tasks
            .spawn(RELOAD_SESSION, crate::core::Revision(0), move |_| {
                let result = match std::fs::read(&path) {
                    Ok(bytes) => match classify_bytes(&bytes) {
                        Ok(outcome) => match XmlDocument::parse(&bytes) {
                            Ok(document) => Ok((DocumentMode::from(outcome.mode), document)),
                            Err(err) => Err((String::from("parse"), err.to_string())),
                        },
                        Err(err) => Err((String::from("too-large"), err.to_string())),
                    },
                    Err(err) => Err((String::from("io"), err.to_string())),
                };
                Box::new(ReloadJobResult {
                    session,
                    path,
                    result,
                })
            });
    }

    /// Creates a new untitled document.
    pub fn new_document(&mut self) {
        let session_id = self.workspace.add_untitled();
        self.expand_root_default(session_id);
    }

    /// Saves the active document in a background job. The bytes saved are
    /// those of the revision at spawn time; if the user keeps editing, the
    /// tab stays dirty when the job finishes. A pending source draft is
    /// what the user sees, so its text is what gets written.
    pub fn save_active(&mut self, path: Option<PathBuf>) -> bool {
        // Untitled document: Save behaves as Save As instead of no-op.
        if path.is_none()
            && self
                .workspace
                .active()
                .is_some_and(|session| session.path.is_none())
        {
            if self
                .workspace
                .active()
                .is_some_and(|session| session.mode != DocumentMode::LargeReadOnly)
            {
                self.pick_and_save_as();
                return true;
            }
            return false;
        }
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
        let source = session
            .source_draft
            .as_ref()
            .map(|draft| draft.buffer.clone())
            .unwrap_or_else(|| session.document.source().to_string());
        let encoding = session.document.encoding();
        // Save As re-targets the session; re-point the file watcher too.
        let retargeted = session.path.as_ref() != Some(&path);
        if retargeted {
            let old_path = session.path.clone();
            session.path = Some(path.clone());
            if let Some(watcher) = self.watcher.as_mut() {
                if let Some(old) = old_path {
                    watcher.unwatch(&old);
                }
                let _ = watcher.watch(&path);
            }
        }
        let job_path = path.clone();
        self.tasks
            .spawn(SAVE_SESSION, crate::core::Revision(0), move |_| {
                let bytes = crate::xml::encoding::encode_xml_text(&source, encoding);
                let outcome =
                    save_bytes_atomically(&job_path, &bytes).map_err(|err| err.to_string());
                Box::new(SaveJobResult {
                    session: session_id,
                    revision,
                    path,
                    outcome,
                })
            });
        true
    }

    /// Synchronously saves every dirty session that has a path. Used by the
    /// exit/close-tab flows, where a background job would not finish before
    /// the window closes. Returns the number of sessions that could not be
    /// saved (untitled documents without a path count as failures).
    pub(crate) fn save_dirty_sessions_sync(&mut self) -> usize {
        let mut failures = 0;
        for index in 0..self.workspace.sessions().len() {
            self.workspace.select(index);
            let Some(session) = self.workspace.active() else {
                continue;
            };
            if !session.is_dirty() || session.mode == DocumentMode::LargeReadOnly {
                continue;
            }
            let session_id = session.id;
            let Some(path) = session.path.clone() else {
                failures += 1;
                continue;
            };
            let source = session
                .source_draft
                .as_ref()
                .map(|draft| draft.buffer.clone())
                .unwrap_or_else(|| session.document.source().to_string());
            let bytes = crate::xml::encoding::encode_xml_text(&source, session.document.encoding());
            match save_bytes_atomically(&path, &bytes) {
                Ok(()) => {
                    if let Some(session) = self.workspace.active_mut() {
                        session.mark_saved();
                    }
                    self.recovery.remove(session_id.0);
                    self.recent_saves
                        .insert(canonical_path(&path), std::time::Instant::now());
                }
                Err(err) => {
                    failures += 1;
                    let text = self.localization.msg_with(
                        "error-io",
                        Some(&fluent_args!("message" => err.to_string())),
                    );
                    self.push_problem(Severity::Error, "io", text);
                }
            }
        }
        failures
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
                if let Some(session) = self.workspace.active() {
                    self.cache
                        .invalidate_revisions(session.id, session.document.revision().0);
                }
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
            self.cache
                .invalidate_revisions(session.id, session.document.revision().0);
        }
    }

    /// Whether the active session has undo history (menu + toolbar).
    pub(crate) fn can_undo(&self) -> bool {
        self.workspace
            .active()
            .is_some_and(|session| session.history.undo_depth() > 0)
    }

    /// Whether the active session has redo history (menu + toolbar).
    pub(crate) fn can_redo(&self) -> bool {
        self.workspace
            .active()
            .is_some_and(|session| session.history.redo_depth() > 0)
    }

    /// Redo on the active session.
    pub fn redo(&mut self) {
        if let Some(session) = self.workspace.active_mut() {
            session.history.redo(&mut session.document);
            self.cache
                .invalidate_revisions(session.id, session.document.revision().0);
        }
    }

    /// Closes the active tab, prompting when dirty.
    pub fn close_active_tab(&mut self) {
        if self
            .workspace
            .active()
            .is_some_and(|session| session.is_dirty())
        {
            self.after_unsaved = Some(AfterUnsaved::CloseTab);
            self.dialog = Some(Dialog::UnsavedExit);
            return;
        }
        self.close_tab_silent();
    }

    /// Requests application exit: closes immediately when clean, prompts
    /// via the unsaved-changes dialog when any session is dirty.
    pub fn request_exit(&mut self, ctx: &Context) {
        if self.workspace.dirty_sessions().is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            self.after_unsaved = Some(AfterUnsaved::Exit);
            self.dialog = Some(Dialog::UnsavedExit);
        }
    }

    fn close_tab_silent(&mut self) {
        if let Some(session) = self.workspace.active() {
            self.recovery.remove(session.id.0);
            self.cache.invalidate_session(session.id);
            self.expanded.remove(&session.id);
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
    }

    pub(crate) fn push_problem(&mut self, severity: Severity, code: &str, message: String) {
        self.alerts.push(severity, code, &message);
    }

    /// Records an alert with a source position (jump-to-line wiring).
    #[cfg(test)]
    pub(crate) fn push_problem_at(
        &mut self,
        severity: Severity,
        code: &str,
        message: String,
        position: (usize, usize),
    ) {
        self.alerts
            .push_with_position(severity, code, &message, Some(position));
    }

    // -----------------------------------------------------------------------
    // Background polling
    // -----------------------------------------------------------------------

    fn poll_background_jobs(&mut self) {
        self.poll_save_jobs();
        self.poll_apply_jobs();
        self.poll_dialog_picks();
        self.poll_reload_jobs();
        self.poll_tool_jobs();
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
                    file_type,
                    mode,
                    document,
                } => {
                    let session_id =
                        self.workspace
                            .add_opened(path.clone(), file_type, mode, document);
                    self.expand_root_default(session_id);
                    if let Some(watcher) = self.watcher.as_mut() {
                        let _ = watcher.watch(&path);
                    }
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

    /// Applies native file-dialog picks delivered by worker threads.
    fn poll_dialog_picks(&mut self) {
        while let Ok(Some(outcome)) = self
            .tasks
            .take_outcome(DIALOG_SESSION, crate::core::Revision(0))
        {
            let pick = *outcome
                .result
                .downcast::<DialogPick>()
                .expect("dialog pick payload type");
            match pick {
                DialogPick::Open(Some(path)) => self.open_path(path),
                DialogPick::SaveAs(Some(path)) => {
                    self.save_active(Some(path));
                }
                DialogPick::ValidateSchema(Some(path)) => self.spawn_validate_job(path),
                DialogPick::DiffWith(Some(path)) => self.spawn_diff_job(path),
                _ => {} // cancelled
            }
        }
    }

    /// Applies heavy tool results (XPath/XSD/diff/EXI) on the UI thread.
    fn poll_tool_jobs(&mut self) {
        while let Ok(Some(outcome)) = self
            .tasks
            .take_outcome(TOOLS_SESSION, crate::core::Revision(0))
        {
            let payload = *outcome
                .result
                .downcast::<ToolJobResult>()
                .expect("tool job payload type");
            self.handle_tool_result(payload);
        }
    }

    fn handle_tool_result(&mut self, payload: ToolJobResult) {
        match payload {
            ToolJobResult::XPath {
                expression,
                outcome,
            } => match outcome {
                Ok(XPathToolOutcome::Nodes(nodes)) => {
                    let count = nodes.len();
                    let text = self.localization.msg_with(
                        "xpath-result-nodes",
                        Some(&crate::fluent_args!("count" => count as i32)),
                    );
                    self.alerts.push_with_outline_node(
                        Severity::Info,
                        "xpath",
                        &text,
                        nodes.first().map(|n| n.node),
                    );
                    self.dialog = Some(Dialog::XPathQuery {
                        expression,
                        results: Some(nodes),
                    });
                }
                Ok(XPathToolOutcome::Text(value)) => {
                    self.push_problem(Severity::Info, "xpath", value);
                }
                Err(message) => {
                    self.push_problem(Severity::Error, "xpath", message);
                }
            },
            ToolJobResult::Validate { outcome } => match outcome {
                Ok(diagnostics) => {
                    if diagnostics.is_empty() {
                        self.push_problem(Severity::Info, "xsd", String::from("valid"));
                    }
                    for diagnostic in &diagnostics {
                        self.alerts.push_diagnostic(diagnostic);
                    }
                }
                Err(message) => {
                    self.push_problem(Severity::Error, "xsd", message);
                }
            },
            ToolJobResult::Diff { outcome } => match outcome {
                Ok(entries) if entries.is_empty() => {
                    self.push_problem(Severity::Info, "diff", String::from("identical"));
                }
                Ok(entries) => {
                    for entry in entries {
                        self.push_problem(Severity::Info, "diff", entry);
                    }
                }
                Err(message) => {
                    self.push_problem(Severity::Error, "diff", message);
                }
            },
            ToolJobResult::ExiEncode { preset, outcome } => match outcome {
                Ok(report) => {
                    let percent = (report.ratio * 100.0).round() as i32;
                    let preset_name = match preset {
                        crate::services::exi_workbench::ExiPreset::FidelityBitPacked => {
                            "exi-preset-fidelity"
                        }
                        crate::services::exi_workbench::ExiPreset::ByteAligned => "exi-preset-byte",
                        crate::services::exi_workbench::ExiPreset::PreCompression => {
                            "exi-preset-precompression"
                        }
                        crate::services::exi_workbench::ExiPreset::MaximumCompression => {
                            "exi-preset-max"
                        }
                    };
                    let text = self.localization.msg_with(
                        "exi-report",
                        Some(&crate::fluent_args!(
                            "preset" => self.localization.msg(preset_name),
                            "input" => report.input_bytes as i32,
                            "output" => report.output_bytes as i32,
                            "percent" => percent,
                            "ms" => report.duration_ms.round() as i32,
                        )),
                    );
                    self.dialog = Some(Dialog::ExiWorkbench {
                        preset,
                        report: Some(text),
                    });
                }
                Err(message) => {
                    self.push_problem(Severity::Error, "exi", message);
                }
            },
        }
    }

    /// Source snapshot of the active session (draft-aware), for workers.
    fn active_source_snapshot(&self) -> Option<String> {
        self.workspace.active().map(|session| {
            session
                .source_draft
                .as_ref()
                .map(|draft| draft.buffer.clone())
                .unwrap_or_else(|| session.document.source().to_string())
        })
    }

    fn spawn_validate_job(&mut self, schema_path: PathBuf) {
        let Some(snapshot) = self.active_source_snapshot() else {
            return;
        };
        self.tasks
            .spawn(TOOLS_SESSION, crate::core::Revision(0), move |_| {
                let outcome = (|| {
                    let validator = crate::services::validation::compile_schema(&schema_path)
                        .map_err(|err| err.to_string())?;
                    let document =
                        XmlDocument::parse(snapshot.as_bytes()).map_err(|err| err.to_string())?;
                    Ok(crate::services::validation::validate(&document, &validator))
                })();
                Box::new(ToolJobResult::Validate { outcome })
            });
    }

    fn spawn_diff_job(&mut self, other_path: PathBuf) {
        let Some(snapshot) = self.active_source_snapshot() else {
            return;
        };
        self.tasks
            .spawn(TOOLS_SESSION, crate::core::Revision(0), move |_| {
                let outcome = (|| {
                    let bytes = std::fs::read(&other_path).map_err(|err| err.to_string())?;
                    let left =
                        XmlDocument::parse(snapshot.as_bytes()).map_err(|err| err.to_string())?;
                    let right = XmlDocument::parse(&bytes).map_err(|err| err.to_string())?;
                    let entries = crate::services::diff::diff_xml(
                        &left,
                        &right,
                        crate::services::diff::DiffOptions::default(),
                    )?;
                    Ok(entries
                        .iter()
                        .map(|entry| {
                            let (tag, label) = match entry {
                                crate::services::diff::DiffEntry::Added { label } => ("+", label),
                                crate::services::diff::DiffEntry::Removed { label } => ("-", label),
                                crate::services::diff::DiffEntry::Modified { label, .. } => {
                                    ("~", label)
                                }
                                crate::services::diff::DiffEntry::Moved { label } => (">", label),
                            };
                            format!("{tag} {label}")
                        })
                        .collect::<Vec<_>>())
                })();
                Box::new(ToolJobResult::Diff { outcome })
            });
    }

    /// Applies file-reload results: the session's document is replaced
    /// wholesale (history/draft/caches reset — the user chose to reload).
    fn poll_reload_jobs(&mut self) {
        while let Ok(Some(outcome)) = self
            .tasks
            .take_outcome(RELOAD_SESSION, crate::core::Revision(0))
        {
            let payload = *outcome
                .result
                .downcast::<ReloadJobResult>()
                .expect("reload job payload type");
            match payload.result {
                Ok((mode, document)) => {
                    if let Some(session) = self
                        .workspace
                        .sessions_mut()
                        .iter_mut()
                        .find(|session| session.id == payload.session)
                    {
                        session.document = document;
                        session.mode = mode;
                        session.history.clear();
                        session.source_draft = None;
                        session.selection = None;
                        self.cache.invalidate_session(payload.session);
                        self.expanded.remove(&payload.session);
                        self.recovery.remove(payload.session.0);
                        self.recent_saves
                            .insert(canonical_path(&payload.path), std::time::Instant::now());
                    }
                }
                Err((code, message)) => {
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
                    self.recent_saves
                        .insert(canonical_path(&payload.path), std::time::Instant::now());
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
        // Own writes are not external modifications: suppress events for
        // paths we saved in the last few seconds.
        let now = std::time::Instant::now();
        self.recent_saves
            .retain(|_, at| now.duration_since(*at) < std::time::Duration::from_secs(3));
        for change in watcher.poll_changes() {
            let path = change.path().to_path_buf();
            let canonical = canonical_path(&path);
            if self.recent_saves.contains_key(&canonical) {
                continue;
            }
            let matching = self.workspace.sessions().iter().find(|session| {
                session
                    .path
                    .as_deref()
                    .is_some_and(|p| canonical_path(p) == canonical)
            });
            let Some(matching) = matching else {
                continue; // not an open document (stale watch entry)
            };
            let session_id = matching.id;
            let dirty = matching.is_dirty();
            self.banner = Some(Banner::Reload {
                session: session_id,
                path,
                dirty,
            });
        }
    }

    /// Writes crash-recovery snapshots for dirty sessions (throttled to
    /// once per 30 seconds; snapshots are removed on save/close/discard).
    fn maybe_snapshot_recovery(&mut self) {
        const INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);
        if self.last_recovery_write.elapsed() < INTERVAL {
            return;
        }
        self.last_recovery_write = std::time::Instant::now();
        for session in self.workspace.sessions() {
            if !session.is_dirty() {
                continue;
            }
            // The visible text is what a crash should restore.
            let source = session
                .source_draft
                .as_ref()
                .map(|draft| draft.buffer.clone())
                .unwrap_or_else(|| session.document.source().to_string());
            let selection_path = session
                .selection
                .and_then(|node| crate::services::outline::node_path(&session.document, node));
            let snapshot = RecoverySnapshot {
                title: session.display_name(),
                path: session.path.clone(),
                source,
                dirty: true,
                cursor: 0,
                selection_path,
                written_at: 0, // stamped by the store
            };
            let _ = self.recovery.write(session.id.0, &snapshot);
        }
    }

    // -----------------------------------------------------------------------
    // Layout
    // -----------------------------------------------------------------------

    fn layout_panels(&mut self, ctx: &Context) {
        let width = ctx.content_rect().width();
        let inspector_inline = width >= 1100.0;
        let outline_inline = width >= 850.0;
        // Empty workspace: side panels and drawers stay hidden, the welcome
        // view owns the whole window.
        let has_document = self.workspace.active().is_some();

        if has_document && outline_inline {
            crate::ui::outline::outline_panel(ctx, self);
        }
        if has_document && inspector_inline {
            panels::inspector_panel(ctx, self);
        }
        panels::central_panel(ctx, self);
        // Narrow windows: panels become overlay drawers (drawn after and
        // above the central panel).
        if has_document && !outline_inline && self.show_outline_drawer {
            crate::ui::outline::outline_drawer(ctx, self);
        }
        if has_document && !inspector_inline && self.show_inspector_drawer {
            panels::inspector_drawer(ctx, self);
        }
    }

    /// Cached outline snapshot (rows + element count) for the active
    /// session under the current expansion state.
    pub(crate) fn outline_snapshot(&mut self) -> (Arc<crate::services::outline::FlatTree>, usize) {
        let Some(session) = self.workspace.active() else {
            return (Arc::new(crate::services::outline::FlatTree::default()), 0);
        };
        let expanded = self.expanded.get(&session.id).cloned().unwrap_or_default();
        let expansion = crate::services::session_cache::expansion_digest(&expanded);
        let session_id = session.id;
        let revision = session.document.revision().0;
        let workspace = &self.workspace;
        let document = &workspace.active().expect("checked above").document;
        let entry = self
            .cache
            .outline(session_id, revision, expansion, document, &expanded);
        (Arc::clone(&entry.tree), entry.elements)
    }

    /// Expands the root element for a session (first-open default).
    pub(crate) fn expand_root_default(&mut self, session: SessionId) {
        if let Some(session_ref) = self.workspace.sessions().iter().find(|s| s.id == session)
            && let Some(root) = session_ref.document.root_element()
        {
            self.expanded.entry(session).or_default().insert(root);
        }
    }

    /// Reveals `node` in the outline: ancestors expanded, row scrolled, inspector focused.
    pub(crate) fn reveal_outline_node(&mut self, node: NodeId) {
        let Some(session_id) = self.workspace.active_id() else {
            return;
        };
        if let Some(session) = self.workspace.active() {
            let mut ancestors = Vec::new();
            let mut current = node;
            while let Some(parent) = session.document.parent(current) {
                if parent == NodeId::DOCUMENT {
                    break;
                }
                ancestors.push(parent);
                current = parent;
            }
            self.expanded
                .entry(session_id)
                .or_default()
                .extend(ancestors);
            if let Some(range) = session.document.source_range(node) {
                self.source_jump = Some((session_id, range.start_line, range.start_column));
                self.pending_scroll = Some((session_id, range.start_line, range.start_column));
            }
        }
        if let Some(session) = self.workspace.active_mut() {
            session.selection = Some(node);
        }
        self.outline_scroll_to = Some(node);
        self.focus = FocusPane::Inspector;
    }

    pub(crate) fn toggle_expanded(&mut self, session: SessionId, node: NodeId) {
        let Some(document) = self
            .workspace
            .sessions()
            .iter()
            .find(|s| s.id == session)
            .map(|s| &s.document)
        else {
            return;
        };
        let set = self.expanded.entry(session).or_default();
        crate::services::outline::FlatTree::toggle(document, set, node);
    }

    pub(crate) fn expand_all(&mut self, session: SessionId) {
        let Some(current) = self.workspace.sessions().iter().find(|s| s.id == session) else {
            return;
        };
        self.expanded.insert(
            session,
            crate::services::outline::FlatTree::collect_expandable(&current.document),
        );
    }

    pub(crate) fn collapse_all(&mut self, session: SessionId) {
        self.expanded.insert(session, HashSet::new());
    }

    /// Element count for the status bar.
    pub(crate) fn outline_elements(&mut self) -> usize {
        self.outline_snapshot().1
    }

    /// Whether tree edits are allowed on the active session.
    pub(crate) fn tree_edits_enabled(&self) -> bool {
        self.workspace.active().is_some_and(|session| {
            session.mode == DocumentMode::Editable && session.source_draft.is_none()
        })
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

        // While a text field owns the keyboard, Ctrl+Z/Y belong to its
        // built-in editing undo — the shell must not consume them.
        let text_editing = ctx.wants_keyboard_input();

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
        if !text_editing && wants(undo) {
            self.undo();
        }
        if !text_editing && wants(redo) {
            self.redo();
        }
        if wants(find) {
            self.search.open = true;
            self.search.focus_pending = true;
            self.focus = FocusPane::Outline;
        }
        if wants(replace) {
            // Batch replace UI arrives with step 7; find is the entry point.
            self.search.open = true;
            self.search.focus_pending = true;
            self.focus = FocusPane::Outline;
        }
        if wants(KeyboardShortcut::new(Modifiers::NONE, Key::F3)) {
            self.jump_to_search_hit(true);
        }
        if wants(KeyboardShortcut::new(Modifiers::SHIFT, Key::F3)) {
            self.jump_to_search_hit(false);
        }
        if wants(KeyboardShortcut::new(Modifiers::NONE, Key::F6)) {
            self.focus = match self.focus {
                FocusPane::Outline => FocusPane::Source,
                FocusPane::Source => FocusPane::Inspector,
                FocusPane::Inspector => FocusPane::Problems,
                FocusPane::Problems => FocusPane::Outline,
            };
            // Make the cycle visible: moving to the source pane focuses the
            // editor; moving to the outline focuses the search field when open.
            match self.focus {
                FocusPane::Source => {
                    if let Some(session) = self.workspace.active() {
                        let id = egui::Id::new((
                            "source",
                            session.document.revision().0,
                            session.source_draft.is_some(),
                        ));
                        ctx.memory_mut(|memory| memory.request_focus(id));
                    }
                }
                FocusPane::Outline if self.search.open => {
                    self.search.focus_pending = true;
                }
                _ => {}
            }
        }
        if wants(KeyboardShortcut::new(Modifiers::NONE, Key::F1)) {
            self.dialog = Some(Dialog::Shortcuts);
        }
        if self.focus == FocusPane::Outline && !text_editing {
            crate::ui::outline::handle_keyboard(ctx, self);
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
        let draft_text = draft.buffer.clone();
        let session_id = session.id;
        self.tasks
            .spawn(APPLY_SESSION, crate::core::Revision(0), move |_| {
                Box::new(ApplyJobResult {
                    session: session_id,
                    base_revision,
                    outcome: crate::services::source_editor::parse_draft(&draft_text),
                    draft_text,
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
            Some(draft) => draft.buffer = text,
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
        if session
            .source_draft
            .as_ref()
            .map(|draft| draft.buffer.clone())
            .as_deref()
            != Some(payload.draft_text.as_str())
        {
            return; // draft edited (or discarded) after Apply: keep typing
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
                        self.cache
                            .invalidate_revisions(session.id, session.document.revision().0);
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
                self.alerts.push_diagnostic(&diagnostic);
            }
        }
    }

    /// Opens the EXI workbench dialog for the active document.
    pub fn open_exi_workbench(&mut self) {
        self.dialog = Some(Dialog::ExiWorkbench {
            preset: crate::services::exi_workbench::ExiPreset::FidelityBitPacked,
            report: None,
        });
    }

    /// Encodes the active document (source snapshot) with the chosen preset
    /// on a worker thread; the report reopens the workbench dialog.
    pub fn exi_encode_current(&mut self, preset: crate::services::exi_workbench::ExiPreset) {
        let Some(snapshot) = self.active_source_snapshot() else {
            return;
        };
        let settings = crate::services::exi_workbench::ExiSettings::preset(preset);
        if !settings.dropped_items().is_empty() {
            let items = settings.dropped_items().join(", ");
            self.push_problem(
                Severity::Warning,
                "exi-fidelity",
                self.localization.msg_with(
                    "exi-fidelity-warning",
                    Some(&crate::fluent_args!("items" => items.as_str())),
                ),
            );
        }
        self.tasks
            .spawn(TOOLS_SESSION, crate::core::Revision(0), move |_| {
                let outcome =
                    crate::services::exi_workbench::encode_with_settings(&snapshot, &settings)
                        .map(|(_bytes, report)| report);
                Box::new(ToolJobResult::ExiEncode { preset, outcome })
            });
    }

    /// Opens the XPath query dialog.
    pub fn run_xpath_dialog(&mut self) {
        self.dialog = Some(Dialog::XPathQuery {
            expression: String::new(),
            results: None,
        });
    }

    /// Executes an XPath expression on a worker thread; results land in
    /// the Problems panel when the job reports back.
    pub fn execute_xpath(&mut self, expression: &str) {
        let Some(snapshot) = self.active_source_snapshot() else {
            return;
        };
        let expression = expression.to_string();
        self.tasks
            .spawn(TOOLS_SESSION, crate::core::Revision(0), move |_| {
                let outcome = (|| {
                    let document =
                        XmlDocument::parse(snapshot.as_bytes()).map_err(|err| err.to_string())?;
                    crate::services::xpath::query(&document, &expression)
                        .map(|outcome| match outcome {
                            crate::services::xpath::XPathOutcome::NodeSet(nodes) => {
                                XPathToolOutcome::Nodes(nodes)
                            }
                            crate::services::xpath::XPathOutcome::String(value) => {
                                XPathToolOutcome::Text(value)
                            }
                            crate::services::xpath::XPathOutcome::Number(value) => {
                                XPathToolOutcome::Text(value.to_string())
                            }
                            crate::services::xpath::XPathOutcome::Boolean(value) => {
                                XPathToolOutcome::Text(value.to_string())
                            }
                        })
                        .map_err(|err| err.to_string())
                })();
                Box::new(ToolJobResult::XPath {
                    expression,
                    outcome,
                })
            });
    }

    /// XSD validation: pick a schema (off-thread), then compile + validate
    /// on a worker; diagnostics land in the Problems panel.
    pub fn run_validation_dialog(&mut self) {
        if self.workspace.active().is_none() {
            return;
        }
        #[cfg(target_os = "macos")]
        {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("XSD", &["xsd"])
                .pick_file()
            {
                self.spawn_validate_job(path);
            }
        }
        #[cfg(not(target_os = "macos"))]
        self.tasks
            .spawn(DIALOG_SESSION, crate::core::Revision(0), |_| {
                let picked = rfd::FileDialog::new()
                    .add_filter("XSD", &["xsd"])
                    .pick_file();
                Box::new(DialogPick::ValidateSchema(picked))
            });
    }

    /// Structural diff against a file on disk; picker, parse, and diff all
    /// run off the UI thread.
    pub fn run_diff_dialog(&mut self) {
        if self.workspace.active().is_none() {
            return;
        }
        #[cfg(target_os = "macos")]
        {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("XML", &["xml"])
                .pick_file()
            {
                self.spawn_diff_job(path);
            }
        }
        #[cfg(not(target_os = "macos"))]
        self.tasks
            .spawn(DIALOG_SESSION, crate::core::Revision(0), |_| {
                let picked = rfd::FileDialog::new()
                    .add_filter("XML", &["xml"])
                    .pick_file();
                Box::new(DialogPick::DiffWith(picked))
            });
    }

    pub fn pick_and_open(&mut self) {
        // The native dialog blocks for as long as it stays open; on the UI
        // thread that froze the whole app (a multi-second "top-bar" frame).
        // Run it on a worker and deliver the pick through the task manager.
        // (macOS requires panels on the main thread — keep it synchronous.)
        #[cfg(target_os = "macos")]
        {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("XML", &["xml"])
                .add_filter("EXI", &["exi", "bin"])
                .pick_file()
            {
                self.open_path(path);
            }
        }
        #[cfg(not(target_os = "macos"))]
        self.tasks
            .spawn(DIALOG_SESSION, crate::core::Revision(0), |_| {
                let picked = rfd::FileDialog::new()
                    .add_filter("XML", &["xml"])
                    .add_filter("EXI", &["exi", "bin"])
                    .pick_file();
                Box::new(DialogPick::Open(picked))
            });
    }

    pub fn pick_and_save_as(&mut self) {
        #[cfg(target_os = "macos")]
        {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("XML", &["xml"])
                .set_file_name("document.xml")
                .save_file()
            {
                self.save_active(Some(path));
            }
        }
        #[cfg(not(target_os = "macos"))]
        self.tasks
            .spawn(DIALOG_SESSION, crate::core::Revision(0), |_| {
                let picked = rfd::FileDialog::new()
                    .add_filter("XML", &["xml"])
                    .set_file_name("document.xml")
                    .save_file();
                Box::new(DialogPick::SaveAs(picked))
            });
    }
}

/// Result of a native file dialog running off the UI thread.
pub enum DialogPick {
    Open(Option<PathBuf>),
    SaveAs(Option<PathBuf>),
    /// XSD schema picker for validation.
    ValidateSchema(Option<PathBuf>),
    /// XML file picker for structural diff.
    DiffWith(Option<PathBuf>),
}

/// Result of a heavy tool job (runs on a worker thread over a source
/// snapshot; the UI thread only formats and presents).
pub enum ToolJobResult {
    XPath {
        expression: String,
        outcome: Result<XPathToolOutcome, String>,
    },
    Validate {
        outcome: Result<Vec<Diagnostic>, String>,
    },
    Diff {
        outcome: Result<Vec<String>, String>,
    },
    ExiEncode {
        preset: crate::services::exi_workbench::ExiPreset,
        outcome: Result<crate::services::exi_workbench::ExiReport, String>,
    },
}

/// UI-ready XPath result.
pub enum XPathToolOutcome {
    Nodes(Vec<crate::services::xpath::XPathNode>),
    Text(String),
}

/// Payload of a file-reload job (watcher banner "Reload").
pub struct ReloadJobResult {
    pub session: SessionId,
    pub path: PathBuf,
    #[allow(clippy::type_complexity)]
    pub result: Result<(DocumentMode, XmlDocument), (String, String)>,
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
    /// The draft text that was parsed; results apply only when the draft
    /// still holds exactly this text (keystrokes after "Apply" win).
    pub draft_text: String,
    pub outcome: crate::services::source_editor::ApplyOutcome,
}

/// Payload of the background save job.
pub struct SaveJobResult {
    pub session: SessionId,
    pub revision: crate::core::Revision,
    pub path: PathBuf,
    pub outcome: Result<(), String>,
}

/// Canonicalizes for path comparisons (watcher events are canonical,
/// session paths may not be); falls back to the input on error.
pub(crate) fn canonical_path(path: &std::path::Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// What to do after the unsaved-changes dialog completes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterUnsaved {
    /// Close the whole window.
    Exit,
    /// Close only the active tab.
    CloseTab,
}

/// Payload of the background open job.
#[allow(clippy::large_enum_variant)]
pub enum OpenJobResult {
    Opened {
        path: PathBuf,
        file_type: FileType,
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
    // EXI binaries decode to XML text first, then take the XML path.
    let is_exi = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exi") || ext.eq_ignore_ascii_case("bin"));
    let (file_type, xml_bytes) = if is_exi {
        let decoded = erxi::decoder::decode(&bytes)
            .and_then(|(events, _)| erxi::xml_serializer::events_to_xml(&events));
        match decoded {
            Ok(xml) => (FileType::Exi, xml.into_bytes()),
            Err(err) => {
                return OpenJobResult::Failed {
                    code: String::from("parse"),
                    message: format!("EXI decode failed: {err}"),
                };
            }
        }
    } else {
        (FileType::Xml, bytes)
    };
    let outcome = match classify_bytes(&xml_bytes) {
        Ok(outcome) => outcome,
        Err(err) => {
            return OpenJobResult::Failed {
                code: String::from("too-large"),
                message: err.to_string(),
            };
        }
    };
    // EXI sessions are views of a binary payload: always read-only (saving
    // XML text over the .exi file would silently destroy the encoding).
    let mode = if file_type == FileType::Exi {
        crate::services::document_io::OpenMode::LargeReadOnly
    } else {
        outcome.mode
    };
    match XmlDocument::parse(&xml_bytes) {
        Ok(document) => OpenJobResult::Opened {
            path,
            file_type,
            mode: DocumentMode::from(mode),
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
                .alerts
                .alerts()
                .iter()
                .any(|alert| alert.code == "draft-active")
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
        shell.alerts.clear();

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
                .alerts
                .alerts()
                .iter()
                .any(|alert| alert.code == "source-draft")
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
                .alerts
                .alerts()
                .iter()
                .any(|alert| alert.code == "source-draft" && alert.position.is_some())
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

#[cfg(test)]
mod alert_tests {
    use super::*;

    #[test]
    fn errors_open_the_problems_panel_once() {
        let mut shell = AppShell::new();
        assert!(!shell.problems_panel_open);
        shell.alerts.push(Severity::Error, "io", "boom");
        assert!(shell.alerts.panel_requested);
        let ctx = Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| shell.update(ctx));
        assert!(
            shell.problems_panel_open,
            "an arriving error must expand the panel"
        );
        assert!(!shell.alerts.panel_requested, "consumed after opening");
        shell.alerts.panel_requested = true; // simulate a repeat
        shell.alerts.panel_requested = false;
        shell.problems_panel_open = false;
        shell.alerts.push(Severity::Info, "info", "quiet");
        let ctx = Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| shell.update(ctx));
        assert!(
            !shell.problems_panel_open,
            "info alerts never force the panel"
        );
    }

    #[test]
    fn repeated_alerts_share_one_row() {
        let mut shell = AppShell::new();
        for _ in 0..5 {
            shell.push_problem(Severity::Warning, "exi-fidelity", "drops comments".into());
        }
        assert_eq!(shell.alerts.len(), 1);
        assert_eq!(shell.alerts.alerts()[0].count, 5);
        shell.alerts.clear_code("exi-fidelity");
        assert!(shell.alerts.is_empty());
    }

    #[test]
    fn source_apply_positions_become_jump_targets() {
        let mut shell = AppShell::new();
        shell.push_problem_at(
            Severity::Error,
            "source-draft",
            "mismatched tag".into(),
            (3, 7),
        );
        let alert = &shell.alerts.alerts()[0];
        assert_eq!(alert.position, Some((3, 7)));
        // The panel turns positions into session-bound jump targets.
        let owner = alert.session.unwrap_or(SessionId(0));
        shell.source_jump = alert.position.map(|(line, column)| (owner, line, column));
        shell.focus = FocusPane::Source;
        assert_eq!(shell.source_jump, Some((owner, 3, 7)));
        assert_eq!(shell.focus, FocusPane::Source);
    }

    #[test]
    fn f3_reveal_expands_ancestors_selects_and_marks_scroll() {
        let mut shell = AppShell::new();
        shell.new_document();
        let session_id = shell.workspace.active_id().expect("active");
        assert!(shell.commit(Command::ReplaceWholeSource {
            new_source: String::from("<r><a><needle/></a></r>"),
        }));

        shell.search.needle = String::from("needle");
        shell.refresh_search();
        assert_eq!(shell.search.hits.len(), 1);

        // Outline starts fully collapsed: the hit row is not even rendered.
        shell.jump_to_search_hit(true);
        let hit = shell.search.selected_node().expect("hit");
        let session = shell.workspace.active().expect("active");
        assert_eq!(session.selection, Some(hit), "jump selects the hit node");
        let expanded = shell.expanded.get(&session_id).expect("expansion state");
        let root = session.document.root_element().expect("root");
        let a = shell
            .workspace
            .active()
            .expect("active")
            .document
            .children(root)[0];
        assert!(expanded.contains(&root), "root expanded");
        assert!(expanded.contains(&a), "middle ancestor expanded");
        assert_eq!(
            shell.outline_scroll_to,
            Some(hit),
            "outline scrolls to the revealed row"
        );
    }

    #[test]
    fn jump_hints_clear_when_switching_sessions() {
        let mut shell = AppShell::new();
        shell.new_document();
        let first = shell.workspace.active_id().expect("first");
        shell.source_jump = Some((first, 4, 2));
        shell.new_document();
        let second = shell.workspace.active_id().expect("second");
        assert_ne!(first, second);
        // The hint belongs to the first tab: it must not follow the switch.
        shell.invalidate_session_scoped_state();
        assert!(shell.source_jump.is_none(), "stale jump hint cleared");
    }
}
