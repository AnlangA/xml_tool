use eframe::egui;

mod app;
mod exi;
mod export;
mod ui;
mod utils;
mod xml;

use app::XmlToolApp;

fn main() -> eframe::Result<()> {
    // Log to stderr for debugging
    #[cfg(debug_assertions)]
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
            .with_title("XML/EXI Tool")
            .with_icon(load_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "XML/EXI Tool",
        options,
        Box::new(|cc| Ok(Box::new(XmlToolApp::new(cc)))),
    )
}

fn load_icon() -> egui::IconData {
    // Create a simple icon (purple square with XML text representation)
    let (icon_width, icon_height) = (64, 64);
    let mut rgba = vec![0u8; icon_width * icon_height * 4];

    for y in 0..icon_height {
        for x in 0..icon_width {
            let idx = (y * icon_width + x) * 4;
            // Create a gradient background
            let cx = x as f32 / icon_width as f32;
            let cy = y as f32 / icon_height as f32;
            let dist = ((cx - 0.5).powi(2) + (cy - 0.5).powi(2)).sqrt();

            if dist < 0.45 {
                // Purple gradient
                rgba[idx] = (80.0 + 40.0 * (1.0 - dist)) as u8; // R
                rgba[idx + 1] = (60.0 + 30.0 * (1.0 - dist)) as u8; // G
                rgba[idx + 2] = (180.0 + 50.0 * (1.0 - dist)) as u8; // B
                rgba[idx + 3] = 255; // A
            }
        }
    }

    egui::IconData {
        rgba,
        width: icon_width as u32,
        height: icon_height as u32,
    }
}
