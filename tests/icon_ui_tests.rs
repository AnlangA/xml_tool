//! Exercise the migrated image flow through the real shell and egui input.
use egui_kittest::{Harness, kittest::Queryable};
use std::io::Cursor;
use std::sync::Arc;
use xml_tool::core::XmlDocument;
use xml_tool::services::workspace::{DocumentMode, FileType};
use xml_tool::ui::{
    AppShell,
    localization::{Language, Localization},
    theme_prefs::ThemeMode,
};

fn bmp24() -> Arc<[u8]> {
    let image = image::RgbImage::from_pixel(16, 14, image::Rgb([40, 80, 180]));
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut bytes, image::ImageFormat::Bmp)
        .unwrap();
    bytes.into_inner().into()
}

fn settle(harness: &mut Harness<'_, AppShell>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while harness.run_ok().is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "UI job did not settle"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn converter_menu_and_window_follow_language_and_theme_without_a_document() {
    for language in [Language::English, Language::Chinese] {
        for theme in [ThemeMode::Light, ThemeMode::Dark] {
            let mut shell = AppShell::new();
            shell.localization = Localization::with_language(language);
            shell.theme_mode = theme;
            let mut harness = Harness::new_state(|ctx, shell| shell.update(ctx), shell);
            harness.set_size(egui::vec2(800.0, 500.0));
            harness.run();
            harness.get_by_label("XML").click();
            harness.run();
            let title = harness.state().localization.msg("icon-title");
            harness.get_by_label(&title).click();
            harness.run();
            let choose = harness.state().localization.msg("icon-choose");
            assert!(harness.query_by_label(&choose).is_some());
            let alternate = if language == Language::English {
                Language::Chinese
            } else {
                Language::English
            };
            harness.state_mut().localization.set_language(alternate);
            harness.run();
            let choose = harness.state().localization.msg("icon-choose");
            assert!(harness.query_by_label(&choose).is_some());
            assert!(harness.state().workspace.sessions().is_empty());
        }
    }
}

#[test]
fn drop_24bpp_image_stage_apply_and_undo_through_inspector() {
    let mut shell = AppShell::new();
    shell.localization = Localization::with_language(Language::English);
    let doc = XmlDocument::parse(b"<ImageData16x14>old</ImageData16x14>".as_slice()).unwrap();
    let root = doc.root_element().unwrap();
    let text_node = doc.children(root)[0];
    shell.workspace.add_opened(
        "test.xml".into(),
        FileType::Xml,
        DocumentMode::Editable,
        doc,
    );
    shell.workspace.active_mut().unwrap().selection = Some(text_node);
    let mut harness = Harness::new_state(|ctx, shell| shell.update(ctx), shell);
    harness.set_size(egui::vec2(1280.0, 800.0));
    settle(&mut harness);
    harness.get_by_label("Import Icon…").click();
    settle(&mut harness);
    harness.input_mut().events.push(egui::Event::PointerGone);
    harness.input_mut().dropped_files.push(egui::DroppedFile {
        name: "图标-24位.bmp".into(),
        bytes: Some(bmp24()),
        ..Default::default()
    });
    harness.step();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while harness.query_by_label("Fill Text Draft").is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "conversion did not reach the UI"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
        settle(&mut harness);
    }
    assert!(
        harness
            .query_all_by_label_contains("24bpp")
            .next()
            .is_some()
    );
    if let Some(path) = std::env::var_os("XML_TOOL_ICON_PREVIEW") {
        harness
            .render()
            .expect("preview renderer")
            .save(path)
            .expect("save preview artifact");
    }
    harness.get_by_label("Fill Text Draft").click();
    settle(&mut harness);
    assert_eq!(
        harness
            .state()
            .workspace
            .active()
            .unwrap()
            .history
            .undo_depth(),
        0
    );
    harness.get_by_label("Apply Changes").click();
    settle(&mut harness);
    let session = harness.state().workspace.active().unwrap();
    assert_eq!(session.history.undo_depth(), 1);
    let content = session.document.children(root)[0];
    let hex = session.document.node_text(content).unwrap();
    let bytes: Vec<_> = hex
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    assert_eq!(u16::from_le_bytes(bytes[28..30].try_into().unwrap()), 4);
    let image = image::load_from_memory(&bytes).unwrap();
    assert_eq!((image.width(), image.height()), (16, 14));
    // Moving focus must not commit the pre-import inspector buffer again.
    harness.get_by_label("Import Icon…").click();
    settle(&mut harness);
    assert_eq!(
        harness
            .state()
            .workspace
            .active()
            .unwrap()
            .history
            .undo_depth(),
        1
    );
    assert_ne!(
        harness
            .state()
            .workspace
            .active()
            .unwrap()
            .document
            .node_text(content),
        Some("old")
    );
    harness.state_mut().undo();
    settle(&mut harness);
    assert_eq!(
        harness
            .state()
            .workspace
            .active()
            .unwrap()
            .document
            .node_text(content),
        Some("old")
    );
}
