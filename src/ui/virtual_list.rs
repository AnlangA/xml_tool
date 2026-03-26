/// Virtual scrolling implementation for large XML trees
///
/// This module provides efficient rendering of large tree structures by only
/// rendering visible nodes, significantly improving performance for documents
/// with thousands of nodes.
use egui::{Rect, Response, Ui, Vec2};
use std::ops::Range;

/// Represents a flattened node in the tree for virtual scrolling
#[derive(Clone)]
pub struct FlatNode<T> {
    pub data: T,
    pub depth: usize,
    pub is_expanded: bool,
    pub has_children: bool,
}

/// Virtual list that only renders visible items
pub struct VirtualList<T> {
    items: Vec<FlatNode<T>>,
    row_height: f32,
    visible_range: Range<usize>,
    scroll_offset: f32,
}

impl<T: Clone> VirtualList<T> {
    pub fn new(row_height: f32) -> Self {
        Self {
            items: Vec::new(),
            row_height,
            visible_range: 0..0,
            scroll_offset: 0.0,
        }
    }

    /// Set the items to display
    pub fn set_items(&mut self, items: Vec<FlatNode<T>>) {
        self.items = items;
    }

    /// Get the total number of items
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Check if the list is empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Calculate which items are visible in the viewport
    fn calculate_visible_range(&mut self, viewport_height: f32) {
        let total_height = self.items.len() as f32 * self.row_height;

        if total_height == 0.0 {
            self.visible_range = 0..0;
            return;
        }

        // Calculate first and last visible indices with buffer
        let buffer_rows = 5; // Render a few extra rows above/below for smooth scrolling

        let first_visible = (self.scroll_offset / self.row_height).floor() as usize;
        let first_visible = first_visible.saturating_sub(buffer_rows);

        let visible_rows = (viewport_height / self.row_height).ceil() as usize;
        let last_visible = (first_visible + visible_rows + buffer_rows * 2).min(self.items.len());

        self.visible_range = first_visible..last_visible;
    }

    /// Show the virtual list
    pub fn show<F>(&mut self, ui: &mut Ui, mut render_item: F) -> Response
    where
        F: FnMut(&mut Ui, usize, &FlatNode<T>),
    {
        let available_height = ui.available_height();
        let total_height = self.items.len() as f32 * self.row_height;

        // Calculate visible range
        self.calculate_visible_range(available_height);

        // Create scroll area
        let scroll_area = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Reserve space for all items
                ui.allocate_space(Vec2::new(ui.available_width(), total_height));

                // Only render visible items
                for idx in self.visible_range.clone() {
                    if let Some(item) = self.items.get(idx) {
                        let y_offset = idx as f32 * self.row_height;

                        // Position the item at the correct vertical offset
                        let rect = Rect::from_min_size(
                            ui.cursor().min + Vec2::new(0.0, y_offset),
                            Vec2::new(ui.available_width(), self.row_height),
                        );

                        let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                        render_item(&mut child_ui, idx, item);
                    }
                }
            });

        // Update scroll offset
        self.scroll_offset = scroll_area.state.offset.y;

        // Return a dummy response since we're inside a scroll area
        ui.allocate_response(Vec2::ZERO, egui::Sense::hover())
    }

    /// Get item at index
    pub fn get(&self, index: usize) -> Option<&FlatNode<T>> {
        self.items.get(index)
    }

    /// Get mutable item at index
    pub fn get_mut(&mut self, index: usize) -> Option<&mut FlatNode<T>> {
        self.items.get_mut(index)
    }

    /// Toggle expansion state of an item
    pub fn toggle_expansion(&mut self, index: usize) {
        if let Some(item) = self.items.get_mut(index) {
            item.is_expanded = !item.is_expanded;
        }
    }

    /// Get the visible range
    pub fn visible_range(&self) -> Range<usize> {
        self.visible_range.clone()
    }
}

/// Helper to flatten a tree structure for virtual scrolling
pub struct TreeFlattener;

impl TreeFlattener {
    /// Flatten a tree into a linear list based on expansion state
    ///
    /// This is a generic helper that can be used with any tree structure.
    /// The caller provides closures to extract children and check expansion state.
    pub fn flatten<T, F, G>(root: &T, get_children: &F, is_expanded: &G) -> Vec<FlatNode<T>>
    where
        T: Clone,
        F: Fn(&T) -> Vec<T>,
        G: Fn(&T) -> bool,
    {
        let mut result = Vec::new();
        Self::flatten_recursive(root, 0, &mut result, get_children, is_expanded);
        result
    }

    fn flatten_recursive<T, F, G>(
        node: &T,
        depth: usize,
        result: &mut Vec<FlatNode<T>>,
        get_children: &F,
        is_expanded: &G,
    ) where
        T: Clone,
        F: Fn(&T) -> Vec<T>,
        G: Fn(&T) -> bool,
    {
        let children = get_children(node);
        let has_children = !children.is_empty();
        let expanded = is_expanded(node);

        result.push(FlatNode {
            data: node.clone(),
            depth,
            is_expanded: expanded,
            has_children,
        });

        // Only recurse into children if expanded
        if expanded {
            for child in children {
                Self::flatten_recursive(&child, depth + 1, result, get_children, is_expanded);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct TestNode {
        _name: String,
        children: Vec<TestNode>,
        expanded: bool,
    }

    impl TestNode {
        fn new(name: &str) -> Self {
            Self {
                _name: name.to_string(),
                children: Vec::new(),
                expanded: false,
            }
        }

        fn with_children(mut self, children: Vec<TestNode>) -> Self {
            self.children = children;
            self
        }

        fn expanded(mut self) -> Self {
            self.expanded = true;
            self
        }
    }

    #[test]
    fn test_flatten_simple_tree() {
        let root = TestNode::new("root")
            .with_children(vec![TestNode::new("child1"), TestNode::new("child2")])
            .expanded();

        let flattened = TreeFlattener::flatten(&root, &|n| n.children.clone(), &|n| n.expanded);

        assert_eq!(flattened.len(), 3); // root + 2 children
        assert_eq!(flattened[0].depth, 0);
        assert_eq!(flattened[1].depth, 1);
        assert_eq!(flattened[2].depth, 1);
    }

    #[test]
    fn test_flatten_collapsed_tree() {
        let root = TestNode::new("root")
            .with_children(vec![TestNode::new("child1"), TestNode::new("child2")]);
        // Not expanded

        let flattened = TreeFlattener::flatten(&root, &|n| n.children.clone(), &|n| n.expanded);

        assert_eq!(flattened.len(), 1); // Only root visible
    }

    #[test]
    fn test_virtual_list_visible_range() {
        let mut list = VirtualList::new(20.0);

        let items: Vec<FlatNode<String>> = (0..100)
            .map(|i| FlatNode {
                data: format!("Item {}", i),
                depth: 0,
                is_expanded: false,
                has_children: false,
            })
            .collect();

        list.set_items(items);

        // Simulate viewport of 200px height
        list.calculate_visible_range(200.0);

        // Should show approximately 10 items (200px / 20px per item) + buffer
        assert!(list.visible_range.len() > 10);
        assert!(list.visible_range.len() < 30); // With buffer
    }
}
