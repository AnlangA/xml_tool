//! Bounded image decoding, file-byte encoding, and ESI bitmap conversion.
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use egui::ColorImage;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, imageops::FilterType};

pub(crate) const MAX_DECODED_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_IMAGE_DIMENSION: u32 = 4_096;
pub(crate) const MAX_IMAGE_PIXELS: u64 = 8 * 1024 * 1024;
pub(crate) const MAX_PREVIEW_TEXTURE_SIDE: u32 = 1_024;
const MAX_DECODE_ALLOC_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageFileFormat {
    Png,
    Jpeg,
    Gif,
    WebP,
    Bmp,
    Ico,
}

impl ImageFileFormat {
    pub(crate) fn from_image_format(format: ImageFormat) -> Option<Self> {
        match format {
            ImageFormat::Png => Some(Self::Png),
            ImageFormat::Jpeg => Some(Self::Jpeg),
            ImageFormat::Gif => Some(Self::Gif),
            ImageFormat::WebP => Some(Self::WebP),
            ImageFormat::Bmp => Some(Self::Bmp),
            ImageFormat::Ico => Some(Self::Ico),
            _ => None,
        }
    }

    pub(crate) fn from_mime_type(mime: &str) -> Option<Self> {
        match mime {
            "image/png" | "image/x-png" => Some(Self::Png),
            "image/jpeg" | "image/jpg" => Some(Self::Jpeg),
            "image/gif" => Some(Self::Gif),
            "image/webp" => Some(Self::WebP),
            "image/bmp" | "image/x-bmp" | "image/x-ms-bmp" => Some(Self::Bmp),
            "image/x-icon" | "image/vnd.microsoft.icon" => Some(Self::Ico),
            _ => None,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Gif => "GIF",
            Self::WebP => "WebP",
            Self::Bmp => "BMP",
            Self::Ico => "ICO",
        }
    }

    fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::WebP => "image/webp",
            Self::Bmp => "image/bmp",
            Self::Ico => "image/x-icon",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ConversionMode {
    #[default]
    Base64,
    DataUri,
    EsiHex,
}

pub(crate) fn is_esi_icon(name: &str) -> bool {
    name.rsplit(':')
        .next()
        .is_some_and(|local| local.eq_ignore_ascii_case("ImageData16x14"))
}

#[derive(Clone)]
pub(crate) struct IconSource {
    pub name: String,
    pub bytes: Arc<[u8]>,
}

pub(crate) fn read_icon(path: &Path) -> Result<IconSource, String> {
    let file = File::open(path).map_err(|e| format!("Cannot open image: {e}"))?;
    let metadata = file
        .metadata()
        .map_err(|e| format!("Cannot read image metadata: {e}"))?;
    if !metadata.is_file() {
        return Err("Choose an image file, not a directory.".into());
    }
    if metadata.len() > MAX_DECODED_BYTES as u64 {
        return Err("Image files are limited to 8 MiB.".into());
    }
    // A bounded read also handles a file that grows after the metadata check.
    let mut bytes = Vec::new();
    file.take(MAX_DECODED_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Cannot read image: {e}"))?;
    if bytes.len() > MAX_DECODED_BYTES {
        return Err("Image files are limited to 8 MiB.".into());
    }
    Ok(IconSource {
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        bytes: bytes.into(),
    })
}

pub(crate) fn pick_icon_path(save: bool, title: &str) -> Option<std::path::PathBuf> {
    let dialog = rfd::FileDialog::new().set_title(title);
    if save {
        dialog
            .add_filter("Text", &["txt"])
            .set_file_name("icon.txt")
            .save_file()
    } else {
        dialog
            .add_filter(
                "Images",
                &["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico"],
            )
            .pick_file()
    }
}

pub(crate) fn save_icon_text(path: &Path, text: &str) -> Result<(), String> {
    super::document_io::save_bytes_atomically(path, text.as_bytes())
        .map_err(|e| format!("Cannot save encoded text: {e}"))
}

#[derive(Debug)]
pub(crate) struct DecodedImage {
    pub image: ColorImage,
    pub format: ImageFileFormat,
    pub width: u32,
    pub height: u32,
}

pub(crate) fn decode_image_bytes(
    bytes: &[u8],
    texture_side: u32,
    esi: bool,
) -> Result<DecodedImage, String> {
    let (format, image) = decode_full_image(bytes)?;
    Ok(make_preview(image, format, texture_side, esi))
}

fn decode_full_image(bytes: &[u8]) -> Result<(ImageFileFormat, DynamicImage), String> {
    if bytes.len() > MAX_DECODED_BYTES {
        return Err("Image files are limited to 8 MiB.".into());
    }
    let image_format =
        image::guess_format(bytes).map_err(|_| "The file is not a supported image.".to_string())?;
    let format = ImageFileFormat::from_image_format(image_format)
        .ok_or("Only PNG, JPEG, GIF, WebP, BMP, and ICO images are supported.")?;
    let mut reader = ImageReader::with_format(Cursor::new(bytes), image_format);
    reader.limits(image_limits());
    let (width, height) = reader
        .into_dimensions()
        .map_err(|e| format!("The image is invalid: {e}"))?;
    validate_image_dimensions(width, height)?;

    let mut reader = ImageReader::with_format(Cursor::new(bytes), image_format);
    reader.limits(image_limits());
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| format!("The image is invalid: {e}"))?;
    let orientation = decoder
        .orientation()
        .map_err(|e| format!("Cannot read image orientation: {e}"))?;
    let mut image =
        DynamicImage::from_decoder(decoder).map_err(|e| format!("The image is invalid: {e}"))?;
    image.apply_orientation(orientation);
    let (width, height) = (image.width(), image.height());
    validate_image_dimensions(width, height)?;
    Ok((format, image))
}

fn make_preview(
    mut image: DynamicImage,
    format: ImageFileFormat,
    texture_side: u32,
    esi: bool,
) -> DecodedImage {
    let (width, height) = (image.width(), image.height());
    if esi {
        let mut rgba = image.into_rgba8();
        for pixel in rgba.pixels_mut() {
            if pixel.0[..3] == [255, 0, 255] {
                pixel.0[3] = 0;
            }
        }
        image = DynamicImage::ImageRgba8(rgba);
    }
    let texture_side = texture_side.clamp(1, MAX_PREVIEW_TEXTURE_SIDE);
    if width > texture_side || height > texture_side {
        image = image.resize(texture_side, texture_side, FilterType::Triangle);
    }
    let rgba = image.into_rgba8();
    DecodedImage {
        image: ColorImage::from_rgba_unmultiplied(
            [rgba.width() as usize, rgba.height() as usize],
            rgba.as_raw(),
        ),
        format,
        width,
        height,
    }
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
        return Err("The decoded image has invalid dimensions.".into());
    }
    if width > MAX_IMAGE_DIMENSION
        || height > MAX_IMAGE_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS
    {
        return Err(format!(
            "Image dimensions exceed the preview limit (maximum {MAX_IMAGE_DIMENSION}x{MAX_IMAGE_DIMENSION} and {MAX_IMAGE_PIXELS} pixels)."
        ));
    }
    Ok(())
}

pub(crate) enum ConversionNote {
    Original,
    Converted {
        format: ImageFileFormat,
        width: u32,
        height: u32,
        depth: Option<u16>,
        single_frame: bool,
    },
}

pub(crate) struct ConvertedIcon {
    pub decoded: DecodedImage,
    pub text: Arc<str>,
    pub note: Option<ConversionNote>,
}

pub(crate) fn convert_icon(
    source: &IconSource,
    mode: ConversionMode,
    texture_side: u32,
) -> Result<ConvertedIcon, String> {
    let (format, image) = decode_full_image(&source.bytes)?;
    let (width, height) = (image.width(), image.height());
    let mut bytes = source.bytes.clone();
    let mut note = None;
    let decoded = if mode == ConversionMode::EsiHex {
        if format == ImageFileFormat::Bmp
            && width == 16
            && height == 14
            && bmp_bit_depth(&bytes) == Some(4)
        {
            note = Some(ConversionNote::Original);
            make_preview(image, format, texture_side, true)
        } else {
            let depth = if format == ImageFileFormat::Bmp {
                bmp_bit_depth(&bytes)
            } else {
                None
            };
            bytes = encode_esi_bmp(image).into();
            note = Some(ConversionNote::Converted {
                format,
                width,
                height,
                depth,
                single_frame: matches!(
                    format,
                    ImageFileFormat::Gif | ImageFileFormat::WebP | ImageFileFormat::Ico
                ),
            });
            decode_image_bytes(&bytes, texture_side, true)?
        }
    } else {
        make_preview(image, format, texture_side, false)
    };
    if mode == ConversionMode::EsiHex {
        validate_esi(&bytes, &decoded)?;
    }
    let text = match mode {
        ConversionMode::Base64 => STANDARD.encode(&bytes),
        ConversionMode::DataUri => format!(
            "data:{};base64,{}",
            decoded.format.mime(),
            STANDARD.encode(&bytes)
        ),
        ConversionMode::EsiHex => {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            let mut text = String::with_capacity(bytes.len() * 2);
            for &byte in bytes.iter() {
                text.push(HEX[(byte >> 4) as usize] as char);
                text.push(HEX[(byte & 15) as usize] as char);
            }
            text
        }
    };
    Ok(ConvertedIcon {
        decoded,
        text: text.into(),
        note,
    })
}

/// Encode the fixed ESI bitmap layout: 14-byte file header, 40-byte DIB,
/// 16 BGRA palette entries, then fourteen bottom-up rows of eight packed bytes.
fn encode_esi_bmp(image: DynamicImage) -> Vec<u8> {
    let mut rgba = image.into_rgba8();
    for pixel in rgba.pixels_mut() {
        if pixel.0[..3] == [255, 0, 255] {
            pixel.0[3] = 0;
        }
        // Premultiply before resampling to avoid dark/color fringes at alpha edges.
        let alpha = u16::from(pixel.0[3]);
        for channel in &mut pixel.0[..3] {
            *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
    let fitted = DynamicImage::ImageRgba8(rgba)
        .resize(16, 14, FilterType::Triangle)
        .into_rgba8();
    let left = (16 - fitted.width()) / 2;
    let top = (14 - fitted.height()) / 2;
    let mut pixels = [None; 16 * 14];
    for (x, y, pixel) in fitted.enumerate_pixels() {
        let alpha = u32::from(pixel.0[3]);
        if alpha >= 128 {
            let color = std::array::from_fn(|i| {
                ((u32::from(pixel.0[i]) * 255 + alpha / 2) / alpha).min(255) as u8
            });
            pixels[((y + top) * 16 + x + left) as usize] = Some(color);
        }
    }
    let transparent = pixels.contains(&None);
    let colors: Vec<[u8; 3]> = pixels.iter().flatten().copied().collect();
    let mut palette = if transparent {
        vec![[255, 0, 255]]
    } else {
        Vec::new()
    };
    palette.extend(quantize_colors(colors, 16 - palette.len()));
    // Never use the transparent color key for an opaque quantized color.
    for color in &mut palette[usize::from(transparent)..] {
        if *color == [255, 0, 255] {
            color[2] = 254;
        }
    }
    let mut bmp = vec![0u8; 230];
    bmp[..2].copy_from_slice(b"BM");
    for (offset, value) in [
        (2, 230u32),
        (10, 118),
        (14, 40),
        (18, 16),
        (22, 14),
        (34, 112),
        (46, 16),
    ] {
        bmp[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
    bmp[28..30].copy_from_slice(&4u16.to_le_bytes());
    for (index, color) in palette.iter().enumerate() {
        bmp[54 + index * 4..58 + index * 4].copy_from_slice(&[color[2], color[1], color[0], 0]);
    }
    for (position, color) in pixels.iter().enumerate() {
        let index = color
            .map(|color| {
                palette
                    .iter()
                    .enumerate()
                    .skip(usize::from(transparent))
                    .min_by_key(|(_, candidate)| {
                        (0..3)
                            .map(|i| (i32::from(color[i]) - i32::from(candidate[i])).pow(2))
                            .sum::<i32>()
                    })
                    .map_or(0, |(index, _)| index as u8)
            })
            .unwrap_or(0);
        let offset = 118 + (13 - position / 16) * 8 + (position % 16) / 2;
        bmp[offset] |= if position % 2 == 0 { index << 4 } else { index };
    }
    bmp
}

/// Deterministic median-cut quantization over at most 224 pixels. Repeated
/// colors retain their weight; already-small palettes are preserved exactly.
fn quantize_colors(colors: Vec<[u8; 3]>, limit: usize) -> Vec<[u8; 3]> {
    let mut unique = colors.clone();
    unique.sort_unstable();
    unique.dedup();
    if unique.len() <= limit {
        return unique;
    }
    fn widest_axis(colors: &[[u8; 3]]) -> (usize, u8) {
        (0..3)
            .map(|axis| {
                let min = colors.iter().map(|c| c[axis]).min().unwrap_or(0);
                let max = colors.iter().map(|c| c[axis]).max().unwrap_or(0);
                (axis, max - min)
            })
            .max_by_key(|&(_, range)| range)
            .unwrap()
    }
    let mut buckets = vec![colors];
    while buckets.len() < limit {
        let Some((index, _)) = buckets
            .iter()
            .enumerate()
            .filter(|(_, bucket)| widest_axis(bucket).1 > 0)
            .max_by_key(|(_, bucket)| usize::from(widest_axis(bucket).1) * bucket.len())
        else {
            break;
        };
        let mut bucket = buckets.remove(index);
        let axis = widest_axis(&bucket).0;
        bucket.sort_unstable_by_key(|c| (c[axis], *c));
        let other = bucket.split_off(bucket.len() / 2);
        buckets.push(bucket);
        buckets.push(other);
    }
    buckets
        .iter()
        .map(|bucket| {
            std::array::from_fn(|i| {
                let sum: usize = bucket.iter().map(|c| usize::from(c[i])).sum();
                ((sum + bucket.len() / 2) / bucket.len()) as u8
            })
        })
        .collect()
}

fn bmp_bit_depth(bytes: &[u8]) -> Option<u16> {
    // BITMAPCOREHEADER stores bit depth at offset 24; other supported DIBs use 28.
    if !bytes.starts_with(b"BM") {
        return None;
    }
    let dib_size = u32::from_le_bytes(bytes.get(14..18)?.try_into().ok()?);
    let offset = if dib_size == 12 { 24 } else { 28 };
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn validate_esi(bytes: &[u8], image: &DecodedImage) -> Result<(), String> {
    let bit_depth = bmp_bit_depth(bytes);
    if image.format != ImageFileFormat::Bmp
        || image.width != 16
        || image.height != 14
        || bit_depth != Some(4)
    {
        let depth = bit_depth.map(|b| format!(", {b}bpp")).unwrap_or_default();
        return Err(format!(
            "ESI requires a 16x14, 16-color indexed BMP (4bpp). Selected: {} {}x{}{depth}. The generated icon failed ESI validation.",
            image.format.label(),
            image.width,
            image.height
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn esi_test_bmp() -> Vec<u8> {
    // A real 16x14, 4bpp BITMAPINFOHEADER file with 16 palette entries.
    let mut bmp = vec![0; 230];
    bmp[..2].copy_from_slice(b"BM");
    bmp[2..6].copy_from_slice(&230_u32.to_le_bytes());
    bmp[10..14].copy_from_slice(&118_u32.to_le_bytes());
    bmp[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bmp[18..22].copy_from_slice(&16_u32.to_le_bytes());
    bmp[22..26].copy_from_slice(&14_u32.to_le_bytes());
    bmp[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bmp[28..30].copy_from_slice(&4_u16.to_le_bytes());
    bmp[34..38].copy_from_slice(&112_u32.to_le_bytes());
    bmp[46..50].copy_from_slice(&16_u32.to_le_bytes());
    bmp[54..58].copy_from_slice(&[255, 0, 255, 0]);
    bmp[58..62].copy_from_slice(&[30, 20, 10, 0]);
    bmp[118..].fill(0x01);
    bmp
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn source(bytes: Vec<u8>) -> IconSource {
        IconSource {
            name: "test.img".into(),
            bytes: bytes.into(),
        }
    }

    fn image_bytes(format: ImageFormat, width: u32, height: u32) -> Vec<u8> {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            width,
            height,
            Rgba([10, 20, 30, 255]),
        ));
        let image = if format == ImageFormat::Jpeg {
            DynamicImage::ImageRgb8(image.into_rgb8())
        } else {
            image
        };
        let mut output = Cursor::new(Vec::new());
        image.write_to(&mut output, format).unwrap();
        output.into_inner()
    }

    #[test]
    fn all_formats_preserve_original_bytes_and_report_actual_mime() {
        for (format, mime) in [
            (ImageFormat::Png, "image/png"),
            (ImageFormat::Jpeg, "image/jpeg"),
            (ImageFormat::Gif, "image/gif"),
            (ImageFormat::WebP, "image/webp"),
            (ImageFormat::Bmp, "image/bmp"),
            (ImageFormat::Ico, "image/x-icon"),
        ] {
            let source = source(image_bytes(format, 16, 16));
            let base64 = convert_icon(&source, ConversionMode::Base64, 1024).unwrap();
            assert_eq!(
                STANDARD.decode(base64.text.as_bytes()).unwrap(),
                &*source.bytes
            );
            assert_eq!(base64.text.len() % 4, 0);
            assert!(!base64.text.contains('\n'));
            let uri = convert_icon(&source, ConversionMode::DataUri, 1024).unwrap();
            assert_eq!(&*uri.text, format!("data:{mime};base64,{}", base64.text));
            assert_eq!((uri.decoded.width, uri.decoded.height), (16, 16));
        }
    }

    #[test]
    fn gif_encoding_preserves_every_frame() {
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
                encoder
                    .encode_frame(image::Frame::new(RgbaImage::from_pixel(2, 2, Rgba(color))))
                    .unwrap();
            }
        }
        let input = source(bytes);
        let result = convert_icon(&input, ConversionMode::Base64, 1024).unwrap();
        assert_eq!(
            STANDARD.decode(result.text.as_bytes()).unwrap(),
            &*input.bytes
        );
    }

    #[test]
    fn esi_hex_preserves_bmp_and_only_preview_changes_transparency() {
        let input = source(esi_test_bmp());
        let result = convert_icon(&input, ConversionMode::EsiHex, 1024).unwrap();
        let decoded: Vec<u8> = result
            .text
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        assert_eq!(decoded, &*input.bytes);
        assert!(
            result
                .text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
        );
        assert_eq!(result.decoded.image.pixels[0].a(), 0);
        assert_eq!(result.decoded.image.pixels[1].a(), 255);
        let generic = convert_icon(&input, ConversionMode::Base64, 1024).unwrap();
        assert_eq!(generic.decoded.image.pixels[0].a(), 255);
    }

    #[test]
    fn esi_converts_other_formats_dimensions_and_bit_depths() {
        for (format, width, height) in [
            (ImageFormat::Png, 80, 40),
            (ImageFormat::Jpeg, 14, 16),
            (ImageFormat::Gif, 16, 14),
            (ImageFormat::WebP, 64, 64),
            (ImageFormat::Bmp, 16, 14),
            (ImageFormat::Ico, 32, 32),
        ] {
            let input = source(image_bytes(format, width, height));
            let result = convert_icon(&input, ConversionMode::EsiHex, 1024).unwrap();
            assert_eq!((result.decoded.width, result.decoded.height), (16, 14));
            let bytes: Vec<u8> = result
                .text
                .as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect();
            assert_eq!(bytes.len(), 230);
            assert_eq!(bmp_bit_depth(&bytes), Some(4));
            validate_esi(&bytes, &result.decoded).unwrap();
            assert_eq!(
                decode_image_bytes(&bytes, 1024, true).unwrap().image.pixels,
                result.decoded.image.pixels
            );
            assert!(matches!(
                result.note,
                Some(ConversionNote::Converted { .. })
            ));
        }
    }

    #[test]
    fn esi_24bpp_conversion_preserves_small_palette_and_row_order() {
        let mut image = image::RgbImage::from_pixel(16, 14, image::Rgb([255, 255, 255]));
        image.put_pixel(0, 0, image::Rgb([30, 80, 200]));
        image.put_pixel(15, 13, image::Rgb([180, 20, 40]));
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image.clone())
            .write_to(&mut bytes, ImageFormat::Bmp)
            .unwrap();
        assert_eq!(bmp_bit_depth(bytes.get_ref()), Some(24));
        let result =
            convert_icon(&source(bytes.into_inner()), ConversionMode::EsiHex, 1024).unwrap();
        assert!(matches!(
            result.note,
            Some(ConversionNote::Converted {
                depth: Some(24),
                ..
            })
        ));
        let expected = ColorImage::from_rgb([16, 14], image.as_raw());
        assert_eq!(result.decoded.image.pixels, expected.pixels);
    }

    #[test]
    fn esi_fits_centers_and_reserves_transparent_palette_entry() {
        let mut image = RgbaImage::from_pixel(8, 8, Rgba([20, 40, 80, 255]));
        image.put_pixel(0, 0, Rgba([255, 0, 255, 255]));
        image.put_pixel(7, 7, Rgba([255, 255, 255, 0]));
        let bytes = encode_esi_bmp(DynamicImage::ImageRgba8(image));
        let decoded = decode_image_bytes(&bytes, 1024, true).unwrap();
        for y in 0..14 {
            assert_eq!(decoded.image.pixels[y * 16].a(), 0);
            assert_eq!(decoded.image.pixels[y * 16 + 15].a(), 0);
        }
        assert_eq!(
            decoded.image.pixels[7 * 16 + 8].to_array(),
            [20, 40, 80, 255]
        );
        let colors: std::collections::HashSet<_> =
            decoded.image.pixels.iter().map(|p| p.to_array()).collect();
        assert!(colors.len() <= 16);
    }

    #[test]
    fn esi_palette_reduction_is_deterministic_and_handles_fully_transparent_images() {
        let image = RgbaImage::from_fn(16, 14, |x, y| {
            Rgba([(x * 16) as u8, (y * 18) as u8, ((x + y) * 8) as u8, 255])
        });
        let first = encode_esi_bmp(DynamicImage::ImageRgba8(image.clone()));
        assert_eq!(first, encode_esi_bmp(DynamicImage::ImageRgba8(image)));
        let decoded = decode_image_bytes(&first, 1024, true).unwrap();
        let colors: std::collections::HashSet<_> =
            decoded.image.pixels.iter().map(|p| p.to_array()).collect();
        assert!(colors.len() <= 16);
        let empty = encode_esi_bmp(DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            16,
            14,
            Rgba([255, 0, 255, 0]),
        )));
        let decoded = decode_image_bytes(&empty, 1024, true).unwrap();
        assert!(decoded.image.pixels.iter().all(|p| p.a() == 0));
    }

    #[test]
    fn invalid_and_oversize_input_is_rejected() {
        for bytes in [
            vec![],
            b"not an image".to_vec(),
            esi_test_bmp()[..60].to_vec(),
            vec![0; MAX_DECODED_BYTES + 1],
        ] {
            assert!(convert_icon(&source(bytes), ConversionMode::Base64, 1024).is_err());
        }
        assert!(validate_image_dimensions(0, 1).is_err());
        assert!(validate_image_dimensions(4097, 1).is_err());
        assert!(validate_image_dimensions(4096, 2049).is_err());
        assert!(validate_image_dimensions(4096, 2048).is_ok());
    }

    #[test]
    fn bounded_file_read_uses_content_instead_of_extension() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("actually-png.jpg");
        std::fs::write(&path, image_bytes(ImageFormat::Png, 16, 14)).unwrap();
        let input = read_icon(&path).unwrap();
        let result = convert_icon(&input, ConversionMode::DataUri, 1024).unwrap();
        assert!(result.text.starts_with("data:image/png;base64,"));
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(MAX_DECODED_BYTES as u64 + 1)
            .unwrap();
        assert!(read_icon(&path).is_err());
        assert!(read_icon(directory.path()).is_err());
        assert!(read_icon(&directory.path().join("missing.png")).is_err());
    }
}
