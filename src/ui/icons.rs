//! Iconography: Phosphor icons loaded with the bundled font.
//!
//! Every icon button renders a Phosphor glyph (font installed by
//! [`crate::ui::fonts`]); the emoji fallbacks are gone, so icon rendering
//! is identical across platforms.

use egui_phosphor::variants::regular;

/// Icon glyphs used by the shell.
pub struct Icons;

impl Icons {
    pub const FILE_PLUS: &'static str = regular::FILE_PLUS;
    pub const FOLDER_OPEN: &'static str = regular::FOLDER_OPEN;
    pub const FLOPPY_DISK: &'static str = regular::FLOPPY_DISK;
    pub const MAGIC_WAND: &'static str = regular::MAGIC_WAND;
    pub const TRASH: &'static str = regular::TRASH;
    pub const MAGNIFYING_GLASS: &'static str = regular::MAGNIFYING_GLASS;
    pub const ARROW_CLOCKWISE: &'static str = regular::ARROW_CLOCKWISE;
    pub const INFO: &'static str = regular::INFO;
    pub const WARNING: &'static str = regular::WARNING;
    pub const CHECK: &'static str = regular::CHECK;
    pub const X: &'static str = regular::X;
    pub const ARROW_COUNTER_CLOCKWISE: &'static str = regular::ARROW_COUNTER_CLOCKWISE;
    pub const TREE_STRUCTURE: &'static str = regular::TREE_STRUCTURE;
    pub const SLIDERS_HORIZONTAL: &'static str = regular::SLIDERS_HORIZONTAL;
    pub const CARET_DOWN: &'static str = regular::CARET_DOWN;
    pub const CARET_RIGHT: &'static str = regular::CARET_RIGHT;
}

/// Loads the Phosphor regular variant into the given definitions.
pub fn add_phosphor_font(fonts: &mut egui::FontDefinitions) {
    egui_phosphor::add_to_fonts(fonts, egui_phosphor::Variant::Regular);
}
