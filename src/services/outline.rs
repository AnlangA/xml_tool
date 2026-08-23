//! Virtualized outline: a flattened row list over the arena DOM.
//!
//! The tree view never walks visible descendants recursively. [`FlatTree`]
//! materializes one row per *currently visible* node — collapsed parents
//! contribute their subtree zero rows — so `egui::ScrollArea::show_rows`
//! renders exactly the viewport. Row height is fixed; expansion state lives
//! in a `HashSet<NodeId>` owned by the UI session.

use std::collections::HashSet;

use crate::core::document::{NodeId, XmlDocument, XmlNodeKind};

/// One visible outline row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    /// The DOM node this row shows.
    pub node: NodeId,
    /// Indentation level (root element = 0).
    pub depth: usize,
    /// Whether the node can be expanded (element with visible children).
    pub expandable: bool,
    /// Whether it currently is expanded.
    pub expanded: bool,
}

/// Flattened visible rows plus the expansion state they came from.
#[derive(Debug, Clone, Default)]
pub struct FlatTree {
    pub rows: Vec<Row>,
}

impl FlatTree {
    /// Flattens `document` honoring `expanded`. Children of collapsed
    /// elements are skipped entirely; non-element children of an *expanded*
    /// element are shown (text, CDATA, comments, PIs).
    pub fn build(document: &XmlDocument, expanded: &HashSet<NodeId>) -> FlatTree {
        let mut rows = Vec::new();
        let Some(root) = document.root_element() else {
            return FlatTree { rows };
        };
        // Top-level comments/PIs before the root element stay visible.
        for sibling in document.children(NodeId::DOCUMENT) {
            if sibling == root {
                break;
            }
            if matches!(
                document.kind(sibling),
                Some(XmlNodeKind::Comment) | Some(XmlNodeKind::ProcessingInstruction)
            ) {
                rows.push(Row {
                    node: sibling,
                    depth: 0,
                    expandable: false,
                    expanded: false,
                });
            }
        }
        Self::push_subtree(document, root, 0, expanded, &mut rows);
        FlatTree { rows }
    }

    /// Whether `child` contributes a row when its parent element is expanded.
    pub fn child_visible(document: &XmlDocument, child: NodeId) -> bool {
        match document.kind(child) {
            Some(XmlNodeKind::Element) => true,
            Some(XmlNodeKind::Text) | Some(XmlNodeKind::CData) => document
                .node_text(child)
                .map(|text| !text.trim().is_empty())
                .unwrap_or(false),
            Some(XmlNodeKind::Comment) | Some(XmlNodeKind::ProcessingInstruction) => true,
            _ => false,
        }
    }

    /// Whether `node` is an element with at least one visible child row.
    pub fn is_expandable(document: &XmlDocument, node: NodeId) -> bool {
        document.kind(node) == Some(XmlNodeKind::Element)
            && document
                .children(node)
                .iter()
                .any(|child| Self::child_visible(document, *child))
    }

    /// Every element node that can be expanded in the outline.
    pub fn collect_expandable(document: &XmlDocument) -> HashSet<NodeId> {
        let mut set = HashSet::new();
        for &id in document.document_order() {
            let node = NodeId(id);
            if Self::is_expandable(document, node) {
                set.insert(node);
            }
        }
        set
    }

    fn push_subtree(
        document: &XmlDocument,
        node: NodeId,
        depth: usize,
        expanded: &HashSet<NodeId>,
        rows: &mut Vec<Row>,
    ) {
        let expandable = Self::is_expandable(document, node);
        let is_expanded = expandable && expanded.contains(&node);
        rows.push(Row {
            node,
            depth,
            expandable,
            expanded: is_expanded,
        });
        if !is_expanded {
            return;
        }
        for child in document.children(node) {
            match document.kind(child) {
                Some(XmlNodeKind::Element) => {
                    Self::push_subtree(document, child, depth + 1, expanded, rows);
                }
                Some(XmlNodeKind::Text) | Some(XmlNodeKind::CData) => {
                    if Self::child_visible(document, child) {
                        rows.push(Row {
                            node: child,
                            depth: depth + 1,
                            expandable: false,
                            expanded: false,
                        });
                    }
                }
                Some(XmlNodeKind::Comment) | Some(XmlNodeKind::ProcessingInstruction) => {
                    rows.push(Row {
                        node: child,
                        depth: depth + 1,
                        expandable: false,
                        expanded: false,
                    });
                }
                _ => {}
            }
        }
    }

    /// Total visible row count (for `show_rows`).
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether there are no visible rows.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The viewport slice with a symmetric `overscan` row buffer.
    pub fn viewport(&self, first_visible: usize, last_visible: usize, overscan: usize) -> &[Row] {
        let start = first_visible.saturating_sub(overscan).min(self.rows.len());
        let end = (last_visible + overscan + 1).min(self.rows.len());
        &self.rows[start..end.max(start)]
    }

    /// Row index of `node`, when visible.
    pub fn row_of(&self, node: NodeId) -> Option<usize> {
        self.rows.iter().position(|row| row.node == node)
    }

    /// Toggles expansion of `node` in `expanded` and reports whether the
    /// node is expandable at all (no-op returns `false`).
    pub fn toggle(document: &XmlDocument, expanded: &mut HashSet<NodeId>, node: NodeId) -> bool {
        if !Self::is_expandable(document, node) {
            return false;
        }
        if expanded.contains(&node) {
            expanded.remove(&node);
        } else {
            expanded.insert(node);
        }
        true
    }
}

/// XPath-like path for `node`, e.g. `/catalog[1]/item[12]/title[1]`.
pub fn node_path(document: &XmlDocument, node: NodeId) -> Option<String> {
    let mut chain = Vec::new();
    let mut current = node;
    loop {
        if current == NodeId::DOCUMENT {
            break;
        }
        if document.kind(current) == Some(XmlNodeKind::Element) {
            chain.push(current);
        }
        current = document.parent(current)?;
    }
    if chain.is_empty() {
        return None;
    }
    chain.reverse();
    let mut path = String::new();
    for element in chain {
        let name = document.qname(element).map(|q| q.render()).unwrap_or_default();
        let parent = document.parent(element).unwrap_or(NodeId::DOCUMENT);
        let index = sibling_element_index(document, parent, element, &name);
        path.push('/');
        path.push_str(&name);
        path.push_str(&format!("[{}]", index + 1));
    }
    Some(path)
}

/// Resolves `path` (from [`node_path`]) back to a node id when possible.
pub fn node_from_path(document: &XmlDocument, path: &str) -> Option<NodeId> {
    let segments: Vec<&str> = path.trim().split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return None;
    }
    let (root_name, root_index) = parse_path_segment(segments[0])?;
    let root = document.root_element()?;
    if document.qname(root).map(|q| q.render()) != Some(root_name.clone()) {
        return None;
    }
    let root_parent = document.parent(root).unwrap_or(NodeId::DOCUMENT);
    if sibling_element_index(document, root_parent, root, &root_name) != root_index {
        return None;
    }
    let mut current = root;
    for segment in segments.iter().skip(1) {
        let (name, index) = parse_path_segment(segment)?;
        let children: Vec<NodeId> = document
            .children(current)
            .into_iter()
            .filter(|child| document.kind(*child) == Some(XmlNodeKind::Element))
            .filter(|child| {
                document
                    .qname(*child)
                    .map(|q| q.render() == name)
                    .unwrap_or(false)
            })
            .collect();
        current = children.get(index).copied()?;
    }
    Some(current)
}

fn parse_path_segment(segment: &str) -> Option<(String, usize)> {
    let open = segment.rfind('[')?;
    if !segment.ends_with(']') {
        return None;
    }
    let close = segment.len() - 1;
    if open >= close {
        return None;
    }
    let name = segment[..open].to_string();
    let index = segment[open + 1..close]
        .parse::<usize>()
        .ok()?
        .saturating_sub(1);
    Some((name, index))
}

fn sibling_element_index(
    document: &XmlDocument,
    parent: NodeId,
    node: NodeId,
    name: &str,
) -> usize {
    let mut index = 0;
    for child in document.children(parent) {
        if document.kind(child) != Some(XmlNodeKind::Element) {
            continue;
        }
        let child_name = document.qname(child).map(|q| q.render()).unwrap_or_default();
        if child_name == name {
            if child == node {
                return index;
            }
            index += 1;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_only_element_is_expandable() {
        let doc = XmlDocument::parse(b"<root>hello</root>".as_slice()).unwrap();
        let root = doc.root_element().unwrap();
        assert!(FlatTree::is_expandable(&doc, root));
        let mut expanded = HashSet::new();
        expanded.insert(root);
        let tree = FlatTree::build(&doc, &expanded);
        assert_eq!(tree.len(), 2, "root + text row");
    }

    #[test]
    fn node_path_round_trip() {
        let doc = XmlDocument::parse(b"<a><b/><b><c/></b></a>".as_slice()).unwrap();
        let a = doc.root_element().unwrap();
        let bs: Vec<NodeId> = doc
            .children(a)
            .into_iter()
            .filter(|id| doc.kind(*id) == Some(XmlNodeKind::Element))
            .collect();
        let path = node_path(&doc, bs[1]).expect("path");
        assert_eq!(path, "/a[1]/b[2]");
        assert_eq!(node_from_path(&doc, &path), Some(bs[1]));
    }
}
