use std::path::PathBuf;
use std::sync::Arc;

use egui::{Context, RichText, TextureHandle, TextureOptions};

use super::base64_image::preview_display_size;
use super::localization::Localization;
use super::theme::Palette;
use crate::core::Revision;
use crate::services::image_conversion::{
    ConversionMode, ConversionNote, ConvertedIcon, IconSource, ImageFileFormat, convert_icon,
    is_esi_icon, read_icon,
};
pub(crate) use crate::services::image_import::IconTarget;
use crate::services::task_manager::{JobId, SessionId, TaskManager};
const ICON_SESSION: SessionId = SessionId(u64::MAX - 6);
use crate::utils::format_size;

pub(crate) struct IconImport {
    pub target: IconTarget,
    pub mode: ConversionMode,
    pub text: Arc<str>,
}

enum IconInput {
    File(PathBuf),
    Loaded(IconSource),
}

enum FileDialogResult {
    OpenImage(Option<PathBuf>),
    SaveIconText(Option<PathBuf>),
}
enum IconJob {
    Converted {
        source: Option<IconSource>,
        result: Result<ConvertedIcon, String>,
    },
    #[cfg(not(target_os = "macos"))]
    Dialog(FileDialogResult),
    Saved(Result<PathBuf, String>),
}

struct ReadyIcon {
    format: ImageFileFormat,
    width: u32,
    height: u32,
    texture: TextureHandle,
    text: Arc<str>,
    note: Option<ConversionNote>,
}

#[derive(Default)]
pub(crate) struct IconConverter {
    visible: bool,
    target: Option<IconTarget>,
    mode: ConversionMode,
    source: Option<IconSource>,
    ready: Option<ReadyIcon>,
    tasks: Arc<TaskManager>,
    worker: Option<JobId>,
    save_worker: Option<JobId>,
    dialog_job: Option<JobId>,
    saved_path: Option<PathBuf>,
    pending_export: Option<Arc<str>>,
    generation: u64,
    dialog_generation: u64,
    error: Option<String>,
    status: Option<String>,
}

impl IconConverter {
    pub(crate) fn with_tasks(tasks: Arc<TaskManager>) -> Self {
        Self {
            tasks,
            ..Default::default()
        }
    }

    fn cancel_conversion(&mut self) {
        if let Some(job) = self.worker.take() {
            self.tasks.cancel(job);
        }
    }

    pub(crate) fn open(&mut self, target: Option<IconTarget>, ctx: &Context) {
        self.generation = self.generation.wrapping_add(1);
        self.visible = true;
        self.mode = if target.as_ref().is_some_and(|t| is_esi_icon(&t.name)) {
            ConversionMode::EsiHex
        } else {
            ConversionMode::Base64
        };
        self.target = target;
        self.cancel_conversion();
        self.ready = None;
        self.error = None;
        self.status = None;
        if let Some(source) = self.source.clone() {
            self.start_conversion(IconInput::Loaded(source), ctx);
        }
    }

    pub(crate) fn sync_target(&mut self, current: Option<&IconTarget>) {
        if self
            .target
            .as_ref()
            .is_some_and(|target| Some(target) != current)
        {
            self.target = None;
            self.status = Some("icon-target-changed".into());
        }
    }

    fn close(&mut self) {
        self.visible = false;
        self.target = None;
        self.cancel_conversion();
        self.generation = self.generation.wrapping_add(1);
    }

    fn start_conversion(&mut self, input: IconInput, ctx: &Context) {
        self.cancel_conversion();
        self.ready = None;
        self.error = None;
        self.status = None;
        self.source = match &input {
            IconInput::File(_) => None,
            IconInput::Loaded(source) => Some(source.clone()),
        };
        let mode = self.mode;
        let side = ctx.input(|i| u32::try_from(i.max_texture_side).unwrap_or(u32::MAX));
        let ctx = ctx.clone();
        self.worker = Some(
            self.tasks
                .spawn(ICON_SESSION, Revision(0), move |_| {
                    let source = match input {
                        IconInput::Loaded(source) => Ok(source),
                        IconInput::File(path) => read_icon(&path),
                    };
                    let reply = match source {
                        Ok(source) => {
                            let result = convert_icon(&source, mode, side);
                            IconJob::Converted {
                                source: Some(source),
                                result,
                            }
                        }
                        Err(error) => IconJob::Converted {
                            source: None,
                            result: Err(error),
                        },
                    };
                    ctx.request_repaint();
                    Box::new(reply)
                })
                .0,
        );
    }

    fn pick_file(&mut self, save: bool, ctx: &Context, loc: &Localization) {
        if self.dialog_job.is_some() {
            return;
        }
        self.dialog_generation = self.generation;
        let title = loc.msg(if save { "icon-save" } else { "icon-choose" });
        #[cfg(target_os = "macos")]
        {
            let path = crate::services::image_conversion::pick_icon_path(save, &title);
            let result = if save {
                FileDialogResult::SaveIconText(path)
            } else {
                FileDialogResult::OpenImage(path)
            };
            self.handle_dialog_result(result, ctx);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let ctx = ctx.clone();
            self.dialog_job = Some(
                self.tasks
                    .spawn(ICON_SESSION, Revision(0), move |_| {
                        let path = crate::services::image_conversion::pick_icon_path(save, &title);
                        let result = if save {
                            FileDialogResult::SaveIconText(path)
                        } else {
                            FileDialogResult::OpenImage(path)
                        };
                        ctx.request_repaint();
                        Box::new(IconJob::Dialog(result))
                    })
                    .0,
            );
        }
    }

    fn handle_dialog_result(&mut self, result: FileDialogResult, ctx: &Context) {
        match result {
            FileDialogResult::OpenImage(Some(path))
                if self.visible && self.dialog_generation == self.generation =>
            {
                self.start_conversion(IconInput::File(path), ctx)
            }
            FileDialogResult::SaveIconText(Some(path)) => {
                if let Some(text) = self.pending_export.take() {
                    let ctx = ctx.clone();
                    self.save_worker = Some(
                        self.tasks
                            .spawn(ICON_SESSION, Revision(0), move |_| {
                                let result =
                                    crate::services::image_conversion::save_icon_text(&path, &text)
                                        .map(|()| path);
                                ctx.request_repaint();
                                Box::new(IconJob::Saved(result))
                            })
                            .0,
                    );
                }
            }
            FileDialogResult::SaveIconText(None) => {
                self.pending_export = None;
                self.status = Some("icon-save-cancelled".into());
            }
            _ => {}
        }
    }

    fn poll(&mut self, ctx: &Context) {
        while let Ok(Some(outcome)) = self.tasks.take_outcome(ICON_SESSION, Revision(0)) {
            match *outcome
                .result
                .downcast::<IconJob>()
                .expect("icon job payload")
            {
                IconJob::Converted { source, result } if self.worker == Some(outcome.job) => {
                    self.worker = None;
                    self.source = source;
                    match result {
                        Ok(converted) => {
                            let image = converted.decoded;
                            let options = if image.width <= 64 && image.height <= 64 {
                                TextureOptions::NEAREST
                            } else {
                                TextureOptions::LINEAR
                            };
                            self.ready = Some(ReadyIcon {
                                format: image.format,
                                width: image.width,
                                height: image.height,
                                texture: ctx.load_texture(
                                    "icon-converter-preview",
                                    image.image,
                                    options,
                                ),
                                text: converted.text,
                                note: converted.note,
                            });
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                #[cfg(not(target_os = "macos"))]
                IconJob::Dialog(result) if self.dialog_job == Some(outcome.job) => {
                    self.dialog_job = None;
                    self.handle_dialog_result(result, ctx);
                }
                IconJob::Saved(result) if self.save_worker == Some(outcome.job) => {
                    self.save_worker = None;
                    match result {
                        Ok(path) => {
                            self.saved_path = Some(path);
                            self.status = Some("icon-saved".into());
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                _ => {}
            }
        }
        if self.worker.is_some() || self.save_worker.is_some() || self.dialog_job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    pub(crate) fn show(
        &mut self,
        ctx: &Context,
        has_text_draft: bool,
        loc: &Localization,
    ) -> Option<IconImport> {
        self.poll(ctx);
        let pal = Palette::resolve(ctx);
        if !self.visible {
            return None;
        }
        let mut open = true;
        let mut import = None;
        let mut save_requested = false;
        let screen = ctx.content_rect();
        let response = egui::Window::new(loc.msg("icon-title"))
            .id(egui::Id::new("icon_converter"))
            .open(&mut open)
            .default_width(560.0)
            .default_height(520.0)
            .min_width(280.0)
            .max_width((screen.width() - 32.0).max(280.0))
            .max_height((screen.height() - 48.0).max(200.0))
            .vscroll(true)
            .show(ctx, |ui| {
                ui.label(loc.msg("icon-drop-hint"));
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(self.dialog_job.is_none() && self.worker.is_none(), egui::Button::new(loc.msg("icon-choose"))).clicked() {
                        self.dialog_generation = self.generation;
                        self.pick_file(false, ctx, loc);
                    }
                    ui.label(RichText::new(loc.msg("icon-formats")).small().color(pal.text_muted));
                });
                ui.add_space(8.0);
                let previous_mode = self.mode;
                ui.add_enabled_ui(self.worker.is_none(), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(loc.msg("icon-output"));
                        for mode in [ConversionMode::Base64, ConversionMode::DataUri, ConversionMode::EsiHex] {
                            ui.selectable_value(&mut self.mode, mode, loc.msg(match mode { ConversionMode::Base64 => "icon-base64", ConversionMode::DataUri => "icon-data-uri", ConversionMode::EsiHex => "icon-esi" }));
                        }
                    });
                });
                if self.mode != previous_mode {
                    if let Some(source) = self.source.clone() {
                        self.start_conversion(IconInput::Loaded(source), ctx);
                    } else {
                        self.error = None;
                    }
                }
                if self.mode == ConversionMode::EsiHex {
                    ui.label(RichText::new(loc.msg("icon-esi-hint")).small().color(pal.info));
                } else {
                    ui.label(RichText::new(loc.msg("icon-original-hint")).small().color(pal.text_muted));
                }
                ui.separator();
                if let Some(source) = &self.source {
                    ui.label(RichText::new(&source.name).strong());
                    ui.label(loc.msg_with("icon-file-size", Some(&crate::fluent_args!("size" => format_size(source.bytes.len())))));
                }
                if self.worker.is_some() {
                    ui.horizontal(|ui| { ui.spinner(); ui.label(loc.msg("icon-loading")); });
                }
                if self.save_worker.is_some() { ui.label(loc.msg("icon-saving")); }
                if let Some(error) = &self.error { ui.colored_label(pal.error, if error.starts_with("icon-") { loc.msg(error) } else { loc.msg_with("icon-error", Some(&crate::fluent_args!("message" => error.as_str()))) }); }
                if let Some(ready) = &self.ready {
                    if let Some(note) = &ready.note {
                        ui.label(RichText::new(conversion_note(note, loc)).small().color(pal.info));
                    }
                    ui.label(loc.msg_with("icon-summary", Some(&crate::fluent_args!("format" => ready.format.label(), "width" => ready.width, "height" => ready.height, "count" => ready.text.len() as i64))));
                    let size = preview_display_size(ready.width, ready.height, egui::vec2(ui.available_width().max(1.0), 160.0));
                    ui.add(egui::Image::from_texture(&ready.texture).fit_to_exact_size(size).bg_fill(pal.input_bg));
                    if matches!(ready.format, ImageFileFormat::Gif | ImageFileFormat::WebP | ImageFileFormat::Ico) {
                        ui.label(RichText::new(loc.msg("icon-frame-note")).small().color(pal.text_muted));
                    }
                    ui.collapsing(loc.msg("icon-encoded"), |ui| {
                        let mut excerpt = &ready.text[..ready.text.len().min(4096)];
                        egui::ScrollArea::vertical().id_salt("icon_encoded_text").max_height(100.0).show(ui, |ui| {
                            ui.add(egui::TextEdit::multiline(&mut excerpt).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY).desired_rows(4));
                        });
                        if ready.text.len() > 4096 { ui.label(loc.msg("icon-excerpt")); }
                    });
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(loc.msg("icon-copy")).clicked() {
                            ctx.copy_text(ready.text.to_string());
                            self.status = Some("icon-copied".into());
                        }
                        if ui.add_enabled(self.dialog_job.is_none() && self.save_worker.is_none(), egui::Button::new(loc.msg("icon-save"))).clicked() {
                            // Freeze the exact output chosen now, even if the mode changes while saving.
                            self.pending_export = Some(ready.text.clone());
                            save_requested = true;
                        }
                    });
                    ui.separator();
                    if let Some(target) = &self.target {
                        let compatible = !is_esi_icon(&target.name) || self.mode == ConversionMode::EsiHex;
                        if !compatible {
                            ui.colored_label(pal.warning, loc.msg("icon-requires-esi"));
                        }
                        if has_text_draft {
                            ui.colored_label(pal.warning, loc.msg("icon-replace-draft"));
                        }
                        ui.label(loc.msg_with("icon-target", Some(&crate::fluent_args!("name" => target.name.as_str()))));
                        if ui.add_enabled(compatible, egui::Button::new(loc.msg("icon-fill"))).clicked() {
                            import = Some(IconImport { target: target.clone(), mode: self.mode, text: ready.text.clone() });
                        }
                        ui.label(RichText::new(loc.msg("icon-review")).small());
                    } else {
                        ui.label(loc.msg("icon-no-target"));
                    }
                }
                if let Some(status) = &self.status { ui.label(RichText::new(loc.msg_with(status, Some(&crate::fluent_args!("path" => self.saved_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default())))).color(pal.info)); }
            });

        if save_requested {
            self.pick_file(true, ctx, loc);
        }
        if !open || import.is_some() {
            self.close();
        } else if let Some(response) = response {
            let inside = ctx.input(|i| {
                i.pointer
                    .latest_pos()
                    // Some desktop backends omit pointer coordinates during an OS drop.
                    .is_none_or(|p| response.response.rect.contains(p))
            });
            if inside && self.dialog_job.is_none() && self.worker.is_none() {
                let dropped = ctx.input(|i| i.raw.dropped_files.clone());
                if !dropped.is_empty() {
                    self.accept_drop(dropped, ctx);
                }
            }
        }
        import
    }

    fn accept_drop(&mut self, mut files: Vec<egui::DroppedFile>, ctx: &Context) {
        if files.len() != 1 {
            self.ready = None;
            self.source = None;
            self.error = Some("icon-drop-one".into());
            return;
        }
        let file = files.pop().expect("one file");
        if let Some(path) = file.path {
            self.start_conversion(IconInput::File(path), ctx);
        } else if let Some(bytes) = file.bytes {
            self.start_conversion(
                IconInput::Loaded(IconSource {
                    name: file.name,
                    bytes,
                }),
                ctx,
            );
        } else {
            self.ready = None;
            self.source = None;
            self.error = Some("icon-drop-invalid".into());
        }
    }
}

fn conversion_note(note: &ConversionNote, loc: &Localization) -> String {
    match note {
        ConversionNote::Original => loc.msg("icon-already-compliant"),
        ConversionNote::Converted {
            format,
            width,
            height,
            depth,
            single_frame,
        } => {
            let mut text = loc.msg_with("icon-converted", Some(&crate::fluent_args!("format" => format.label(), "width" => *width, "height" => *height, "depth" => depth.map(|d| format!(", {d}bpp")).unwrap_or_default())));
            if *single_frame {
                text.push(' ');
                text.push_str(&loc.msg("icon-single-frame"));
            }
            text
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::image_conversion::esi_test_bmp;

    fn source() -> IconSource {
        IconSource {
            name: "图标.bmp".into(),
            bytes: esi_test_bmp().into(),
        }
    }

    fn target() -> IconTarget {
        IconTarget {
            session: SessionId(1),
            revision: Revision(1),
            selection: crate::core::NodeId(2),
            element: crate::core::NodeId(2),
            content: None,
            name: "ImageData16x14".into(),
        }
    }

    fn wait_for_workers(converter: &mut IconConverter, ctx: &Context) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while converter.worker.is_some() || converter.save_worker.is_some() {
            assert!(
                std::time::Instant::now() < deadline,
                "worker did not finish"
            );
            converter.poll(ctx);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[test]
    fn selecting_another_target_permanently_clears_binding() {
        let ctx = Context::default();
        let mut converter = IconConverter::default();
        converter.open(Some(target()), &ctx);
        assert_eq!(converter.mode, ConversionMode::EsiHex);
        converter.sync_target(Some(&target()));
        assert!(converter.target.is_some());
        converter.sync_target(None);
        converter.sync_target(Some(&target()));
        assert!(converter.target.is_none());
    }

    #[test]
    fn closed_session_ignores_late_conversion_and_file_selection() {
        let ctx = Context::default();
        let mut converter = IconConverter::default();
        converter.open(Some(target()), &ctx);
        converter.dialog_generation = converter.generation;
        converter.start_conversion(IconInput::Loaded(source()), &ctx);
        converter.close();
        converter.open(None, &ctx);
        converter.handle_dialog_result(FileDialogResult::OpenImage(Some("stale.bmp".into())), &ctx);
        converter.poll(&ctx);
        wait_for_workers(&mut converter, &ctx);
        assert_eq!(converter.mode, ConversionMode::Base64);
        assert!(
            converter
                .ready
                .as_ref()
                .is_some_and(|r| !r.text.starts_with("424D"))
        );
        assert!(converter.error.is_none());
    }

    #[test]
    fn drop_converts_one_file_and_rejects_multiple_files() {
        let ctx = Context::default();
        let mut converter = IconConverter::default();
        converter.open(None, &ctx);
        let file = egui::DroppedFile {
            name: "图标.bmp".into(),
            bytes: Some(source().bytes),
            ..Default::default()
        };
        converter.accept_drop(vec![file.clone()], &ctx);
        wait_for_workers(&mut converter, &ctx);
        let expected = convert_icon(&source(), ConversionMode::Base64, 1024)
            .unwrap()
            .text;
        assert_eq!(converter.ready.as_ref().unwrap().text, expected);
        converter.handle_dialog_result(FileDialogResult::OpenImage(None), &ctx);
        assert_eq!(converter.ready.as_ref().unwrap().text, expected);
        converter.accept_drop(vec![file.clone(), file], &ctx);
        assert!(converter.ready.is_none());
        assert!(converter.error.as_ref().unwrap().contains("icon-drop-one"));
    }

    #[test]
    fn save_uses_frozen_output_even_after_changing_mode_and_closing() {
        let ctx = Context::default();
        let mut converter = IconConverter::default();
        converter.open(None, &ctx);
        converter.start_conversion(IconInput::Loaded(source()), &ctx);
        wait_for_workers(&mut converter, &ctx);
        let expected = converter.ready.as_ref().unwrap().text.clone();
        converter.pending_export = Some(expected.clone());
        converter.mode = ConversionMode::DataUri;
        converter.start_conversion(IconInput::Loaded(source()), &ctx);
        wait_for_workers(&mut converter, &ctx);
        converter.close();
        let file = tempfile::NamedTempFile::new().unwrap();
        converter.handle_dialog_result(
            FileDialogResult::SaveIconText(Some(file.path().to_path_buf())),
            &ctx,
        );
        wait_for_workers(&mut converter, &ctx);
        assert_eq!(std::fs::read_to_string(file.path()).unwrap(), &*expected);
        assert!(converter.error.is_none());
    }

    #[test]
    fn failed_new_file_cannot_leave_previous_output_importable() {
        let ctx = Context::default();
        let mut converter = IconConverter::default();
        converter.open(None, &ctx);
        converter.start_conversion(IconInput::Loaded(source()), &ctx);
        wait_for_workers(&mut converter, &ctx);
        assert!(converter.ready.is_some());
        let directory = tempfile::tempdir().unwrap();
        converter.start_conversion(IconInput::File(directory.path().join("missing.png")), &ctx);
        assert!(converter.ready.is_none());
        wait_for_workers(&mut converter, &ctx);
        assert!(converter.ready.is_none());
        assert!(converter.error.is_some());
    }

    #[test]
    fn os_drop_without_pointer_coordinates_is_received() {
        let ctx = Context::default();
        let mut converter = IconConverter::default();
        converter.open(None, &ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 500.0),
            )),
            dropped_files: vec![egui::DroppedFile {
                name: "icon.bmp".into(),
                bytes: Some(source().bytes),
                ..Default::default()
            }],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            converter.show(
                ctx,
                false,
                &Localization::with_language(super::super::localization::Language::English),
            );
        });
        wait_for_workers(&mut converter, &ctx);
        assert!(converter.ready.is_some());
    }

    #[test]
    fn copy_button_at_minimum_window_size_copies_the_full_output() {
        let ctx = Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 500.0));
        let input = || egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let mut converter = IconConverter::default();
        converter.open(Some(target()), &ctx);
        converter.start_conversion(IconInput::Loaded(source()), &ctx);
        wait_for_workers(&mut converter, &ctx);
        let expected = converter.ready.as_ref().unwrap().text.clone();
        for _ in 0..3 {
            let _ = ctx.run(input(), |ctx| {
                converter.show(
                    ctx,
                    false,
                    &Localization::with_language(super::super::localization::Language::English),
                );
            });
        }
        let output = ctx.run(input(), |ctx| {
            converter.show(
                ctx,
                false,
                &Localization::with_language(super::super::localization::Language::English),
            );
        });
        let (rect, clip) = output
            .shapes
            .iter()
            .find_map(|clipped| {
                if let egui::Shape::Text(text) = &clipped.shape {
                    (text.galley.job.text == "Copy")
                        .then(|| (text.visual_bounding_rect(), clipped.clip_rect))
                } else {
                    None
                }
            })
            .expect("Copy button is rendered");
        assert!(screen.contains_rect(rect));
        assert!(
            clip.contains(rect.center()),
            "Copy must be visible at 800x500"
        );
        for pressed in [true, false] {
            let mut input = input();
            input.events = vec![
                egui::Event::PointerMoved(rect.center()),
                egui::Event::PointerButton {
                    pos: rect.center(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ];
            let output = ctx.run(input, |ctx| {
                converter.show(
                    ctx,
                    false,
                    &Localization::with_language(super::super::localization::Language::English),
                );
            });
            if !pressed {
                assert!(output.platform_output.commands.iter().any(|command| {
                    matches!(command, egui::OutputCommand::CopyText(text) if text == &*expected)
                }));
            }
        }
    }
}
