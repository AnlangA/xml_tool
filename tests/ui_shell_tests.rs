//! Acceptance tests for `AGENTS_PLAN.md` step 5: localization parity and
//! headless UI structure snapshots.
//!
//! - The en-US and zh-CN Fluent resources must expose identical key sets;
//!   drift fails this suite.
//! - The shell renders in 12 combinations (3 window sizes × 2 languages ×
//!   2 themes); each render asserts the core controls exist and stores a
//!   pixel snapshot under `tests/snapshots/` for regression comparison.
//! - Keyboard shortcuts and menu clicks drive the shell without a pointer.

use std::collections::HashSet;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use xml_tool::ui::localization::{Language, Localization};
use xml_tool::ui::shell::AppShell;
use xml_tool::ui::theme_prefs::ThemeMode;

// ---------------------------------------------------------------------------
// Localization
// ---------------------------------------------------------------------------

#[test]
fn locales_have_identical_key_sets() {
    let english: HashSet<String> = Localization::keys_of(Language::English)
        .into_iter()
        .collect();
    let chinese: HashSet<String> = Localization::keys_of(Language::Chinese)
        .into_iter()
        .collect();
    assert!(!english.is_empty(), "en-US resource must not be empty");
    assert_eq!(
        english.len(),
        chinese.len(),
        "locale key counts differ (en {}, zh {})",
        english.len(),
        chinese.len()
    );
    let missing_in_chinese: Vec<_> = english.difference(&chinese).collect();
    let missing_in_english: Vec<_> = chinese.difference(&english).collect();
    assert!(
        missing_in_chinese.is_empty() && missing_in_english.is_empty(),
        "zh missing {:?}; en missing {:?}",
        missing_in_chinese,
        missing_in_english
    );
}

#[test]
fn translation_switches_without_restart() {
    let mut localization = Localization::with_language(Language::English);
    assert_eq!(localization.msg("menu-file"), "File");
    localization.set_language(Language::Chinese);
    assert_eq!(localization.msg("menu-file"), "文件");
    let hits = localization.msg_with("search-hits", Some(&xml_tool::fluent_args!("count" => 3)));
    assert!(hits.contains('3'), "formatted: {hits}");
}

#[test]
fn unknown_keys_surface_visibly() {
    let localization = Localization::with_language(Language::English);
    let fallback = localization.msg("definitely-not-a-key");
    assert!(
        fallback.contains("definitely-not-a-key"),
        "missing keys must be visible, got {fallback}"
    );
}

// ---------------------------------------------------------------------------
// Headless UI structure snapshots
// ---------------------------------------------------------------------------

fn combo_shell(language: Language, theme: ThemeMode) -> AppShell {
    let mut shell = AppShell::new();
    shell.localization.set_language(language);
    shell.theme_mode = theme;
    shell.new_document(); // one untitled tab so every panel has content
    shell
}

fn language_tag(language: Language) -> &'static str {
    match language {
        Language::English => "en",
        Language::Chinese => "zh",
    }
}

fn theme_tag(theme: ThemeMode) -> &'static str {
    match theme {
        ThemeMode::Light => "light",
        _ => "dark",
    }
}

#[test]
fn ui_renders_in_all_twelve_combinations() {
    for (width, height) in [(800u32, 500u32), (1280, 800), (1920, 1080)] {
        for language in [Language::English, Language::Chinese] {
            for theme in [ThemeMode::Dark, ThemeMode::Light] {
                let mut harness = Harness::new_state(
                    |ctx, shell| shell.update(ctx),
                    combo_shell(language, theme),
                );
                harness.set_size(egui::vec2(width as f32, height as f32));
                harness.run();

                let combo = format!(
                    "{}x{} {}/{}",
                    width,
                    height,
                    language_tag(language),
                    theme_tag(theme)
                );
                let file_menu = match language {
                    Language::English => "File",
                    Language::Chinese => "文件",
                };
                assert!(
                    harness.query_by_label(file_menu).is_some(),
                    "{combo}: File menu missing"
                );
                let outline = match language {
                    Language::English => "Outline",
                    Language::Chinese => "大纲",
                };
                assert!(
                    !harness
                        .query_all_by_label_contains(outline)
                        .next()
                        .is_none(),
                    "{combo}: outline panel missing"
                );
                // Pixel snapshot: compare against the stored baseline; on
                // hosts without any renderer (plain software CI images)
                // degrade to the structural assertions above.
                let result = harness.try_snapshot(format!(
                    "shell-{}x{}-{}-{}",
                    width,
                    height,
                    language_tag(language),
                    theme_tag(theme)
                ));
                if let Err(egui_kittest::SnapshotError::RenderError { .. }) = &result {
                    // No renderer available: structural coverage only.
                } else {
                    result.expect("snapshot matches the stored baseline");
                }
            }
        }
    }
}

#[test]
fn narrow_windows_keep_controls_in_bounds() {
    let mut harness = Harness::new_state(
        |ctx, shell| shell.update(ctx),
        combo_shell(Language::English, ThemeMode::Dark),
    );
    harness.set_size(egui::vec2(800.0, 500.0));
    harness.run();
    let file = harness.query_by_label("File").expect("file menu");
    let rect = file.rect();
    assert!(
        rect.left() >= 0.0 && rect.right() <= 800.0,
        "menu in bounds"
    );
    assert!(
        !harness
            .query_all_by_label_contains("Outline")
            .next()
            .is_none(),
        "outline stays reachable at 800×500"
    );
}

#[test]
fn chinese_menu_renders_with_cjk_font() {
    let mut harness = Harness::new_state(
        |ctx, shell| shell.update(ctx),
        combo_shell(Language::Chinese, ThemeMode::Dark),
    );
    harness.set_size(egui::vec2(1280.0, 800.0));
    harness.run();
    assert!(harness.query_by_label("文件").is_some());
    assert!(!harness.query_all_by_label_contains("大纲").next().is_none());
}

// ---------------------------------------------------------------------------
// Keyboard-only flows
// ---------------------------------------------------------------------------

#[test]
fn ctrl_n_creates_a_tab_keyboard_only() {
    let mut harness = Harness::new_state(|ctx, shell| shell.update(ctx), AppShell::new());
    harness.set_size(egui::vec2(1280.0, 800.0));
    harness.run();
    assert_eq!(harness.state().workspace.sessions().len(), 0);

    harness.key_down_modifiers(egui::Modifiers::CTRL, egui::Key::N);
    harness.key_up_modifiers(egui::Modifiers::CTRL, egui::Key::N);
    harness.run();

    assert_eq!(
        harness.state().workspace.sessions().len(),
        1,
        "Ctrl+N must create a document without a pointer"
    );
    assert_eq!(
        harness.state().workspace.active().unwrap().display_name(),
        "Untitled-1"
    );
}

#[test]
fn f1_opens_shortcut_help() {
    let mut harness = Harness::new_state(|ctx, shell| shell.update(ctx), AppShell::new());
    harness.set_size(egui::vec2(1280.0, 800.0));
    harness.run();
    harness.key_down(egui::Key::F1);
    harness.key_up(egui::Key::F1);
    harness.run();
    assert!(
        harness.state().dialog.is_some(),
        "F1 must open the shortcuts dialog"
    );
    harness.run();
    assert!(
        !harness
            .query_all_by_label_contains("Keyboard")
            .next()
            .is_none()
            || !harness
                .query_all_by_label_contains("快捷键")
                .next()
                .is_none()
    );
}

#[test]
fn menu_click_new_creates_a_document() {
    let mut harness = Harness::new_state(
        |ctx, shell| shell.update(ctx),
        combo_shell(Language::English, ThemeMode::Dark),
    );
    harness.set_size(egui::vec2(1280.0, 800.0));
    harness.run();
    let before = harness.state().workspace.sessions().len();
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label("New").click();
    harness.run();
    assert_eq!(
        harness.state().workspace.sessions().len(),
        before + 1,
        "menu New must create a document"
    );
}

#[test]
fn welcome_view_offers_new_and_open_actions() {
    let mut shell = AppShell::new();
    shell.localization.set_language(Language::English);
    let mut harness = Harness::new_state(|ctx, shell| shell.update(ctx), shell);
    harness.set_size(egui::vec2(1280.0, 800.0));
    harness.run();
    assert_eq!(harness.state().workspace.sessions().len(), 0);
    assert!(
        harness
            .query_all_by_label_contains("Welcome to XML Tool")
            .next()
            .is_some(),
        "empty workspace renders the welcome view"
    );
    harness.get_by_label_contains("New").click();
    harness.run();
    assert_eq!(
        harness.state().workspace.sessions().len(),
        1,
        "welcome New button must create a document"
    );
}

// ---------------------------------------------------------------------------
// Alert handling (step "优化告警处理")
// ---------------------------------------------------------------------------

#[test]
fn problems_panel_lists_filters_and_clears() {
    let mut shell = AppShell::new();
    shell.localization.set_language(Language::English);
    let mut harness = Harness::new_state(|ctx, shell| shell.update(ctx), shell);
    harness.set_size(egui::vec2(1280.0, 800.0));
    harness.run();
    {
        let shell = harness.state_mut();
        shell
            .alerts
            .push(xml_tool::core::Severity::Error, "io", "disk on fire");
        shell.alerts.push(
            xml_tool::core::Severity::Warning,
            "exi-fidelity",
            "drops comments",
        );
    }
    harness.run();

    // The arriving error auto-opened the panel; both rows render.
    assert!(harness.state().problems_panel_open);
    assert!(
        harness
            .query_all_by_label_contains("disk on fire")
            .next()
            .is_some()
    );
    assert!(
        harness
            .query_all_by_label_contains("drops comments")
            .next()
            .is_some()
    );

    // Filter out warnings: the warning row disappears, error stays.
    harness
        .query_all_by_label_contains("Warnings")
        .next()
        .expect("warnings filter toggle")
        .click();
    harness.run();
    assert!(
        harness
            .query_all_by_label_contains("drops comments")
            .next()
            .is_none(),
        "filtered warning disappears"
    );
    assert!(
        harness
            .query_all_by_label_contains("disk on fire")
            .next()
            .is_some()
    );

    // Clear all: back to the empty state.
    harness
        .query_all_by_label_contains("Clear all")
        .next()
        .expect("clear button")
        .click();
    harness.run();
    assert!(harness.state().alerts.is_empty());
    assert!(
        harness
            .query_all_by_label_contains("No problems")
            .next()
            .is_some()
    );
}

#[test]
fn reload_banner_is_non_blocking_and_dismissable() {
    let mut shell = AppShell::new();
    shell.localization.set_language(Language::English);
    let mut harness = Harness::new_state(|ctx, shell| shell.update(ctx), shell);
    harness.set_size(egui::vec2(1280.0, 800.0));
    harness.run();
    harness.state_mut().banner = Some(xml_tool::ui::shell::Banner::Reload {
        session: xml_tool::services::task_manager::SessionId(0),
        path: std::path::PathBuf::from("/tmp/watched.xml"),
        dirty: false,
    });
    harness.run();

    // The banner renders as a strip with actions — not a centered modal.
    assert!(
        harness
            .query_all_by_label_contains("watched.xml")
            .next()
            .is_some(),
        "banner shows the file name"
    );
    assert!(harness.query_by_label("Reload").is_some(), "reload action");

    // Dismissing removes it on the next frame.
    harness.get_by_label("Cancel").click();
    harness.run();
    assert!(harness.state().banner.is_none());
    assert!(
        harness
            .query_all_by_label_contains("watched.xml")
            .next()
            .is_none()
    );
}

#[test]
fn jump_target_hint_appears_in_source_pane() {
    let mut shell = combo_shell(Language::English, ThemeMode::Dark);
    shell.new_document();
    shell.source_jump = Some((4, 2));
    shell.problems_panel_open = false;
    let mut harness = Harness::new_state(|ctx, shell| shell.update(ctx), shell);
    harness.set_size(egui::vec2(1280.0, 800.0));
    harness.run();
    assert!(
        harness
            .query_all_by_label_contains("Jump target: line")
            .next()
            .is_some(),
        "the source pane shows the jump hint"
    );
    // Fluent interpolates the coordinates (wrapped in bidi isolation
    // marks in the rendered label; assert the digits via the API).
    let localization = Localization::with_language(Language::English);
    let hint = localization.msg_with(
        "source-jump-hint",
        Some(&xml_tool::fluent_args!("line" => 4i32, "column" => 2i32)),
    );
    assert!(hint.contains('4') && hint.contains('2'), "hint: {hint}");
}
