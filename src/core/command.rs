//! Atomic, reversible editing commands.
//!
//! Every mutation of an [`XmlDocument`] goes through [`Command::apply`]:
//! the command validates completely first; any failure returns a
//! [`CommandError`] with the document byte-identical, revision untouched.
//! On success the document advances one revision, the caller receives the
//! exact reverse command and a [`ChangedSet`] naming what to invalidate.
//!
//! Source updates are minimal: content and attribute edits re-render only
//! the affected node's byte range; structural edits splice the smallest
//! region that changes. Only `FormatDocument` (and `ReplaceWholeSource`,
//! which the source editor drives) may rewrite the whole document.

use uppsala::dom::{Document as EngineDom, NodeKind, QName as EngineQName};

use super::document::{NodeId, QNameSpec, XmlDocument, XmlNodeKind, is_ncname, validate_qname};
use crate::xml::XmlError;

/// Maximum element nesting, matching the engine's parse limit.
const MAX_DEPTH: usize = 512;

/// Why a command was rejected before committing anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError {
    /// Stable, snake_case rejection reason.
    pub code: &'static str,
    /// Human-readable detail.
    pub message: String,
    /// Node the rejection is about, when applicable.
    pub node: Option<NodeId>,
}

impl CommandError {
    fn new(code: &'static str, message: impl Into<String>, node: Option<NodeId>) -> Self {
        CommandError {
            code,
            message: message.into(),
            node,
        }
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for CommandError {}

/// What a committed command changed, for cache invalidation downstream.
#[derive(Debug, Clone, Default)]
pub struct ChangedSet {
    /// Tree shape changed (nodes inserted/removed/moved).
    pub structure_changed: bool,
    /// The entire source was replaced (whole-document undo granularity).
    pub whole_source: bool,
    /// Nodes whose name, attributes, or content changed.
    pub touched: Vec<NodeId>,
    /// Nodes removed from the tree (still in the arena for undo).
    pub removed: Vec<NodeId>,
}

/// Where an inserted node goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertPosition {
    /// Before all current children.
    First,
    /// After all current children.
    Last,
    /// Directly before this sibling.
    Before(NodeId),
    /// Directly after this sibling.
    After(NodeId),
}

/// Initial content for a newly created node. Nodes are created shallow;
/// duplicate/paste flows build deeper structures another way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewNode {
    Element {
        name: String,
    },
    Text {
        text: String,
    },
    CData {
        text: String,
    },
    Comment {
        text: String,
    },
    ProcessingInstruction {
        target: String,
        data: Option<String>,
    },
}

/// Replacement content for `SetNodeContent`. The variant must match the
/// node being edited (text into Text nodes, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeContent {
    Text(String),
    CData(String),
    Comment(String),
    ProcessingInstruction {
        target: String,
        data: Option<String>,
    },
}

/// One replacement inside a [`Command::BatchReplace`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceOp {
    /// Node whose content changes.
    pub node: NodeId,
    /// Which field changes.
    pub target: ReplaceTarget,
    /// Value before the change (for the reverse op).
    pub old_value: String,
    /// Value after the change.
    pub new_value: String,
}

/// Field targeted by a [`ReplaceOp`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplaceTarget {
    /// Text or CData node content.
    NodeText,
    /// Comment node content.
    Comment,
    /// Attribute value on the (element) node, by rendered name.
    Attribute(String),
}

/// Identifies the editable field a command touches; used for history
/// coalescing decisions only.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum CoalesceKey {
    Attribute(NodeId, String),
    Content(NodeId),
}

/// The complete editing vocabulary. The `RestoreNode` variant is an internal
/// undo primitive (re-attaches arena nodes with exact original bytes) and is
/// not meant to be constructed by UI code.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Rename an element (and optionally rebind its namespace).
    RenameElement { node: NodeId, new_name: QNameSpec },
    /// Add a new attribute to an element.
    AddAttribute {
        element: NodeId,
        name: String,
        value: String,
    },
    /// Rename an attribute, keeping its value.
    RenameAttribute {
        element: NodeId,
        old_name: String,
        new_name: String,
    },
    /// Overwrite an attribute's value.
    SetAttributeValue {
        element: NodeId,
        name: String,
        value: String,
    },
    /// Remove an attribute.
    RemoveAttribute { element: NodeId, name: String },
    /// Replace the content of a Text/CData/Comment/PI node.
    SetNodeContent { node: NodeId, content: NodeContent },
    /// Create and attach a new node.
    InsertNode {
        parent: NodeId,
        position: InsertPosition,
        node: NewNode,
    },
    /// Detach a node (kept in the arena; undo re-attaches it).
    DeleteNode { node: NodeId },
    /// Move a node to a new parent.
    MoveNode {
        node: NodeId,
        new_parent: NodeId,
        position: InsertPosition,
    },
    /// Insert a copy of a subtree directly after the original.
    DuplicateSubtree { node: NodeId },
    /// Swap the entire source text (source-editor Apply, reload).
    ReplaceWholeSource { new_source: String },
    /// Apply many validated replacements as one undoable step.
    BatchReplace { ops: Vec<ReplaceOp> },
    /// Re-render the whole document with fixed indentation.
    FormatDocument { indent: String },
    /// Internal: re-attach previously deleted arena nodes, splicing back
    /// their exact original bytes.
    #[allow(clippy::enum_variant_names)]
    RestoreNode {
        node: NodeId,
        parent: NodeId,
        position: InsertPosition,
        offset: usize,
        bytes: String,
    },
}

impl Command {
    /// Validates and applies the command to `document`.
    ///
    /// On success: one revision advanced, reverse command and change set
    /// returned. On failure: document bytes, ranges, indexes, and revision
    /// are exactly as before.
    pub fn apply(&self, document: &mut XmlDocument) -> Result<(Command, ChangedSet), CommandError> {
        match self {
            Command::RenameElement { node, new_name } => {
                apply_rename_element(document, *node, new_name)
            }
            Command::AddAttribute {
                element,
                name,
                value,
            } => apply_add_attribute(document, *element, name, value),
            Command::RenameAttribute {
                element,
                old_name,
                new_name,
            } => apply_rename_attribute(document, *element, old_name, new_name),
            Command::SetAttributeValue {
                element,
                name,
                value,
            } => apply_set_attribute_value(document, *element, name, value),
            Command::RemoveAttribute { element, name } => {
                apply_remove_attribute(document, *element, name)
            }
            Command::SetNodeContent { node, content } => {
                apply_set_node_content(document, *node, content)
            }
            Command::InsertNode {
                parent,
                position,
                node,
            } => apply_insert_node(document, *parent, position, node),
            Command::DeleteNode { node } => apply_delete_node(document, *node),
            Command::MoveNode {
                node,
                new_parent,
                position,
            } => apply_move_node(document, *node, *new_parent, position),
            Command::DuplicateSubtree { node } => apply_duplicate_subtree(document, *node),
            Command::ReplaceWholeSource { new_source } => {
                apply_replace_whole_source(document, new_source)
            }
            Command::BatchReplace { ops } => apply_batch_replace(document, ops),
            Command::FormatDocument { indent } => apply_format_document(document, indent),
            Command::RestoreNode {
                node,
                parent,
                position,
                offset,
                bytes,
            } => apply_restore_node(document, *node, *parent, position, *offset, bytes),
        }
    }

    /// Key deciding whether consecutive commands merge into one history
    /// entry: same node and same editing field within the coalescing window.
    pub(crate) fn coalesce_key(&self) -> Option<CoalesceKey> {
        match self {
            Command::SetAttributeValue { element, name, .. } => {
                Some(CoalesceKey::Attribute(*element, name.clone()))
            }
            Command::SetNodeContent { node, .. } => Some(CoalesceKey::Content(*node)),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Shared validation helpers
// ---------------------------------------------------------------------------

fn require_element(document: &XmlDocument, node: NodeId) -> Result<(), CommandError> {
    match document.kind(node) {
        Some(XmlNodeKind::Element) if document.is_attached(node) => Ok(()),
        _ => Err(CommandError::new(
            "not_an_element",
            format!("node {node:?} is not an attached element"),
            Some(node),
        )),
    }
}

fn require_attached(document: &XmlDocument, node: NodeId) -> Result<(), CommandError> {
    if document.is_attached(node) && document.kind(node).is_some() {
        Ok(())
    } else {
        Err(CommandError::new(
            "unknown_node",
            format!("node {node:?} does not exist or is detached"),
            Some(node),
        ))
    }
}

/// Height of the subtree rooted at `node` (a leaf is 1).
fn subtree_height(document: &XmlDocument, node: NodeId) -> usize {
    let mut height = 1;
    for child in document.children(node) {
        height = height.max(1 + subtree_height(document, child));
    }
    height
}

/// Depth of `node`, counting the document node as level 0.
fn node_depth(document: &XmlDocument, node: NodeId) -> usize {
    let mut depth = 0;
    let mut current = Some(node);
    while let Some(id) = current {
        depth += 1;
        current = document.parent(id);
    }
    depth
}

fn is_descendant(document: &XmlDocument, ancestor: NodeId, candidate: NodeId) -> bool {
    let mut current = Some(candidate);
    while let Some(id) = current {
        if id == ancestor {
            return true;
        }
        current = document.parent(id);
    }
    false
}

/// Sibling the position refers to, validated to be a child of `parent`.
fn position_sibling(
    document: &XmlDocument,
    parent: NodeId,
    position: &InsertPosition,
) -> Result<Option<NodeId>, CommandError> {
    match position {
        InsertPosition::First | InsertPosition::Last => Ok(None),
        InsertPosition::Before(sibling) | InsertPosition::After(sibling) => {
            require_attached(document, *sibling)?;
            if document.parent(*sibling) != Some(parent) {
                return Err(CommandError::new(
                    "position_not_child_of_parent",
                    "insertion sibling is not a child of the target parent",
                    Some(*sibling),
                ));
            }
            Ok(Some(*sibling))
        }
    }
}

fn validate_new_node(node: &NewNode) -> Result<(), CommandError> {
    match node {
        NewNode::Element { name } => {
            validate_qname(name)
                .map_err(|message| CommandError::new("invalid_name", message, None))?;
        }
        NewNode::Text { text } => {
            if text.contains('<') {
                return Err(CommandError::new(
                    "invalid_text",
                    "text content must not contain '<' (use a CDATA node)",
                    None,
                ));
            }
        }
        NewNode::CData { text } => {
            if text.contains("]]>") {
                return Err(CommandError::new(
                    "invalid_cdata",
                    "CDATA content must not contain ']]>'",
                    None,
                ));
            }
        }
        NewNode::Comment { text } => {
            if text.contains("--") || text.ends_with('-') {
                return Err(CommandError::new(
                    "invalid_comment",
                    "comments must not contain '--' or end with '-'",
                    None,
                ));
            }
        }
        NewNode::ProcessingInstruction { target, .. } => {
            if !is_ncname(target) {
                return Err(CommandError::new(
                    "invalid_pi_target",
                    format!("PI target '{target}' is not a valid name"),
                    None,
                ));
            }
        }
    }
    Ok(())
}

/// Attaches `child` per `position`; `sibling` is the validated relative when
/// the position names one.
fn attach(
    dom: &mut EngineDom<'_>,
    parent: NodeId,
    position: &InsertPosition,
    sibling: Option<NodeId>,
    child: NodeId,
) {
    let (parent_e, child_e) = (parent.to_engine(), child.to_engine());
    match (position, sibling) {
        (InsertPosition::Before(sib), _) => {
            dom.insert_before(parent_e, child_e, sib.to_engine());
        }
        (InsertPosition::After(sib), _) => {
            dom.insert_after(parent_e, child_e, sib.to_engine());
        }
        (InsertPosition::First, _) => {
            if let Some(first) = dom.children_iter(parent_e).next() {
                dom.insert_before(parent_e, child_e, first);
            } else {
                dom.append_child(parent_e, child_e);
            }
        }
        (InsertPosition::Last, _) => {
            dom.append_child(parent_e, child_e);
        }
    }
}

/// Position of `node` among its siblings, as a concrete InsertPosition for
/// restore/reverse commands.
fn position_among(document: &XmlDocument, node: NodeId) -> Option<InsertPosition> {
    let parent = document.parent(node)?;
    let siblings = document.children(parent);
    let index = siblings.iter().position(|&id| id == node)?;
    Some(if index == 0 {
        InsertPosition::First
    } else {
        InsertPosition::After(siblings[index - 1])
    })
}

/// Removes and returns the range entries of `node`'s subtree (including
/// `node`). Callers splicing the subtree's bytes to a new location re-seed
/// the entries afterwards; without this, the generic shift in
/// [`XmlDocument::splice_source`] would corrupt entries inside the spliced
/// region.
fn take_subtree_entries(document: &mut XmlDocument, node: NodeId) -> Vec<(NodeId, (usize, usize))> {
    let mut ids = Vec::new();
    let mut stack = vec![node];
    while let Some(id) = stack.pop() {
        ids.push(id);
        stack.extend(document.children(id));
    }
    let mut taken = Vec::new();
    for id in ids {
        if let Some(entry) = document.range_entry(id) {
            document.remove_range_entry(id);
            taken.push((id, entry));
        }
    }
    taken
}

// ---------------------------------------------------------------------------
// Element re-render machinery (attribute/name edits)
// ---------------------------------------------------------------------------

/// Re-renders one element after an attribute or name mutation: splices the
/// new serialization over the element's old range and remaps descendant
/// ranges.
fn rerender_element(document: &mut XmlDocument, element: NodeId) -> Result<(), CommandError> {
    let old_range = document.range_entry(element).ok_or_else(|| {
        CommandError::new("range_unknown", "element byte range unknown", Some(element))
    })?;
    let frag = document.serialize_node(element);
    document.drop_ranges_within(old_range.0..old_range.1);
    document.splice_source(old_range.0..old_range.1, &frag);
    document.set_range_entry(element, (old_range.0, old_range.0 + frag.len()));
    remap_descendants(document, element, &frag, old_range.0);
    Ok(())
}

/// Recomputes ranges for an element's descendants after a re-render by
/// re-parsing the fragment and matching nodes in document order by kind.
/// Nodes that cannot be matched lose their range (callers fall back).
fn remap_descendants(document: &mut XmlDocument, element: NodeId, frag: &str, frag_offset: usize) {
    let Ok(parsed) = XmlDocument::parse(frag.as_bytes()) else {
        return;
    };
    let Some(root) = parsed.root_element() else {
        return;
    };

    let mut old_walk = Vec::new();
    let mut stack: Vec<NodeId> = document.children(element).into_iter().rev().collect();
    while let Some(id) = stack.pop() {
        old_walk.push(id);
        stack.extend(document.children(id).into_iter().rev());
    }
    let mut new_walk = Vec::new();
    let mut stack: Vec<NodeId> = parsed.children(root).into_iter().rev().collect();
    while let Some(id) = stack.pop() {
        new_walk.push(id);
        stack.extend(parsed.children(id).into_iter().rev());
    }
    if old_walk.len() != new_walk.len() {
        return;
    }
    for (index, (&old_id, &new_id)) in old_walk.iter().zip(&new_walk).enumerate() {
        if document.kind(old_id) != parsed.kind(new_id) {
            // Structure diverged (should not happen for these commands);
            // keep the ranges dropped from here on.
            let _ = index;
            return;
        }
        if let Some(range) = parsed.dom().node_range(new_id.to_engine()) {
            document.set_range_entry(old_id, (frag_offset + range.start, frag_offset + range.end));
        } else {
            document.remove_range_entry(old_id);
        }
    }
}

// ---------------------------------------------------------------------------
// Commands: element and attributes
// ---------------------------------------------------------------------------

fn apply_rename_element(
    document: &mut XmlDocument,
    node: NodeId,
    new_name: &QNameSpec,
) -> Result<(Command, ChangedSet), CommandError> {
    require_element(document, node)?;
    let (new_prefix, new_local) = new_name
        .split_prefix()
        .map_err(|message| CommandError::new("invalid_name", message, Some(node)))?;

    let old_qname = document.qname(node).expect("validated element");

    // Resolve the target binding: an explicit URI wins; keeping the old
    // prefix keeps its URI; otherwise the prefix must resolve in scope.
    let new_uri = match (&new_name.namespace_uri, new_prefix) {
        (Some(uri), _) => Some(uri.clone()),
        (None, prefix) if prefix == old_qname.prefix() => {
            old_qname.namespace_uri().map(str::to_string)
        }
        (None, Some(prefix)) => resolve_prefix_in_scope(document, node, prefix).map_err(|_| {
            CommandError::new(
                "undeclared_prefix",
                format!(
                    "prefix '{prefix}' is not declared in scope; supply a namespace URI or declare it first"
                ),
                Some(node),
            )
        })?,
        (None, None) => None,
    };

    // The reverse always binds explicitly so undo cannot fail resolution.
    let reverse_uri = old_qname.namespace_uri().map(str::to_string);
    let old_spec = QNameSpec {
        name: old_qname.render(),
        namespace_uri: reverse_uri,
    };

    let engine_name = EngineQName {
        prefix: new_prefix.map(str::to_string).map(Into::into),
        local_name: new_local.to_string().into(),
        namespace_uri: new_uri.clone().map(Into::into),
    };

    {
        let engine = document.engine_doc_mut();
        let dom = engine.document_mut();
        let element = dom
            .element_mut(node.to_engine())
            .expect("validated element");
        element.name = engine_name;
        // An explicit (or scope-resolved rebinding of a *different*) prefix
        // is declared on the element itself so the output stays
        // namespace-well-formed even when no ancestor declares it.
        if let (Some(prefix), Some(uri)) = (new_prefix, new_uri.as_deref()) {
            let declared =
                element
                    .namespace_declarations
                    .iter()
                    .any(|(existing_prefix, existing_uri)| {
                        existing_prefix == prefix && existing_uri == uri
                    });
            if !declared {
                element
                    .namespace_declarations
                    .push((prefix.to_string().into(), uri.to_string().into()));
            }
        }
    }
    document.bump_revision();
    document.index_rename(node, &old_qname.render(), &new_name.name);
    rerender_element(document, node)?;
    let reverse = Command::RenameElement {
        node,
        new_name: old_spec,
    };
    Ok((reverse, ChangedSet::content(node)))
}

/// Resolves `prefix` for names on `node` by walking ancestors (including
/// `node` itself); `Err(())` when no declaration is in scope.
fn resolve_prefix_in_scope(
    document: &XmlDocument,
    node: NodeId,
    prefix: &str,
) -> Result<Option<String>, ()> {
    let mut current = Some(node);
    while let Some(id) = current {
        for (declared_prefix, uri) in document.namespace_declarations(id) {
            if declared_prefix == prefix {
                return Ok(Some(uri));
            }
        }
        current = document.parent(id);
    }
    Err(())
}

fn apply_add_attribute(
    document: &mut XmlDocument,
    element: NodeId,
    name: &str,
    value: &str,
) -> Result<(Command, ChangedSet), CommandError> {
    require_element(document, element)?;
    validate_qname(name)
        .map_err(|message| CommandError::new("invalid_name", message, Some(element)))?;
    if document
        .attributes(element)
        .iter()
        .any(|(existing, _)| existing == name)
    {
        return Err(CommandError::new(
            "duplicate_attribute",
            format!("attribute '{name}' already exists"),
            Some(element),
        ));
    }

    {
        let engine = document.engine_doc_mut();
        let dom = engine.document_mut();
        dom.element_mut(element.to_engine())
            .expect("validated")
            .set_attribute(
                EngineQName::local(name.to_string()),
                value.to_string().into(),
            );
    }
    document.bump_revision();
    rerender_element(document, element)?;
    let reverse = Command::RemoveAttribute {
        element,
        name: name.to_string(),
    };
    Ok((reverse, ChangedSet::content(element)))
}

fn apply_rename_attribute(
    document: &mut XmlDocument,
    element: NodeId,
    old_name: &str,
    new_name: &str,
) -> Result<(Command, ChangedSet), CommandError> {
    require_element(document, element)?;
    validate_qname(new_name)
        .map_err(|message| CommandError::new("invalid_name", message, Some(element)))?;
    let attrs = document.attributes(element);
    let value = attrs
        .iter()
        .find(|(existing, _)| existing == old_name)
        .map(|(_, value)| value.clone())
        .ok_or_else(|| {
            CommandError::new(
                "unknown_attribute",
                format!("attribute '{old_name}' does not exist"),
                Some(element),
            )
        })?;
    if old_name != new_name && attrs.iter().any(|(existing, _)| existing == new_name) {
        return Err(CommandError::new(
            "duplicate_attribute",
            format!("attribute '{new_name}' already exists"),
            Some(element),
        ));
    }

    {
        let engine = document.engine_doc_mut();
        let dom = engine.document_mut();
        let target = dom.element_mut(element.to_engine()).expect("validated");
        target.remove_attribute(old_name);
        target.set_attribute(EngineQName::local(new_name.to_string()), value.into());
    }
    document.bump_revision();
    rerender_element(document, element)?;
    let reverse = Command::RenameAttribute {
        element,
        old_name: new_name.to_string(),
        new_name: old_name.to_string(),
    };
    Ok((reverse, ChangedSet::content(element)))
}

fn apply_set_attribute_value(
    document: &mut XmlDocument,
    element: NodeId,
    name: &str,
    value: &str,
) -> Result<(Command, ChangedSet), CommandError> {
    require_element(document, element)?;
    let old_value = document
        .attributes(element)
        .into_iter()
        .find(|(existing, _)| existing == name)
        .map(|(_, value)| value)
        .ok_or_else(|| {
            CommandError::new(
                "unknown_attribute",
                format!("attribute '{name}' does not exist"),
                Some(element),
            )
        })?;

    {
        let engine = document.engine_doc_mut();
        let dom = engine.document_mut();
        dom.element_mut(element.to_engine())
            .expect("validated")
            .set_attribute(
                EngineQName::local(name.to_string()),
                value.to_string().into(),
            );
    }
    document.bump_revision();
    rerender_element(document, element)?;
    let reverse = Command::SetAttributeValue {
        element,
        name: name.to_string(),
        value: old_value,
    };
    Ok((reverse, ChangedSet::content(element)))
}

fn apply_remove_attribute(
    document: &mut XmlDocument,
    element: NodeId,
    name: &str,
) -> Result<(Command, ChangedSet), CommandError> {
    require_element(document, element)?;
    let old_value = document
        .attributes(element)
        .into_iter()
        .find(|(existing, _)| existing == name)
        .map(|(_, value)| value)
        .ok_or_else(|| {
            CommandError::new(
                "unknown_attribute",
                format!("attribute '{name}' does not exist"),
                Some(element),
            )
        })?;

    {
        let engine = document.engine_doc_mut();
        let dom = engine.document_mut();
        dom.element_mut(element.to_engine())
            .expect("validated")
            .remove_attribute(name);
    }
    document.bump_revision();
    rerender_element(document, element)?;
    let reverse = Command::AddAttribute {
        element,
        name: name.to_string(),
        value: old_value,
    };
    Ok((reverse, ChangedSet::content(element)))
}

impl ChangedSet {
    fn content(node: NodeId) -> ChangedSet {
        ChangedSet {
            touched: vec![node],
            ..ChangedSet::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Commands: node content
// ---------------------------------------------------------------------------

fn apply_set_node_content(
    document: &mut XmlDocument,
    node: NodeId,
    content: &NodeContent,
) -> Result<(Command, ChangedSet), CommandError> {
    require_attached(document, node)?;
    if document.root_element() == Some(node) {
        return Err(CommandError::new(
            "wrong_node_kind",
            "cannot set content on the root element; edit its children",
            Some(node),
        ));
    }
    let kind = document.kind(node).expect("attached");

    // Validate + capture the reverse content.
    let old_content = match (kind, content) {
        (XmlNodeKind::Text, NodeContent::Text(new_text)) => {
            if new_text.contains('<') {
                return Err(CommandError::new(
                    "invalid_text",
                    "text content must not contain '<'",
                    Some(node),
                ));
            }
            NodeContent::Text(document.node_text(node).expect("text node").to_string())
        }
        (XmlNodeKind::CData, NodeContent::CData(new_text)) => {
            if new_text.contains("]]>") {
                return Err(CommandError::new(
                    "invalid_cdata",
                    "CDATA content must not contain ']]>'",
                    Some(node),
                ));
            }
            NodeContent::CData(document.node_text(node).expect("cdata node").to_string())
        }
        (XmlNodeKind::Comment, NodeContent::Comment(new_text)) => {
            if new_text.contains("--") || new_text.ends_with('-') {
                return Err(CommandError::new(
                    "invalid_comment",
                    "comments must not contain '--' or end with '-'",
                    Some(node),
                ));
            }
            NodeContent::Comment(document.comment_text(node).expect("comment").to_string())
        }
        (XmlNodeKind::ProcessingInstruction, NodeContent::ProcessingInstruction { target, .. }) => {
            if !is_ncname(target) {
                return Err(CommandError::new(
                    "invalid_pi_target",
                    format!("PI target '{target}' is not a valid name"),
                    Some(node),
                ));
            }
            let (old_target, old_data) = document.pi(node).expect("pi node");
            NodeContent::ProcessingInstruction {
                target: old_target.to_string(),
                data: old_data.map(str::to_string),
            }
        }
        (kind, _) => {
            return Err(CommandError::new(
                "wrong_node_kind",
                format!("content variant does not match node kind {kind:?}"),
                Some(node),
            ));
        }
    };

    // Mutate the DOM.
    {
        let engine = document.engine_doc_mut();
        let dom = engine.document_mut();
        match content {
            NodeContent::Text(text) => {
                if let Some(NodeKind::Text(slot)) = dom.node_kind_mut(node.to_engine()) {
                    *slot = text.clone().into();
                }
            }
            NodeContent::CData(text) => {
                if let Some(NodeKind::CData(slot)) = dom.node_kind_mut(node.to_engine()) {
                    *slot = text.clone().into();
                }
            }
            NodeContent::Comment(text) => {
                if let Some(NodeKind::Comment(slot)) = dom.node_kind_mut(node.to_engine()) {
                    *slot = text.clone().into();
                }
            }
            NodeContent::ProcessingInstruction { target, data } => {
                if let Some(NodeKind::ProcessingInstruction(pi)) =
                    dom.node_kind_mut(node.to_engine())
                {
                    pi.target = target.clone().into();
                    pi.data = Some(data.clone().unwrap_or_default().into());
                }
            }
        }
    }
    document.bump_revision();

    // Splice: these nodes have no descendants; re-render in place.
    let range = document
        .range_entry(node)
        .ok_or_else(|| CommandError::new("range_unknown", "node byte range unknown", Some(node)))?;
    let frag = document.serialize_node(node);
    document.splice_source(range.0..range.1, &frag);
    document.set_range_entry(node, (range.0, range.0 + frag.len()));

    let reverse = Command::SetNodeContent {
        node,
        content: old_content,
    };
    Ok((reverse, ChangedSet::content(node)))
}

// ---------------------------------------------------------------------------
// Commands: structure
// ---------------------------------------------------------------------------

fn apply_insert_node(
    document: &mut XmlDocument,
    parent: NodeId,
    position: &InsertPosition,
    node: &NewNode,
) -> Result<(Command, ChangedSet), CommandError> {
    require_element(document, parent)?;
    let sibling = position_sibling(document, parent, position)?;
    validate_new_node(node)?;
    if node_depth(document, parent) + 1 > MAX_DEPTH {
        return Err(CommandError::new(
            "depth_limit",
            format!("insertion would nest deeper than {MAX_DEPTH} levels"),
            Some(parent),
        ));
    }
    let was_childless = document.children(parent).is_empty();
    let offset = insertion_offset(document, parent, position, sibling)?;

    let new_id = {
        let engine = document.engine_doc_mut();
        let dom = engine.document_mut();
        let id = match node {
            NewNode::Element { name } => {
                let (prefix, local) = validate_qname(name).expect("validated");
                dom.create_element(EngineQName {
                    prefix: prefix.map(str::to_string).map(Into::into),
                    local_name: local.to_string().into(),
                    namespace_uri: None,
                })
            }
            NewNode::Text { text } => dom.create_text(text.clone()),
            NewNode::CData { text } => dom.create_cdata(text.clone()),
            NewNode::Comment { text } => dom.create_comment(text.clone()),
            NewNode::ProcessingInstruction { target, data } => {
                dom.create_processing_instruction(target.clone(), data.clone().map(Into::into))
            }
        };
        attach(dom, parent, position, sibling, NodeId::from_engine(id));
        id
    };
    let new_id = NodeId::from_engine(new_id);
    document.bump_revision();
    document.invalidate_doc_order();

    let frag = document.serialize_node(new_id);
    if was_childless {
        // `<e/>` → `<e>…</e>`: re-render the parent wholesale.
        rerender_element(document, parent)?;
    } else {
        document.splice_source(offset..offset, &frag);
        document.set_range_entry(new_id, (offset, offset + frag.len()));
    }
    if let NewNode::Element { name } = node {
        document.index_add(new_id, name);
    }

    let reverse = Command::DeleteNode { node: new_id };
    Ok((
        reverse,
        ChangedSet {
            structure_changed: true,
            touched: vec![new_id],
            ..ChangedSet::default()
        },
    ))
}

/// Byte offset where a new child's bytes must go. Only called when the
/// parent already has children.
fn insertion_offset(
    document: &XmlDocument,
    parent: NodeId,
    position: &InsertPosition,
    sibling: Option<NodeId>,
) -> Result<usize, CommandError> {
    if let Some(sibling) = sibling {
        let (start, end) = document.range_entry(sibling).ok_or_else(|| {
            CommandError::new("range_unknown", "sibling byte range unknown", Some(sibling))
        })?;
        return Ok(match position {
            InsertPosition::After(_) => end,
            _ => start,
        });
    }
    let children = document.children(parent);
    if children.is_empty() {
        return Ok(0); // caller re-renders the parent; offset unused
    }
    let target = match position {
        InsertPosition::First => children[0],
        InsertPosition::Last => *children.last().expect("non-empty"),
        InsertPosition::Before(_) | InsertPosition::After(_) => unreachable!(),
    };
    let (start, end) = document.range_entry(target).ok_or_else(|| {
        CommandError::new("range_unknown", "node byte range unknown", Some(target))
    })?;
    Ok(match position {
        InsertPosition::First => start,
        _ => end,
    })
}

fn apply_delete_node(
    document: &mut XmlDocument,
    node: NodeId,
) -> Result<(Command, ChangedSet), CommandError> {
    require_attached(document, node)?;
    if document.root_element() == Some(node) {
        return Err(CommandError::new(
            "cannot_delete_root",
            "the root element cannot be deleted",
            Some(node),
        ));
    }
    let parent = document.parent(node).expect("attached");
    let parent_is_document = parent == NodeId::DOCUMENT;
    let range = document
        .range_entry(node)
        .ok_or_else(|| CommandError::new("range_unknown", "node byte range unknown", Some(node)))?;
    let bytes = document.source()[range.0..range.1].to_string();
    let rendered = document.qname(node).map(|q| q.render());
    let position = position_among(document, node).expect("child of parent");

    {
        let engine = document.engine_doc_mut();
        let dom = engine.document_mut();
        dom.remove_child(parent.to_engine(), node.to_engine());
    }
    document.bump_revision();
    document.invalidate_doc_order();
    document.remove_range_entry(node);
    if !parent_is_document && document.children(parent).is_empty() {
        // Deleting the only child rewrites the parent's empty-element
        // spelling (`<e></e>` collapses to `<e/>`), so re-render it; the
        // RestoreNode reverse re-expands it symmetrically.
        rerender_element(document, parent)?;
    } else {
        document.splice_source(range.0..range.1, "");
    }
    if let Some(rendered) = rendered {
        document.index_remove(node, &rendered);
    }

    let reverse = Command::RestoreNode {
        node,
        parent,
        position,
        offset: range.0,
        bytes,
    };
    Ok((
        reverse,
        ChangedSet {
            structure_changed: true,
            removed: vec![node],
            ..ChangedSet::default()
        },
    ))
}

fn apply_move_node(
    document: &mut XmlDocument,
    node: NodeId,
    new_parent: NodeId,
    position: &InsertPosition,
) -> Result<(Command, ChangedSet), CommandError> {
    require_attached(document, node)?;
    if document.root_element() == Some(node) {
        return Err(CommandError::new(
            "cannot_move_root",
            "the root element cannot be moved",
            Some(node),
        ));
    }
    require_element(document, new_parent)?;
    if node == new_parent || is_descendant(document, node, new_parent) {
        return Err(CommandError::new(
            "invalid_move_target",
            "cannot move a node into its own subtree",
            Some(node),
        ));
    }
    let sibling = position_sibling(document, new_parent, position)?;
    if subtree_height(document, node) + node_depth(document, new_parent) > MAX_DEPTH {
        return Err(CommandError::new(
            "depth_limit",
            format!("move would nest deeper than {MAX_DEPTH} levels"),
            Some(node),
        ));
    }

    let old_parent = document.parent(node).expect("attached");
    let old_position = position_among(document, node).expect("child of parent");
    let range = document
        .range_entry(node)
        .ok_or_else(|| CommandError::new("range_unknown", "node byte range unknown", Some(node)))?;
    let bytes = document.source()[range.0..range.1].to_string();

    // Pull the subtree's range entries out before any splicing so the
    // removal cannot shift them; they are re-seeded at the destination.
    let subtree_entries = take_subtree_entries(document, node);
    let old_parent_is_document = old_parent == NodeId::DOCUMENT;

    let (target_sibling, effective_position) =
        normalize_position(document, new_parent, position, sibling, node);
    {
        let engine = document.engine_doc_mut();
        let dom = engine.document_mut();
        dom.detach(node.to_engine());
    }
    // Remove the bytes first so anchor offsets are computed on the pruned
    // source (the earlier version computed them before the removal and
    // spliced at stale positions).
    if !old_parent_is_document && document.children(old_parent).is_empty() {
        rerender_element(document, old_parent)?;
    } else {
        document.splice_source(range.0..range.1, "");
    }

    // Where do the bytes go in the pruned source?
    let children = document.children(new_parent);
    let target_offset = if children.is_empty() {
        None // destination is childless (`<e/>`): re-render parent below
    } else {
        let anchor = match &effective_position {
            InsertPosition::Before(s) | InsertPosition::After(s) => Some(*s),
            InsertPosition::First => children.first().copied(),
            InsertPosition::Last => children.last().copied(),
        };
        let anchor = anchor.expect("non-empty parent");
        let (start, end) = document.range_entry(anchor).ok_or_else(|| {
            CommandError::new("range_unknown", "anchor byte range unknown", Some(anchor))
        })?;
        Some(match &effective_position {
            InsertPosition::After(_) => end,
            _ => start,
        })
    };

    {
        let engine = document.engine_doc_mut();
        let dom = engine.document_mut();
        attach(dom, new_parent, &effective_position, target_sibling, node);
    }
    document.bump_revision();
    document.invalidate_doc_order();

    match target_offset {
        Some(offset) => {
            document.splice_source(offset..offset, &bytes);
            let delta = offset as i64 - range.0 as i64;
            for (id, (start, end)) in subtree_entries {
                document.set_range_entry(
                    id,
                    (
                        (start as i64 + delta) as usize,
                        (end as i64 + delta) as usize,
                    ),
                );
            }
        }
        None => {
            rerender_element(document, new_parent)?;
        }
    }

    let reverse = Command::MoveNode {
        node,
        new_parent: old_parent,
        position: old_position,
    };
    Ok((
        reverse,
        ChangedSet {
            structure_changed: true,
            touched: vec![node],
            ..ChangedSet::default()
        },
    ))
}

/// Resolves First/Last into concrete sibling-relative positions for attach,
/// ignoring `excluded` (the node being moved) when picking anchors.
fn normalize_position(
    document: &XmlDocument,
    parent: NodeId,
    position: &InsertPosition,
    sibling: Option<NodeId>,
    excluded: NodeId,
) -> (Option<NodeId>, InsertPosition) {
    match position {
        InsertPosition::First => {
            let children: Vec<NodeId> = document
                .children(parent)
                .into_iter()
                .filter(|&id| id != excluded)
                .collect();
            match children.first() {
                Some(&first) => (Some(first), InsertPosition::Before(first)),
                None => (None, InsertPosition::Last),
            }
        }
        InsertPosition::Last => (None, InsertPosition::Last),
        InsertPosition::Before(_) | InsertPosition::After(_) => (sibling, position.clone()),
    }
}

fn apply_duplicate_subtree(
    document: &mut XmlDocument,
    node: NodeId,
) -> Result<(Command, ChangedSet), CommandError> {
    require_attached(document, node)?;
    if document.root_element() == Some(node) {
        return Err(CommandError::new(
            "cannot_duplicate_root",
            "the root element cannot be duplicated",
            Some(node),
        ));
    }
    let parent = document.parent(node).expect("attached");
    let range = document
        .range_entry(node)
        .ok_or_else(|| CommandError::new("range_unknown", "node byte range unknown", Some(node)))?;
    let bytes = document.source()[range.0..range.1].to_string();

    let copy_id = {
        let engine = document.engine_doc_mut();
        let dom = engine.document_mut();
        let copy = clone_subtree(dom, node);
        dom.insert_after(parent.to_engine(), copy.to_engine(), node.to_engine());
        copy
    };
    document.bump_revision();
    document.invalidate_doc_order();

    let offset = range.1;
    document.splice_source(offset..offset, &bytes);
    copy_ranges(document, node, copy_id, offset - range.0);
    if let Some(q) = document.qname(node) {
        document.index_add(copy_id, &q.render());
    }

    let reverse = Command::DeleteNode { node: copy_id };
    Ok((
        reverse,
        ChangedSet {
            structure_changed: true,
            touched: vec![copy_id],
            ..ChangedSet::default()
        },
    ))
}

/// Deep-clones a subtree inside the same arena, returning the copy's root.
fn clone_subtree(dom: &mut EngineDom<'_>, root: NodeId) -> NodeId {
    let engine_root = root.to_engine();
    let kind = match dom.node_kind(engine_root) {
        Some(NodeKind::Element(element)) => NodeKind::Element(element.clone()),
        Some(NodeKind::Text(text)) => NodeKind::Text(text.clone()),
        Some(NodeKind::CData(text)) => NodeKind::CData(text.clone()),
        Some(NodeKind::Comment(text)) => NodeKind::Comment(text.clone()),
        Some(NodeKind::ProcessingInstruction(pi)) => NodeKind::ProcessingInstruction(pi.clone()),
        _ => return root,
    };
    // The arena has no generic "create node with kind" API, so allocate a
    // placeholder and overwrite its kind before it is linked anywhere.
    let placeholder = dom.create_text("");
    if let Some(slot) = dom.node_kind_mut(placeholder) {
        *slot = kind;
    }
    for child in dom.children(engine_root) {
        let child_copy = clone_subtree(dom, NodeId::from_engine(child));
        dom.append_child(placeholder, child_copy.to_engine());
    }
    NodeId::from_engine(placeholder)
}

/// Copies range entries from `original`'s subtree onto `copy`'s subtree,
/// shifted by `delta`.
fn copy_ranges(document: &mut XmlDocument, original: NodeId, copy: NodeId, delta: usize) {
    if let Some((start, end)) = document.range_entry(original) {
        document.set_range_entry(copy, (start + delta, end + delta));
    }
    let orig_children = document.children(original);
    let copy_children = document.children(copy);
    if orig_children.len() == copy_children.len() {
        for (orig, dup) in orig_children.into_iter().zip(copy_children) {
            copy_ranges(document, orig, dup, delta);
        }
    }
}

fn apply_replace_whole_source(
    document: &mut XmlDocument,
    new_source: &str,
) -> Result<(Command, ChangedSet), CommandError> {
    let old_source = document.source().to_string();
    document
        .replace_with_source(new_source.to_string())
        .map_err(|err: XmlError| CommandError::new("invalid_source", err.to_string(), None))?;

    let reverse = Command::ReplaceWholeSource {
        new_source: old_source,
    };
    Ok((
        reverse,
        ChangedSet {
            structure_changed: true,
            whole_source: true,
            ..ChangedSet::default()
        },
    ))
}

fn apply_batch_replace(
    document: &mut XmlDocument,
    ops: &[ReplaceOp],
) -> Result<(Command, ChangedSet), CommandError> {
    if ops.is_empty() {
        return Err(CommandError::new(
            "empty_batch",
            "no replacements given",
            None,
        ));
    }
    // Validate everything and build the reverse batch before touching state.
    let mut reverse_ops = Vec::with_capacity(ops.len());
    for op in ops {
        require_attached(document, op.node)?;
        match &op.target {
            ReplaceTarget::NodeText => match document.kind(op.node) {
                Some(XmlNodeKind::Text) => {
                    if op.new_value.contains('<') {
                        return Err(CommandError::new(
                            "invalid_text",
                            "replacement text must not contain '<'",
                            Some(op.node),
                        ));
                    }
                }
                Some(XmlNodeKind::CData) => {
                    if op.new_value.contains("]]>") {
                        return Err(CommandError::new(
                            "invalid_cdata",
                            "replacement CDATA must not contain ']]>'",
                            Some(op.node),
                        ));
                    }
                }
                _ => {
                    return Err(CommandError::new(
                        "wrong_node_kind",
                        "node has no text content to replace",
                        Some(op.node),
                    ));
                }
            },
            ReplaceTarget::Comment => {
                if document.comment_text(op.node).is_none() {
                    return Err(CommandError::new(
                        "wrong_node_kind",
                        "node is not a comment",
                        Some(op.node),
                    ));
                }
                if op.new_value.contains("--") || op.new_value.ends_with('-') {
                    return Err(CommandError::new(
                        "invalid_comment",
                        "replacement comment must not contain '--'",
                        Some(op.node),
                    ));
                }
            }
            ReplaceTarget::Attribute(name) => {
                require_element(document, op.node)?;
                if !document
                    .attributes(op.node)
                    .iter()
                    .any(|(existing, _)| existing == name)
                {
                    return Err(CommandError::new(
                        "unknown_attribute",
                        format!("attribute '{name}' does not exist"),
                        Some(op.node),
                    ));
                }
            }
        }
        reverse_ops.push(ReplaceOp {
            node: op.node,
            target: op.target.clone(),
            old_value: op.new_value.clone(),
            new_value: op.old_value.clone(),
        });
    }

    let revision_before = document.revision();
    let mut touched = Vec::with_capacity(ops.len());
    for op in ops {
        match &op.target {
            ReplaceTarget::NodeText | ReplaceTarget::Comment => {
                let content = match &op.target {
                    ReplaceTarget::Comment => NodeContent::Comment(op.new_value.clone()),
                    _ if document.kind(op.node) == Some(XmlNodeKind::CData) => {
                        NodeContent::CData(op.new_value.clone())
                    }
                    _ => NodeContent::Text(op.new_value.clone()),
                };
                Command::SetNodeContent {
                    node: op.node,
                    content,
                }
                .apply(document)?;
            }
            ReplaceTarget::Attribute(name) => {
                Command::SetAttributeValue {
                    element: op.node,
                    name: name.clone(),
                    value: op.new_value.clone(),
                }
                .apply(document)?;
            }
        }
        touched.push(op.node);
    }
    // The batch is one command: collapse the per-op revision bumps.
    document.collapse_revision_to(revision_before.0 + 1);

    Ok((
        Command::BatchReplace { ops: reverse_ops },
        ChangedSet {
            touched,
            ..ChangedSet::default()
        },
    ))
}

fn apply_format_document(
    document: &mut XmlDocument,
    indent: &str,
) -> Result<(Command, ChangedSet), CommandError> {
    let old_source = document.source().to_string();
    let rendered = {
        let dom = document.dom();
        let write_options = uppsala::dom::XmlWriteOptions::pretty(indent.to_string())
            .with_doctype(true)
            .with_expand_empty_elements(false);
        dom.to_xml_with_options(&write_options)
    };
    document
        .replace_with_source(rendered)
        .map_err(|err| CommandError::new("format_failed", err.to_string(), None))?;

    let reverse = Command::ReplaceWholeSource {
        new_source: old_source,
    };
    Ok((
        reverse,
        ChangedSet {
            structure_changed: true,
            whole_source: true,
            ..ChangedSet::default()
        },
    ))
}

fn apply_restore_node(
    document: &mut XmlDocument,
    node: NodeId,
    parent: NodeId,
    position: &InsertPosition,
    offset: usize,
    bytes: &str,
) -> Result<(Command, ChangedSet), CommandError> {
    require_element(document, parent)?;
    let was_childless = document.children(parent).is_empty();
    let sibling = position_sibling(document, parent, position)?;

    // Descendants kept stale (but original, LIFO-consistent) range entries;
    // pull them out so the insertion splice cannot shift them, then put
    // them back unchanged.
    let subtree_entries = take_subtree_entries(document, node);

    {
        let engine = document.engine_doc_mut();
        let dom = engine.document_mut();
        attach(dom, parent, position, sibling, node);
    }
    document.bump_revision();
    document.invalidate_doc_order();

    if was_childless {
        rerender_element(document, parent)?;
    } else {
        document.splice_source(offset..offset, bytes);
        for (id, entry) in subtree_entries {
            document.set_range_entry(id, entry);
        }
        document.set_range_entry(node, (offset, offset + bytes.len()));
    }
    if let Some(q) = document.qname(node) {
        document.index_add(node, &q.render());
    }

    let reverse = Command::DeleteNode { node };
    Ok((
        reverse,
        ChangedSet {
            structure_changed: true,
            touched: vec![node],
            ..ChangedSet::default()
        },
    ))
}
