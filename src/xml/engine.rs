//! The XML engine isolation layer.
//!
//! [`XmlEngine`] is the only seam through which the rest of the application
//! may parse, serialize, and (in later steps) query XML. The single
//! production implementation is [`UppsalaXmlEngine`], wrapping the pinned
//! `uppsala` crate. UI code must never touch `uppsala` types directly.
//!
//! Fidelity contract:
//!
//! - Parsing always starts from bytes and records the detected
//!   [`SourceEncoding`] plus the original bytes in a [`SourceSnapshot`].
//! - Serializing an unmodified document replays the original bytes
//!   byte-for-byte (BOM, line endings, quoting, entity references included).
//! - Serializing an edited document re-renders the tree and re-encodes it in
//!   the original encoding, BOM included.
//!
//! Security contract (defaults fixed by the product plan):
//!
//! - Inputs larger than [`SecurityLimits::max_input_bytes`] are rejected
//!   before any decoding or DOM allocation.
//! - Element nesting is capped at 512 and total entity expansion at 16 MiB,
//!   defeating billion-laughs and quadratic-blowup expansions.
//! - External entities (SYSTEM/PUBLIC) are declared-but-never-loaded by the
//!   engine; referencing one is an error. There is no network code path
//!   anywhere in the dependency tree.

use uppsala::dom::{Document, NodeId, XmlWriteOptions};
use uppsala::parser::Parser;

use super::encoding::{SourceEncoding, decode_xml_source, encode_xml_text};
use super::error::{SourceLocation, XmlError, XmlErrorCode};

/// Plan-fixed safety limits. `Default` yields the shipped configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecurityLimits {
    /// Maximum accepted input size. Larger inputs are refused outright.
    pub max_input_bytes: usize,
    /// Maximum element nesting depth.
    pub max_depth: u32,
    /// Maximum total bytes of entity expansion per parse.
    pub max_entity_expansion: usize,
}

impl SecurityLimits {
    /// The plan-fixed production limits: 256 MiB input, depth 512,
    /// 16 MiB entity expansion.
    pub const fn production() -> Self {
        SecurityLimits {
            max_input_bytes: 256 * 1024 * 1024,
            max_depth: 512,
            max_entity_expansion: 16 * 1024 * 1024,
        }
    }
}

impl Default for SecurityLimits {
    fn default() -> Self {
        SecurityLimits::production()
    }
}

/// Options for [`XmlEngine::parse_bytes`].
#[derive(Debug, Clone)]
pub struct ParseOptions {
    /// Security limits applied before and during parsing.
    pub limits: SecurityLimits,
    /// Whether element and attribute names are resolved against declared
    /// namespaces. The legacy compatibility facade parses with this off to
    /// match its historical behavior; the document engine always parses with
    /// it on.
    pub namespace_aware: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        ParseOptions {
            limits: SecurityLimits::production(),
            namespace_aware: true,
        }
    }
}

/// Options for [`XmlEngine::serialize`]. Defaults preserve the document's
/// own formatting: no forced indentation, DOCTYPE kept, empty elements kept
/// in their original spelling.
#[derive(Debug, Clone, Default)]
pub struct SerializeOptions {
    /// Force indentation with this string (e.g. two spaces). `None` keeps
    /// the document's existing whitespace.
    pub indent: Option<String>,
    /// Include the preserved DOCTYPE declaration in the output.
    pub include_doctype: bool,
    /// Write `<e></e>` instead of `<e/>`.
    pub expand_empty_elements: bool,
}

impl SerializeOptions {
    /// Options that change nothing about the document's own formatting.
    pub fn faithful() -> Self {
        SerializeOptions {
            indent: None,
            include_doctype: true,
            expand_empty_elements: false,
        }
    }
}

/// The original byte stream and its encoding, captured at parse time.
pub struct SourceSnapshot {
    /// The exact bytes the document was parsed from.
    pub original: Vec<u8>,
    /// The decoded source text. Kept alongside the (owned) DOM because
    /// `uppsala` drops its input reference on `into_static`; source ranges
    /// are still valid byte offsets into this text.
    pub decoded: String,
    /// Encoding detected for `original` (and used when re-encoding).
    pub encoding: SourceEncoding,
}

/// A parsed XML document plus everything needed for fidelity saves:
/// the engine DOM, its source snapshot, and an edited flag.
pub struct EngineDocument {
    document: Document<'static>,
    snapshot: Option<SourceSnapshot>,
    dirty: bool,
}

/// Summary `Debug`: prints status flags and source sizes, never the tree.
impl std::fmt::Debug for EngineDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineDocument")
            .field("dirty", &self.dirty)
            .field(
                "snapshot",
                &self.snapshot.as_ref().map(|snap| {
                    (
                        snap.original.len(),
                        snap.decoded.len(),
                        snap.encoding.declaration_name(),
                    )
                }),
            )
            .finish_non_exhaustive()
    }
}

impl EngineDocument {
    /// Read access to the engine DOM.
    pub fn document(&self) -> &Document<'static> {
        &self.document
    }

    /// Mutable access to the engine DOM. Marks the document edited, which
    /// switches serialization from byte replay to re-rendering.
    pub fn document_mut(&mut self) -> &mut Document<'static> {
        self.dirty = true;
        &mut self.document
    }

    /// The encoding this document was parsed as; UTF-8 for synthesized
    /// documents.
    pub fn encoding(&self) -> SourceEncoding {
        self.snapshot
            .as_ref()
            .map_or(SourceEncoding::Utf8, |snap| snap.encoding)
    }

    /// The original bytes, when this document was parsed from a byte stream.
    pub fn original_bytes(&self) -> Option<&[u8]> {
        self.snapshot.as_ref().map(|snap| snap.original.as_slice())
    }

    /// The decoded source text, when parsed from a byte stream.
    pub fn decoded_text(&self) -> Option<&str> {
        self.snapshot.as_ref().map(|snap| snap.decoded.as_str())
    }

    /// Whether the DOM has been mutated since parsing.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Marks the document edited (used when callers mutate through
    /// borrowed DOM access obtained before a `document_mut` call).
    pub fn mark_edited(&mut self) {
        self.dirty = true;
    }

    /// Byte range of `id` in the source text, when the node came from a
    /// parse.
    pub fn node_range(&self, id: NodeId) -> Option<std::ops::Range<usize>> {
        self.document.node_range(id)
    }

    /// The original source slice of `id`, computed from the snapshot text
    /// (the engine DOM itself does not retain the input).
    pub fn node_source(&self, id: NodeId) -> Option<&str> {
        let range = self.document.node_range(id)?;
        let text = self.decoded_text()?;
        text.get(range)
    }

    /// 1-based line/column of a byte offset in the source text.
    pub fn source_location(&self, byte_offset: usize) -> Option<SourceLocation> {
        let text = self.decoded_text()?;
        let byte_offset = byte_offset.min(text.len());
        let prefix = &text.as_bytes()[..byte_offset];
        let line = 1 + prefix.iter().filter(|&&b| b == b'\n').count();
        let last_line_start = prefix
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |pos| pos + 1);
        let column = 1 + text[last_line_start..byte_offset].chars().count();
        Some(SourceLocation { line, column })
    }
}

/// The seam between the application and the XML backend. The only production
/// implementation is [`UppsalaXmlEngine`]; tests may provide their own.
pub trait XmlEngine: Send + Sync {
    /// Parses `bytes` (any supported encoding, BOM optional) into an
    /// [`EngineDocument`].
    fn parse_bytes(&self, bytes: &[u8], options: &ParseOptions)
    -> Result<EngineDocument, XmlError>;

    /// Serializes `document`. Unmodified documents replay their original
    /// bytes exactly; edited documents are re-rendered and re-encoded in the
    /// original encoding.
    fn serialize(
        &self,
        document: &EngineDocument,
        options: &SerializeOptions,
    ) -> Result<Vec<u8>, XmlError>;
}

/// Production [`XmlEngine`] backed by the pinned `uppsala` crate.
#[derive(Debug, Default, Clone, Copy)]
pub struct UppsalaXmlEngine;

impl XmlEngine for UppsalaXmlEngine {
    fn parse_bytes(
        &self,
        bytes: &[u8],
        options: &ParseOptions,
    ) -> Result<EngineDocument, XmlError> {
        if bytes.len() > options.limits.max_input_bytes {
            return Err(XmlError::new(
                XmlErrorCode::InputTooLarge,
                format!(
                    "input is {} bytes; the maximum is {}",
                    bytes.len(),
                    options.limits.max_input_bytes
                ),
            ));
        }

        let (text, encoding) = decode_xml_source(bytes)?;
        let parser = Parser::with_namespace_aware(options.namespace_aware)
            .with_max_depth(options.limits.max_depth)
            .with_max_entity_expansion(options.limits.max_entity_expansion);
        let document = parser
            .parse(&text)
            .map_err(XmlError::from_uppsala)?
            .into_static();

        Ok(EngineDocument {
            document,
            snapshot: Some(SourceSnapshot {
                original: bytes.to_vec(),
                decoded: text,
                encoding,
            }),
            dirty: false,
        })
    }

    fn serialize(
        &self,
        document: &EngineDocument,
        options: &SerializeOptions,
    ) -> Result<Vec<u8>, XmlError> {
        if !document.is_dirty()
            && let Some(snapshot) = &document.snapshot
        {
            return Ok(snapshot.original.clone());
        }

        let write_options = match &options.indent {
            Some(indent) => XmlWriteOptions::pretty(indent.clone()),
            None => XmlWriteOptions::compact(),
        };
        let write_options = write_options
            .with_doctype(options.include_doctype)
            .with_expand_empty_elements(options.expand_empty_elements);
        let text = document.document.to_xml_with_options(&write_options);
        Ok(encode_xml_text(&text, document.encoding()))
    }
}

/// Parses raw bytes with the production engine and default (namespace-aware)
/// options. Convenience wrapper mirroring the plan's public API.
pub fn parse_xml_bytes(bytes: &[u8], options: ParseOptions) -> Result<EngineDocument, XmlError> {
    UppsalaXmlEngine.parse_bytes(bytes, &options)
}

/// Serializes with faithful formatting: unmodified documents replay their
/// original bytes.
pub fn serialize_xml_bytes(document: &EngineDocument) -> Result<Vec<u8>, XmlError> {
    UppsalaXmlEngine.serialize(document, &SerializeOptions::faithful())
}

/// Serializes with explicit options.
pub fn serialize_xml_with_options(
    document: &EngineDocument,
    options: SerializeOptions,
) -> Result<Vec<u8>, XmlError> {
    UppsalaXmlEngine.serialize(document, &options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uppsala::dom::NodeKind;

    const DOC: &str = "<catalog><book id=\"1\">text</book></catalog>";

    #[test]
    fn unedited_documents_replay_original_bytes() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(DOC.as_bytes());
        let doc = parse_xml_bytes(&bytes, ParseOptions::default()).unwrap();
        let out = serialize_xml_bytes(&doc).unwrap();
        assert_eq!(out, bytes);
    }

    #[test]
    fn edited_documents_rerender_in_original_encoding() {
        let doc = parse_xml_bytes(DOC.as_bytes(), ParseOptions::default()).unwrap();
        let mut edited = doc;
        edited.mark_edited();
        let out = serialize_xml_bytes(&edited).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text, DOC);
    }

    #[test]
    fn edited_utf16_documents_stay_utf16() {
        let text = "<?xml version=\"1.0\" encoding=\"UTF-16\"?><a>中文</a>";
        let bytes = encode_xml_text(text, SourceEncoding::Utf16LeBom);
        let doc = parse_xml_bytes(&bytes, ParseOptions::default()).unwrap();
        let mut edited = doc;
        edited.mark_edited();
        let out = serialize_xml_bytes(&edited).unwrap();
        assert_eq!(&out[..2], &[0xFF, 0xFE]);
        let units: Vec<u16> = out[2..]
            .chunks_exact(2)
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect();
        assert!(String::from_utf16(&units).unwrap().contains("中文"));
    }

    #[test]
    fn input_over_limit_is_refused_before_parsing() {
        let mut options = ParseOptions::default();
        options.limits.max_input_bytes = 8;
        let err = parse_xml_bytes(DOC.as_bytes(), options).unwrap_err();
        assert_eq!(err.code(), XmlErrorCode::InputTooLarge);
    }

    #[test]
    fn deep_nesting_hits_depth_limit() {
        let mut xml = String::new();
        for _ in 0..600 {
            xml.push_str("<d>");
        }
        for _ in 0..600 {
            xml.push_str("</d>");
        }
        let err = parse_xml_bytes(xml.as_bytes(), ParseOptions::default()).unwrap_err();
        assert_eq!(err.code(), XmlErrorCode::NestingTooDeep);
    }

    #[test]
    fn billion_laughs_is_contained_by_entity_budget() {
        let xml = concat!(
            "<?xml version=\"1.0\"?>\n",
            "<!DOCTYPE lolz [\n",
            "  <!ENTITY lol \"lol\">\n",
            "  <!ENTITY lol1 \"&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;\">\n",
            "  <!ENTITY lol2 \"&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;\">\n",
            "  <!ENTITY lol3 \"&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;\">\n",
            "  <!ENTITY lol4 \"&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;\">\n",
            "  <!ENTITY lol5 \"&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;\">\n",
            "  <!ENTITY lol6 \"&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;\">\n",
            "  <!ENTITY lol7 \"&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;\">\n",
            "  <!ENTITY lol8 \"&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;\">\n",
            "]>\n",
            "<lolz>&lol8;</lolz>\n",
        );
        let err = parse_xml_bytes(xml.as_bytes(), ParseOptions::default()).unwrap_err();
        assert_eq!(err.code(), XmlErrorCode::EntityBudgetExceeded);
        assert!(err.location().is_some());
    }

    #[test]
    fn external_entities_are_never_loaded() {
        let xml = concat!(
            "<?xml version=\"1.0\"?>\n",
            "<!DOCTYPE doc [\n",
            "  <!ENTITY secret SYSTEM \"file:///etc/passwd\">",
            "]>\n",
            "<doc>&secret;</doc>\n",
        );
        let err = parse_xml_bytes(xml.as_bytes(), ParseOptions::default()).unwrap_err();
        assert_eq!(err.code(), XmlErrorCode::EntityUndefined);
    }

    #[test]
    fn source_ranges_and_locations_survive_into_static() {
        let doc = parse_xml_bytes(DOC.as_bytes(), ParseOptions::default()).unwrap();
        let dom = doc.document();
        let root = dom.document_element().unwrap();
        // A node's range spans its entire subtree.
        assert_eq!(doc.node_source(root).unwrap(), DOC);
        let book = dom.children(root)[0];
        assert_eq!(doc.node_source(book).unwrap(), "<book id=\"1\">text</book>");

        let location = doc
            .source_location(doc.node_range(book).unwrap().start)
            .unwrap();
        assert_eq!(location.line, 1);
        assert_eq!(location.column, "<catalog>".chars().count() + 1);
    }

    #[test]
    fn dom_preserves_all_node_kinds() {
        let xml = concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
            "<!DOCTYPE r [\n<!ENTITY e \"v\">]>\n",
            "<?target data?>\n",
            "<r xmlns:p=\"urn:p\"><!--c--><p:c/><![CDATA[x]]>&e;<?pi?></r>",
        );
        let doc = parse_xml_bytes(xml.as_bytes(), ParseOptions::default()).unwrap();
        let dom = doc.document();
        assert!(dom.xml_declaration.is_some());
        assert!(dom.doctype.as_deref().unwrap().contains("!ENTITY e"));
        let root = dom.document_element().unwrap();
        let kinds: Vec<&NodeKind> = dom
            .children(root)
            .iter()
            .map(|id| dom.node_kind(*id).unwrap())
            .collect();
        assert!(kinds.iter().any(|k| matches!(k, NodeKind::Comment(_))));
        assert!(kinds.iter().any(|k| matches!(k, NodeKind::CData(_))));
        assert!(
            kinds
                .iter()
                .any(|k| matches!(k, NodeKind::ProcessingInstruction(_)))
        );
        // Entity reference expanded into text.
        assert!(
            kinds
                .iter()
                .any(|k| matches!(k, NodeKind::Text(t) if t == "v"))
        );
    }
}
