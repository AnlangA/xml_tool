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
    /// Whether the node can be expanded (has element children).
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

    fn push_subtree(
        document: &XmlDocument,
        node: NodeId,
        depth: usize,
        expanded: &HashSet<NodeId>,
        rows: &mut Vec<Row>,
    ) {
        let children = document.children(node);
        let expandable = children
            .iter()
            .any(|child| document.kind(*child) == Some(XmlNodeKind::Element));
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
        for child in children {
            match document.kind(child) {
                Some(XmlNodeKind::Element) => {
                    Self::push_subtree(document, child, depth + 1, expanded, rows);
                }
                Some(XmlNodeKind::Text) | Some(XmlNodeKind::CData) => {
                    let shown = document
                        .node_text(child)
                        .map(|text| !text.trim().is_empty())
                        .unwrap_or(false);
                    if shown {
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
        let expandable = document
            .children(node)
            .iter()
            .any(|child| document.kind(*child) == Some(XmlNodeKind::Element));
        if !expandable {
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
