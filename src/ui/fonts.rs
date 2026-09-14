//! Bundled CJK font loading.
//!
//! `NotoSansSC-Regular.otf` (SIL OFL 1.1, license alongside it) is compiled
//! into the binary so Chinese renders identically on every platform. The
//! font is registered as the first fallback for both proportional and
//! monospace text; Latin glyphs still come from egui's built-in fonts.

use egui::{Context, FontData, FontDefinitions, FontFamily};

const NOTO_SANS_SC: &[u8] = include_bytes!("../../assets/fonts/NotoSansSC-Regular.otf");

/// Installs the bundled fonts into `ctx` (idempotent).
pub fn install_cjk_font(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .entry("noto-sans-sc".into())
        .or_insert_with(|| std::sync::Arc::new(FontData::from_owned(NOTO_SANS_SC.to_vec())));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("noto-sans-sc".into());
    }
    super::icons::add_phosphor_font(&mut fonts);
    ctx.set_fonts(fonts);
}
