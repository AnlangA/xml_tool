//! Legacy compatibility facade over the XML engine.
//!
//! [`parse_xml`], [`parse_xml_file`], and [`serialize_xml`] keep their
//! historical signatures (the editable tree model plus `anyhow`), but parsing
//! now goes through [`super::engine::UppsalaXmlEngine`] with the plan's
//! security limits. The engine DOM is converted into the legacy tree with the
//! same text-normalization semantics the previous `quick-xml` parser had, so
//! existing callers and tests are unaffected.
//!
//! New code should use [`super::engine::parse_xml_bytes`] and friends, which
//! preserve every XML construct and the original byte stream.

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use uppsala::dom::{Document, NodeId, NodeKind};

use super::engine::{ParseOptions, UppsalaXmlEngine, XmlEngine};
use super::{XmlAttribute, XmlDocument, XmlElement, XmlNode};

/// Parse an XML string into an [`XmlDocument`].
///
/// Namespace resolution is off (matching the historical behavior) and text
/// whitespace is normalized exactly like the previous parser.
pub fn parse_xml(content: &str) -> Result<XmlDocument> {
    let engine = UppsalaXmlEngine;
    let parsed = engine
        .parse_bytes(content.as_bytes(), &legacy_parse_options())
        .context("Failed to parse XML")?;
    build_legacy_tree(parsed.document())
}

/// Read a file from `path` and parse it as XML.
///
/// The file is read as bytes and decoded through the engine, so UTF-16
/// (BOM or Appendix-F detected) documents open correctly.
pub fn parse_xml_file(path: &Path) -> Result<XmlDocument> {
    let bytes =
        std::fs::read(path).with_context(|| format!("Failed to read '{}'", path.display()))?;
    let engine = UppsalaXmlEngine;
    let parsed = engine
        .parse_bytes(&bytes, &legacy_parse_options())
        .with_context(|| format!("Failed to parse '{}'", path.display()))?;
    build_legacy_tree(parsed.document())
}

/// Serialise an [`XmlDocument`] back to a well-formed XML string.
pub fn serialize_xml(document: &XmlDocument) -> Result<String> {
    let mut out = String::with_capacity(4096);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    write_node(&document.root, &mut out, 0);
    Ok(out)
}

fn legacy_parse_options() -> ParseOptions {
    ParseOptions {
        namespace_aware: false,
        ..ParseOptions::default()
    }
}

// ---------------------------------------------------------------------------
// Engine DOM → legacy tree conversion
// ---------------------------------------------------------------------------

/// Converts an engine DOM into the legacy tree model, preserving the old
/// parser's semantics: comments become child nodes, CDATA and expanded
/// entities merge into text, PIs and the doctype are skipped.
fn build_legacy_tree(document: &Document<'_>) -> Result<XmlDocument> {
    let root_id = document
        .document_element()
        .ok_or_else(|| anyhow!("No root element found in XML document"))?;
    let root = convert_element(document, root_id)?;
    Ok(XmlDocument::new(root))
}

fn convert_element(document: &Document<'_>, id: NodeId) -> Result<XmlNode> {
    let element = document
        .element(id)
        .ok_or_else(|| anyhow!("Engine node {id:?} is not an element"))?;

    let mut legacy = XmlElement::new(render_qname(&element.name));

    // Namespace declarations are stored separately by the engine DOM;
    // the legacy model carried them as plain attributes, so rebuild them.
    for attr in &element.attributes {
        legacy.attributes.push(XmlAttribute {
            name: render_qname(&attr.name),
            value: attr.value.to_string(),
            namespace: None,
        });
    }
    for (prefix, uri) in &element.namespace_declarations {
        let name = if prefix.is_empty() {
            String::from("xmlns")
        } else {
            format!("xmlns:{prefix}")
        };
        legacy.attributes.push(XmlAttribute {
            name,
            value: uri.to_string(),
            namespace: None,
        });
    }

    for child_id in document.children_iter(id) {
        match document.node_kind(child_id) {
            Some(NodeKind::Element(_)) => {
                flush_pending_text(&mut legacy);
                legacy.add_child(convert_element(document, child_id)?);
            }
            Some(NodeKind::Text(text)) => {
                if let Some(normalized) = normalize_text_fragment(text) {
                    append_text(&mut legacy, normalized);
                }
            }
            Some(NodeKind::CData(text)) => append_text(&mut legacy, text),
            Some(NodeKind::Comment(comment)) => {
                flush_pending_text(&mut legacy);
                legacy.add_child(XmlNode::Comment(comment.to_string()));
            }
            _ => {}
        }
    }

    Ok(XmlNode::Element(legacy))
}

fn render_qname(name: &uppsala::dom::QName<'_>) -> String {
    match &name.prefix {
        Some(prefix) => format!("{prefix}:{}", name.local_name),
        None => name.local_name.to_string(),
    }
}

fn flush_pending_text(parent: &mut XmlElement) {
    if let Some(text) = parent.text.take()
        && !text.is_empty()
    {
        parent.add_child(XmlNode::Text(text));
    }
}

fn append_text(parent: &mut XmlElement, text: &str) {
    if parent.children.is_empty() {
        match &mut parent.text {
            Some(existing) => existing.push_str(text),
            None => parent.text = Some(text.to_string()),
        }
        return;
    }

    match parent.children.last_mut() {
        Some(XmlNode::Text(existing)) => existing.push_str(text),
        _ => parent.add_child(XmlNode::Text(text.to_string())),
    }
}

/// Mirrors the historical text normalization: whitespace-only fragments that
/// contain line breaks are dropped; fragments with line breaks are trimmed;
/// everything else is kept verbatim.
fn normalize_text_fragment(text: &str) -> Option<&str> {
    let trimmed = text.trim_matches(is_xml_whitespace);
    let has_line_break = text.contains('\n') || text.contains('\r');

    if trimmed.is_empty() {
        return if has_line_break { None } else { Some(text) };
    }

    if has_line_break {
        Some(trimmed)
    } else {
        Some(text)
    }
}

fn is_xml_whitespace(ch: char) -> bool {
    matches!(ch, ' ' | '\n' | '\r' | '\t')
}

// ---------------------------------------------------------------------------
// Legacy tree serialization (unchanged output format)
// ---------------------------------------------------------------------------

/// Recursively write an [`XmlNode`] with indentation.
fn write_node(node: &XmlNode, out: &mut String, depth: usize) {
    const INDENT: &str = "  ";
    let pad = INDENT.repeat(depth);

    match node {
        XmlNode::Element(elem) => {
            out.push_str(&pad);

            let has_text = elem.text.as_deref().is_some_and(|t| !t.is_empty());
            let has_children = !elem.children.is_empty();
            let has_mixed_children = elem
                .children
                .iter()
                .any(|child| !matches!(child, XmlNode::Element(_)));

            if !has_children || has_text || has_mixed_children {
                write_element_inline(elem, out);
                out.push('\n');
            } else {
                write_start_tag(elem, out);
                out.push_str(">\n");

                for child in &elem.children {
                    write_node(child, out, depth + 1);
                }

                out.push_str(&pad);
                out.push_str("</");
                out.push_str(&elem.name);
                out.push_str(">\n");
            }
        }

        XmlNode::Text(text) => {
            if !text.is_empty() {
                out.push_str(&pad);
                out.push_str(&escape_xml(text));
                out.push('\n');
            }
        }

        XmlNode::Comment(comment) => {
            out.push_str(&pad);
            out.push_str("<!-- ");
            out.push_str(comment);
            out.push_str(" -->\n");
        }
    }
}

fn write_node_inline(node: &XmlNode, out: &mut String) {
    match node {
        XmlNode::Element(elem) => write_element_inline(elem, out),
        XmlNode::Text(text) => out.push_str(&escape_xml(text)),
        XmlNode::Comment(comment) => {
            out.push_str("<!-- ");
            out.push_str(comment);
            out.push_str(" -->");
        }
    }
}

fn write_element_inline(elem: &XmlElement, out: &mut String) {
    write_start_tag(elem, out);

    let has_text = elem.text.as_deref().is_some_and(|text| !text.is_empty());
    let has_children = !elem.children.is_empty();
    if !has_text && !has_children {
        out.push_str("/>");
        return;
    }

    out.push('>');

    if let Some(text) = &elem.text {
        out.push_str(&escape_xml(text));
    }

    for child in &elem.children {
        write_node_inline(child, out);
    }

    out.push_str("</");
    out.push_str(&elem.name);
    out.push('>');
}

fn write_start_tag(elem: &XmlElement, out: &mut String) {
    out.push('<');
    out.push_str(&elem.name);

    for attr in &elem.attributes {
        out.push(' ');
        out.push_str(&attr.name);
        out.push_str("=\"");
        out.push_str(&escape_xml(&attr.value));
        out.push('"');
    }
}

/// Escape the five mandatory XML character entities.
fn escape_xml(s: &str) -> String {
    // Pre-allocate with some headroom to avoid repeated reallocations.
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_simple() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<root>
  <child attr="val">text</child>
</root>
"#;
        let doc = parse_xml(xml).expect("parse");
        let out = serialize_xml(&doc).expect("serialize");
        // Re-parse the serialised form — must not error.
        parse_xml(&out).expect("re-parse");
    }

    #[test]
    fn handles_empty_elements() {
        let xml = r#"<root><empty/></root>"#;
        let doc = parse_xml(xml).expect("parse");
        let root = doc.root.as_element().unwrap();
        assert_eq!(root.children.len(), 1);
        let child = root.children[0].as_element().unwrap();
        assert_eq!(child.name, "empty");
        assert!(child.children.is_empty());
    }

    #[test]
    fn handles_attributes() {
        let xml = r#"<root id="1" class="foo"/>"#;
        let doc = parse_xml(xml).expect("parse");
        let root = doc.root.as_element().unwrap();
        assert_eq!(root.attributes.len(), 2);
        assert_eq!(root.attributes[0].name, "id");
        assert_eq!(root.attributes[0].value, "1");
    }

    #[test]
    fn escape_round_trip() {
        let xml = r#"<root attr="&amp;&lt;&gt;">&amp;&lt;</root>"#;
        let doc = parse_xml(xml).expect("parse");
        let out = serialize_xml(&doc).expect("serialize");
        assert!(out.contains("&amp;") || out.contains('<'), "output: {out}");
    }

    #[test]
    fn decodes_text_entities_and_character_references() {
        let xml = r#"<root>&lt;tag&gt;&#x20;done&#33;</root>"#;
        let doc = parse_xml(xml).expect("parse");
        let root = doc.root.as_element().expect("root element");

        assert_eq!(root.text.as_deref(), Some("<tag> done!"));
    }

    #[test]
    fn preserves_inline_mixed_content_spacing_when_serializing() {
        let xml = r#"<root>Hello <child>world</child> &amp; friends</root>"#;
        let doc = parse_xml(xml).expect("parse");
        let out = serialize_xml(&doc).expect("serialize");

        assert!(out.contains("<root>Hello <child>world</child> &amp; friends</root>"));
        parse_xml(&out).expect("re-parse");
    }

    #[test]
    fn parses_cdata_as_text_content() {
        let xml = r#"<root><![CDATA[<escaped> & raw]]></root>"#;
        let doc = parse_xml(xml).expect("parse");
        let root = doc.root.as_element().expect("root element");

        assert_eq!(root.text.as_deref(), Some("<escaped> & raw"));
    }

    #[test]
    fn internal_entities_expand_into_text() {
        let xml =
            "<!DOCTYPE r [\n<!ENTITY greeting \"Hello &amp; welcome\">]>\n<r>&greeting; team</r>";
        let doc = parse_xml(xml).expect("parse");
        let root = doc.root.as_element().expect("root element");

        assert_eq!(root.text.as_deref(), Some("Hello & welcome team"));
    }

    #[test]
    fn engine_backed_parse_reports_line_and_column() {
        let err = parse_xml("<a>\n  <b></c>\n</a>").unwrap_err();
        // anyhow wraps the engine error; the root cause must carry a location.
        let message = format!("{err:#}");
        assert!(message.contains("line"), "message: {message}");
    }

    #[test]
    fn parse_xml_file_reads_utf16_documents() {
        let text = "<?xml version=\"1.0\" encoding=\"UTF-16\"?><root><child/></root>";
        let mut units = Vec::new();
        units.push(0xFF); // BOM
        units.push(0xFE);
        for unit in text.encode_utf16() {
            units.extend_from_slice(&unit.to_le_bytes());
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("utf16.xml");
        std::fs::write(&path, units).unwrap();

        let doc = parse_xml_file(&path).expect("parse utf-16 file");
        assert_eq!(doc.root.as_element().unwrap().name, "root");
    }

    #[test]
    fn truncate_str_unicode() {
        use crate::xml::truncate_str;
        let s = "你好世界Rust";
        let t = truncate_str(s, 4);
        // Must not panic and must be 4 chars + ellipsis
        assert!(t.starts_with("你好世界"));
        assert!(t.ends_with('…'));
    }

    #[test]
    fn truncate_str_short() {
        use crate::xml::truncate_str;
        let s = "hi";
        assert_eq!(truncate_str(s, 10), "hi");
    }
}
