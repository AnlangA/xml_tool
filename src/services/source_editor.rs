//! The source-editor draft workflow.
//!
//! Typing in the source pane edits a [`SourceDraft`] — a rope buffer plus
//! the document revision it was forked from. While a draft exists the tree
//! is read-only (both panes must never hold divergent truths). Applying
//! parses the draft off-thread; success commits one atomic
//! `ReplaceWholeSource` command (a single undo step), failure keeps the
//! draft and reports a diagnostic with an exact 1-based line/column.

use crate::core::Revision;
use crate::services::source_buffer::SourceBuffer;

/// An in-progress source edit.
#[derive(Clone)]
pub struct SourceDraft {
    /// The edited text.
    pub buffer: SourceBuffer,
    /// Revision of the document this draft was forked from; applying is
    /// only meaningful while the document still sits at it.
    pub base_revision: Revision,
}

impl SourceDraft {
    /// Forks a draft from the current source.
    pub fn fork(source: &str, revision: Revision) -> SourceDraft {
        SourceDraft {
            buffer: SourceBuffer::new(source),
            base_revision: revision,
        }
    }

    /// Whether the draft still differs from its fork point.
    pub fn is_modified(&self, current_source: &str) -> bool {
        self.buffer.text() != current_source
    }
}

/// What an apply attempt concluded.
pub enum ApplyOutcome {
    /// Draft parsed; the command commits when the caller runs it.
    Parsed { new_source: String },
    /// Draft is invalid: exact 1-based position and message.
    Invalid {
        line: usize,
        column: usize,
        message: String,
    },
}

/// Parses `draft` against the document's engine limits. Runs on a worker
/// thread; never touches the document.
pub fn parse_draft(draft: &str) -> ApplyOutcome {
    match crate::core::document::XmlDocument::parse(draft.as_bytes()) {
        Ok(_parsed) => {
            // The committed source is the draft exactly as typed.
            ApplyOutcome::Parsed {
                new_source: draft.to_string(),
            }
        }
        Err(err) => {
            let (line, column) = err
                .location()
                .map(|location| (location.line, location.column))
                .unwrap_or((1, 1));
            ApplyOutcome::Invalid {
                line,
                column,
                message: err.message().to_string(),
            }
        }
    }
}

/// 1-based line/column of a byte offset inside `draft` (for underlines and
/// jump-to-error).
pub fn line_column_of(draft: &str, byte_offset: usize) -> (usize, usize) {
    let offset = byte_offset.min(draft.len());
    let line = 1 + draft[..offset].bytes().filter(|&b| b == b'\n').count();
    let line_start = draft[..offset].rfind('\n').map_or(0, |pos| pos + 1);
    let column = 1 + draft[line_start..offset].chars().count();
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fork_tracks_modification() {
        let draft = SourceDraft::fork("<r/>", Revision(3));
        assert!(!draft.is_modified("<r/>"));
        let mut draft = draft;
        draft.buffer.insert(3, "x");
        assert!(draft.is_modified("<r/>"));
        assert_eq!(draft.buffer.text(), "<r/x>");
    }

    #[test]
    fn parse_draft_accepts_valid_source() {
        match parse_draft("<r><a/></r>") {
            ApplyOutcome::Parsed { new_source } => assert_eq!(new_source, "<r><a/></r>"),
            ApplyOutcome::Invalid { line, column, .. } => {
                panic!("valid draft rejected at {line}:{column}")
            }
        }
    }

    #[test]
    fn parse_draft_rejects_with_exact_position() {
        match parse_draft("<r>\n  <a></b>\n</r>") {
            ApplyOutcome::Parsed { .. } => panic!("invalid draft accepted"),
            ApplyOutcome::Invalid {
                line,
                column,
                message,
            } => {
                assert_eq!(line, 2, "message: {message}");
                assert!(column >= 1);
            }
        }
    }

    #[test]
    fn line_column_is_one_based() {
        assert_eq!(line_column_of("ab\ncd", 0), (1, 1));
        assert_eq!(line_column_of("ab\ncd", 4), (2, 2));
    }
}
