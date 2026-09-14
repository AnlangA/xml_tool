//! Structural XML diff.
//!
//! Elements are compared in document order by qualified name, attributes
//! (order-insensitive), and normalized text (pure-formatting whitespace
//! ignored; meaningful text, CDATA, comments, and PIs preserved). Sequence
//! alignment runs through `similar` with a 5-second deadline.

use std::time::{Duration, Instant};

use similar::ChangeTag;

use crate::core::document::{NodeId, XmlDocument, XmlNodeKind};

/// Hard compute deadline fixed by the plan.
pub const DEADLINE: Duration = Duration::from_secs(5);

/// One structural difference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffEntry {
    /// Present only on the right side.
    Added { label: String },
    /// Present only on the left side.
    Removed { label: String },
    /// Same identity, changed attributes or text.
    Modified { label: String, detail: String },
    /// Same content, different position.
    Moved { label: String },
}

/// Options: attribute order never matters; formatting whitespace never
/// matters; element order always matters (fixed by the plan).
#[derive(Debug, Clone, Copy, Default)]
pub struct DiffOptions {
    /// Treat comments and PIs as significant (default true per plan).
    pub include_comments_and_pis: bool,
}

/// Compares two documents; returns the entries in diff order. Returns
/// `Err` when the deadline passes before the alignment completes.
pub fn diff_xml(
    left: &XmlDocument,
    right: &XmlDocument,
    options: DiffOptions,
) -> Result<Vec<DiffEntry>, String> {
    let started = Instant::now();
    let left_items = sequence(left, options);
    let right_items = sequence(right, options);

    let left_refs: Vec<&str> = left_items.iter().map(|item| item.key.as_str()).collect();
    let right_refs: Vec<&str> = right_items.iter().map(|item| item.key.as_str()).collect();
    let diff = similar::TextDiff::configure()
        .algorithm(similar::Algorithm::Patience)
        .timeout(DEADLINE)
        .diff_slices(&left_refs, &right_refs);
    if started.elapsed() > DEADLINE {
        return Err(String::from("diff exceeded the 5-second deadline"));
    }

    let mut entries = Vec::new();
    for (idx, change) in diff.iter_all_changes().enumerate() {
        let _ = idx;
        match change.tag() {
            ChangeTag::Equal => {}
            ChangeTag::Delete => {
                let old_index = change.old_index().expect("delete has old index");
                entries.push(DiffEntry::Removed {
                    label: left_items[old_index].label.clone(),
                });
            }
            ChangeTag::Insert => {
                let new_index = change.new_index().expect("insert has new index");
                entries.push(DiffEntry::Added {
                    label: right_items[new_index].label.clone(),
                });
            }
        }
    }

    // Content-identical items at different offsets are moves.
    let mut left_keys: Vec<&String> = left_items.iter().map(|item| &item.key).collect();
    let mut right_keys: Vec<&String> = right_items.iter().map(|item| &item.key).collect();
    left_keys.sort();
    right_keys.sort();
    if left_keys == right_keys {
        // Same content multiset: only items whose partner sits at a
        // different index are moves — not the whole document.
        let moved: Vec<DiffEntry> = left_items
            .iter()
            .zip(right_items.iter())
            .filter(|(left, right)| left.key != right.key)
            .map(|(left, _)| DiffEntry::Moved {
                label: left.label.clone(),
            })
            .collect();
        entries.clear();
        entries.extend(moved);
    }
    Ok(entries)
}

struct Item {
    /// Content identity: qname + sorted attributes + normalized text.
    key: String,
    label: String,
}

fn sequence(document: &XmlDocument, options: DiffOptions) -> Vec<Item> {
    let mut items = Vec::new();
    let Some(root) = document.root_element() else {
        return items;
    };
    walk(document, root, &mut items, options);
    items
}

fn walk(document: &XmlDocument, node: NodeId, items: &mut Vec<Item>, options: DiffOptions) {
    // Field separators: \x1f/\x1e are illegal in XML 1.0 names and content,
    // so key concatenation is unambiguous (`a="1 b=2"` vs `a="1" b="2"`).
    const FIELD: char = '\u{1f}';
    const PAIR: char = '\u{1e}';
    let mut sorted_attrs = document.attributes(node);
    sorted_attrs.sort();
    let mut key = document.qname(node).map(|q| q.render()).unwrap_or_default();
    for (name, value) in &sorted_attrs {
        key.push(FIELD);
        key.push_str(name);
        key.push(PAIR);
        key.push_str(value);
    }
    let label = document.qname(node).map(|q| q.render()).unwrap_or_default();
    let mut label = label;
    for (name, value) in &sorted_attrs {
        label.push_str(&format!(" {name}=\"{value}\""));
    }

    // Direct text content (normalized): formatting whitespace collapses.
    let mut text = String::new();
    for child in document.children(node) {
        match document.kind(child) {
            Some(XmlNodeKind::Text) => {
                let content = document.node_text(child).unwrap_or_default();
                let normalized = normalize_whitespace(content);
                if !normalized.is_empty() {
                    text.push(FIELD);
                    text.push_str(&normalized);
                }
            }
            Some(XmlNodeKind::CData) => {
                text.push(FIELD);
                text.push_str("<![CDATA[");
                text.push_str(document.node_text(child).unwrap_or_default());
                text.push_str("]]>");
            }
            Some(XmlNodeKind::Comment) if options.include_comments_and_pis => {
                text.push(FIELD);
                text.push_str("<!--");
                text.push_str(document.comment_text(child).unwrap_or_default());
                text.push_str("-->");
            }
            Some(XmlNodeKind::ProcessingInstruction) if options.include_comments_and_pis => {
                if let Some((target, data)) = document.pi(child) {
                    text.push(FIELD);
                    match data {
                        Some(data) => text.push_str(&format!("<?{target} {data}?>")),
                        None => text.push_str(&format!("<?{target}?>")),
                    }
                }
            }
            _ => {}
        }
    }
    key.push('|');
    key.push_str(&text);
    items.push(Item { key, label });

    for child in document.children(node) {
        if document.kind(child) == Some(XmlNodeKind::Element) {
            walk(document, child, items, options);
        }
    }
}

/// Collapses XML whitespace runs to single spaces and trims: pure
/// formatting (pretty-print indentation) becomes identical.
fn normalize_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for ch in text.chars() {
        if matches!(ch, ' ' | '\t' | '\n' | '\r') {
            pending_space = true;
        } else {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.push(ch);
        }
    }
    out
}
