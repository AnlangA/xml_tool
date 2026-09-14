use egui::ThemePreference;

use crate::ui::MainPanel;

pub struct XmlToolApp {
    main_panel: MainPanel,
}

impl XmlToolApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Set theme
        cc.egui_ctx.set_theme(ThemePreference::Dark);

        // Apply catppuccin mocha theme
        catppuccin_egui::set_theme(&cc.egui_ctx, catppuccin_egui::MOCHA);
        install_system_font_fallback(&cc.egui_ctx);

        Self {
            main_panel: MainPanel::new(),
        }
    }
}

// egui's bundled fonts do not include CJK glyphs. Use a locally installed
// fallback so file names and XML text remain readable without bundling a font.
fn install_system_font_fallback(ctx: &egui::Context) {
    #[cfg(target_os = "windows")]
    let paths: Vec<std::path::PathBuf> = {
        let windows = std::env::var_os("WINDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "C:\\Windows".into());
        ["msyh.ttc", "simhei.ttf", "simsun.ttc"]
            .into_iter()
            .map(|name| windows.join("Fonts").join(name))
            .collect()
    };
    #[cfg(target_os = "macos")]
    let paths: Vec<std::path::PathBuf> = vec!["/System/Library/Fonts/PingFang.ttc".into()];
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let paths: Vec<std::path::PathBuf> = vec![
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc".into(),
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc".into(),
    ];
    if let Some(bytes) = paths.iter().find_map(|path| std::fs::read(path).ok()) {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "system-cjk".into(),
            egui::FontData::from_owned(bytes).into(),
        );
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push("system-cjk".into());
        }
        ctx.set_fonts(fonts);
    }
}

impl eframe::App for XmlToolApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.main_panel.show(ctx);
    }
}
