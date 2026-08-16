//! Safe document I/O: mode classification, encoding-aware byte production,
//! and crash-safe atomic saves.
//!
//! Atomic save sequence (fixed): create a temp file in the *same directory*,
//! write all bytes, flush, `sync_all`, then `rename` over the target. If any
//! step fails the temp file is removed and the original file is untouched —
//! a save interrupted by crash or error can never leave a half-written
//! document behind.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::core::Revision;
use crate::core::document::XmlDocument;
use crate::fixtures::{EDIT_MAX_BYTES, EDIT_MAX_ELEMENTS, OPEN_MAX_BYTES};

/// Whether an opened document is fully editable or read-only by size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    /// At or below both thresholds: full editing.
    Editable,
    /// Above an editing threshold but at or below the open limit:
    /// read-only browsing (step 4 builds the UI for this).
    LargeReadOnly,
}

/// What `classify_bytes` decided about a byte stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenOutcome {
    pub mode: OpenMode,
    pub bytes: usize,
    /// Element count from the structural scan.
    pub elements: usize,
    /// Maximum nesting depth seen by the scan.
    pub max_depth: usize,
}

/// Classifies raw bytes for opening: refuses anything above
/// [`OPEN_MAX_BYTES`], structurally scans (skipping comments, PIs, CDATA,
/// and the doctype) for element count and depth, then applies the
/// 20 MiB / 200,000-element editable thresholds.
pub fn classify_bytes(bytes: &[u8]) -> Result<OpenOutcome, crate::xml::XmlError> {
    if bytes.len() > OPEN_MAX_BYTES {
        return Err(crate::xml::XmlError::new(
            crate::xml::XmlErrorCode::InputTooLarge,
            format!(
                "file is {} bytes; the 256 MiB open limit applies",
                bytes.len()
            ),
        ));
    }
    let (elements, max_depth) = scan_structure(bytes);
    let mode = if bytes.len() <= EDIT_MAX_BYTES && elements <= EDIT_MAX_ELEMENTS {
        OpenMode::Editable
    } else {
        OpenMode::LargeReadOnly
    };
    Ok(OpenOutcome {
        mode,
        bytes: bytes.len(),
        elements,
        max_depth,
    })
}

/// Counts element start tags and maximum nesting depth with a small state
/// machine over the raw bytes. UTF-16 input is detected via BOM and
/// normalized to UTF-8 text first so the scan is encoding-correct.
fn scan_structure(bytes: &[u8]) -> (usize, usize) {
    let text = match crate::xml::encoding::decode_xml_source(bytes) {
        Ok((text, _)) => text,
        Err(_) => return (usize::MAX / 2, usize::MAX / 2), // unreadable: force read-only
    };
    let bytes = text.as_bytes();
    let mut count = 0usize;
    let mut depth = 0usize;
    let mut max_depth = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &bytes[i..];
        if rest.starts_with(b"<!--") {
            i += 4;
            while i + 2 < bytes.len() && !bytes[i..].starts_with(b"-->") {
                i += 1;
            }
            i = (i + 3).min(bytes.len());
        } else if rest.starts_with(b"<![CDATA[") {
            i += 9;
            while i + 2 < bytes.len() && !bytes[i..].starts_with(b"]]>") {
                i += 1;
            }
            i = (i + 3).min(bytes.len());
        } else if rest.starts_with(b"<?") || rest.starts_with(b"<!") {
            // PI or doctype/internal subset: skip to the matching close.
            let close: &[u8] = if rest.starts_with(b"<?") { b"?>" } else { b">" };
            i += 2;
            while i < bytes.len() && !bytes[i..].starts_with(close) {
                i += 1;
            }
            i = (i + close.len()).min(bytes.len());
        } else if rest.starts_with(b"</") {
            depth = depth.saturating_sub(1);
            i += 2;
            while i < bytes.len() && bytes[i] != b'>' {
                i += 1;
            }
            i = (i + 1).min(bytes.len());
        } else {
            count += 1;
            depth += 1;
            max_depth = max_depth.max(depth);
            // Self-closing tags do not open a nesting level.
            let mut j = i + 1;
            let mut self_closing = false;
            let mut in_quote = 0u8;
            while j < bytes.len() {
                let byte = bytes[j];
                if in_quote != 0 {
                    if byte == in_quote {
                        in_quote = 0;
                    }
                } else if byte == b'"' || byte == b'\'' {
                    in_quote = byte;
                } else if byte == b'>' {
                    self_closing = bytes[j - 1] == b'/';
                    break;
                }
                j += 1;
            }
            if self_closing {
                depth = depth.saturating_sub(1);
            }
            i = (j + 1).min(bytes.len());
        }
    }
    (count, max_depth)
}

/// Produces the bytes for saving `document`:
///
/// - unedited since parse (revision 0) → the original bytes replayed
///   verbatim (BOM, line endings, quoting, entity references preserved);
/// - edited → the current source re-encoded in the original encoding.
pub fn document_bytes(document: &XmlDocument) -> Vec<u8> {
    if document.revision() == Revision(0)
        && let Some(original) = document.original_bytes()
    {
        return original.to_vec();
    }
    crate::xml::encoding::encode_xml_text(document.source(), document.encoding())
}

/// Writes `bytes` to `path` atomically.
pub fn save_bytes_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    save_bytes_with_hooks(path, bytes, &mut NoHooks)
}

/// Failure-injection hooks used by tests to prove the atomic sequence.
pub trait SaveHooks {
    /// Called after the temp file is fully written and synced, before the
    /// rename. Returning `Err` aborts the save.
    fn before_rename(&mut self, _temp: &Path) -> std::io::Result<()> {
        Ok(())
    }
}

struct NoHooks;

impl SaveHooks for NoHooks {}

/// The atomic save sequence with injection points. On any failure the temp
/// file is removed and the original is untouched.
pub fn save_bytes_with_hooks(
    path: &Path,
    bytes: &[u8],
    hooks: &mut dyn SaveHooks,
) -> std::io::Result<()> {
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| String::from("document"));
    let temp: PathBuf = directory.join(format!(".{}.{}.tmp", file_name, std::process::id()));

    let mut write = || -> std::io::Result<()> {
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        hooks.before_rename(&temp)?;
        fs::rename(&temp, path)?;
        // Sync the directory so the rename itself is durable.
        if let Ok(dir) = fs::File::open(directory) {
            let _ = dir.sync_all();
        }
        Ok(())
    };

    match write() {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = fs::remove_file(&temp);
            Err(err)
        }
    }
}
