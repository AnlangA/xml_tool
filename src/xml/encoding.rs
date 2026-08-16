//! Byte-level encoding detection, decoding, and re-encoding for XML input.
//!
//! Implements the XML 1.0 Appendix F subset this tool commits to: UTF-8
//! (with or without BOM) and UTF-16 LE/BE (with or without BOM). The detected
//! encoding is kept with the document so an unedited file can be saved back
//! in its original byte representation, BOM included.

use super::error::{SourceLocation, XmlError, XmlErrorCode};

/// The source encodings the editor reads and writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SourceEncoding {
    /// UTF-8 without a BOM.
    Utf8,
    /// UTF-8 with a leading `EF BB BF` byte-order mark.
    Utf8Bom,
    /// UTF-16 little-endian, no BOM (Appendix F auto-detection).
    Utf16Le,
    /// UTF-16 little-endian with `FF FE` BOM.
    Utf16LeBom,
    /// UTF-16 big-endian, no BOM.
    Utf16Be,
    /// UTF-16 big-endian with `FE FF` BOM.
    Utf16BeBom,
}

impl SourceEncoding {
    /// Whether serialization under this encoding must prepend a BOM.
    pub fn writes_bom(self) -> bool {
        matches!(
            self,
            SourceEncoding::Utf8Bom | SourceEncoding::Utf16LeBom | SourceEncoding::Utf16BeBom
        )
    }

    /// Whether this is a UTF-16 variant.
    pub fn is_utf16(self) -> bool {
        matches!(
            self,
            SourceEncoding::Utf16Le
                | SourceEncoding::Utf16LeBom
                | SourceEncoding::Utf16Be
                | SourceEncoding::Utf16BeBom
        )
    }

    /// Canonical name as it would appear in an XML declaration.
    pub fn declaration_name(self) -> &'static str {
        if self.is_utf16() { "UTF-16" } else { "UTF-8" }
    }
}

/// Detects the encoding of `bytes` per XML 1.0 Appendix F:
/// BOM first, then the `00 3C` / `3C 00` signatures for BOM-less UTF-16,
/// and UTF-8 as the default.
pub fn detect_encoding(bytes: &[u8]) -> SourceEncoding {
    match bytes {
        [0xEF, 0xBB, 0xBF, ..] => SourceEncoding::Utf8Bom,
        [0xFF, 0xFE, ..] => SourceEncoding::Utf16LeBom,
        [0xFE, 0xFF, ..] => SourceEncoding::Utf16BeBom,
        [0x00, 0x3C, ..] => SourceEncoding::Utf16Be,
        [0x3C, 0x00, ..] => SourceEncoding::Utf16Le,
        _ => SourceEncoding::Utf8,
    }
}

/// Decodes `bytes` into text plus the detected encoding. The BOM is
/// stripped; the declared encoding in the prolog is cross-checked against
/// the detected one so silent mojibake cannot happen.
pub fn decode_xml_source(bytes: &[u8]) -> Result<(String, SourceEncoding), XmlError> {
    let encoding = detect_encoding(bytes);
    let (text, encoding) = match encoding {
        SourceEncoding::Utf8 | SourceEncoding::Utf8Bom => {
            let body = match encoding {
                SourceEncoding::Utf8Bom => &bytes[3..],
                _ => bytes,
            };
            let text = String::from_utf8(body.to_vec()).map_err(|_| {
                XmlError::new(
                    XmlErrorCode::EncodingUndecodable,
                    "input is not valid UTF-8",
                )
            })?;
            (text, encoding)
        }
        SourceEncoding::Utf16Le | SourceEncoding::Utf16LeBom => {
            let body = match encoding {
                SourceEncoding::Utf16LeBom => &bytes[2..],
                _ => bytes,
            };
            (decode_utf16(body, true)?, encoding)
        }
        SourceEncoding::Utf16Be | SourceEncoding::Utf16BeBom => {
            let body = match encoding {
                SourceEncoding::Utf16BeBom => &bytes[2..],
                _ => bytes,
            };
            (decode_utf16(body, false)?, encoding)
        }
    };
    verify_declared_encoding(&text, encoding)?;
    Ok((text, encoding))
}

/// Re-encodes `text` under `encoding`, prepending the BOM when required.
pub fn encode_xml_text(text: &str, encoding: SourceEncoding) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() + 2);
    match encoding {
        SourceEncoding::Utf8 | SourceEncoding::Utf8Bom => {
            if encoding.writes_bom() {
                bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
            }
            bytes.extend_from_slice(text.as_bytes());
        }
        SourceEncoding::Utf16Le | SourceEncoding::Utf16LeBom => {
            if encoding.writes_bom() {
                bytes.extend_from_slice(&[0xFF, 0xFE]);
            }
            for unit in text.encode_utf16() {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
        }
        SourceEncoding::Utf16Be | SourceEncoding::Utf16BeBom => {
            if encoding.writes_bom() {
                bytes.extend_from_slice(&[0xFE, 0xFF]);
            }
            for unit in text.encode_utf16() {
                bytes.extend_from_slice(&unit.to_be_bytes());
            }
        }
    }
    bytes
}

fn decode_utf16(body: &[u8], little_endian: bool) -> Result<String, XmlError> {
    if !body.len().is_multiple_of(2) {
        return Err(XmlError::new(
            XmlErrorCode::EncodingUndecodable,
            "UTF-16 input has an odd number of bytes",
        ));
    }
    let units: Vec<u16> = body
        .chunks_exact(2)
        .map(|pair| {
            if little_endian {
                u16::from_le_bytes([pair[0], pair[1]])
            } else {
                u16::from_be_bytes([pair[0], pair[1]])
            }
        })
        .collect();
    String::from_utf16(&units).map_err(|_| {
        XmlError::new(
            XmlErrorCode::EncodingUndecodable,
            "input is not valid UTF-16 (lone surrogate)",
        )
    })
}

/// Cross-checks the declared encoding against the detected byte encoding.
/// Only the UTF-8 and UTF-16 families are supported; anything else fails
/// with `EncodingUnsupported`, and contradictions fail with
/// `EncodingMismatch`.
fn verify_declared_encoding(text: &str, detected: SourceEncoding) -> Result<(), XmlError> {
    let Some(declared) = declared_encoding(text) else {
        return Ok(());
    };
    let normalized = declared.to_ascii_lowercase().replace(['-', '_'], "");
    match normalized.as_str() {
        "utf8" | "usascii" | "ascii" => {
            if detected.is_utf16() {
                return Err(XmlError::at(
                    XmlErrorCode::EncodingMismatch,
                    format!(
                        "declaration says {declared} but the byte stream is {}",
                        detected.declaration_name()
                    ),
                    SourceLocation { line: 1, column: 1 },
                ));
            }
            Ok(())
        }
        "utf16" | "utf16le" | "utf16be" => {
            if !detected.is_utf16() {
                return Err(XmlError::at(
                    XmlErrorCode::EncodingMismatch,
                    format!(
                        "declaration says {declared} but the byte stream is {}",
                        detected.declaration_name()
                    ),
                    SourceLocation { line: 1, column: 1 },
                ));
            }
            Ok(())
        }
        other => Err(XmlError::at(
            XmlErrorCode::EncodingUnsupported,
            format!("declared encoding '{other}' is not supported (UTF-8 and UTF-16 only)"),
            SourceLocation { line: 1, column: 1 },
        )),
    }
}

/// Extracts the `encoding=...` value from the XML declaration, if present.
fn declared_encoding(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("<?xml")?;
    let end = rest.find("?>")?;
    let prolog = &rest[..end];
    for quote in ['"', '\''] {
        let Some(start) = prolog.find("encoding") else {
            continue;
        };
        let tail = &prolog[start + "encoding".len()..];
        let tail = tail.trim_start();
        let Some(tail) = tail.strip_prefix('=') else {
            continue;
        };
        let tail = tail.trim_start();
        let Some(value) = tail.strip_prefix(quote) else {
            continue; // try the other quote style
        };
        let Some(value_end) = value.find(quote) else {
            continue;
        };
        return Some(&value[..value_end]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_bom_signatures() {
        assert_eq!(
            detect_encoding(&[0xEF, 0xBB, 0xBF, b'<']),
            SourceEncoding::Utf8Bom
        );
        assert_eq!(
            detect_encoding(&[0xFF, 0xFE, b'<', 0]),
            SourceEncoding::Utf16LeBom
        );
        assert_eq!(
            detect_encoding(&[0xFE, 0xFF, 0, b'<']),
            SourceEncoding::Utf16BeBom
        );
        assert_eq!(
            detect_encoding(&[0x00, 0x3C, 0x00, b'?']),
            SourceEncoding::Utf16Be
        );
        assert_eq!(
            detect_encoding(&[0x3C, 0x00, b'?', 0x00]),
            SourceEncoding::Utf16Le
        );
        assert_eq!(
            detect_encoding(b"<?xml version=\"1.0\"?>"),
            SourceEncoding::Utf8
        );
    }

    #[test]
    fn decodes_utf8_with_and_without_bom() {
        let (text, encoding) = decode_xml_source("<a>中文</a>".as_bytes()).unwrap();
        assert_eq!(text, "<a>中文</a>");
        assert_eq!(encoding, SourceEncoding::Utf8);

        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice("<a/>".as_bytes());
        let (text, encoding) = decode_xml_source(&bom).unwrap();
        assert_eq!(text, "<a/>");
        assert_eq!(encoding, SourceEncoding::Utf8Bom);
    }

    #[test]
    fn decodes_utf16_le_and_be() {
        let text = "<?xml version=\"1.0\" encoding=\"UTF-16\"?><a>中文</a>";
        let le = encode_xml_text(text, SourceEncoding::Utf16LeBom);
        let be = encode_xml_text(text, SourceEncoding::Utf16BeBom);
        let (le_text, le_enc) = decode_xml_source(&le).unwrap();
        let (be_text, be_enc) = decode_xml_source(&be).unwrap();
        assert_eq!(le_text, text);
        assert_eq!(be_text, text);
        assert_eq!(le_enc, SourceEncoding::Utf16LeBom);
        assert_eq!(be_enc, SourceEncoding::Utf16BeBom);
    }

    #[test]
    fn encode_round_trips_every_encoding() {
        let text = "<a>Ünïcødé 中文 Δ</a>";
        for encoding in [
            SourceEncoding::Utf8,
            SourceEncoding::Utf8Bom,
            SourceEncoding::Utf16Le,
            SourceEncoding::Utf16LeBom,
            SourceEncoding::Utf16Be,
            SourceEncoding::Utf16BeBom,
        ] {
            let bytes = encode_xml_text(text, encoding);
            assert_eq!(detect_encoding(&bytes), encoding, "{encoding:?}");
            let (decoded, found) = decode_xml_source(&bytes).unwrap();
            assert_eq!(decoded, text);
            assert_eq!(found, encoding);
        }
    }

    #[test]
    fn rejects_undecodable_utf8() {
        let err = decode_xml_source(&[0xFF, 0x00, 0x00, 0x00, 0x00]).unwrap_err();
        assert_eq!(err.code(), XmlErrorCode::EncodingUndecodable);
    }

    #[test]
    fn rejects_odd_length_utf16() {
        let err = decode_xml_source(&[0xFF, 0xFE, 0x3C]).unwrap_err();
        assert_eq!(err.code(), XmlErrorCode::EncodingUndecodable);
    }

    #[test]
    fn rejects_unsupported_declared_encoding() {
        let err =
            decode_xml_source("<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a/>".as_bytes())
                .unwrap_err();
        assert_eq!(err.code(), XmlErrorCode::EncodingUnsupported);
    }

    #[test]
    fn rejects_utf16_declaration_over_utf8_bytes() {
        let err = decode_xml_source("<?xml version=\"1.0\" encoding=\"UTF-16\"?><a/>".as_bytes())
            .unwrap_err();
        assert_eq!(err.code(), XmlErrorCode::EncodingMismatch);
    }

    #[test]
    fn accepts_single_quoted_encoding_declaration() {
        let (..) =
            decode_xml_source("<?xml version='1.0' encoding='utf-8'?><a/>".as_bytes()).unwrap();
    }

    #[test]
    fn rejects_single_quoted_utf16_declaration_over_utf8_bytes() {
        // Single quotes must not smuggle a mismatched declaration past
        // verification (previously the first-quote `?` aborted the scan).
        let err = decode_xml_source("<?xml version='1.0' encoding='UTF-16'?><a/>".as_bytes())
            .unwrap_err();
        assert_eq!(err.code(), XmlErrorCode::EncodingMismatch);
    }
}
