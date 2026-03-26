use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// NodeId
// ---------------------------------------------------------------------------

static NODE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Opaque, globally-unique identifier for an [`XmlElement`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u64);

impl NodeId {
    pub fn new() -> Self {
        Self(NODE_COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for NodeId {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// XmlAttribute
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XmlAttribute {
    pub name: String,
    pub value: String,
    pub namespace: Option<String>,
}

// ---------------------------------------------------------------------------
// XmlElement
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XmlElement {
    pub id: NodeId,
    pub name: String,
    pub namespace: Option<String>,
    pub attributes: Vec<XmlAttribute>,
    pub children: Vec<XmlNode>,
    pub text: Option<String>,
}

#[allow(dead_code)]
impl XmlElement {
    pub fn new(name: String) -> Self {
        Self {
            id: NodeId::new(),
            name,
            namespace: None,
            attributes: Vec::new(),
            children: Vec::new(),
            text: None,
        }
    }

    pub fn add_child(&mut self, child: XmlNode) {
        self.children.push(child);
    }

    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    /// Returns `"namespace:name"` when a namespace prefix is present.
    pub fn display_name(&self) -> &str {
        &self.name
    }

    /// Returns a summary string of all attributes for display purposes.
    pub fn attributes_summary(&self) -> String {
        self.attributes
            .iter()
            .map(|a| format!("{}=\"{}\"", a.name, a.value))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Returns up to `limit` attributes formatted as `key="value"` pairs.
    pub fn attributes_preview(&self, limit: usize) -> String {
        let shown: Vec<String> = self
            .attributes
            .iter()
            .take(limit)
            .map(|a| format!("{}=\"{}\"", a.name, truncate_str(&a.value, 20)))
            .collect();
        let mut s = shown.join(" ");
        if self.attributes.len() > limit {
            s.push_str(&format!(" … +{}", self.attributes.len() - limit));
        }
        s
    }

    /// Total number of direct child *element* nodes.
    pub fn element_child_count(&self) -> usize {
        self.children
            .iter()
            .filter(|n| matches!(n, XmlNode::Element(_)))
            .count()
    }
}

// ---------------------------------------------------------------------------
// XmlNode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum XmlNode {
    Element(XmlElement),
    Text(String),
    Comment(String),
}

#[allow(dead_code)]
impl XmlNode {
    pub fn as_element(&self) -> Option<&XmlElement> {
        match self {
            XmlNode::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn as_element_mut(&mut self) -> Option<&mut XmlElement> {
        match self {
            XmlNode::Element(e) => Some(e),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// XmlDocument
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct XmlDocument {
    pub root: Arc<XmlNode>,
    version: u64, // Document version for cache invalidation
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementPosition {
    pub parent_id: Option<u64>,
    pub element_index: usize,
    pub element_count: usize,
    pub reorderable: bool,
}

impl ElementPosition {
    pub fn can_move_up(self) -> bool {
        self.reorderable && self.element_index > 0
    }

    pub fn can_move_down(self) -> bool {
        self.reorderable && self.element_index + 1 < self.element_count
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MoveDirection {
    Up,
    Down,
}

impl XmlDocument {
    pub fn new(root: XmlNode) -> Self {
        Self {
            root: Arc::new(root),
            version: 0,
        }
    }

    pub fn find_element(&self, id: u64) -> Option<&XmlElement> {
        find_element_by_id(self.root.as_ref(), id)
    }

    pub fn find_element_mut(&mut self, id: u64) -> Option<&mut XmlElement> {
        let root = Arc::make_mut(&mut self.root);
        find_element_by_id_mut(root, id)
    }

    pub fn rename_element(&mut self, id: u64, new_name: String) -> Result<bool> {
        let new_name = new_name.trim().to_string();
        validate_xml_name(&new_name)?;

        let Some(element) = self.find_element_mut(id) else {
            return Ok(false);
        };

        if element.name == new_name {
            return Ok(false);
        }

        element.name = new_name;
        self.increment_version();
        Ok(true)
    }

    pub fn set_attribute_value(
        &mut self,
        id: u64,
        attribute_name: &str,
        new_value: String,
    ) -> bool {
        let Some(element) = self.find_element_mut(id) else {
            return false;
        };

        let Some(attribute) = element
            .attributes
            .iter_mut()
            .find(|attr| attr.name == attribute_name)
        else {
            return false;
        };

        if attribute.value == new_value {
            return false;
        }

        attribute.value = new_value;
        self.increment_version();
        true
    }

    pub fn add_attribute(
        &mut self,
        id: u64,
        attribute_name: String,
        value: String,
    ) -> Result<bool> {
        let attribute_name = attribute_name.trim().to_string();
        validate_xml_name(&attribute_name)?;

        let Some(element) = self.find_element_mut(id) else {
            return Ok(false);
        };

        if element
            .attributes
            .iter()
            .any(|attribute| attribute.name == attribute_name)
        {
            bail!("Attribute '{attribute_name}' already exists on this element");
        }

        element.attributes.push(XmlAttribute {
            name: attribute_name,
            value,
            namespace: None,
        });
        self.increment_version();
        Ok(true)
    }

    pub fn remove_attribute(&mut self, id: u64, attribute_name: &str) -> bool {
        let Some(element) = self.find_element_mut(id) else {
            return false;
        };

        let previous_len = element.attributes.len();
        element
            .attributes
            .retain(|attribute| attribute.name != attribute_name);
        if element.attributes.len() == previous_len {
            return false;
        }

        self.increment_version();
        true
    }

    pub fn set_text_content(&mut self, id: u64, new_text: Option<String>) -> Result<bool> {
        let Some(element) = self.find_element_mut(id) else {
            return Ok(false);
        };

        if !element.children.is_empty() {
            bail!("Text editing is only supported for simple leaf elements");
        }

        let new_text = new_text.filter(|text| !text.is_empty());
        if element.text == new_text {
            return Ok(false);
        }

        element.text = new_text;
        self.increment_version();
        Ok(true)
    }

    pub fn element_position(&self, id: u64) -> Option<ElementPosition> {
        let root = self.root.as_element()?;
        if root.id.0 == id {
            return Some(ElementPosition {
                parent_id: None,
                element_index: 0,
                element_count: 1,
                reorderable: false,
            });
        }

        find_element_position(self.root.as_ref(), id)
    }

    pub fn append_child_element(
        &mut self,
        parent_id: u64,
        child_name: String,
    ) -> Result<Option<u64>> {
        let child_name = child_name.trim().to_string();
        validate_xml_name(&child_name)?;

        let Some(parent) = self.find_element_mut(parent_id) else {
            return Ok(None);
        };

        promote_inline_text_to_children(parent);

        let child = XmlElement::new(child_name);
        let child_id = child.id.0;
        parent.children.push(XmlNode::Element(child));
        self.increment_version();
        Ok(Some(child_id))
    }

    pub fn insert_sibling_element_after(
        &mut self,
        id: u64,
        sibling_name: String,
    ) -> Result<Option<u64>> {
        if self.root.as_element().is_some_and(|root| root.id.0 == id) {
            bail!("Cannot add a sibling to the document root element");
        }

        let sibling_name = sibling_name.trim().to_string();
        validate_xml_name(&sibling_name)?;

        let root = Arc::make_mut(&mut self.root);
        let inserted_id = insert_sibling_element_after_by_id(root, id, &sibling_name);
        if inserted_id.is_some() {
            self.increment_version();
        }

        Ok(inserted_id)
    }

    pub fn remove_element(&mut self, id: u64) -> Result<Option<u64>> {
        if self.root.as_element().is_some_and(|root| root.id.0 == id) {
            bail!("Cannot remove the document root element");
        }

        let root = Arc::make_mut(&mut self.root);
        let removed_parent_id = remove_element_by_id(root, id);
        if removed_parent_id.is_some() {
            self.increment_version();
        }
        Ok(removed_parent_id)
    }

    pub fn move_element_up(&mut self, id: u64) -> Result<bool> {
        self.move_element(id, MoveDirection::Up)
    }

    pub fn move_element_down(&mut self, id: u64) -> Result<bool> {
        self.move_element(id, MoveDirection::Down)
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn increment_version(&mut self) {
        self.version += 1;
    }

    fn move_element(&mut self, id: u64, direction: MoveDirection) -> Result<bool> {
        if self.root.as_element().is_some_and(|root| root.id.0 == id) {
            bail!("Cannot move the document root element");
        }

        let root = Arc::make_mut(&mut self.root);
        let moved = move_element_by_id(root, id, direction)?;
        if moved {
            self.increment_version();
        }

        Ok(moved)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Truncate a string to at most `max_chars` *Unicode scalar values*.
/// Appends `"…"` when truncated. This is safe for multi-byte UTF-8 strings.
pub fn truncate_str(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let prefix: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

fn find_element_by_id(node: &XmlNode, id: u64) -> Option<&XmlElement> {
    let element = node.as_element()?;
    if element.id.0 == id {
        return Some(element);
    }

    for child in &element.children {
        if let Some(found) = find_element_by_id(child, id) {
            return Some(found);
        }
    }

    None
}

fn find_element_by_id_mut(node: &mut XmlNode, id: u64) -> Option<&mut XmlElement> {
    let element = node.as_element_mut()?;
    if element.id.0 == id {
        return Some(element);
    }

    for child in &mut element.children {
        if let Some(found) = find_element_by_id_mut(child, id) {
            return Some(found);
        }
    }

    None
}

fn find_element_position(node: &XmlNode, id: u64) -> Option<ElementPosition> {
    let element = node.as_element()?;
    let element_children = direct_element_child_positions(element);

    if let Some(element_index) = element_children
        .iter()
        .position(|(_, child_id)| *child_id == id)
    {
        return Some(ElementPosition {
            parent_id: Some(element.id.0),
            element_index,
            element_count: element_children.len(),
            reorderable: has_only_element_children(element),
        });
    }

    for child in &element.children {
        if let Some(found) = find_element_position(child, id) {
            return Some(found);
        }
    }

    None
}

fn validate_xml_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        bail!("XML name cannot be empty");
    };

    if first.is_ascii_digit() || matches!(first, '-' | '.') || is_invalid_name_char(first) {
        bail!("XML name '{name}' starts with an unsupported character");
    }

    if chars.any(is_invalid_name_char) {
        bail!("XML name '{name}' contains unsupported characters");
    }

    Ok(())
}

fn is_invalid_name_char(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '<' | '>' | '&' | '"' | '\'' | '/' | '=')
}

fn direct_element_child_positions(element: &XmlElement) -> Vec<(usize, u64)> {
    element
        .children
        .iter()
        .enumerate()
        .filter_map(|(index, child)| {
            child
                .as_element()
                .map(|child_elem| (index, child_elem.id.0))
        })
        .collect()
}

fn has_only_element_children(element: &XmlElement) -> bool {
    element
        .children
        .iter()
        .all(|child| matches!(child, XmlNode::Element(_)))
}

fn promote_inline_text_to_children(element: &mut XmlElement) {
    if let Some(text) = element.text.take()
        && !text.is_empty()
    {
        element.children.insert(0, XmlNode::Text(text));
    }
}

fn normalize_element_content(element: &mut XmlElement) {
    if element.children.len() == 1
        && let Some(XmlNode::Text(text)) = element.children.first()
    {
        element.text = (!text.is_empty()).then_some(text.clone());
        element.children.clear();
        return;
    }

    if element.text.as_deref() == Some("") {
        element.text = None;
    }
}

fn remove_element_by_id(node: &mut XmlNode, id: u64) -> Option<u64> {
    let element = node.as_element_mut()?;

    if let Some(index) = element
        .children
        .iter()
        .position(|child| matches!(child, XmlNode::Element(child_elem) if child_elem.id.0 == id))
    {
        element.children.remove(index);
        normalize_element_content(element);
        return Some(element.id.0);
    }

    for child in &mut element.children {
        if let Some(parent_id) = remove_element_by_id(child, id) {
            return Some(parent_id);
        }
    }

    None
}

fn insert_sibling_element_after_by_id(
    node: &mut XmlNode,
    id: u64,
    sibling_name: &str,
) -> Option<u64> {
    let element = node.as_element_mut()?;

    if let Some(index) = element
        .children
        .iter()
        .position(|child| matches!(child, XmlNode::Element(child_elem) if child_elem.id.0 == id))
    {
        let sibling = XmlElement::new(sibling_name.to_string());
        let sibling_id = sibling.id.0;
        element
            .children
            .insert(index + 1, XmlNode::Element(sibling));
        return Some(sibling_id);
    }

    for child in &mut element.children {
        if let Some(inserted_id) = insert_sibling_element_after_by_id(child, id, sibling_name) {
            return Some(inserted_id);
        }
    }

    None
}

fn move_element_by_id(node: &mut XmlNode, id: u64, direction: MoveDirection) -> Result<bool> {
    let Some(element) = node.as_element_mut() else {
        return Ok(false);
    };

    let element_children = direct_element_child_positions(element);
    if let Some(element_index) = element_children
        .iter()
        .position(|(_, child_id)| *child_id == id)
    {
        if !has_only_element_children(element) {
            bail!("Reordering is only supported when the parent contains only element children");
        }

        let swap_with = match direction {
            MoveDirection::Up if element_index > 0 => Some(element_index - 1),
            MoveDirection::Down if element_index + 1 < element_children.len() => {
                Some(element_index + 1)
            }
            _ => None,
        };

        let Some(target_index) = swap_with else {
            return Ok(false);
        };

        element.children.swap(
            element_children[element_index].0,
            element_children[target_index].0,
        );
        return Ok(true);
    }

    for child in &mut element.children {
        if move_element_by_id(child, id, direction)? {
            return Ok(true);
        }
    }

    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::ElementPosition;
    use crate::xml::{XmlNode, parse_xml};

    #[test]
    fn rename_element_updates_tree_and_version() {
        let mut doc = parse_xml(r#"<root><child>text</child></root>"#).expect("parse");
        let child_id = doc
            .root
            .as_element()
            .and_then(|root| root.children[0].as_element())
            .expect("child element")
            .id
            .0;

        let before = doc.version();
        assert!(
            doc.rename_element(child_id, "renamed-child".to_string())
                .unwrap()
        );
        assert_eq!(doc.version(), before + 1);
        assert_eq!(
            doc.find_element(child_id)
                .map(|element| element.name.as_str()),
            Some("renamed-child")
        );
    }

    #[test]
    fn set_attribute_value_updates_existing_attribute() {
        let mut doc = parse_xml(r#"<root><child status="old"/></root>"#).expect("parse");
        let child_id = doc
            .root
            .as_element()
            .and_then(|root| root.children[0].as_element())
            .expect("child element")
            .id
            .0;

        assert!(doc.set_attribute_value(child_id, "status", "new".to_string()));
        let child = doc.find_element(child_id).expect("updated child");
        assert_eq!(child.attributes[0].value, "new");
    }

    #[test]
    fn add_attribute_rejects_duplicates() {
        let mut doc = parse_xml(r#"<root><child status="old"/></root>"#).expect("parse");
        let child_id = doc
            .root
            .as_element()
            .and_then(|root| root.children[0].as_element())
            .expect("child element")
            .id
            .0;

        let err = doc
            .add_attribute(child_id, "status".to_string(), "new".to_string())
            .expect_err("duplicate attributes should fail");
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn append_child_promotes_inline_text_to_mixed_content() {
        let mut doc = parse_xml(r#"<root><parent>hello</parent></root>"#).expect("parse");
        let parent_id = doc
            .root
            .as_element()
            .and_then(|root| root.children[0].as_element())
            .expect("parent element")
            .id
            .0;

        let child_id = doc
            .append_child_element(parent_id, "child".to_string())
            .expect("append child")
            .expect("parent exists");
        let parent = doc.find_element(parent_id).expect("updated parent");

        assert!(parent.text.is_none());
        assert!(matches!(&parent.children[0], XmlNode::Text(text) if text == "hello"));
        assert_eq!(
            parent.children[1].as_element().map(|child| child.id.0),
            Some(child_id)
        );
    }

    #[test]
    fn remove_element_normalizes_back_to_leaf_text() {
        let mut doc =
            parse_xml(r#"<root><parent>hello<child>value</child></parent></root>"#).expect("parse");
        let parent_id = doc
            .root
            .as_element()
            .and_then(|root| root.children[0].as_element())
            .expect("parent element")
            .id
            .0;
        let child_id = doc
            .find_element(parent_id)
            .and_then(|parent| parent.children[1].as_element())
            .expect("child element")
            .id
            .0;

        assert_eq!(
            doc.remove_element(child_id).expect("remove child"),
            Some(parent_id)
        );

        let parent = doc.find_element(parent_id).expect("remaining parent");
        assert_eq!(parent.text.as_deref(), Some("hello"));
        assert!(parent.children.is_empty());
    }

    #[test]
    fn set_text_content_rejects_mixed_content_nodes() {
        let mut doc = parse_xml(r#"<root>Hello <child>world</child></root>"#).expect("parse");
        let root_id = doc.root.as_element().expect("root element").id.0;

        let err = doc
            .set_text_content(root_id, Some("override".to_string()))
            .expect_err("mixed-content edit should fail");
        assert!(err.to_string().contains("simple leaf elements"));
    }

    #[test]
    fn rename_element_rejects_invalid_names() {
        let mut doc = parse_xml(r#"<root><child/></root>"#).expect("parse");
        let child_id = doc
            .root
            .as_element()
            .and_then(|root| root.children[0].as_element())
            .expect("child element")
            .id
            .0;

        let err = doc
            .rename_element(child_id, "bad name".to_string())
            .expect_err("invalid name should fail");
        assert!(err.to_string().contains("unsupported"));
    }

    #[test]
    fn remove_root_element_is_rejected() {
        let mut doc = parse_xml(r#"<root><child/></root>"#).expect("parse");
        let root_id = doc.root.as_element().expect("root element").id.0;

        let err = doc
            .remove_element(root_id)
            .expect_err("root removal should fail");
        assert!(err.to_string().contains("document root"));
    }

    #[test]
    fn insert_sibling_and_move_element_updates_order() {
        let mut doc = parse_xml(r#"<root><alpha/><beta/></root>"#).expect("parse");
        let root_id = doc.root.as_element().expect("root element").id.0;
        let alpha_id = doc
            .root
            .as_element()
            .and_then(|root| root.children[0].as_element())
            .expect("alpha element")
            .id
            .0;
        let beta_id = doc
            .root
            .as_element()
            .and_then(|root| root.children[1].as_element())
            .expect("beta element")
            .id
            .0;

        let inserted_id = doc
            .insert_sibling_element_after(alpha_id, "gamma".to_string())
            .expect("insert sibling")
            .expect("inserted sibling");
        assert_eq!(
            doc.element_position(inserted_id)
                .expect("inserted position"),
            ElementPosition {
                parent_id: Some(root_id),
                element_index: 1,
                element_count: 3,
                reorderable: true,
            }
        );

        assert!(doc.move_element_up(beta_id).expect("move beta"));
        let root = doc.root.as_element().expect("updated root");
        let child_names: Vec<&str> = root
            .children
            .iter()
            .map(|child| child.as_element().expect("element child").name.as_str())
            .collect();
        assert_eq!(child_names, vec!["alpha", "beta", "gamma"]);

        assert!(doc.move_element_down(beta_id).expect("move beta back"));
        let root = doc.root.as_element().expect("reordered root");
        let child_names: Vec<&str> = root
            .children
            .iter()
            .map(|child| child.as_element().expect("element child").name.as_str())
            .collect();
        assert_eq!(child_names, vec!["alpha", "gamma", "beta"]);
    }

    #[test]
    fn move_element_rejects_mixed_content_parents() {
        let mut doc = parse_xml(r#"<root>Hello <first/><second/></root>"#).expect("parse");
        let first_id = doc
            .root
            .as_element()
            .and_then(|root| root.children[1].as_element())
            .expect("first element")
            .id
            .0;

        let err = doc
            .move_element_down(first_id)
            .expect_err("mixed-content reordering should fail");
        assert!(err.to_string().contains("only element children"));
    }
}
