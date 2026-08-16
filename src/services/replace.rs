//! Batch find-and-replace preview construction.
//!
//! The search index produces matches; this module turns them into
//! [`ReplaceOp`]s for [`crate::core::Command::BatchReplace`]: the whole
//! batch validates, applies, and undoes as one step. Any replacement that
//! would produce illegal XML content rejects the entire preview.

use crate::core::command::{ReplaceOp, ReplaceTarget};
use crate::core::document::{NodeId, XmlDocument, XmlNodeKind};
use crate::services::search::{SearchHit, SearchIndex};

/// Which node fields the replacement touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceScope {
    /// Text and CDATA content.
    Text,
    /// Attribute values.
    AttributeValues,
    /// Comment content.
    Comments,
    /// All of the above.
    All,
}

/// A preview of one prospective replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacementPreview {
    pub node: NodeId,
    pub target: ReplaceTarget,
    pub old_value: String,
    pub new_value: String,
}

/// Builds the full replacement list for `needle → replacement` over
/// `index`. Literal matching only (the plan's Literal mode; regex and XPath
/// modes build on the same op list). Returns `Err` naming the first node
/// whose replacement would be illegal XML.
pub fn build_replacements(
    document: &XmlDocument,
    index: &mut SearchIndex,
    order: &[u64],
    needle: &str,
    replacement: &str,
    scope: ReplaceScope,
    case_sensitive: bool,
) -> Result<Vec<ReplacementPreview>, String> {
    let hits = index.search(needle, case_sensitive, order);
    let mut previews = Vec::with_capacity(hits.len());
    for hit in hits {
        let Some(kind) = document.kind(hit.node) else {
            continue;
        };
        let applies = match scope {
            ReplaceScope::All => true,
            ReplaceScope::Text => matches!(kind, XmlNodeKind::Text | XmlNodeKind::CData),
            ReplaceScope::Comments => matches!(kind, XmlNodeKind::Comment),
            ReplaceScope::AttributeValues => matches!(kind, XmlNodeKind::Element),
        };
        if !applies {
            continue;
        }
        for candidate in candidates_for(document, hit.node, kind, &hit) {
            if !candidate.old_value.contains(needle)
                && !folded_contains(&candidate.old_value, needle, case_sensitive)
            {
                continue;
            }
            let new_value =
                replace_all_folded(&candidate.old_value, needle, replacement, case_sensitive);
            validate_replacement(kind, &new_value)
                .map_err(|problem| format!("node {:?}: {problem}", hit.node))?;
            previews.push(candidate.with_new_value(new_value));
        }
    }
    Ok(previews)
}

/// Converts a preview into the command op list.
pub fn to_ops(previews: &[ReplacementPreview]) -> Vec<ReplaceOp> {
    previews
        .iter()
        .map(|preview| ReplaceOp {
            node: preview.node,
            target: preview.target.clone(),
            old_value: preview.old_value.clone(),
            new_value: preview.new_value.clone(),
        })
        .collect()
}

fn candidates_for(
    document: &XmlDocument,
    node: NodeId,
    kind: XmlNodeKind,
    hit: &SearchHit,
) -> Vec<ReplacementPreview> {
    match kind {
        XmlNodeKind::Element => document
            .attributes(node)
            .into_iter()
            .map(|(name, value)| ReplacementPreview {
                node,
                target: ReplaceTarget::Attribute(name),
                old_value: value,
                new_value: String::new(),
            })
            .collect(),
        XmlNodeKind::Text | XmlNodeKind::CData => vec![match document.node_text(node) {
            Some(text) => ReplacementPreview {
                node,
                target: ReplaceTarget::NodeText,
                old_value: text.to_string(),
                new_value: String::new(),
            },
            None => return vec![],
        }]
        .into_iter()
        .filter(|preview| hit.node == preview.node)
        .collect(),
        XmlNodeKind::Comment => match document.comment_text(node) {
            Some(text) => vec![ReplacementPreview {
                node,
                target: ReplaceTarget::Comment,
                old_value: text.to_string(),
                new_value: String::new(),
            }],
            None => vec![],
        },
        _ => Vec::new(),
    }
}

impl ReplacementPreview {
    fn with_new_value(self, new_value: String) -> ReplacementPreview {
        ReplacementPreview {
            node: self.node,
            target: self.target,
            old_value: self.old_value,
            new_value,
        }
    }
}

fn folded_contains(haystack: &str, needle: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        haystack.contains(needle)
    } else {
        haystack.to_lowercase().contains(&needle.to_lowercase())
    }
}

fn replace_all_folded(
    haystack: &str,
    needle: &str,
    replacement: &str,
    case_sensitive: bool,
) -> String {
    if case_sensitive {
        haystack.replace(needle, replacement)
    } else {
        // Char-wise fold keeps non-ASCII correct; the batch preview is not
        // a hot path.
        char_folded_replace(haystack, needle, replacement)
    }
}

fn char_folded_replace(haystack: &str, needle: &str, replacement: &str) -> String {
    let needle_lower: Vec<char> = needle.to_lowercase().chars().collect();
    let chars: Vec<char> = haystack.chars().collect();
    let mut out = String::with_capacity(haystack.len());
    let mut index = 0;
    while index < chars.len() {
        let window: String = chars[index..].iter().take(needle_lower.len()).collect();
        if window.to_lowercase() == needle_lower.iter().collect::<String>() {
            out.push_str(replacement);
            index += needle_lower.len();
        } else {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

fn validate_replacement(kind: XmlNodeKind, value: &str) -> Result<(), String> {
    match kind {
        XmlNodeKind::Text => {
            if value.contains('<') {
                Err(String::from("text content must not contain '<'"))
            } else {
                Ok(())
            }
        }
        XmlNodeKind::CData => {
            if value.contains("]]>") {
                Err(String::from("CDATA content must not contain ']]>'"))
            } else {
                Ok(())
            }
        }
        XmlNodeKind::Comment => {
            if value.contains("--") || value.ends_with('-') {
                Err(String::from("comments must not contain '--'"))
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}
