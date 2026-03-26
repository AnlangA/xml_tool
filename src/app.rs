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

        Self {
            main_panel: MainPanel::new(),
        }
    }
}

impl eframe::App for XmlToolApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.main_panel.show(ctx);
    }
}
