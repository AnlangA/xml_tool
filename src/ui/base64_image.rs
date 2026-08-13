use std::io::Cursor;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use egui::{ColorImage, RichText, TextureHandle, TextureOptions};
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};

use crate::utils::format_size;

use super::theme::Theme;

const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;
const MAX_DECODED_BYTES: usize = 8 * 1024 * 1024;
const MAX_ENCODED_CHARS: usize = MAX_DECODED_BYTES.div_ceil(3) * 4;
const MAX_IMAGE_DIMENSION: u32 = 4_096;
const MAX_IMAGE_PIXELS: u64 = 8 * 1024 * 1024;
const MAX_DECODE_ALLOC_BYTES: u64 = 64 * 1024 * 1024;
const MIN_PLAIN_BASE64_CHARS: usize = 32;
const MAX_DATA_URI_METADATA_BYTES: usize = 1_024;
const MAX_PREVIEW_TEXTURE_SIDE: u32 = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceEncoding {
    Base64,
    Hexadecimal,
}

impl SourceEncoding {
    fn label(self) -> &'static str {
        match self {
            Self::Base64 => "Base64",
            Self::Hexadecimal => "hex",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewImageFormat {
    Png,
    Jpeg,
    Gif,
    WebP,
    Bmp,
}

impl PreviewImageFormat {
    fn from_image_format(format: ImageFormat) -> Option<Self> {
        match format {
            ImageFormat::Png => Some(Self::Png),
            ImageFormat::Jpeg => Some(Self::Jpeg),
            ImageFormat::Gif => Some(Self::Gif),
            ImageFormat::WebP => Some(Self::WebP),
            ImageFormat::Bmp => Some(Self::Bmp),
            _ => None,
        }
    }

    fn from_mime_type(mime_type: &str) -> Option<Self> {
        match mime_type {
            "image/png" | "image/x-png" => Some(Self::Png),
            "image/jpeg" | "image/jpg" => Some(Self::Jpeg),
            "image/gif" => Some(Self::Gif),
            "image/webp" => Some(Self::WebP),
            "image/bmp" | "image/x-bmp" | "image/x-ms-bmp" => Some(Self::Bmp),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Gif => "GIF",
            Self::WebP => "WebP",
            Self::Bmp => "BMP",
        }
    }
}

#[derive(Debug)]
struct DecodedPreview {
    image: ColorImage,
    format: PreviewImageFormat,
    encoding: SourceEncoding,
    width: u32,
    height: u32,
    encoded_bytes: usize,
    warning: Option<String>,
}

#[derive(Debug)]
enum Detection {
    NotImage,
    Ready(DecodedPreview),
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PreviewKey {
    node_id: u64,
    text_revision: u64,
}

struct RenderedPreview {
    texture: TextureHandle,
    format: PreviewImageFormat,
    encoding: SourceEncoding,
    width: u32,
    height: u32,
    encoded_bytes: usize,
    warning: Option<String>,
}

#[derive(Default)]
enum PreviewState {
    #[default]
    Hidden,
    Ready(RenderedPreview),
    Error(String),
}

/// Cached renderer for an encoded image contained in the selected element's text.
///
/// Decoding happens only when the selection or text revision changes, rather than
/// on every egui frame.
#[derive(Default)]
pub(super) struct EncodedImagePreview {
    key: Option<PreviewKey>,
    state: PreviewState,
}

impl EncodedImagePreview {
    pub(super) fn clear(&mut self) {
        self.key = None;
        self.state = PreviewState::Hidden;
    }

    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        node_id: u64,
        text_revision: u64,
        element_name: &str,
        source: &str,
        previewing_unapplied_text: bool,
    ) {
        let key = PreviewKey {
            node_id,
            text_revision,
        };
        if self.key != Some(key) {
            self.rebuild(ui.ctx(), key, element_name, source);
        }

        match &self.state {
            PreviewState::Hidden => {}
            PreviewState::Ready(preview) => {
                ui.add_space(10.0);
                egui::Frame::new()
                    .fill(Theme::CARD_BG)
                    .inner_margin(8.0)
                    .corner_radius(4.0)
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new("Image Preview").strong().color(Theme::INFO));
                            ui.separator();
                            ui.label(
                                RichText::new(format!(
                                    "{} | {} | {}x{} | {}",
                                    preview.format.label(),
                                    preview.encoding.label(),
                                    preview.width,
                                    preview.height,
                                    format_size(preview.encoded_bytes),
                                ))
                                .small()
                                .color(Theme::TEXT_MUTED),
                            );
                        });

                        if previewing_unapplied_text {
                            ui.label(
                                RichText::new("Previewing unapplied text")
                                    .small()
                                    .italics()
                                    .color(Theme::WARNING),
                            );
                        }

                        if let Some(warning) = &preview.warning {
                            ui.label(RichText::new(warning).small().color(Theme::WARNING));
                        }

                        ui.add_space(8.0);
                        let max_width = ui.available_width().clamp(1.0, 512.0);
                        let display_size = preview_display_size(
                            preview.width,
                            preview.height,
                            egui::vec2(max_width, 360.0),
                        );
                        ui.vertical_centered(|ui| {
                            ui.add(
                                egui::Image::from_texture(&preview.texture)
                                    .fit_to_exact_size(display_size)
                                    .bg_fill(Theme::SURFACE1)
                                    .alt_text("Encoded image preview"),
                            );
                        });
                    });
            }
            PreviewState::Error(message) => {
                ui.add_space(10.0);
                egui::Frame::new()
                    .fill(Theme::ERROR_BG)
                    .inner_margin(8.0)
                    .corner_radius(4.0)
                    .show(ui, |ui| {
                        ui.label(RichText::new("Image Preview").strong().color(Theme::ERROR));
                        ui.label(RichText::new(message).small().color(Theme::TEXT_SECONDARY));
                    });
            }
        }
    }

    fn rebuild(&mut self, ctx: &egui::Context, key: PreviewKey, element_name: &str, source: &str) {
        self.key = Some(key);
        let runtime_texture_side = ctx.input(|input| input.max_texture_side);
        let runtime_texture_side = u32::try_from(runtime_texture_side).unwrap_or(u32::MAX);
        let use_ethercat_transparency = element_name
            .rsplit(':')
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("ImageData16x14"));
        self.state = match detect_encoded_image_for_texture(
            source,
            runtime_texture_side,
            use_ethercat_transparency,
        ) {
            Detection::NotImage => PreviewState::Hidden,
            Detection::Error(message) => PreviewState::Error(message),
            Detection::Ready(decoded) => {
                let texture_options = if decoded.width <= 64 && decoded.height <= 64 {
                    TextureOptions::NEAREST
                } else {
                    TextureOptions::LINEAR
                };
                let texture = ctx.load_texture(
                    format!(
                        "encoded-image-preview-{}-{}",
                        key.node_id, key.text_revision
                    ),
                    decoded.image,
                    texture_options,
                );
                PreviewState::Ready(RenderedPreview {
                    texture,
                    format: decoded.format,
                    encoding: decoded.encoding,
                    width: decoded.width,
                    height: decoded.height,
                    encoded_bytes: decoded.encoded_bytes,
                    warning: decoded.warning,
                })
            }
        };
    }
}

#[cfg(test)]
fn detect_base64_image(source: &str) -> Detection {
    detect_encoded_image_for_texture(source, MAX_PREVIEW_TEXTURE_SIDE, false)
}

fn detect_encoded_image_for_texture(
    source: &str,
    runtime_texture_side: u32,
    use_ethercat_transparency: bool,
) -> Detection {
    let source = source.trim_matches(char::is_whitespace);
    if source.is_empty() {
        return Detection::NotImage;
    }

    let candidate = match image_candidate(source) {
        Ok(Some(candidate)) => candidate,
        Ok(None) => return Detection::NotImage,
        Err(message) => return Detection::Error(message),
    };

    let bytes = match decode_candidate(&candidate) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Detection::NotImage,
        Err(message) => return Detection::Error(message),
    };

    if bytes.len() > MAX_DECODED_BYTES {
        return Detection::Error(format!(
            "Image preview is limited to {} of decoded data.",
            format_size(MAX_DECODED_BYTES)
        ));
    }

    let image_format = match image::guess_format(&bytes) {
        Ok(format) => format,
        Err(_) if !candidate.explicit => return Detection::NotImage,
        Err(_) => {
            return Detection::Error("The decoded data is not a supported image.".to_string());
        }
    };
    let Some(format) = PreviewImageFormat::from_image_format(image_format) else {
        return Detection::Error(
            "Only PNG, JPEG, GIF, WebP, and BMP previews are supported.".to_string(),
        );
    };

    let mut dimension_reader = ImageReader::with_format(Cursor::new(&bytes), image_format);
    dimension_reader.limits(image_limits());
    let (width, height) = match dimension_reader.into_dimensions() {
        Ok(dimensions) => dimensions,
        Err(error) => {
            return Detection::Error(format!("The decoded image is invalid: {error}"));
        }
    };

    if let Err(message) = validate_image_dimensions(width, height) {
        return Detection::Error(message);
    }

    let mut decode_reader = ImageReader::with_format(Cursor::new(&bytes), image_format);
    decode_reader.limits(image_limits());
    let mut decoder = match decode_reader.into_decoder() {
        Ok(decoder) => decoder,
        Err(error) => {
            return Detection::Error(format!("The decoded image is invalid: {error}"));
        }
    };
    let orientation = match decoder.orientation() {
        Ok(orientation) => orientation,
        Err(error) => {
            return Detection::Error(format!("Cannot read the image orientation: {error}"));
        }
    };
    let mut image = match DynamicImage::from_decoder(decoder) {
        Ok(image) => image,
        Err(error) => {
            return Detection::Error(format!("The decoded image is invalid: {error}"));
        }
    };
    image.apply_orientation(orientation);
    let (width, height) = (image.width(), image.height());
    if let Err(message) = validate_image_dimensions(width, height) {
        return Detection::Error(message);
    }

    if use_ethercat_transparency {
        let mut rgba = image.into_rgba8();
        for pixel in rgba.pixels_mut() {
            if pixel.0[..3] == [0xff, 0x00, 0xff] {
                pixel.0[3] = 0;
            }
        }
        image = DynamicImage::ImageRgba8(rgba);
    }

    let max_texture_side = runtime_texture_side
        .clamp(1, MAX_PREVIEW_TEXTURE_SIDE)
        .min(MAX_IMAGE_DIMENSION);
    if width > max_texture_side || height > max_texture_side {
        image = image.resize(max_texture_side, max_texture_side, FilterType::Triangle);
    }
    let rgba = image.into_rgba8();
    let color_image = ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    );

    let warning = candidate.declared_format.and_then(|declared| {
        (declared != format).then(|| {
            format!(
                "The data URI declares {}, but the image bytes are {}.",
                declared.label(),
                format.label()
            )
        })
    });

    Detection::Ready(DecodedPreview {
        image: color_image,
        format,
        encoding: candidate.encoding,
        width,
        height,
        encoded_bytes: bytes.len(),
        warning,
    })
}

fn preview_display_size(width: u32, height: u32, max_size: egui::Vec2) -> egui::Vec2 {
    let natural_size = egui::vec2(width as f32, height as f32);
    let fit_scale = (max_size.x / natural_size.x).min(max_size.y / natural_size.y);
    let scale = if natural_size.max_elem() < 96.0 {
        (96.0 / natural_size.max_elem()).min(fit_scale)
    } else {
        1.0_f32.min(fit_scale)
    };
    natural_size * scale
}

struct ImageCandidate<'a> {
    payload: &'a str,
    declared_format: Option<PreviewImageFormat>,
    explicit: bool,
    encoding: SourceEncoding,
}

fn image_candidate(source: &str) -> Result<Option<ImageCandidate<'_>>, String> {
    let is_data_uri = source
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"));
    if !is_data_uri {
        if source.len() > MAX_SOURCE_BYTES {
            return if looks_like_base64(source) {
                Err(format!(
                    "Encoded image text is limited to {}.",
                    format_size(MAX_SOURCE_BYTES)
                ))
            } else {
                Ok(None)
            };
        }
        if has_hex_image_signature(source) {
            return Ok(Some(ImageCandidate {
                payload: source,
                declared_format: None,
                explicit: true,
                encoding: SourceEncoding::Hexadecimal,
            }));
        }
        return Ok(Some(ImageCandidate {
            payload: source,
            declared_format: None,
            explicit: false,
            encoding: SourceEncoding::Base64,
        }));
    }

    if source.len() > MAX_SOURCE_BYTES + MAX_DATA_URI_METADATA_BYTES + 6 {
        return Err(format!(
            "Encoded image text is limited to {}.",
            format_size(MAX_SOURCE_BYTES)
        ));
    }

    let Some((metadata, payload)) = source[5..].split_once(',') else {
        return if source[5..]
            .trim_start()
            .get(..6)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("image/"))
        {
            Err("The image data URI is missing its comma-separated payload.".to_string())
        } else {
            Ok(None)
        };
    };
    if metadata.len() > MAX_DATA_URI_METADATA_BYTES {
        return Err("The image data URI metadata is too long.".to_string());
    }

    let mut parts = metadata.split(';');
    let mime_type = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    if !mime_type.starts_with("image/") {
        return Ok(None);
    }

    let Some(declared_format) = PreviewImageFormat::from_mime_type(&mime_type) else {
        return Err("Only PNG, JPEG, GIF, WebP, and BMP data URIs can be previewed.".to_string());
    };
    let mut has_base64_marker = false;
    for parameter in parts {
        let parameter = parameter.trim();
        if parameter.eq_ignore_ascii_case("base64") {
            if has_base64_marker {
                return Err("The image data URI has a duplicate Base64 marker.".to_string());
            }
            has_base64_marker = true;
        } else if has_base64_marker {
            return Err("The Base64 marker must be the final data URI parameter.".to_string());
        } else if parameter.is_empty() || !parameter.contains('=') {
            return Err("The image data URI contains an invalid parameter.".to_string());
        }
    }
    if !has_base64_marker {
        return Err("The image data URI is not Base64 encoded.".to_string());
    }

    Ok(Some(ImageCandidate {
        payload,
        declared_format: Some(declared_format),
        explicit: true,
        encoding: SourceEncoding::Base64,
    }))
}

fn decode_candidate(candidate: &ImageCandidate<'_>) -> Result<Option<Vec<u8>>, String> {
    match candidate.encoding {
        SourceEncoding::Base64 => {
            let Some(payload) = clean_payload(candidate.payload, candidate.explicit)? else {
                return Ok(None);
            };
            match decode_payload(&payload) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(()) if !candidate.explicit => Ok(None),
                Err(()) => Err("The image payload is not valid Base64.".to_string()),
            }
        }
        SourceEncoding::Hexadecimal => decode_hex_payload(candidate.payload).map(Some),
    }
}

fn clean_payload(payload: &str, explicit: bool) -> Result<Option<Vec<u8>>, String> {
    if payload.len() > MAX_SOURCE_BYTES {
        if explicit || looks_like_base64(payload) {
            return Err(format!(
                "Encoded image text is limited to {}.",
                format_size(MAX_SOURCE_BYTES)
            ));
        }
        return Ok(None);
    }

    let cleaned: Vec<u8> = payload
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    if cleaned.is_empty() {
        return if explicit {
            Err("The image data URI has an empty payload.".to_string())
        } else {
            Ok(None)
        };
    }
    if !explicit && cleaned.len() < MIN_PLAIN_BASE64_CHARS {
        return Ok(None);
    }
    if !cleaned.iter().copied().all(is_base64_byte) {
        return if explicit {
            Err("The image payload contains characters that are not valid Base64.".to_string())
        } else {
            Ok(None)
        };
    }
    if cleaned.len() > MAX_ENCODED_CHARS {
        return Err(format!(
            "Image preview is limited to {} of decoded data.",
            format_size(MAX_DECODED_BYTES)
        ));
    }

    Ok(Some(cleaned))
}

fn has_hex_image_signature(value: &str) -> bool {
    let digits: Vec<u8> = value
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .map(|byte| byte.to_ascii_uppercase())
        .collect();
    if digits.len() < 8
        || !digits.len().is_multiple_of(2)
        || !digits.iter().all(u8::is_ascii_hexdigit)
    {
        return false;
    }

    digits.starts_with(b"424D")
        || digits.starts_with(b"89504E47")
        || digits.starts_with(b"FFD8FF")
        || digits.starts_with(b"47494638")
        || (digits.starts_with(b"52494646") && digits.get(16..24) == Some(b"57454250"))
}

fn decode_hex_payload(payload: &str) -> Result<Vec<u8>, String> {
    if payload.len() > MAX_SOURCE_BYTES {
        return Err(format!(
            "Hex image text is limited to {}.",
            format_size(MAX_SOURCE_BYTES)
        ));
    }

    let digits: Vec<u8> = payload
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    if !digits.len().is_multiple_of(2) {
        return Err("The hexadecimal image payload has an odd number of digits.".to_string());
    }
    if digits.len() / 2 > MAX_DECODED_BYTES {
        return Err(format!(
            "Image preview is limited to {} of decoded data.",
            format_size(MAX_DECODED_BYTES)
        ));
    }

    digits
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let high = hex_value(pair[0])?;
            let low = hex_value(pair[1])?;
            Ok(high << 4 | low)
        })
        .collect::<Result<Vec<_>, _>>()
}

fn hex_value(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err("The image payload contains a non-hexadecimal character.".to_string()),
    }
}

fn looks_like_base64(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_whitespace() || is_base64_byte(byte))
}

fn is_base64_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'-' | b'_' | b'=')
}

fn decode_payload(payload: &[u8]) -> Result<Vec<u8>, ()> {
    let uses_url_safe = payload.iter().any(|byte| matches!(byte, b'-' | b'_'));
    let uses_standard_symbols = payload.iter().any(|byte| matches!(byte, b'+' | b'/'));
    if uses_url_safe && uses_standard_symbols {
        return Err(());
    }

    let decoded = if uses_url_safe {
        URL_SAFE
            .decode(payload)
            .or_else(|_| URL_SAFE_NO_PAD.decode(payload))
    } else {
        STANDARD
            .decode(payload)
            .or_else(|_| STANDARD_NO_PAD.decode(payload))
    };
    decoded.map_err(|_| ())
}

fn image_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC_BYTES);
    limits
}

fn validate_image_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("The decoded image has invalid dimensions.".to_string());
    }

    let pixel_count = u64::from(width) * u64::from(height);
    if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION || pixel_count > MAX_IMAGE_PIXELS
    {
        return Err(format!(
            "Image dimensions exceed the preview limit (maximum {MAX_IMAGE_DIMENSION}x{MAX_IMAGE_DIMENSION} and {MAX_IMAGE_PIXELS} pixels)."
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgba, RgbaImage};

    fn image_bytes(format: ImageFormat, width: u32, height: u32) -> Vec<u8> {
        let image = RgbaImage::from_pixel(width, height, Rgba([10, 20, 30, 255]));
        let mut output = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image)
            .write_to(&mut output, format)
            .expect("encode test image");
        output.into_inner()
    }

    fn png_bytes() -> Vec<u8> {
        image_bytes(ImageFormat::Png, 2, 1)
    }

    fn encoded_png() -> String {
        STANDARD.encode(png_bytes())
    }

    const ETHERCAT_4BPP_BMP_HEX: &str = "424DE6000000000000007600000028000000100000000E000000010004000000000070000000120B0000120B0000100000001000000000000000000080000080000000808000800000008000800080800000C0C0C000808080000000FF0000FF000000FFFF00FF000000FF00FF00FFFF0000FFFFFF009D9DD99DD9DDD9DD9D9D9DD9D9DDD9DD999D9DD9D999D9999D9D9DD9D9DDD9DD9D9DD99DD999D999DDDDDDDDDDDDDDDD88888888888888888888888888888888DDDDDDDDDDDDDDDD999D999DD99DD9D99D9D9DDD9DD9D9D999DD999D9DDDD99D9D9D9DDD9DD9D99D999D999DD99DD9D9";

    fn jpeg_with_exif_orientation(orientation: u8) -> Vec<u8> {
        let jpeg = image_bytes(ImageFormat::Jpeg, 2, 1);
        assert!(jpeg.starts_with(&[0xff, 0xd8]));

        let mut app1 = vec![
            0xff,
            0xe1,
            0x00,
            0x22,
            b'E',
            b'x',
            b'i',
            b'f',
            0x00,
            0x00,
            b'M',
            b'M',
            0x00,
            0x2a,
            0x00,
            0x00,
            0x00,
            0x08,
            0x00,
            0x01,
            0x01,
            0x12,
            0x00,
            0x03,
            0x00,
            0x00,
            0x00,
            0x01,
            0x00,
            orientation,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
        ];
        let mut oriented = Vec::with_capacity(jpeg.len() + app1.len());
        oriented.extend_from_slice(&jpeg[..2]);
        oriented.append(&mut app1);
        oriented.extend_from_slice(&jpeg[2..]);
        oriented
    }

    fn expect_ready(detection: Detection) -> DecodedPreview {
        match detection {
            Detection::Ready(preview) => preview,
            Detection::NotImage => panic!("expected image, got ordinary text"),
            Detection::Error(message) => panic!("expected image, got error: {message}"),
        }
    }

    #[test]
    fn detects_plain_base64_png() {
        let preview = expect_ready(detect_base64_image(&encoded_png()));

        assert_eq!(preview.format, PreviewImageFormat::Png);
        assert_eq!((preview.width, preview.height), (2, 1));
        assert_eq!(preview.image.size, [2, 1]);
        assert!(preview.warning.is_none());
    }

    #[test]
    fn detects_ethercat_hex_binary_bmp() {
        let preview = expect_ready(detect_base64_image(ETHERCAT_4BPP_BMP_HEX));

        assert_eq!(preview.format, PreviewImageFormat::Bmp);
        assert_eq!(preview.encoding, SourceEncoding::Hexadecimal);
        assert_eq!((preview.width, preview.height), (16, 14));
        assert_eq!(preview.encoded_bytes, 230);
        assert_eq!(preview.image.size, [16, 14]);
    }

    #[test]
    fn applies_ethercat_magenta_transparency() {
        let preview = expect_ready(detect_encoded_image_for_texture(
            ETHERCAT_4BPP_BMP_HEX,
            MAX_PREVIEW_TEXTURE_SIDE,
            true,
        ));

        assert!(preview.image.pixels.iter().any(|pixel| pixel.a() == 0));
    }

    #[test]
    fn small_images_are_scaled_to_a_visible_size() {
        assert_eq!(
            preview_display_size(16, 14, egui::vec2(512.0, 360.0)),
            egui::vec2(96.0, 84.0)
        );
        assert_eq!(
            preview_display_size(640, 480, egui::vec2(512.0, 360.0)),
            egui::vec2(480.0, 360.0)
        );
    }

    #[test]
    fn accepts_multiline_hex_image_data() {
        let source = ETHERCAT_4BPP_BMP_HEX
            .as_bytes()
            .chunks(64)
            .map(|chunk| std::str::from_utf8(chunk).expect("hex is ASCII"))
            .collect::<Vec<_>>()
            .join("\r\n  ");

        let preview = expect_ready(detect_base64_image(&source));

        assert_eq!(preview.encoding, SourceEncoding::Hexadecimal);
        assert_eq!((preview.width, preview.height), (16, 14));
    }

    #[test]
    fn ordinary_hexadecimal_values_are_not_images() {
        assert!(matches!(
            detect_base64_image("DEADBEEF00112233445566778899AABB"),
            Detection::NotImage
        ));
    }

    #[test]
    fn accepts_data_uri_whitespace_and_case_insensitive_metadata() {
        let encoded = encoded_png();
        let wrapped = encoded
            .as_bytes()
            .chunks(20)
            .map(|chunk| std::str::from_utf8(chunk).expect("Base64 is ASCII"))
            .collect::<Vec<_>>()
            .join("\r\n  ");
        let source = format!("DATA:IMAGE/PNG;charset=utf-8;BASE64,{wrapped}");

        let preview = expect_ready(detect_base64_image(&source));

        assert_eq!(preview.format, PreviewImageFormat::Png);
        assert_eq!((preview.width, preview.height), (2, 1));
    }

    #[test]
    fn accepts_unpadded_base64() {
        let source = encoded_png().trim_end_matches('=').to_string();

        let preview = expect_ready(detect_base64_image(&source));

        assert_eq!(preview.format, PreviewImageFormat::Png);
    }

    #[test]
    fn detects_all_supported_image_formats() {
        let formats = [
            (ImageFormat::Png, PreviewImageFormat::Png),
            (ImageFormat::Jpeg, PreviewImageFormat::Jpeg),
            (ImageFormat::Gif, PreviewImageFormat::Gif),
            (ImageFormat::WebP, PreviewImageFormat::WebP),
            (ImageFormat::Bmp, PreviewImageFormat::Bmp),
        ];

        for (image_format, preview_format) in formats {
            let source = STANDARD.encode(image_bytes(image_format, 2, 1));
            let preview = expect_ready(detect_base64_image(&source));

            assert_eq!(preview.format, preview_format);
            assert_eq!((preview.width, preview.height), (2, 1));
        }
    }

    #[test]
    fn applies_exif_orientation() {
        let source = STANDARD.encode(jpeg_with_exif_orientation(6));

        let preview = expect_ready(detect_base64_image(&source));

        assert_eq!((preview.width, preview.height), (1, 2));
        assert_eq!(preview.image.size, [1, 2]);
    }

    #[test]
    fn accepts_url_safe_base64() {
        let source = URL_SAFE_NO_PAD.encode(png_bytes());

        let preview = expect_ready(detect_base64_image(&source));

        assert_eq!(preview.format, PreviewImageFormat::Png);
    }

    #[test]
    fn ordinary_text_and_base64_encoded_text_are_not_images() {
        assert!(matches!(
            detect_base64_image("This is ordinary XML text."),
            Detection::NotImage
        ));
        assert!(matches!(
            detect_base64_image(&STANDARD.encode("This is Base64, but not an image.")),
            Detection::NotImage
        ));
    }

    #[test]
    fn invalid_explicit_data_uri_reports_an_error() {
        let detection = detect_base64_image("data:image/png;base64,not!base64");

        assert!(matches!(detection, Detection::Error(message) if message.contains("characters")));
    }

    #[test]
    fn rejects_invalid_data_uri_parameter_order() {
        let source = format!("data:image/png;base64;charset=utf-8,{}", encoded_png());

        let detection = detect_base64_image(&source);

        assert!(matches!(detection, Detection::Error(message) if message.contains("final")));
    }

    #[test]
    fn corrupt_image_with_valid_signature_reports_an_error() {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&[0; 32]);
        let detection = detect_base64_image(&STANDARD.encode(bytes));

        assert!(matches!(detection, Detection::Error(message) if message.contains("invalid")));
    }

    #[test]
    fn rejects_images_over_the_dimension_limit() {
        let bytes = image_bytes(ImageFormat::Png, MAX_IMAGE_DIMENSION + 1, 1);

        let detection = detect_base64_image(&STANDARD.encode(bytes));

        assert!(matches!(detection, Detection::Error(_)));
    }

    #[test]
    fn downsamples_to_the_runtime_texture_limit() {
        let source = STANDARD.encode(image_bytes(ImageFormat::Png, 2_000, 1));

        let preview = expect_ready(detect_encoded_image_for_texture(&source, 512, false));

        assert_eq!((preview.width, preview.height), (2_000, 1));
        assert_eq!(preview.image.size, [512, 1]);
    }

    #[test]
    fn mime_mismatch_is_exposed_as_a_warning() {
        let source = format!("data:image/jpeg;base64,{}", encoded_png());

        let preview = expect_ready(detect_base64_image(&source));

        assert!(
            preview
                .warning
                .is_some_and(|warning| { warning.contains("JPEG") && warning.contains("PNG") })
        );
    }

    #[test]
    fn parses_multiline_base64_from_xml_text() {
        let encoded = encoded_png();
        let xml = format!("<root><image>\n  {encoded}\n</image></root>");
        let document = crate::xml::parse_xml(&xml).expect("parse XML");
        let image_text = document
            .root
            .as_element()
            .and_then(|root| root.children[0].as_element())
            .and_then(|image| image.text.as_deref())
            .expect("image text");

        let preview = expect_ready(detect_base64_image(image_text));

        assert_eq!((preview.width, preview.height), (2, 1));
    }

    #[test]
    fn renderer_refreshes_when_the_text_revision_changes() {
        let context = egui::Context::default();
        let mut renderer = EncodedImagePreview::default();
        let first = STANDARD.encode(image_bytes(ImageFormat::Png, 2, 1));

        let _ = context.run(Default::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                renderer.show(ui, 7, 1, "image", &first, false);
            });
        });
        assert!(
            matches!(&renderer.state, PreviewState::Ready(preview) if (preview.width, preview.height) == (2, 1))
        );

        let second = STANDARD.encode(image_bytes(ImageFormat::Png, 1, 2));
        let _ = context.run(Default::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                renderer.show(ui, 7, 2, "image", &second, true);
            });
        });
        assert!(
            matches!(&renderer.state, PreviewState::Ready(preview) if (preview.width, preview.height) == (1, 2))
        );
    }
}
