# Changelog

## Unreleased

### UI overhaul
- Dynamic light/dark palettes: all custom widgets resolve Catppuccin
  Latte/Mocha per frame instead of hard-coded dark colors.
- Source view renders live XML syntax highlighting (disabled above 200k
  characters to keep huge documents responsive).
- Modernized shell: single-row menu+toolbar header, restyled document
  tabs, full-row outline selection with accent bar, rounded hover states
  in the problems panel, panel headings with icons.
- Empty workspace shows a centered welcome view (New/Open actions); side
  panels and the tab strip stay hidden until a document is open.
- Toolbar disables document actions when no session is active; tooltips
  show keyboard shortcuts.

### Fixes
- Encoding declarations in single quotes are now verified (mismatches
  were silently ignored).
- Undo of top-level node deletions (comments/PIs beside the root) works;
  failed undo/redo no longer drops the history entry.
- PI edits preserve `<?pi?>` without inventing empty data.
- Unsaved-changes dialog actually completes the requested action (save &
  exit/close, discard & exit/close) instead of just closing the dialog.
- Saving writes the visible source (including a pending draft); our own
  saves no longer trigger the "file changed on disk" banner; Save As
  re-points the file watcher at the new path.
- Apply-source results no longer overwrite keystrokes typed while the
  parse job was running; Shift+F3 with no hits no longer underflows;
  unterminated CDATA no longer hides characters in the source view.
- Ctrl+Z/Y inside text fields now reach the field's own undo; search hits
  are rebuilt when switching tabs; the delete confirmation counts the
  node's subtree instead of the whole document.
- Closing a tab frees its caches; background-job bookkeeping no longer
  grows unbounded; concurrent saves of one file use unique temp names.

### Fixes (round 2)
- Inspector fields (rename, attributes, text/comment bodies) now commit on
  Enter / focus loss via persistent edit buffers — no more per-keystroke
  commits (which spammed error alerts on invalid intermediate names), and
  multiline text editing actually keeps what you type.
- The problems panel's jump-to-source now scrolls the target line into
  view; Ctrl+F / F6 move keyboard focus to the search field / source
  editor.
- Crash-recovery snapshots are actually written (every 30 s for dirty
  sessions) — previously the store only ever read/deleted them.
- Case-insensitive large-file search over non-ASCII text no longer drops
  every match (folded→source byte offsets are mapped explicitly).
- Parse-error locations with a zero line or column no longer surface as
  half-valid positions.
- Read-only (large) documents render the source without a per-frame full
  copy; the source draft is a plain `String` instead of a rope rebuilt on
  every keystroke.
- Legacy `Theme::SKY` constant had the wrong RGB (duplicated BLUE).
- Document index rebuild no longer walks the DOM twice; dead code
  (`OutlineCache`, unused search index slot, unreachable drawer branches)
  removed; toolbar/menu undo-redo enablement unified.

### Fixes (round 3)
- The window close button now routes through the unsaved-changes dialog
  instead of discarding edits; saving an untitled document falls back to
  Save As instead of silently doing nothing.
- The file-changed banner's Reload actually reloads the session
  (previously a no-op that could even open a duplicate tab); path
  comparisons are canonicalized everywhere.
- Crash-recovery restore marks sessions dirty, keeps untitled snapshots
  pathless, and deletes consumed snapshots (no more repeat prompts).
- `.exi` files open as decoded, read-only XML views (previously every
  EXI open failed with a parse error).
- XPath/XSD/diff/EXI encode and their file pickers run on worker threads —
  no more multi-second UI freezes on large documents.
- Structural diff: attribute/text keys no longer collide across field
  boundaries; swapping two siblings reports two moves instead of marking
  the whole document moved.
- Problems-panel jumps switch to the alert's owning tab first.
- Find panel gains a batch-replace row (one atomic undoable
  `BatchReplace`), matching the documented Ctrl+H shortcut.
- XPath dialog auto-focuses its field and Enter runs the query.
- Zoom/theme preferences apply on change only (no longer fight egui's
  built-in zoom gestures); the Chinese bundle uses a `zh-CN` langid.
- Removed the unused `parking_lot`/`lru`/`thiserror` dependencies, dead
  icon aliases, and stale `.ftl` keys; README claims trimmed to what the
  UI actually exposes.

### Cleanup
- Removed the unused `egui_extras` dependency and the legacy
  `ui::xml_tree`/`cache` modules (~840 lines of dead code); the tree
  search benchmark now measures `services::search::SearchIndex`.

## 0.3.0 — Professional upgrade

Complete re-architecture from the 0.2.0 demo editor to a professional
desktop XML/EXI tool. Implemented per `AGENTS_PLAN.md` (steps 0–10);
per-stage details in `docs/BASELINE-step-*.md`.

### XML engine
- `uppsala 0.9.0` (pinned) replaces `quick-xml`: arena DOM, namespaces,
  XPath 1.0, XSD validation, source ranges, streaming parser.
- Byte-level input: BOM + Appendix F detection for UTF-8/UTF-16LE/BE;
  unsupported declared encodings and encoding mismatches are structured
  errors.
- Fidelity: unedited saves replay the original bytes exactly (BOM, CRLF,
  quoting, entity references); edited documents re-encode in the original
  encoding.
- Security: 256 MiB open limit, 512 depth cap, 16 MiB entity budget,
  external entities never loaded, zero network surface; Billion-Laughs
  contained.

### Document model and editing
- Command-based editing (13 commands) with validate-then-commit semantics,
  reverse commands, and `ChangedSet` cache invalidation; failed commands
  change nothing.
- Incremental history (no snapshots): 1,000 entries / 64 MiB budget,
  750 ms same-field coalescing, saved-cursor dirty tracking.
- Minimal source splicing: edits touch only the affected byte range.

### Workspace and I/O
- Multi-tab sessions, same-path focus, `Untitled-N`, Save As, Save All.
- Background tasks with `JobId+SessionId+Revision` staleness filtering and
  cancellation; opens, saves, and source-draft applies never block the UI
  thread. Atomic saves (temp file → sync → rename) leave originals intact
  on failure.
- Crash-recovery snapshots for dirty documents; external-change monitoring
  (500 ms debounce) with Reload/Keep banners.
- Large-file mode: >20 MiB or >200,000 elements opens read-only (browse,
  literal search, jump, path copy, subtree export, save-as).

### UI
- New shell: menus, toolbar, document tabs, outline (virtualized), source
  editor with draft workflow (Apply Source / Discard Draft; tree edits
  locked while a draft exists), inspector, problems panel, status bar.
- Chinese/English localization (Fluent), system-following on first start;
  System/Light/Dark themes; 80–180% font scaling; bundled Noto Sans SC;
  Phosphor icons; keyboard-complete main flows; 12-combination UI
  snapshots.
- Bidirectional tree/source editing: Apply commits one undoable
  `ReplaceWholeSource`; invalid drafts keep their text and report exact
  line/column.

### Pro toolkit
- XPath 1.0 (namespace-aware, root-visible bindings auto-registered),
  XSD validation (schema-root-confined includes/imports, compiled-schema
  cache), structural diff (similar; Added/Removed/Modified/Moved;
  formatting-only changes ignored; 5 s deadline), batch replace with
  whole-batch validation and single-step undo, lossless JSON export
  (legacy mapping preserved).

### EXI workbench
- All EXI options surfaced with four fixed presets, pre-run conflict
  validation, fidelity warnings, reports (bytes, ratio, timing,
  throughput, effective options, SHA-256, dropped items), 512/256 MiB
  budgets, and structured errors on malformed streams.

### Performance and quality
- Unified session caches (128 MiB LRU, revision-keyed, hit statistics);
  frame observer (>16 ms debug, >50 ms warning); expanded benchmarks
  (20 MiB parse/serialize, 200k search, incremental edits, flat tree,
  visible-line highlight, four EXI presets).
- Property tests (random legal trees parse/serialize/undo) and robustness
  suites across every input surface; measured release performance: 20 MiB
  open 93 ms, 200k first search 131 ms, cached 45 ns.
- CI on Ubuntu 24.04 / Windows Server 2025 / macOS 15 with audit + deny;
  MSRV Rust 1.88.

### Licensing note
The EXI backend (erxi, commit `4148209c`) is PolyForm-Noncommercial:
binary distributions of xml_tool inherit that restriction for the EXI
feature. See README "Licensing".

## 0.2.0
Demo XML/EXI viewer/editor.
