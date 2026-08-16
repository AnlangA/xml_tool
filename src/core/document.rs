//! The authoritative XML document model.
//!
//! [`XmlDocument`] pairs the engine DOM with the current source text and a
//! per-node byte-range map, kept in lockstep by the command layer. Fields are
//! private; reads go through accessors and writes only through
//! [`crate::core::command::Command`].
//!
//! Invariants maintained by every committed command:
//!
//! - `source` is always well-formed XML text equivalent to the DOM
//!   (entity references stay in their original spelling in `source` while
//!   the DOM holds the expanded text, exactly like a fresh parse).
//! - `ranges` maps live, attached nodes to byte ranges into `source`;
//!   detached (orphaned) nodes keep no range.
//! - `revision` increases by exactly one per committed command.

use std::collections::HashMap;
use std::ops::Range;

use uppsala::dom::{Document as EngineDom, NodeId as EngineNodeId, NodeKind};

use crate::xml::XmlError;
use crate::xml::encoding::SourceEncoding;
use crate::xml::engine::{EngineDocument, ParseOptions, UppsalaXmlEngine, XmlEngine};

/// Stable handle to a node in the document arena.
///
/// Ids stay valid while the node lives in the arena — even when the node is
/// detached by an undoable delete — because the arena is append-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u64);

impl NodeId {
    /// The document node itself (parent of the root element and any
    /// top-level comments/PIs).
    pub const DOCUMENT: NodeId = NodeId(0);

    pub(crate) fn from_engine(id: EngineNodeId) -> NodeId {
        NodeId(id.index() as u64)
    }

    pub(crate) fn to_engine(self) -> EngineNodeId {
        EngineNodeId::new(self.0 as usize)
    }
}

/// Monotonic edit revision of a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Revision(pub u64);

/// Node kinds of the fidelity document model.
///
/// `Doctype` maps to the document-level declaration and `EntityReference`
/// is reserved: the engine expands internal entities into text at parse
/// time, so no separate reference nodes exist today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum XmlNodeKind {
    Element,
    Text,
    CData,
    Comment,
    ProcessingInstruction,
    Doctype,
    EntityReference,
}

/// A qualified name: prefix, local part, and resolved namespace URI.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QName {
    prefix: Option<String>,
    local_name: String,
    namespace_uri: Option<String>,
}

impl QName {
    fn from_engine(name: &uppsala::dom::QName<'_>) -> QName {
        QName {
            prefix: name.prefix.as_deref().map(str::to_string),
            local_name: name.local_name.to_string(),
            namespace_uri: name.namespace_uri.as_deref().map(str::to_string),
        }
    }

    /// The namespace prefix, if the name carries one.
    pub fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }

    /// The local part of the name.
    pub fn local_name(&self) -> &str {
        &self.local_name
    }

    /// The resolved namespace URI, if any.
    pub fn namespace_uri(&self) -> Option<&str> {
        self.namespace_uri.as_deref()
    }

    /// Renders `prefix:local` or `local`.
    pub fn render(&self) -> String {
        match &self.prefix {
            Some(prefix) => format!("{prefix}:{}", self.local_name),
            None => self.local_name.clone(),
        }
    }
}

/// Name specification for editing commands. `namespace_uri` of `None`
/// keeps the node's current binding; `Some(uri)` rebinds the prefix
/// explicitly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QNameSpec {
    /// Raw name as typed, `prefix:local` or `local`.
    pub name: String,
    /// Optional explicit namespace URI rebinding.
    pub namespace_uri: Option<String>,
}

impl QNameSpec {
    /// A plain local name with no prefix and no explicit rebinding.
    pub fn local(name: impl Into<String>) -> QNameSpec {
        QNameSpec {
            name: name.into(),
            namespace_uri: None,
        }
    }

    pub(crate) fn split_prefix(&self) -> Result<(Option<&str>, &str), String> {
        validate_qname(&self.name)
    }
}

impl From<&str> for QNameSpec {
    fn from(name: &str) -> QNameSpec {
        QNameSpec {
            name: name.to_string(),
            namespace_uri: None,
        }
    }
}

/// A half-open byte range plus 1-based line/column endpoints in `source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRange {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

/// Summary `Debug` for test diagnostics; never prints the full source.
impl std::fmt::Debug for XmlDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XmlDocument")
            .field("revision", &self.revision.0)
            .field("source_len", &self.source.len())
            .finish_non_exhaustive()
    }
}

/// The authoritative document: engine DOM + current source + ranges +
/// indexes + revision.
pub struct XmlDocument {
    engine: EngineDocument,
    source: String,
    /// Engine arena index → byte range in `source` (attached nodes only).
    ranges: HashMap<u64, (usize, usize)>,
    /// Rendered element name → engine arena indices, in document order.
    qname_index: HashMap<String, Vec<u64>>,
    /// Cached document order of all attached nodes (rebuilt on structural
    /// changes).
    doc_order: Vec<u64>,
    doc_order_valid: bool,
    revision: Revision,
}

impl XmlDocument {
    /// Parses bytes into a document. Accepts every supported encoding.
    pub fn parse(bytes: &[u8]) -> Result<XmlDocument, XmlError> {
        Self::parse_with_options(bytes, ParseOptions::default())
    }

    /// Parses bytes with explicit engine options.
    pub fn parse_with_options(
        bytes: &[u8],
        options: ParseOptions,
    ) -> Result<XmlDocument, XmlError> {
        let engine = UppsalaXmlEngine.parse_bytes(bytes, &options)?;
        let source = engine
            .decoded_text()
            .expect("parsed documents carry a snapshot")
            .to_string();
        let mut document = XmlDocument {
            engine,
            source,
            ranges: HashMap::new(),
            qname_index: HashMap::new(),
            doc_order: Vec::new(),
            doc_order_valid: false,
            revision: Revision(0),
        };
        document.rebuild_ranges_and_indexes();
        Ok(document)
    }

    /// The current revision.
    pub fn revision(&self) -> Revision {
        self.revision
    }

    /// The current source text (decoded form; entity references in their
    /// original spelling).
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The detected source encoding.
    pub fn encoding(&self) -> SourceEncoding {
        self.engine.encoding()
    }

    /// The original bytes this document was parsed from, if unedited since.
    pub fn original_bytes(&self) -> Option<&[u8]> {
        self.engine.original_bytes()
    }

    /// The engine DOM. Read-only access for services (XPath in a later
    /// step); all mutations go through commands.
    pub fn dom(&self) -> &EngineDom<'static> {
        self.engine.document()
    }

    // -----------------------------------------------------------------------
    // Node access
    // -----------------------------------------------------------------------

    /// The root element, if the document has one.
    pub fn root_element(&self) -> Option<NodeId> {
        self.dom().document_element().map(NodeId::from_engine)
    }

    /// The kind of `node`, or `None` for unknown ids.
    pub fn kind(&self, node: NodeId) -> Option<XmlNodeKind> {
        match self.dom().node_kind(node.to_engine())? {
            NodeKind::Document => None,
            NodeKind::Element(_) => Some(XmlNodeKind::Element),
            NodeKind::Text(_) => Some(XmlNodeKind::Text),
            NodeKind::CData(_) => Some(XmlNodeKind::CData),
            NodeKind::Comment(_) => Some(XmlNodeKind::Comment),
            NodeKind::ProcessingInstruction(_) => Some(XmlNodeKind::ProcessingInstruction),
            NodeKind::Attribute(..) => None,
        }
    }

    /// Whether `node` is currently attached to the tree.
    pub fn is_attached(&self, node: NodeId) -> bool {
        node == NodeId::DOCUMENT || self.dom().parent(node.to_engine()).is_some()
    }

    /// The qualified name of an element node.
    pub fn qname(&self, node: NodeId) -> Option<QName> {
        self.dom()
            .element(node.to_engine())
            .map(|element| QName::from_engine(&element.name))
    }

    /// The parent of `node` (`None` for the document node).
    pub fn parent(&self, node: NodeId) -> Option<NodeId> {
        self.dom().parent(node.to_engine()).map(NodeId::from_engine)
    }

    /// Children of `node` in document order.
    pub fn children(&self, node: NodeId) -> Vec<NodeId> {
        self.dom()
            .children_iter(node.to_engine())
            .map(NodeId::from_engine)
            .collect()
    }

    /// Attribute list of an element: `(rendered name, value)` pairs.
    pub fn attributes(&self, element: NodeId) -> Vec<(String, String)> {
        self.dom()
            .element(element.to_engine())
            .map(|element| {
                element
                    .attributes
                    .iter()
                    .map(|attr| {
                        (
                            QName::from_engine(&attr.name).render(),
                            attr.value.to_string(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Namespace declarations of an element: `(prefix or "", uri)` pairs.
    pub fn namespace_declarations(&self, element: NodeId) -> Vec<(String, String)> {
        self.dom()
            .element(element.to_engine())
            .map(|element| {
                element
                    .namespace_declarations
                    .iter()
                    .map(|(prefix, uri)| (prefix.to_string(), uri.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Text of a Text/CData node.
    pub fn node_text(&self, node: NodeId) -> Option<&str> {
        match self.dom().node_kind(node.to_engine())? {
            NodeKind::Text(text) | NodeKind::CData(text) => Some(text),
            _ => None,
        }
    }

    /// Comment text.
    pub fn comment_text(&self, node: NodeId) -> Option<&str> {
        match self.dom().node_kind(node.to_engine())? {
            NodeKind::Comment(text) => Some(text),
            _ => None,
        }
    }

    /// Processing instruction target and data.
    pub fn pi(&self, node: NodeId) -> Option<(&str, Option<&str>)> {
        match self.dom().node_kind(node.to_engine())? {
            NodeKind::ProcessingInstruction(pi) => Some((&pi.target, pi.data.as_deref())),
            _ => None,
        }
    }

    /// The raw DOCTYPE text, if any.
    pub fn doctype(&self) -> Option<&str> {
        self.dom().doctype.as_deref()
    }

    /// The XML declaration, as `(version, encoding, standalone)`.
    pub fn declaration(&self) -> Option<(String, Option<String>, Option<bool>)> {
        self.dom().xml_declaration.as_ref().map(|decl| {
            (
                decl.version.to_string(),
                decl.encoding.as_deref().map(str::to_string),
                decl.standalone,
            )
        })
    }

    /// The byte range of `node` in the current source, with line/column
    /// endpoints.
    pub fn source_range(&self, node: NodeId) -> Option<SourceRange> {
        let &(start, end) = self.ranges.get(&node.0)?;
        let (start_line, start_column) = self.line_column(start)?;
        let (end_line, end_column) = self.line_column(end.max(start))?;
        Some(SourceRange {
            start_byte: start,
            end_byte: end,
            start_line,
            start_column,
            end_line,
            end_column,
        })
    }

    /// 1-based line/column of a byte offset.
    pub fn line_column(&self, byte_offset: usize) -> Option<(usize, usize)> {
        if byte_offset > self.source.len() {
            return None;
        }
        let bytes = self.source.as_bytes();
        let line = 1 + bytes[..byte_offset].iter().filter(|&&b| b == b'\n').count();
        let line_start = bytes[..byte_offset]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |pos| pos + 1);
        let column = 1 + self.source[line_start..byte_offset].chars().count();
        Some((line, column))
    }

    /// All attached nodes in document order.
    pub fn document_order(&self) -> &[u64] {
        debug_assert!(self.doc_order_valid);
        &self.doc_order
    }

    /// Engine arena indices whose rendered element name equals `name`
    /// (e.g. `"book"` or `"dc:title"`), in document order.
    pub fn elements_named(&self, name: &str) -> Vec<NodeId> {
        self.qname_index
            .get(name)
            .map(|ids| {
                ids.iter()
                    .map(|&index| NodeId(index))
                    .filter(|&id| self.is_attached(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    // -----------------------------------------------------------------------
    // Internals shared with the command layer
    // -----------------------------------------------------------------------

    pub(crate) fn engine_doc_mut(&mut self) -> &mut EngineDocument {
        &mut self.engine
    }

    pub(crate) fn bump_revision(&mut self) {
        self.revision.0 += 1;
    }

    /// Collapses the revision to `value`; used by composite commands
    /// (`BatchReplace`) whose per-op applies bump the counter internally.
    pub(crate) fn collapse_revision_to(&mut self, value: u64) {
        self.revision = Revision(value);
    }

    /// Source slice for a stored range, clamped to the current length.
    /// Range staleness must degrade to cosmetics (a shorter slice), never
    /// panic — the DOM is the source of truth.
    pub(crate) fn source_slice(&self, range: (usize, usize)) -> &str {
        let end = range.1.min(self.source.len());
        let start = range.0.min(end);
        &self.source[start..end]
    }

    pub(crate) fn range_entry(&self, node: NodeId) -> Option<(usize, usize)> {
        self.ranges.get(&node.0).copied()
    }

    pub(crate) fn set_range_entry(&mut self, node: NodeId, range: (usize, usize)) {
        self.ranges.insert(node.0, range);
    }

    pub(crate) fn remove_range_entry(&mut self, node: NodeId) {
        self.ranges.remove(&node.0);
    }

    /// Replaces `range` in the source with `replacement`, shifting every
    /// stored range after it by the length delta.
    pub(crate) fn splice_source(&mut self, range: Range<usize>, replacement: &str) {
        // Total function: a stale caller range clamps to the current
        // length instead of panicking (DOM remains the source of truth).
        let end = range.end.min(self.source.len());
        let start = range.start.min(end);
        let range = start..end;
        self.source.replace_range(range.clone(), replacement);
        let delta = replacement.len() as i64 - (range.end - range.start) as i64;
        if delta != 0 {
            for entry in self.ranges.values_mut() {
                if entry.0 >= range.end {
                    // Node starts after the splice: shift both ends.
                    entry.0 = (entry.0 as i64 + delta) as usize;
                    entry.1 = (entry.1 as i64 + delta) as usize;
                } else if entry.1 > range.end {
                    // Ancestor strictly containing the splice: its end
                    // moves with the content. A node ending exactly at the
                    // splice point keeps its end (insertion at its tail
                    // is not inside it).
                    entry.1 = (entry.1 as i64 + delta) as usize;
                }
            }
        }
    }

    /// Drops range entries that start inside `range` (used before remapping
    /// a re-rendered subtree). The subtree root itself must be re-set
    /// afterwards.
    pub(crate) fn drop_ranges_within(&mut self, range: Range<usize>) {
        self.ranges
            .retain(|_, entry| !(entry.0 >= range.start && entry.0 < range.end));
    }

    /// Serializes one node with compact, entity-escaped output.
    pub(crate) fn serialize_node(&self, node: NodeId) -> String {
        self.dom()
            .node_to_xml_with_options(node.to_engine(), &uppsala::dom::XmlWriteOptions::compact())
    }

    /// Rebuilds ranges and indexes from the engine DOM. Ranges come from the
    /// engine (valid at parse time); the name index and document order are
    /// recomputed by walking.
    pub(crate) fn rebuild_ranges_and_indexes(&mut self) {
        self.ranges.clear();
        self.qname_index.clear();
        let engine = self.engine.document();
        for id in self.collect_attached(engine) {
            if let Some(range) = engine.node_range(EngineNodeId::new(id as usize)) {
                self.ranges.insert(id, (range.start, range.end));
            }
            if let Some(element) = engine.element(EngineNodeId::new(id as usize)) {
                let rendered = QName::from_engine(&element.name).render();
                self.qname_index.entry(rendered).or_default().push(id);
            }
        }
        self.doc_order = self.collect_attached(engine);
        self.doc_order_valid = true;
    }

    /// Rebuilds the document-order cache after a structural change.
    pub(crate) fn invalidate_doc_order(&mut self) {
        let engine = self.engine.document();
        self.doc_order = self.collect_attached(engine);
        self.doc_order_valid = true;
    }

    pub(crate) fn index_rename(&mut self, node: NodeId, old_rendered: &str, new_rendered: &str) {
        if let Some(ids) = self.qname_index.get_mut(old_rendered) {
            ids.retain(|&id| id != node.0);
        }
        self.qname_index
            .entry(new_rendered.to_string())
            .or_default()
            .push(node.0);
    }

    pub(crate) fn index_add(&mut self, node: NodeId, rendered: &str) {
        self.qname_index
            .entry(rendered.to_string())
            .or_default()
            .push(node.0);
    }

    pub(crate) fn index_remove(&mut self, node: NodeId, rendered: &str) {
        if let Some(ids) = self.qname_index.get_mut(rendered) {
            ids.retain(|&id| id != node.0);
        }
    }

    fn collect_attached(&self, engine: &EngineDom<'_>) -> Vec<u64> {
        // Walk from the document node in document order, skipping orphans.
        fn walk(engine: &EngineDom<'_>, id: EngineNodeId, out: &mut Vec<u64>) {
            out.push(id.index() as u64);
            for child in engine.children_iter(id) {
                walk(engine, child, out);
            }
        }
        let mut out = Vec::new();
        walk(engine, engine.root(), &mut out);
        out
    }

    /// Replaces the whole document (engine DOM + source) wholesale; used by
    /// `ReplaceWholeSource` and `FormatDocument`.
    pub(crate) fn replace_with_source(&mut self, new_source: String) -> Result<(), XmlError> {
        let bytes = crate::xml::encoding::encode_xml_text(&new_source, self.engine.encoding());
        let parsed = UppsalaXmlEngine.parse_bytes(&bytes, &ParseOptions::default())?;
        let decoded = parsed
            .decoded_text()
            .expect("parsed documents carry a snapshot")
            .to_string();
        debug_assert_eq!(decoded, new_source);
        self.engine = parsed;
        self.source = decoded;
        self.rebuild_ranges_and_indexes();
        self.revision.0 += 1;
        Ok(())
    }
}

/// Validates `prefix:local` or `local` as an XML qualified name; returns the
/// split form. ASCII rules plus non-ASCII permissive ranges; the engine
/// re-validates on parse anyway.
pub(crate) fn validate_qname(name: &str) -> Result<(Option<&str>, &str), String> {
    let (prefix, local) = match name.split_once(':') {
        Some((prefix, local)) => (Some(prefix), local),
        None => (None, name),
    };
    if let Some(prefix) = prefix
        && !is_ncname(prefix)
    {
        return Err(format!("invalid namespace prefix '{prefix}'"));
    }
    if !is_ncname(local) {
        return Err(format!("invalid local name '{local}'"));
    }
    Ok((prefix, local))
}

/// ASCII-subset NCName check: the engine accepts full Unicode names; this
/// rejects the characters that would make output unparseable.
pub(crate) fn is_ncname(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_alphabetic() || first == '_' || (first as u32) > 0x7F) {
        return false;
    }
    chars.all(|ch| {
        ch.is_alphanumeric()
            || ch == '_'
            || ch == '-'
            || ch == '.'
            || matches!(ch, '\u{00B7}' | '\u{0387}')
            || (ch as u32) > 0x7F
    })
}
