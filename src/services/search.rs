//! Literal search index over document nodes, with incremental rebuild.
//!
//! The index stores each node's searchable text twice — verbatim and case
//! folded — so both case-sensitive and case-insensitive queries are plain
//! substring scans. Building is O(total text); [`SearchIndex::apply_changes`]
//! re-indexes only the nodes a `ChangedSet` touched, keeping warm queries
//! cheap after edits.

use std::collections::HashMap;

use crate::core::command::ChangedSet;
use crate::core::document::{NodeId, XmlDocument, XmlNodeKind};

/// Per-node searchable text.
struct NodeText {
    original: String,
    folded: String,
}

/// Literal search index for one document revision.
#[derive(Default)]
pub struct SearchIndex {
    texts: HashMap<u64, NodeText>,
    /// Revision the index was last updated against.
    revision: u64,
    /// Memoized last query (needle, case flag) and its hits; identical
    /// repeats return without rescanning the document.
    memo: Option<MemoizedQuery>,
}

struct MemoizedQuery {
    needle: String,
    case_sensitive: bool,
    hits: Vec<SearchHit>,
}

/// A search hit: the node and the byte range inside its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub node: NodeId,
    /// Byte range within the node's indexed text.
    pub range: std::ops::Range<usize>,
}

impl SearchIndex {
    /// Indexes every node of `document`.
    pub fn build(document: &XmlDocument) -> SearchIndex {
        let mut index = SearchIndex {
            texts: HashMap::new(),
            revision: document.revision().0,
            memo: None,
        };
        for &arena_id in document.document_order() {
            let node = NodeId(arena_id);
            let text = searchable_text(document, node);
            if !text.is_empty() {
                let folded = text.to_lowercase();
                index.texts.insert(
                    arena_id,
                    NodeText {
                        original: text,
                        folded,
                    },
                );
            }
        }
        index
    }

    /// Applies a committed change set: touched nodes are re-indexed,
    /// removed nodes dropped. Structure changes keep other entries valid
    /// because node ids are stable arena handles.
    pub fn apply_changes(&mut self, document: &XmlDocument, changed: &ChangedSet) {
        self.memo = None;
        for node in &changed.removed {
            self.texts.remove(&node.0);
        }
        for node in &changed.touched {
            let text = searchable_text(document, *node);
            if text.is_empty() {
                self.texts.remove(&node.0);
            } else {
                let folded = text.to_lowercase();
                self.texts.insert(
                    node.0,
                    NodeText {
                        original: text,
                        folded,
                    },
                );
            }
        }
        self.revision = document.revision().0;
    }

    /// The revision this index reflects.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Literal search in document order. `whole_document_order` supplies
    /// the current document order (the index is a map, not a sequence).
    pub fn search(
        &mut self,
        needle: &str,
        case_sensitive: bool,
        document_order: &[u64],
    ) -> Vec<SearchHit> {
        if needle.is_empty() {
            return Vec::new();
        }
        if let Some(memo) = &self.memo
            && memo.needle == needle
            && memo.case_sensitive == case_sensitive
        {
            return memo.hits.clone();
        }
        let folded_needle = if case_sensitive {
            None
        } else {
            Some(needle.to_lowercase())
        };
        let mut hits = Vec::new();
        for &arena_id in document_order {
            let Some(entry) = self.texts.get(&arena_id) else {
                continue;
            };
            let (haystack, needle) = match &folded_needle {
                Some(folded) => (entry.folded.as_str(), folded.as_str()),
                None => (entry.original.as_str(), needle),
            };
            let mut from = 0;
            while let Some(found) = haystack[from..].find(needle) {
                let start = from + found;
                hits.push(SearchHit {
                    node: NodeId(arena_id),
                    range: start..start + needle.len(),
                });
                from = start + needle.len();
            }
        }
        self.memo = Some(MemoizedQuery {
            needle: needle.to_string(),
            case_sensitive,
            hits: hits.clone(),
        });
        hits
    }
}

/// The searchable text of one node: element names and attribute pairs for
/// elements, content for text-like nodes.
fn searchable_text(document: &XmlDocument, node: NodeId) -> String {
    match document.kind(node) {
        Some(XmlNodeKind::Element) => {
            let mut text = document.qname(node).map(|q| q.render()).unwrap_or_default();
            for (name, value) in document.attributes(node) {
                text.push_str(format!(" {name}=\"{value}\"").as_str());
            }
            text
        }
        Some(XmlNodeKind::Text) | Some(XmlNodeKind::CData) => {
            document.node_text(node).unwrap_or_default().to_string()
        }
        Some(XmlNodeKind::Comment) => document.comment_text(node).unwrap_or_default().to_string(),
        Some(XmlNodeKind::ProcessingInstruction) => match document.pi(node) {
            Some((target, data)) => match data {
                Some(data) => format!("{target} {data}"),
                None => target.to_string(),
            },
            None => String::new(),
        },
        _ => String::new(),
    }
}
