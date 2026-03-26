use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

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
    version: u64,  // Document version for cache invalidation
}

impl XmlDocument {
    pub fn new(root: XmlNode) -> Self {
        Self { 
            root: Arc::new(root),
            version: 0,
        }
    }
    
    pub fn version(&self) -> u64 {
        self.version
    }
    
    pub fn increment_version(&mut self) {
        self.version += 1;
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
