//! Desktop UI: the shell, its panels, and reusable widgets.

pub mod alerts;
pub mod base64_image;
pub mod dialogs;
pub mod fonts;
pub(crate) mod icon_converter;
mod icon_import;
pub mod icons;
pub mod inspector;
pub mod localization;
pub mod outline;
pub mod panels;
pub mod shell;
pub mod syntax_highlighter;
pub mod theme;
pub mod theme_prefs;
pub use shell::{AppShell, FocusPane};
