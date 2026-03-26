use egui::Color32;

/// Application color theme based on Catppuccin Mocha.
#[allow(dead_code)]
pub struct Theme;

#[allow(dead_code)]
impl Theme {
    // Primary colors
    pub const ROSEWATER: Color32 = Color32::from_rgb(245, 224, 220);
    pub const FLAMINGO: Color32 = Color32::from_rgb(242, 205, 205);
    pub const PINK: Color32 = Color32::from_rgb(245, 194, 231);
    pub const MAUVE: Color32 = Color32::from_rgb(203, 166, 247);
    pub const RED: Color32 = Color32::from_rgb(243, 139, 168);
    pub const MAROON: Color32 = Color32::from_rgb(235, 160, 172);
    pub const PEACH: Color32 = Color32::from_rgb(250, 179, 135);
    pub const YELLOW: Color32 = Color32::from_rgb(249, 226, 175);
    pub const GREEN: Color32 = Color32::from_rgb(166, 227, 161);
    pub const TEAL: Color32 = Color32::from_rgb(148, 226, 213);
    pub const SKY: Color32 = Color32::from_rgb(137, 180, 250);
    pub const SAPPHIRE: Color32 = Color32::from_rgb(116, 199, 232);
    pub const BLUE: Color32 = Color32::from_rgb(137, 180, 250);
    pub const LAVENDER: Color32 = Color32::from_rgb(180, 190, 254);

    // Base colors
    pub const CRUST: Color32 = Color32::from_rgb(17, 17, 27);
    pub const MANTLE: Color32 = Color32::from_rgb(24, 24, 37);
    pub const SURFACE0: Color32 = Color32::from_rgb(49, 50, 68);
    pub const SURFACE1: Color32 = Color32::from_rgb(69, 71, 90);
    pub const BASE: Color32 = Color32::from_rgb(30, 30, 46);
    pub const TEXT: Color32 = Color32::from_rgb(205, 214, 244);
    pub const SUBTEXT1: Color32 = Color32::from_rgb(186, 194, 222);
    pub const SUBTEXT0: Color32 = Color32::from_rgb(166, 173, 200);
    pub const OVERLAY0: Color32 = Color32::from_rgb(147, 153, 178);
    pub const OVERLAY1: Color32 = Color32::from_rgb(127, 132, 156);

    // Semantic colors
    pub const ACCENT: Color32 = Self::LAVENDER;
    pub const ACCENT_HOVER: Color32 = Self::BLUE;
    pub const SUCCESS: Color32 = Self::GREEN;
    pub const WARNING: Color32 = Self::YELLOW;
    pub const ERROR: Color32 = Self::RED;
    pub const INFO: Color32 = Self::SKY;

    // UI element colors
    pub const PANEL_BG: Color32 = Self::MANTLE;
    pub const CARD_BG: Color32 = Self::SURFACE0;
    pub const INPUT_BG: Color32 = Self::SURFACE1;
    pub const BORDER: Color32 = Self::SURFACE1;
    pub const SELECTION: Color32 = Color32::from_rgba_premultiplied(100, 120, 200, 40);

    // Text colors
    pub const TEXT_PRIMARY: Color32 = Self::TEXT;
    pub const TEXT_SECONDARY: Color32 = Self::SUBTEXT0;
    pub const TEXT_MUTED: Color32 = Self::OVERLAY0;
    pub const TEXT_HIGHLIGHT: Color32 = Self::LAVENDER;

    // Element-specific colors
    pub const ELEMENT_NAME: Color32 = Self::GREEN;
    pub const ATTRIBUTE_KEY: Color32 = Self::YELLOW;
    pub const ATTRIBUTE_VALUE: Color32 = Self::PEACH;
    pub const TEXT_CONTENT: Color32 = Self::TEXT;
    pub const COMMENT: Color32 = Color32::from_rgb(108, 135, 108);
    
    // Syntax highlighting colors (for raw XML view)
    pub const SYNTAX_TAG: Color32 = Self::GREEN;
    pub const SYNTAX_TAG_BRACKET: Color32 = Self::OVERLAY1;
    pub const SYNTAX_ATTR_NAME: Color32 = Self::YELLOW;
    pub const SYNTAX_ATTR_VALUE: Color32 = Self::PEACH;
    pub const SYNTAX_STRING: Color32 = Self::PEACH;
    pub const SYNTAX_COMMENT: Color32 = Self::OVERLAY0;
    pub const SYNTAX_TEXT: Color32 = Self::TEXT;
    pub const SYNTAX_KEYWORD: Color32 = Self::MAUVE;
    
    // Interactive states
    pub const HOVER_BG: Color32 = Color32::from_rgba_premultiplied(255, 255, 255, 10);
    pub const ACTIVE_BG: Color32 = Color32::from_rgba_premultiplied(255, 255, 255, 20);
    pub const FOCUS_BORDER: Color32 = Self::LAVENDER;
    
    // Status backgrounds
    pub const SUCCESS_BG: Color32 = Color32::from_rgba_premultiplied(166, 227, 161, 30);
    pub const WARNING_BG: Color32 = Color32::from_rgba_premultiplied(249, 226, 175, 30);
    pub const ERROR_BG: Color32 = Color32::from_rgba_premultiplied(243, 139, 168, 30);
    pub const INFO_BG: Color32 = Color32::from_rgba_premultiplied(137, 180, 250, 30);
}

/// Spacing constants for consistent layout
#[allow(dead_code)]
pub struct Spacing;

#[allow(dead_code)]
impl Spacing {
    pub const XXS: f32 = 2.0;
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
}

/// Typography constants
#[allow(dead_code)]
pub struct Typography;

#[allow(dead_code)]
impl Typography {
    pub const HEADING_1: f32 = 24.0;
    pub const HEADING_2: f32 = 18.0;
    pub const HEADING_3: f32 = 16.0;
    pub const BODY: f32 = 14.0;
    pub const SMALL: f32 = 12.0;
    pub const TINY: f32 = 10.0;
    pub const CODE: f32 = 12.0;
}
