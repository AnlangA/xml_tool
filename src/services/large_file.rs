//! Large-file read-only mode: a streaming skeleton index plus literal
//! search over the decoded source.
//!
//! Documents above the editing thresholds (20 MiB / 200,000 elements) but
//! within the 256 MiB open limit open in `LargeReadOnly` mode: tree edits,
//! source edits, replace, format, XPath, XSD, and structural diff are all
//! disabled — this module provides exactly the allowed operations:
//! virtualized structure browsing, literal search with jump-to-match,
//! copying node paths and visible text, exporting a selected subtree, and
//! saving the file under a new name.
//!
//! The skeleton is one compact entry per element (rendered name, depth,
//! byte range of the whole subtree, byte range of the start tag): roughly
//! 50 bytes per element on top of the source text, so even a 256 MiB
//! document stays far below the memory budget.

use std::ops::Range;

use uppsala::dom::QName as EngineQName;
use uppsala::pull::{PullEvent, PullParser};

use crate::xml::encoding::{SourceEncoding, decode_xml_source};

/// One element in the read-only skeleton.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementEntry {
    /// Rendered `prefix:local` name.
    pub name: String,
    /// Nesting depth, root element at 0.
    pub depth: u32,
    /// Byte range of the whole element (start tag through end tag).
    pub span: Range<usize>,
    /// Byte range of just the start tag.
    pub start_tag: Range<usize>,
}

/// A read-only opened document: decoded source plus element skeleton.
pub struct ReadOnlyDocument {
    pub source: String,
    pub encoding: SourceEncoding,
    pub elements: Vec<ElementEntry>,
}

/// Why a read-only open failed.
#[derive(Debug)]
pub enum LargeFileError {
    /// Above the 256 MiB open limit; nothing was built.
    TooLarge(usize),
    /// The stream did not decode or parse.
    Parse(String),
}

impl ReadOnlyDocument {
    /// Builds the skeleton by streaming `bytes`. Refuses anything above the
    /// open limit before decoding; never materializes a DOM.
    pub fn open(bytes: &[u8]) -> Result<ReadOnlyDocument, LargeFileError> {
        if bytes.len() > crate::fixtures::OPEN_MAX_BYTES {
            return Err(LargeFileError::TooLarge(bytes.len()));
        }
        let (source, encoding) =
            decode_xml_source(bytes).map_err(|err| LargeFileError::Parse(err.to_string()))?;

        let mut elements: Vec<ElementEntry> = Vec::new();
        // Open-element stack: (name, start-tag byte range). Self-closing
        // tags arrive as StartElement followed by a synthetic EndElement,
        // so one code path covers both spellings.
        let mut open: Vec<(EngineQName<'_>, Range<usize>)> = Vec::new();
        let mut parser = PullParser::new(&source);
        loop {
            match parser
                .next_event()
                .map_err(|err| LargeFileError::Parse(err.to_string()))?
            {
                Some(PullEvent::StartElement {
                    name,
                    byte_start,
                    byte_end,
                    ..
                }) => {
                    open.push((name, byte_start..byte_end));
                }
                Some(PullEvent::EndElement { byte_end, .. }) => {
                    if let Some((name, start_tag)) = open.pop() {
                        elements.push(ElementEntry {
                            name: render_name(&name),
                            depth: open.len() as u32,
                            span: start_tag.start..byte_end,
                            start_tag,
                        });
                    }
                }
                Some(_) => {}
                None => break,
            }
        }

        Ok(ReadOnlyDocument {
            source,
            encoding,
            elements,
        })
    }

    /// Total element count.
    pub fn element_count(&self) -> usize {
        self.elements.len()
    }

    /// Literal search over the source; returns match byte ranges in order.
    /// Case-insensitive when `fold`.
    pub fn search_literal(&self, needle: &str, fold: bool) -> Vec<Range<usize>> {
        if needle.is_empty() {
            return Vec::new();
        }
        let mut matches = Vec::new();
        if fold {
            let haystack = self.source.to_lowercase();
            let needle = needle.to_lowercase();
            if haystack.len() == self.source.len() {
                // Length-preserving fold (ASCII): offsets line up directly.
                let mut from = 0;
                while let Some(found) = haystack[from..].find(&needle) {
                    let start = from + found;
                    matches.push(start..start + needle.len());
                    from = start + needle.len();
                }
            } else {
                // Unicode folding changed byte lengths: scan with an
                // explicit folded→source offset map.
                matches = fold_scan(&self.source, &needle);
            }
        } else {
            let mut from = 0;
            while let Some(found) = self.source[from..].find(needle) {
                let start = from + found;
                matches.push(start..start + needle.len());
                from = start + needle.len();
            }
        }
        matches
    }

    /// The innermost element containing `byte_offset` (for jump-to-match).
    pub fn element_at(&self, byte_offset: usize) -> Option<usize> {
        // Elements are in completion order; find any containing element
        // with the greatest depth via linear scan of candidates.
        let mut best: Option<(u32, usize)> = None;
        for (index, entry) in self.elements.iter().enumerate() {
            if entry.span.contains(&byte_offset)
                && best.is_none_or(|(depth, _)| entry.depth >= depth)
            {
                best = Some((entry.depth, index));
            }
        }
        best.map(|(_, index)| index)
    }

    /// Copyable path of `index`, e.g. `/catalog/record[12]/title`.
    pub fn node_path(&self, index: usize) -> String {
        let target = &self.elements[index];
        let mut parts: Vec<(String, usize)> = Vec::new();
        for (i, entry) in self.elements.iter().enumerate() {
            if entry.depth < target.depth
                && entry.span.start <= target.span.start
                && entry.span.end >= target.span.end
            {
                let position = self
                    .elements
                    .iter()
                    .enumerate()
                    .filter(|(_, other)| {
                        other.depth == entry.depth
                            && other.name == entry.name
                            && other.span.start >= entry.span.start
                            && other.span.end <= entry.span.end
                    })
                    .position(|(j, _)| j == i)
                    .unwrap_or(0);
                parts.push((entry.name.clone(), position));
            }
        }
        let own = self
            .elements
            .iter()
            .enumerate()
            .filter(|(_, other)| other.depth == target.depth && other.name == target.name)
            .position(|(i, _)| i == index)
            .unwrap_or(0);
        let mut path = String::new();
        for (name, position) in parts {
            path.push('/');
            path.push_str(&name);
            path.push_str(&format!("[{}]", position + 1));
        }
        path.push('/');
        path.push_str(&target.name);
        path.push_str(&format!("[{}]", own + 1));
        path
    }

    /// Source slice of a whole element (export selected subtree).
    pub fn element_source(&self, index: usize) -> &str {
        let entry = &self.elements[index];
        &self.source[entry.span.clone()]
    }
}

fn render_name(name: &EngineQName<'_>) -> String {
    match &name.prefix {
        Some(prefix) => format!("{prefix}:{}", name.local_name),
        None => name.local_name.to_string(),
    }
}

/// Char-wise case-insensitive scan for non-ASCII sources.
fn fold_scan(source: &str, needle: &str) -> Vec<Range<usize>> {
    // Fold char by char, remembering for every folded byte offset the
    // source byte offset it came from — Unicode case folding can change
    // byte lengths (e.g. 'İ'), so offsets cannot be reused directly.
    let mut folded = String::with_capacity(source.len());
    let mut map: Vec<u32> = Vec::with_capacity(source.len() + 1);
    for (src_index, ch) in source.char_indices() {
        let folded_start = folded.len();
        folded.extend(ch.to_lowercase());
        map.resize(map.len() + (folded.len() - folded_start), src_index as u32);
    }
    map.push(source.len() as u32);

    let mut matches = Vec::new();
    let mut from = 0;
    while let Some(found) = folded[from..].find(needle) {
        let start = from + found;
        let end = start + needle.len();
        matches.push(map[start] as usize..map[end] as usize);
        from = end;
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_insensitive_search_maps_offsets_for_non_ascii_sources() {
        // 'İ' folds to two chars ("i\u{307}"), shifting every later byte:
        // folded offsets cannot index the source. The map must fix them up.
        let doc = ReadOnlyDocument::open("<r>İSTANBUL x Value-1</r>".as_bytes()).expect("parse");
        let hits = doc.search_literal("value-1", true);
        assert_eq!(hits.len(), 1);
        assert_eq!(&doc.source[hits[0].clone()], "Value-1");
        let hits = doc.search_literal("i̇stanbul", true);
        assert_eq!(hits.len(), 1);
        assert_eq!(&doc.source[hits[0].clone()], "İSTANBUL");
    }
}
