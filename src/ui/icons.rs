/// Icon constants using Unicode symbols and egui_phosphor
/// 
/// This module provides a centralized location for all icons used in the application.
/// We use a mix of Unicode symbols (for compatibility) and Phosphor icons (for modern look).

pub struct Icons;

impl Icons {
    // File operations
    pub const FILE: &'static str = "📄";
    pub const FOLDER_OPEN: &'static str = "📂";
    pub const SAVE: &'static str = "💾";
    
    // Actions
    pub const SEARCH: &'static str = "🔍";
    pub const EDIT: &'static str = "✏️";
    pub const DELETE: &'static str = "🗑️";
    pub const ADD: &'static str = "➕";
    pub const CLOSE: &'static str = "✕";
    
    // Navigation
    pub const EXPAND: &'static str = "▶";
    pub const COLLAPSE: &'static str = "▼";
    pub const ARROW_RIGHT: &'static str = "→";
    pub const ARROW_LEFT: &'static str = "←";
    
    // Status
    pub const SUCCESS: &'static str = "✓";
    pub const ERROR: &'static str = "✗";
    pub const WARNING: &'static str = "⚠";
    pub const INFO: &'static str = "ℹ";
    
    // Tools
    pub const COMPRESS: &'static str = "▶";
    pub const DECOMPRESS: &'static str = "◀";
    pub const REFRESH: &'static str = "🔄";
    pub const SETTINGS: &'static str = "⚙";
    
    // XML specific
    pub const XML_TAG: &'static str = "<>";
    pub const ATTRIBUTE: &'static str = "@";
    pub const TEXT_NODE: &'static str = "\"\"";
    pub const COMMENT: &'static str = "💬";
}

/// Phosphor icon variants (when egui_phosphor is available)
#[cfg(feature = "phosphor-icons")]
pub mod phosphor {
    use egui_phosphor::regular as icons;
    
    pub struct PhosphorIcons;
    
    impl PhosphorIcons {
        pub const FILE: &'static str = icons::FILE;
        pub const FOLDER_OPEN: &'static str = icons::FOLDER_OPEN;
        pub const MAGNIFYING_GLASS: &'static str = icons::MAGNIFYING_GLASS;
        pub const FLOPPY_DISK: &'static str = icons::FLOPPY_DISK;
        pub const TRASH: &'static str = icons::TRASH;
        pub const PLUS: &'static str = icons::PLUS;
        pub const X: &'static str = icons::X;
        pub const ARROW_CLOCKWISE: &'static str = icons::ARROW_CLOCKWISE;
        pub const GEAR: &'static str = icons::GEAR;
    }
}
