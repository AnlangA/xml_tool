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
