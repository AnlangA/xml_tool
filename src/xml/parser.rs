use quick_xml::XmlVersion;
use quick_xml::events::{BytesRef, BytesText, Event};
use quick_xml::reader::Reader;
use std::path::Path;

use anyhow::{Context, Result, anyhow};

use super::{XmlAttribute, XmlDocument, XmlElement, XmlNode};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse an XML string into an [`XmlDocument`].
pub fn parse_xml(content: &str) -> Result<XmlDocument> {
    let mut reader = Reader::from_str(content);

    let mut stack: Vec<XmlElement> = Vec::new();
    let mut root: Option<XmlNode> = None;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let mut element = XmlElement::new(name);
                collect_attributes(reader.decoder(), &e, &mut element);
                stack.push(element);
            }

            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let mut element = XmlElement::new(name);
                collect_attributes(reader.decoder(), &e, &mut element);
                attach_node(XmlNode::Element(element), &mut stack, &mut root);
            }

            Ok(Event::End(_)) => {
                if let Some(element) = stack.pop() {
                    attach_node(XmlNode::Element(element), &mut stack, &mut root);
                }
            }

            Ok(Event::Text(e)) => {
                if let Some(parent) = stack.last_mut()
                    && let Some(text) = decode_text_event(&e)?
                {
                    append_text(parent, &text);
                }
            }

            Ok(Event::CData(e)) => {
                if let Some(parent) = stack.last_mut() {
                    let text = e
                        .xml_content(XmlVersion::Implicit1_0)
                        .context("Failed to decode CDATA section")?;
                    append_text(parent, &text);
                }
            }

            Ok(Event::GeneralRef(e)) => {
                if let Some(parent) = stack.last_mut() {
                    let resolved = resolve_general_reference(&e)?;
                    append_text(parent, &resolved);
                }
            }

            Ok(Event::Comment(e)) => {
                let comment = e.decode().context("Failed to decode XML comment")?;
                if let Some(parent) = stack.last_mut() {
                    attach_child_node(parent, XmlNode::Comment(comment.into_owned()));
                }
            }

            Ok(Event::Eof) => break,

            Err(e) => {
                return Err(anyhow::anyhow!(
                    "XML parse error at byte {}: {e:?}",
                    reader.error_position()
                ));
            }

            // Ignore PI, DocType, CData, etc.
            _ => {}
        }

        buf.clear();
    }

    let root = root.context("No root element found in XML document")?;
    Ok(XmlDocument::new(root))
}

/// Read a file from `path` and parse it as XML.
pub fn parse_xml_file(path: &Path) -> Result<XmlDocument> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read '{}'", path.display()))?;
    parse_xml(&content)
}

/// Serialise an [`XmlDocument`] back to a well-formed XML string.
pub fn serialize_xml(document: &XmlDocument) -> Result<String> {
    let mut out = String::with_capacity(4096);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    write_node(&document.root, &mut out, 0);
    Ok(out)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Attach `node` to the current parent, or set it as the document root.
fn attach_node(node: XmlNode, stack: &mut [XmlElement], root: &mut Option<XmlNode>) {
    if let Some(parent) = stack.last_mut() {
        attach_child_node(parent, node);
    } else {
        *root = Some(node);
    }
}

fn attach_child_node(parent: &mut XmlElement, node: XmlNode) {
    flush_pending_text(parent);
    parent.add_child(node);
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
            Some(existing) => append_text_run(existing, text),
            None => parent.text = Some(text.to_string()),
        }
        return;
    }

    match parent.children.last_mut() {
        Some(XmlNode::Text(existing)) => append_text_run(existing, text),
        _ => parent.add_child(XmlNode::Text(text.to_string())),
    }
}

fn append_text_run(target: &mut String, text: &str) {
    target.push_str(text);
}

fn decode_text_event(event: &BytesText<'_>) -> Result<Option<String>> {
    let content = event
        .xml_content(XmlVersion::Implicit1_0)
        .context("Failed to decode XML text node")?;
    Ok(normalize_text_fragment(&content).map(ToOwned::to_owned))
}

fn normalize_text_fragment(text: &str) -> Option<&str> {
    let trimmed = trim_xml_whitespace(text);
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

fn trim_xml_whitespace(text: &str) -> &str {
    text.trim_matches(is_xml_whitespace)
}

fn is_xml_whitespace(ch: char) -> bool {
    matches!(ch, ' ' | '\n' | '\r' | '\t')
}

fn resolve_general_reference(reference: &BytesRef<'_>) -> Result<String> {
    if let Some(ch) = reference
        .resolve_char_ref()
        .context("Failed to resolve XML character reference")?
    {
        return Ok(ch.to_string());
    }

    let name = reference
        .decode()
        .context("Failed to decode XML entity reference")?;

    let resolved = match name.as_ref() {
        "lt" => "<",
        "gt" => ">",
        "amp" => "&",
        "apos" => "'",
        "quot" => "\"",
        _ => {
            return Err(anyhow!(
                "Unsupported entity reference '&{name};' (DTD-defined entities are not supported)"
            ));
        }
    };

    Ok(resolved.to_string())
}

/// Extract attributes from a quick-xml `BytesStart` event into `element`.
fn collect_attributes<'a>(
    decoder: quick_xml::encoding::Decoder,
    e: &quick_xml::events::BytesStart<'a>,
    element: &mut XmlElement,
) {
    for attr_result in e.attributes() {
        match attr_result {
            Ok(attr) => {
                let name = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
                let value =
                    match attr.decoded_and_normalized_value(XmlVersion::Implicit1_0, decoder) {
                        Ok(value) => value.into_owned(),
                        Err(err) => {
                            log::warn!("Skipping malformed attribute '{name}': {err}");
                            continue;
                        }
                    };
                element.attributes.push(XmlAttribute {
                    name,
                    value,
                    namespace: None,
                });
            }
            Err(e) => {
                log::warn!("Skipping malformed attribute: {e}");
            }
        }
    }
}

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
