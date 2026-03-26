use std::sync::Arc;

use egui::{FontId, RichText, Ui};

use crate::cache::{SearchCache, SearchKey, SearchResults};
use crate::xml::{XmlElement, XmlNode, truncate_str};

use super::theme::Theme;

// ---------------------------------------------------------------------------
// SelectedNodeInfo
// ---------------------------------------------------------------------------

/// Snapshot of a selected element's data for display in the details panel.
#[derive(Debug, Clone)]
pub struct SelectedNodeInfo {
    pub id: u64,
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub text: Option<String>,
    pub child_count: usize,
}

// ---------------------------------------------------------------------------
// XmlTreeView
// ---------------------------------------------------------------------------

pub struct XmlTreeView {
    selected_id: Option<u64>,
    selected_info: Option<SelectedNodeInfo>,
    search_cache: SearchCache,
}

impl Default for XmlTreeView {
    fn default() -> Self {
        Self {
            selected_id: None,
            selected_info: None,
            search_cache: SearchCache::new(32),
        }
    }
}

impl XmlTreeView {
    pub fn new() -> Self {
        Self::default()
    }

    /// Render the tree with search filtering.
    pub fn show_with_search(
        &mut self,
        ui: &mut Ui,
        root: &XmlNode,
        doc_version: u64,
        query: &str,
        case_sensitive: bool,
    ) {
        let search = if query.is_empty() {
            None
        } else {
            let cached_results = self.search_results_for(root, doc_version, query, case_sensitive);
            Some(SearchContext {
                query,
                case_sensitive,
                folded_query: (!case_sensitive).then(|| query.to_lowercase()),
                cached_results,
            })
        };
        self.show_node(ui, root, 0, search.as_ref());
    }

    /// Return a reference to the currently selected node info, if any.
    pub fn get_selected_info(&self) -> Option<&SelectedNodeInfo> {
        self.selected_info.as_ref()
    }

    pub fn selected_id(&self) -> Option<u64> {
        self.selected_id
    }

    pub fn select_id(&mut self, root: &XmlNode, id: u64) {
        self.selected_id = Some(id);
        self.sync_selected_info(root);
    }

    pub fn sync_selected_info(&mut self, root: &XmlNode) {
        let Some(selected_id) = self.selected_id else {
            self.selected_info = None;
            return;
        };

        self.selected_info = Self::find_element(root, selected_id).map(Self::build_selected_info);
        if self.selected_info.is_none() {
            self.selected_id = None;
        }
    }

    /// Clear the current selection.
    pub fn clear_selection(&mut self) {
        self.selected_id = None;
        self.selected_info = None;
    }

    pub fn clear_search_cache(&mut self) {
        self.search_cache.clear();
    }

    // -----------------------------------------------------------------------
    // Private
    // -----------------------------------------------------------------------

    fn search_results_for(
        &mut self,
        root: &XmlNode,
        doc_version: u64,
        query: &str,
        case_sensitive: bool,
    ) -> Arc<SearchResults> {
        let normalized_query = if case_sensitive {
            query.to_string()
        } else {
            query.to_lowercase()
        };
        let key = SearchKey::new(normalized_query.clone(), case_sensitive, doc_version);

        self.search_cache.get_or_insert_with(key, || {
            Arc::new(Self::build_search_results(
                root,
                &normalized_query,
                case_sensitive,
            ))
        })
    }

    fn build_search_results(root: &XmlNode, query: &str, case_sensitive: bool) -> SearchResults {
        let matcher = SearchMatcher {
            query,
            case_sensitive,
        };
        let mut visible_elements = Vec::new();
        let mut name_matches = Vec::new();

        Self::collect_search_matches(root, &matcher, &mut visible_elements, &mut name_matches);
        visible_elements.sort_unstable();
        visible_elements.dedup();
        name_matches.sort_unstable();
        name_matches.dedup();

        SearchResults {
            visible_elements,
            name_matches,
        }
    }

    fn collect_search_matches(
        node: &XmlNode,
        matcher: &SearchMatcher<'_>,
        visible_elements: &mut Vec<u64>,
        name_matches: &mut Vec<u64>,
    ) -> bool {
        match node {
            XmlNode::Element(elem) => {
                let mut subtree_matches = false;

                if matcher.matches(&elem.name) {
                    name_matches.push(elem.id.0);
                    subtree_matches = true;
                }

                if elem
                    .attributes
                    .iter()
                    .any(|attr| matcher.matches(&attr.name) || matcher.matches(&attr.value))
                {
                    subtree_matches = true;
                }

                if elem
                    .text
                    .as_deref()
                    .is_some_and(|text| matcher.matches(text))
                {
                    subtree_matches = true;
                }

                for child in &elem.children {
                    if Self::collect_search_matches(child, matcher, visible_elements, name_matches)
                    {
                        subtree_matches = true;
                    }
                }

                if subtree_matches {
                    visible_elements.push(elem.id.0);
                }

                subtree_matches
            }
            XmlNode::Text(text) => {
                let trimmed = text.trim();
                !trimmed.is_empty() && matcher.matches(trimmed)
            }
            XmlNode::Comment(comment) => matcher.matches(comment),
        }
    }

    fn show_node(
        &mut self,
        ui: &mut Ui,
        node: &XmlNode,
        depth: usize,
        search: Option<&SearchContext>,
    ) {
        match node {
            XmlNode::Element(elem) => self.show_element(ui, elem, depth, search),

            XmlNode::Text(text) => {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    // Skip if search is active and doesn't match
                    if let Some(s) = search
                        && !s.matches(trimmed)
                    {
                        return;
                    }
                    ui.horizontal(|ui| {
                        ui.add_space(indent_px(depth));
                        ui.label(
                            RichText::new(format!("\"{}\"", truncate_str(trimmed, 60)))
                                .font(FontId::monospace(11.0))
                                .color(Theme::TEXT_MUTED),
                        );
                    });
                }
            }

            XmlNode::Comment(comment) => {
                // Skip if search is active and doesn't match
                if let Some(s) = search
                    && !s.matches(comment)
                {
                    return;
                }
                ui.horizontal(|ui| {
                    ui.add_space(indent_px(depth));
                    ui.label(
                        RichText::new(format!("<!-- {} -->", truncate_str(comment, 50)))
                            .font(FontId::monospace(11.0))
                            .color(Theme::COMMENT),
                    );
                });
            }
        }
    }

    fn show_element(
        &mut self,
        ui: &mut Ui,
        elem: &XmlElement,
        depth: usize,
        search: Option<&SearchContext>,
    ) {
        let id = elem.id.0;
        let is_selected = self.selected_id == Some(id);

        // Check if this element or any descendant matches search
        let matches_search = search.is_none_or(|s| s.is_visible_element(id));
        if !matches_search {
            return;
        }

        // Highlight if matches search
        let highlight = search.is_some_and(|s| s.is_name_match(id));

        let label = Self::format_label(elem, highlight);

        if elem.has_children() {
            // Collapsible header for elements with children.
            let header = egui::CollapsingHeader::new(&label)
                .id_salt(id)
                .default_open(depth < 2 || highlight);

            let response = header.show(ui, |ui| {
                for child in &elem.children {
                    self.show_node(ui, child, depth + 1, search);
                }
            });

            if response.header_response.clicked() {
                self.select_element(elem);
            }

            // Highlight the header when selected.
            if is_selected {
                ui.painter()
                    .rect_filled(response.header_response.rect, 4.0, Theme::SELECTION);
            }
        } else {
            // Leaf element — plain selectable row.
            ui.horizontal(|ui| {
                ui.add_space(indent_px(depth) + 20.0);
                let res = ui.selectable_label(is_selected, &label);
                if res.clicked() {
                    self.select_element(elem);
                }
            });
        }
    }

    fn select_element(&mut self, elem: &XmlElement) {
        self.selected_id = Some(elem.id.0);
        self.selected_info = Some(Self::build_selected_info(elem));
    }

    /// Build a concise label string for an element node.
    fn format_label(elem: &XmlElement, highlight: bool) -> String {
        let mut label = String::new();

        // Element name with optional highlight marker
        if highlight {
            label.push_str("🔍 ");
        }
        label.push('<');
        label.push_str(&elem.name);

        if !elem.attributes.is_empty() {
            label.push(' ');
            label.push_str(&elem.attributes_preview(2));
        }

        if let Some(text) = &elem.text {
            let t = text.trim();
            if !t.is_empty() {
                label.push_str(&format!("> {}", truncate_str(t, 24)));
            }
        }

        if !elem.children.is_empty() {
            label.push_str(&format!(" [{}]", elem.children.len()));
        }

        label
    }

    fn build_selected_info(elem: &XmlElement) -> SelectedNodeInfo {
        SelectedNodeInfo {
            id: elem.id.0,
            name: elem.name.clone(),
            attributes: elem
                .attributes
                .iter()
                .map(|a| (a.name.clone(), a.value.clone()))
                .collect(),
            text: elem.text.clone(),
            child_count: elem.children.len(),
        }
    }

    fn find_element(node: &XmlNode, id: u64) -> Option<&XmlElement> {
        let element = node.as_element()?;
        if element.id.0 == id {
            return Some(element);
        }

        for child in &element.children {
            if let Some(found) = Self::find_element(child, id) {
                return Some(found);
            }
        }

        None
    }
}

// ---------------------------------------------------------------------------
// Search context
// ---------------------------------------------------------------------------

struct SearchContext<'a> {
    query: &'a str,
    case_sensitive: bool,
    folded_query: Option<String>,
    cached_results: Arc<SearchResults>,
}

impl SearchContext<'_> {
    fn is_visible_element(&self, id: u64) -> bool {
        self.cached_results.contains_visible_element(id)
    }

    fn is_name_match(&self, id: u64) -> bool {
        self.cached_results.contains_name_match(id)
    }

    fn matches(&self, text: &str) -> bool {
        if self.case_sensitive {
            text.contains(self.query)
        } else {
            self.folded_query
                .as_deref()
                .is_some_and(|query| text.to_lowercase().contains(query))
        }
    }
}

struct SearchMatcher<'a> {
    query: &'a str,
    case_sensitive: bool,
}

impl SearchMatcher<'_> {
    fn matches(&self, text: &str) -> bool {
        if self.case_sensitive {
            text.contains(self.query)
        } else {
            text.to_lowercase().contains(self.query)
        }
    }
}

/// Convert tree depth to left-padding pixels.
#[inline]
fn indent_px(depth: usize) -> f32 {
    5.0 + depth as f32 * 16.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::{XmlAttribute, XmlElement};

    fn sample_tree() -> XmlNode {
        let mut root = XmlElement::new("root".to_string());
        let mut branch = XmlElement::new("branch".to_string());
        let mut item = XmlElement::new("NeedleItem".to_string());
        item.attributes.push(XmlAttribute {
            name: "kind".to_string(),
            value: "needle".to_string(),
            namespace: None,
        });
        branch.children.push(XmlNode::Element(item));
        root.children.push(XmlNode::Element(branch));
        XmlNode::Element(root)
    }

    #[test]
    fn search_results_keep_matching_ancestors_visible() {
        let root = sample_tree();
        let root_elem = root.as_element().expect("root element");
        let branch_elem = root_elem.children[0].as_element().expect("branch element");
        let item_elem = branch_elem.children[0].as_element().expect("item element");

        let mut view = XmlTreeView::new();
        let results = view.search_results_for(&root, 0, "needle", false);

        assert!(results.contains_visible_element(root_elem.id.0));
        assert!(results.contains_visible_element(branch_elem.id.0));
        assert!(results.contains_visible_element(item_elem.id.0));
        assert!(results.contains_name_match(item_elem.id.0));
        assert!(!results.contains_name_match(branch_elem.id.0));
    }

    #[test]
    fn search_results_are_cached_by_version_and_query() {
        let root = sample_tree();
        let mut view = XmlTreeView::new();

        let first = view.search_results_for(&root, 0, "needle", false);
        let second = view.search_results_for(&root, 0, "needle", false);
        let different_version = view.search_results_for(&root, 1, "needle", false);

        assert!(Arc::ptr_eq(&first, &second));
        assert!(!Arc::ptr_eq(&first, &different_version));
        assert_eq!(view.search_cache.len(), 2);
    }
}
