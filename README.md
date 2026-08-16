# XML Tool

A professional XML/EXI desktop editor: fidelity-preserving XML editing,
XPath/XSD/diff tooling, a full EXI workbench, and Chinese/English
localization. Built with Rust and egui.

Current version: **0.3.0**. The roadmap and per-stage engineering records
live in [`AGENTS_PLAN.md`](AGENTS_PLAN.md) and
[`docs/BASELINE-step-*.md`](docs).

## Features

Every feature below has automated coverage (test file in parentheses).

- **Fidelity XML engine** — UTF-8/UTF-16 (BOM and Appendix F), declaration
  checking, namespaces, CDATA, comments, PIs, DOCTYPE, internal entities,
  mixed content. Unedited saves are byte-identical to the input
  (`tests/engine_acceptance_tests.rs`).
- **Safe by construction** — 256 MiB open cap, 512 depth limit, 16 MiB
  entity budget (Billion-Laughs contained), external entities never
  loaded, zero network code (`tests/engine_acceptance_tests.rs`,
  `tests/property_fuzz_tests.rs`).
- **Command-based editing** — 13 atomic commands with undo/redo, bounded
  history (1,000 entries / 64 MiB), 750 ms typing coalescing; failed
  commands change nothing; edits touch only the affected byte range
  (`tests/command_history_tests.rs`).
- **Multi-tab workspace** — background opens/saves with revision-guarded
  results, atomic saves, crash-recovery snapshots, external-change
  detection with Reload/Keep (`tests/services_tests.rs`).
- **Large-file mode** — over 20 MiB or 200,000 elements opens read-only
  (browse, literal search, jump, path copy, subtree export, save-as);
  measured: 20 MiB open in ~93 ms, 200k-node first search ~131 ms, cached
  ~45 ns, memory within budget (`tests/large_file_tests.rs`,
  `tests/memory_budget_tests.rs`).
- **Bidirectional editing** — source editor with a draft workflow: Apply
  commits one undoable whole-source replacement; invalid drafts report
  exact line/column and keep your text; tree edits lock while a draft is
  open (`src/ui/shell.rs` tests, `src/services/source_editor.rs` tests).
- **XPath 1.0** — namespace-aware with automatic root-visible bindings;
  node-set and typed scalar results (`tests/pro_toolkit_tests.rs`).
- **XSD validation** — local schemas, includes/imports confined to the
  schema's own directory tree, compiled-schema cache, positioned
  diagnostics (`tests/pro_toolkit_tests.rs`).
- **Structural diff** — Added/Removed/Modified/Moved; formatting-only
  changes ignored; 5-second deadline (`tests/pro_toolkit_tests.rs`).
- **Batch replace** — literal search with scope and case folding;
  whole-batch validation (any illegal replacement rejects the batch);
  1,000 hits preview → one apply → one undo (`tests/pro_toolkit_tests.rs`).
- **JSON export** — legacy mapping plus a lossless ordered mode preserving
  every node kind (`tests/pro_toolkit_tests.rs`).
- **EXI workbench** — four presets and all EXI options, pre-run conflict
  validation, fidelity warnings, full reports (bytes/ratio/timing/
  throughput/effective options/SHA-256/dropped items), malformed-stream
  robustness (`tests/exi_workbench_tests.rs`).
- **Chinese/English UI** — Fluent localization with enforced key parity,
  system-language first start, Light/Dark/System themes, 80–180% font
  scaling, bundled Noto Sans SC, 12 UI snapshot combinations
  (`tests/ui_shell_tests.rs`).

### Keyboard map

| Shortcut | Action |
|---|---|
| `Ctrl/Cmd+N/O/S/Shift+S/W` | New / Open / Save / Save As / Close |
| `Ctrl/Cmd+Z/Y` | Undo / Redo |
| `Ctrl/Cmd+F/H` | Find / Replace |
| `F3` / `Shift+F3` | Next / previous match |
| `F6` | Cycle focus (Outline → Source → Inspector → Problems) |
| `F1` | Shortcut help |

## Performance envelope

Measured in release builds on the benchmark machine (see
`docs/BASELINE-step-4.md`): 20 MiB documents open editable in under
100 ms; 200,000-node literal searches: ~131 ms cold, cached queries
effectively instant; 100 incremental edits stay within a 64 MiB memory
delta. Inputs above 20 MiB or 200,000 elements are read-only; above
256 MiB they are refused before any parsing.

## Security limits

| Limit | Value |
|---|---|
| Maximum open size | 256 MiB (refused above) |
| Maximum nesting depth | 512 |
| Entity expansion budget | 16 MiB per parse |
| External entities | declared but never loaded; references error |
| Network access | none anywhere in the dependency tree |
| Schema includes/imports | schema root directory subtree only |

## Build

Requires Rust 1.88 (MSRV).

```bash
git clone <repository>
cd xml_tool
cargo build --release
cargo run --release
```

Development checks:

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo bench --no-run
cargo audit && cargo deny check
```

Deterministic test fixtures regenerate with
`cargo run --example gen_fixtures` (large fixtures are git-ignored).

## Licensing

This project is MIT licensed, with two embedded components:

- **Noto Sans SC** (`assets/fonts/`) — SIL Open Font License 1.1.
- **erxi** (the EXI backend, git dependency) — **PolyForm-Noncommercial
  1.0.0**. Binary distributions of xml_tool therefore inherit a
  non-commercial restriction for the EXI feature; commercial builds must
  replace or remove the EXI backend. The XML-only feature set is
  unaffected.

See [`MIGRATION.md`](MIGRATION.md) for the 0.2 → 0.3 API and behavior
changes and [`CHANGELOG.md`](CHANGELOG.md) for release history.
