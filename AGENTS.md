# AGENTS.md

## Cursor Cloud specific instructions

`xml_tool` is a single Rust desktop application (an XML/EXI editor built with
`eframe`/`egui`). There is one crate and one binary — there are no separate
backend/frontend services. Standard commands live in `README.md` ("Build" and
"Development checks"); the notes below only cover cloud/headless caveats.

### Toolchain & standard commands
- MSRV / CI toolchain is **Rust 1.88.0** (`rust-toolchain@1.88.0` in
  `.github/workflows/ci.yml`). The VM's default toolchain is already set to
  1.88.0; the startup update script runs `cargo fetch`.
- Lint / test / build / run commands are documented in `README.md`. Build takes
  a while because the release profile enables LTO.

### Running tests (needs a virtual display)
The suite includes `egui_kittest` `wgpu` snapshot tests (`tests/ui_shell_tests.rs`)
that require an X display, so run tests under a virtual framebuffer:

```bash
xvfb-run -a -s "-screen 0 1280x800x24" cargo test --all-targets
```

All 310 tests pass this way (software/`llvmpipe` rendering is fine).

### clippy is red on `master` (pre-existing, not an environment issue)
`cargo clippy --all-targets --all-features -- -D warnings` currently fails with
`uninlined_format_args` / `overly_complex_bool_expr` on `master`. This is a
pre-existing code issue that also fails in GitHub CI — it is **not** caused by
the environment. `cargo fmt --all --check` passes.

### Running the GUI app (headless VNC + D-Bus portal)
The interactive desktop is a TigerVNC + xfce4 session on **`DISPLAY=:1`**
(this is what computer-use / screenshots see). The app's Open/Save file dialogs
use `rfd` 0.15, which talks to the **XDG Desktop Portal over a D-Bus session
bus**. A plain `DISPLAY=:1 ./target/debug/xml_tool` launches and renders, but
file dialogs will error without a session bus + portal. Launch it like this:

```bash
export DISPLAY=:1 XDG_RUNTIME_DIR=/tmp/xdg-runtime-ubuntu XDG_CURRENT_DESKTOP=GNOME
mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"
dbus-run-session -- bash -c '
  export XDG_CURRENT_DESKTOP=GNOME
  /usr/libexec/xdg-desktop-portal-gtk >/tmp/xdp-gtk.log 2>&1 &
  /usr/libexec/xdg-desktop-portal    >/tmp/xdp.log     2>&1 &
  sleep 2
  exec ./target/debug/xml_tool
'
```

`xdg-desktop-portal` + `xdg-desktop-portal-gtk` are installed in the VM snapshot.

### Driving/verifying the UI headlessly (important gotchas)
- `eframe` runs in **reactive** mode and the background `TaskManager` workers do
  **not** call `ctx.request_repaint()` (`src/app.rs`, `src/services/task_manager.rs`).
  So results of threaded jobs (file **Open**, source-editor **Apply**, **Save**)
  only surface on the *next input event*. When automating with `xdotool`, send a
  mouse move/click after such an action to force a repaint.
- In this headless VNC, the portal file-open returns the picked path but the
  async open pipeline does not reliably render the loaded document. For a
  deterministic hello-world / smoke test, prefer **synchronous** actions:
  `Ctrl+N` (new document), select a node in the Outline → use the Inspector
  buttons (`+ <e/>` add child, `Add attribute`, `Delete node`), and `Ctrl+Z` /
  `Ctrl+Y` (undo/redo). These update the tree, source, and status-bar element
  count immediately.
- Source-editor typing quirk: the source `TextEdit` id includes the draft state,
  so the **first** keystroke starts a draft and changes the widget id, dropping
  focus (you see only one character). Paste the whole text in one op
  (`Ctrl+V`, e.g. via `xclip -selection clipboard`) instead of typing char-by-char.
- The in-VM screen recorder can capture sparse/laggy frames on this VNC; trust
  direct `scrot` screenshots (and re-verify videos) over a single video pass.
