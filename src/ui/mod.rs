//! Desktop UI: the shell, its panels, and reusable widgets.

pub mod alerts;
// base64_image keeps its own unit tests and is re-wired by a later plan
// step (image decoding); its non-test helpers are unused until then.
#[allow(dead_code)]
pub mod base64_image;
pub mod dialogs;
pub mod fonts;
pub mod icons;
pub mod inspector;
pub mod localization;
pub mod panels;
pub mod shell;
pub mod syntax_highlighter;
pub mod theme;
pub mod theme_prefs;
#[allow(dead_code)]
pub mod virtual_list;

pub use shell::{AppShell, FocusPane};
