use egui::{FontId, RichText, Ui};

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

#[derive(Default)]
pub struct XmlTreeView {
    selected_id: Option<u64>,
    selected_info: Option<SelectedNodeInfo>,
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
        query: &str,
        case_sensitive: bool,
    ) {
        let search = if query.is_empty() {
            None
        } else {
            Some(SearchContext {
                query,
                case_sensitive,
                folded_query: (!case_sensitive).then(|| query.to_lowercase()),
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

    // -----------------------------------------------------------------------
    // Private
    // -----------------------------------------------------------------------

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
        let matches_search = search.is_none_or(|s| self.element_matches(elem, s));
        if !matches_search {
            return;
        }

        // Highlight if matches search
        let highlight = search.is_some_and(|s| s.matches(&elem.name));

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

    /// Check if element or any descendant matches search.
    fn element_matches(&self, elem: &XmlElement, search: &SearchContext) -> bool {
        // Check element name
        if search.matches(&elem.name) {
            return true;
        }

        // Check attributes
        for attr in &elem.attributes {
            if search.matches(&attr.name) || search.matches(&attr.value) {
                return true;
            }
        }

        // Check text content
        if let Some(text) = &elem.text
            && search.matches(text)
        {
            return true;
        }

        // Recursively check children
        for child in &elem.children {
            if let XmlNode::Element(child_elem) = child {
                if self.element_matches(child_elem, search) {
                    return true;
                }
            } else if let XmlNode::Text(t) = child
                && search.matches(t)
            {
                return true;
            }
        }

        false
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
}

impl SearchContext<'_> {
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

/// Convert tree depth to left-padding pixels.
#[inline]
fn indent_px(depth: usize) -> f32 {
    5.0 + depth as f32 * 16.0
}
