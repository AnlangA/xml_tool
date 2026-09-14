//! User-visible theme and typography preferences.
//!
//! `ThemeMode::System` follows the OS preference live (egui reads it each
//! frame). Font scaling multiplies the base pixel size in 10% steps between
//! 80% and 180%; both settings persist through the settings store in a
//! later step and apply immediately when changed.

use egui::{Context, ThemePreference};

/// Light/dark selection with a system-following default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

impl ThemeMode {
    /// Applies the preference to the egui context.
    pub fn apply(self, ctx: &Context) {
        let preference = match self {
            ThemeMode::System => ThemePreference::System,
            ThemeMode::Light => ThemePreference::Light,
            ThemeMode::Dark => ThemePreference::Dark,
        };
        ctx.set_theme(preference);
    }
}

/// Font scaling in percent, clamped to the plan's 80–180% range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontScale(pub u8);

impl Default for FontScale {
    fn default() -> Self {
        FontScale(100)
    }
}

impl FontScale {
    /// Clamps an arbitrary percent into the allowed range.
    pub fn new(percent: u8) -> FontScale {
        FontScale(percent.clamp(80, 180))
    }

    /// One step larger, if possible.
    pub fn step_up(self) -> FontScale {
        FontScale::new(self.0.saturating_add(10))
    }

    /// One step smaller, if possible.
    pub fn step_down(self) -> FontScale {
        FontScale::new(self.0.saturating_sub(10))
    }

    /// The scale factor egui expects.
    pub fn factor(self) -> f32 {
        f32::from(self.0) / 100.0
    }

    /// Applies the scale to the context's native zoom.
    pub fn apply(self, ctx: &Context) {
        ctx.set_zoom_factor(self.factor());
    }
}

/// Installs the Catppuccin palette matching the current egui theme, plus
/// the shared spacing/radius tweaks that give the shell its density.
pub fn apply_accent_theme(ctx: &Context) {
    match ctx.theme() {
        egui::Theme::Light => catppuccin_egui::set_theme(ctx, catppuccin_egui::LATTE),
        egui::Theme::Dark => catppuccin_egui::set_theme(ctx, catppuccin_egui::MOCHA),
    }

    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.spacing.menu_margin = egui::Margin::symmetric(8, 6);
        style.spacing.window_margin = egui::Margin::symmetric(12, 10);
        style.spacing.indent = 16.0;

        let radius = egui::CornerRadius::same(6);
        style.visuals.window_corner_radius = egui::CornerRadius::same(8);
        style.visuals.menu_corner_radius = radius;
        style.visuals.widgets.noninteractive.corner_radius = radius;
        style.visuals.widgets.inactive.corner_radius = radius;
        style.visuals.widgets.hovered.corner_radius = radius;
        style.visuals.widgets.active.corner_radius = radius;
        style.visuals.widgets.open.corner_radius = radius;
    });
}
