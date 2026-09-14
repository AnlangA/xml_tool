use std::path::PathBuf;
use std::sync::{
    Arc,
    mpsc::{self, Receiver, TryRecvError},
};

use egui::{Context, RichText, TextureHandle, TextureOptions};

use super::base64_image::preview_display_size;
use super::file_dialog::{FileDialogAction, FileDialogManager, FileDialogResult};
use super::image_conversion::{
    ConversionMode, ConvertedIcon, IconSource, ImageFileFormat, convert_icon, is_esi_icon,
    read_icon,
};
use super::theme::Theme;
use crate::utils::format_size;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct IconTarget {
    pub root_id: u64,
    pub node_id: u64,
    pub name: String,
}

pub(super) struct IconImport {
    pub target: IconTarget,
    pub mode: ConversionMode,
    pub text: Arc<str>,
}

enum IconInput {
    File(PathBuf),
    Loaded(IconSource),
}

struct ConversionReply {
    source: Option<IconSource>,
    result: Result<ConvertedIcon, String>,
}

struct ReadyIcon {
    format: ImageFileFormat,
    width: u32,
    height: u32,
    texture: TextureHandle,
    text: Arc<str>,
    note: Option<String>,
}

#[derive(Default)]
pub(super) struct IconConverter {
    visible: bool,
    target: Option<IconTarget>,
    mode: ConversionMode,
    source: Option<IconSource>,
    ready: Option<ReadyIcon>,
    worker: Option<Receiver<ConversionReply>>,
    save_worker: Option<Receiver<Result<PathBuf, String>>>,
    file_dialog: FileDialogManager,
    pending_export: Option<Arc<str>>,
    generation: u64,
    dialog_generation: u64,
    error: Option<String>,
    status: Option<String>,
}

impl IconConverter {
    pub(super) fn open(&mut self, target: Option<IconTarget>, ctx: &Context) {
        self.generation = self.generation.wrapping_add(1);
        self.visible = true;
        self.mode = if target.as_ref().is_some_and(|t| is_esi_icon(&t.name)) {
            ConversionMode::EsiHex
        } else {
            ConversionMode::Base64
        };
        self.target = target;
        self.worker = None;
        self.ready = None;
        self.error = None;
        self.status = None;
        if let Some(source) = self.source.clone() {
            self.start_conversion(IconInput::Loaded(source), ctx);
        }
    }

    pub(super) fn sync_target(&mut self, current: Option<&IconTarget>) {
        if self
            .target
            .as_ref()
            .is_some_and(|target| Some(target) != current)
        {
            self.target = None;
            self.status = Some("Import target changed. Reopen Import Icon on the desired leaf element. You can still copy or save this conversion.".into());
        }
    }

    fn close(&mut self) {
        self.visible = false;
        self.target = None;
        self.worker = None;
        self.generation = self.generation.wrapping_add(1);
    }

    // Each request owns a separate channel. Replacing/dropping its receiver
    // ensures a late result can never overwrite a newer conversion.
    fn start_conversion(&mut self, input: IconInput, ctx: &Context) {
        self.ready = None;
        self.error = None;
        self.status = None;
        self.source = match &input {
            IconInput::File(_) => None,
            IconInput::Loaded(source) => Some(source.clone()),
        };
        let (tx, rx) = mpsc::channel();
        self.worker = Some(rx);
        let mode = self.mode;
        let side = ctx.input(|i| u32::try_from(i.max_texture_side).unwrap_or(u32::MAX));
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let source = match input {
                IconInput::Loaded(source) => Ok(source),
                IconInput::File(path) => read_icon(&path),
            };
            let reply = match source {
                Ok(source) => {
                    let result = convert_icon(&source, mode, side);
                    ConversionReply {
                        source: Some(source),
                        result,
                    }
                }
                Err(error) => ConversionReply {
                    source: None,
                    result: Err(error),
                },
            };
            let _ = tx.send(reply);
            ctx.request_repaint();
        });
    }

    fn handle_dialog_result(&mut self, result: FileDialogResult, ctx: &Context) {
        match result {
            FileDialogResult::OpenImage(Some(path))
                if self.visible && self.dialog_generation == self.generation =>
            {
                self.start_conversion(IconInput::File(path), ctx);
            }
            FileDialogResult::SaveIconText(Some(path)) => {
                if let Some(text) = self.pending_export.take() {
                    let (tx, rx) = mpsc::channel();
                    self.save_worker = Some(rx);
                    let ctx = ctx.clone();
                    std::thread::spawn(move || {
                        let result = std::fs::write(&path, text.as_bytes())
                            .map(|()| path)
                            .map_err(|e| format!("Cannot save encoded text: {e}"));
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    });
                }
            }
            FileDialogResult::SaveIconText(None) => {
                self.pending_export = None;
                self.status = Some("Text save cancelled.".into());
            }
            _ => {}
        }
    }

    fn poll(&mut self, ctx: &Context) {
        if let Some(result) = self.file_dialog.poll() {
            self.handle_dialog_result(result, ctx);
        }
        if let Some(rx) = &self.worker {
            match rx.try_recv() {
                Ok(reply) => {
                    self.worker = None;
                    self.source = reply.source;
                    match reply.result {
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
                Err(TryRecvError::Disconnected) => {
                    self.worker = None;
                    self.error = Some(
                        "Image conversion stopped unexpectedly. Choose the file again.".into(),
                    );
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if let Some(rx) = &self.save_worker {
            match rx.try_recv() {
                Ok(result) => {
                    self.save_worker = None;
                    match result {
                        Ok(path) => {
                            self.status = Some(format!("Saved encoded text: {}", path.display()))
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Err(TryRecvError::Disconnected) => {
                    self.save_worker = None;
                    self.error = Some("Text save stopped unexpectedly. Please retry.".into());
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if self.worker.is_some() || self.save_worker.is_some() || self.file_dialog.is_pending() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    pub(super) fn show(&mut self, ctx: &Context, has_text_draft: bool) -> Option<IconImport> {
        self.poll(ctx);
        if !self.visible {
            return None;
        }
        let mut open = true;
        let mut import = None;
        let screen = ctx.content_rect();
        let response = egui::Window::new("Icon Converter")
            .id(egui::Id::new("icon_converter"))
            .open(&mut open)
            .default_width(560.0)
            .default_height(420.0)
            .min_width(280.0)
            .max_width((screen.width() - 32.0).max(280.0))
            .max_height((screen.height() - 48.0).max(200.0))
            .vscroll(true)
            .show(ctx, |ui| {
                ui.label("Choose an image or drop one file anywhere in this window.");
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(!self.file_dialog.is_pending() && self.worker.is_none(), egui::Button::new("Choose Image…")).clicked() {
                        self.dialog_generation = self.generation;
                        self.file_dialog.open(FileDialogAction::OpenImage);
                    }
                    ui.label(RichText::new("PNG · JPEG · GIF · WebP · BMP · ICO | max 8 MiB").small().color(Theme::TEXT_MUTED));
                });
                ui.add_space(8.0);
                let previous_mode = self.mode;
                ui.add_enabled_ui(self.worker.is_none(), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Output:");
                        for mode in [ConversionMode::Base64, ConversionMode::DataUri, ConversionMode::EsiHex] {
                            ui.selectable_value(&mut self.mode, mode, mode.label());
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
                    ui.label(RichText::new("Creates a 16x14, 16-color BMP (4bpp). Fits the image with transparent padding; magenta (#FF00FF) is transparent.").small().color(Theme::INFO));
                } else {
                    ui.label(RichText::new("Encodes the original file bytes without changing the image.").small().color(Theme::TEXT_MUTED));
                }
                ui.separator();
                if let Some(source) = &self.source {
                    ui.label(RichText::new(&source.name).strong());
                    ui.label(format!("File size: {}", format_size(source.bytes.len())));
                }
                if self.worker.is_some() {
                    ui.horizontal(|ui| { ui.spinner(); ui.label("Reading and converting image…"); });
                }
                if self.save_worker.is_some() { ui.label("Saving encoded text…"); }
                if let Some(error) = &self.error { ui.colored_label(Theme::ERROR, error); }
                if let Some(ready) = &self.ready {
                    if let Some(note) = &ready.note {
                        ui.label(RichText::new(note).small().color(Theme::INFO));
                    }
                    ui.label(format!("{} | {}x{} | {} characters", ready.format.label(), ready.width, ready.height, ready.text.len()));
                    let size = preview_display_size(ready.width, ready.height, egui::vec2(ui.available_width().max(1.0), 160.0));
                    ui.add(egui::Image::from_texture(&ready.texture).fit_to_exact_size(size).bg_fill(Theme::SURFACE1));
                    if matches!(ready.format, ImageFileFormat::Gif | ImageFileFormat::WebP | ImageFileFormat::Ico) {
                        ui.label(RichText::new("Preview shows the decoder's default frame/icon. The complete original file is preserved.").small().color(Theme::TEXT_MUTED));
                    }
                    ui.collapsing("Encoded text", |ui| {
                        let mut excerpt = &ready.text[..ready.text.len().min(4096)];
                        egui::ScrollArea::vertical().id_salt("icon_encoded_text").max_height(100.0).show(ui, |ui| {
                            ui.add(egui::TextEdit::multiline(&mut excerpt).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY).desired_rows(4));
                        });
                        if ready.text.len() > 4096 { ui.label("Showing the first 4096 characters. Copy and Save include the full result."); }
                    });
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Copy").clicked() {
                            ctx.copy_text(ready.text.to_string());
                            self.status = Some("Complete encoded text copied.".into());
                        }
                        if ui.add_enabled(!self.file_dialog.is_pending() && self.save_worker.is_none(), egui::Button::new("Save Text…")).clicked() {
                            // Freeze the exact output chosen now, even if the mode changes while saving.
                            self.pending_export = Some(ready.text.clone());
                            self.file_dialog.open(FileDialogAction::SaveIconText);
                        }
                    });
                    ui.separator();
                    if let Some(target) = &self.target {
                        let compatible = !is_esi_icon(&target.name) || self.mode == ConversionMode::EsiHex;
                        if !compatible {
                            ui.colored_label(Theme::WARNING, "ImageData16x14 requires ESI icon (hex) output.");
                        }
                        if has_text_draft {
                            ui.colored_label(Theme::WARNING, "This replaces your unapplied text draft. Name and attribute drafts are kept.");
                        }
                        ui.label(format!("Target: <{}> (node {})", target.name, target.node_id));
                        if ui.add_enabled(compatible, egui::Button::new("Fill Text Draft")).clicked() {
                            import = Some(IconImport { target: target.clone(), mode: self.mode, text: ready.text.clone() });
                        }
                        ui.label(RichText::new("Review in Details, then use Apply Changes or Reset.").small());
                    } else {
                        ui.label("To import, select a leaf element and open Import Icon in Details.");
                    }
                }
                if let Some(status) = &self.status { ui.label(RichText::new(status).color(Theme::INFO)); }
            });

        if !open || import.is_some() {
            self.close();
        } else if let Some(response) = response {
            let inside = ctx.input(|i| {
                i.pointer
                    .latest_pos()
                    // Some desktop backends omit pointer coordinates during an OS drop.
                    .is_none_or(|p| response.response.rect.contains(p))
            });
            if inside && !self.file_dialog.is_pending() && self.worker.is_none() {
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
            self.error = Some("Drop one image at a time.".into());
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
            self.error = Some("The dropped item has no readable image data.".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::image_conversion::esi_test_bmp;
    use super::*;

    fn source() -> IconSource {
        IconSource {
            name: "图标.bmp".into(),
            bytes: esi_test_bmp().into(),
        }
    }

    fn target() -> IconTarget {
        IconTarget {
            root_id: 1,
            node_id: 2,
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
        let (tx, rx) = mpsc::channel();
        converter.worker = Some(rx);
        tx.send(ConversionReply {
            source: Some(source()),
            result: convert_icon(&source(), ConversionMode::EsiHex, 1024),
        })
        .ok()
        .unwrap();
        converter.close();
        converter.open(None, &ctx);
        converter.handle_dialog_result(FileDialogResult::OpenImage(Some("stale.bmp".into())), &ctx);
        converter.poll(&ctx);
        assert!(converter.source.is_none());
        assert!(converter.ready.is_none());
        assert!(converter.worker.is_none());
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
        assert!(converter.error.as_ref().unwrap().contains("one image"));
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
            converter.show(ctx, false);
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
                converter.show(ctx, false);
            });
        }
        let output = ctx.run(input(), |ctx| {
            converter.show(ctx, false);
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
                converter.show(ctx, false);
            });
            if !pressed {
                assert!(output.platform_output.commands.iter().any(|command| {
                    matches!(command, egui::OutputCommand::CopyText(text) if text == &*expected)
                }));
            }
        }
    }
}
