use egui::{Color32, FontId, RichText, Ui};

use crate::xml::{truncate_str, XmlElement, XmlNode};

// ---------------------------------------------------------------------------
// SelectedNodeInfo
// ---------------------------------------------------------------------------

/// Snapshot of a selected element's data for display in the details panel.
#[derive(Debug, Clone)]
pub struct SelectedNodeInfo {
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
}

impl Default for XmlTreeView {
    fn default() -> Self {
        Self::new()
    }
}

impl XmlTreeView {
    pub fn new() -> Self {
        Self {
            selected_id: None,
            selected_info: None,
        }
    }

    /// Render the tree starting from `root` into `ui`.
    pub fn show(&mut self, ui: &mut Ui, root: &XmlNode) {
        self.show_node(ui, root, 0);
    }

    /// Return a reference to the currently selected node info, if any.
    pub fn get_selected_info(&self) -> Option<&SelectedNodeInfo> {
        self.selected_info.as_ref()
    }

    /// Clear the current selection.
    pub fn clear_selection(&mut self) {
        self.selected_id = None;
        self.selected_info = None;
    }

    // -----------------------------------------------------------------------
    // Private
    // -----------------------------------------------------------------------

    fn show_node(&mut self, ui: &mut Ui, node: &XmlNode, depth: usize) {
        match node {
            XmlNode::Element(elem) => self.show_element(ui, elem, depth),

            XmlNode::Text(text) => {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(indent_px(depth));
                        ui.label(
                            RichText::new(format!("\"{}\"", truncate_str(trimmed, 60)))
                                .font(FontId::monospace(11.0))
                                .color(Color32::from_rgb(150, 150, 150)),
                        );
                    });
                }
            }

            XmlNode::Comment(comment) => {
                ui.horizontal(|ui| {
                    ui.add_space(indent_px(depth));
                    ui.label(
                        RichText::new(format!("<!-- {} -->", truncate_str(comment, 50)))
                            .font(FontId::monospace(11.0))
                            .color(Color32::from_rgb(108, 135, 108)),
                    );
                });
            }
        }
    }

    fn show_element(&mut self, ui: &mut Ui, elem: &XmlElement, depth: usize) {
        let id = elem.id.0;
        let is_selected = self.selected_id == Some(id);
        let label = Self::format_label(elem);

        if elem.has_children() {
            // Collapsible header for elements with children.
            let header = egui::CollapsingHeader::new(&label)
                .default_open(depth < 3)
                .show(ui, |ui| {
                    for child in &elem.children {
                        self.show_node(ui, child, depth + 1);
                    }
                });

            if header.header_response.clicked() {
                self.select_element(elem);
            }

            // Highlight the header when selected.
            if is_selected {
                ui.painter().rect_filled(
                    header.header_response.rect,
                    2.0,
                    Color32::from_rgba_premultiplied(100, 120, 200, 40),
                );
            }
        } else {
            // Leaf element — plain selectable row.
            ui.horizontal(|ui| {
                ui.add_space(indent_px(depth) + 20.0);
                if ui.selectable_label(is_selected, &label).clicked() {
                    self.select_element(elem);
                }
            });
        }
    }

    fn select_element(&mut self, elem: &XmlElement) {
        self.selected_id = Some(elem.id.0);
        self.selected_info = Some(SelectedNodeInfo {
            name: elem.name.clone(),
            attributes: elem
                .attributes
                .iter()
                .map(|a| (a.name.clone(), a.value.clone()))
                .collect(),
            text: elem.text.clone(),
            child_count: elem.children.len(),
        });
    }

    /// Build a concise label string for an element node.
    fn format_label(elem: &XmlElement) -> String {
        let mut label = format!("<{}", elem.name);

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
}

/// Convert tree depth to left-padding pixels.
#[inline]
fn indent_px(depth: usize) -> f32 {
    5.0 + depth as f32 * 16.0
}
