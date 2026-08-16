//! Desktop UI: the shell, its panels, and reusable widgets.

// base64_image and xml_tree keep their own unit tests and are re-wired by
// later plan steps (image decoding in step 9); their non-test helpers are
// unused until then.
#[allow(dead_code)]
pub mod base64_image;
pub mod dialogs;
pub mod fonts;
pub mod icons;
pub mod inspector;
pub mod localization;
pub mod panels;
pub mod shell;
#[allow(dead_code)]
pub mod syntax_highlighter;
pub mod theme;
pub mod theme_prefs;
#[allow(dead_code)]
pub mod virtual_list;
pub mod xml_tree;

pub use shell::{AppShell, FocusPane};
