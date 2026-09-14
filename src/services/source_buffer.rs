//! Rope-backed source buffer.
//!
//! The source editor stores the document text in a [`ropey::Rope`]: edits
//! are logarithmic, and — unlike `String` — cloning for background save
//! snapshots shares the underlying tree instead of copying megabytes. Line
//! and column conversions go through the rope so the editor, status bar,
//! and jump-to-line all agree.

use ropey::Rope;

/// A source text buffer over a rope.
#[derive(Clone, Default)]
pub struct SourceBuffer {
    rope: Rope,
}

impl SourceBuffer {
    /// Builds a buffer from source text.
    pub fn new(text: &str) -> SourceBuffer {
        SourceBuffer {
            rope: Rope::from_str(text),
        }
    }

    /// Total characters.
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.rope.len_chars() == 0
    }

    /// Total lines (a trailing newline counts as ending the last line).
    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    /// Full text (allocates; prefer slice accessors in hot paths).
    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// The zero-based `line` as an owned string.
    pub fn line(&self, line: usize) -> String {
        self.rope.line(line).to_string()
    }

    /// 1-based line / 1-based char column of a char offset.
    pub fn line_column_of_char(&self, char_offset: usize) -> (usize, usize) {
        let offset = char_offset.min(self.rope.len_chars());
        let line = self.rope.char_to_line(offset);
        let line_start = self.rope.line_to_char(line);
        (line + 1, offset - line_start + 1)
    }

    /// Char offset of a 1-based line / column.
    pub fn char_offset_of_line_column(&self, line: usize, column: usize) -> usize {
        let line = line
            .saturating_sub(1)
            .min(self.rope.len_lines().saturating_sub(1));
        let line_start = self.rope.line_to_char(line);
        (line_start + column.saturating_sub(1)).min(self.rope.len_chars())
    }

    /// Inserts `text` at a char offset.
    pub fn insert(&mut self, char_offset: usize, text: &str) {
        let offset = char_offset.min(self.rope.len_chars());
        self.rope.insert(offset, text);
    }

    /// Removes `chars` characters at a char offset.
    pub fn remove(&mut self, char_offset: usize, chars: usize) {
        let offset = char_offset.min(self.rope.len_chars());
        let end = (offset + chars).min(self.rope.len_chars());
        if end > offset {
            self.rope.remove(offset..end);
        }
    }

    /// Read access to the underlying rope (iterators, slices).
    pub fn rope(&self) -> &Rope {
        &self.rope
    }
}
