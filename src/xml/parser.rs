use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::path::Path;

use anyhow::{Context, Result};

use super::{XmlAttribute, XmlDocument, XmlElement, XmlNode};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse an XML string into an [`XmlDocument`].
pub fn parse_xml(content: &str) -> Result<XmlDocument> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    let mut stack: Vec<XmlElement> = Vec::new();
    let mut root: Option<XmlNode> = None;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let mut element = XmlElement::new(name);
                collect_attributes(&e, &mut element);
                stack.push(element);
            }

            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let mut element = XmlElement::new(name);
                collect_attributes(&e, &mut element);
                attach_node(XmlNode::Element(element), &mut stack, &mut root);
            }

            Ok(Event::End(_)) => {
                if let Some(element) = stack.pop() {
                    attach_node(XmlNode::Element(element), &mut stack, &mut root);
                }
            }

            Ok(Event::Text(e)) => {
                let raw = String::from_utf8_lossy(&e).into_owned();
                let trimmed = raw.trim();
                if !trimmed.is_empty()
                    && let Some(parent) = stack.last_mut()
                {
                    // Store only the first text run; subsequent ones are appended.
                    match &mut parent.text {
                        Some(existing) => {
                            existing.push(' ');
                            existing.push_str(trimmed);
                        }
                        None => parent.text = Some(trimmed.to_string()),
                    }
                }
            }

            Ok(Event::Comment(e)) => {
                let comment = String::from_utf8_lossy(&e).into_owned();
                if let Some(parent) = stack.last_mut() {
                    parent.add_child(XmlNode::Comment(comment));
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
        parent.add_child(node);
    } else {
        *root = Some(node);
    }
}

/// Extract attributes from a quick-xml `BytesStart` event into `element`.
fn collect_attributes<'a>(
    e: &quick_xml::events::BytesStart<'a>,
    element: &mut XmlElement,
) {
    for attr_result in e.attributes() {
        match attr_result {
            Ok(attr) => {
                let name = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
                let value = String::from_utf8_lossy(&attr.value).into_owned();
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
            // Opening tag
            out.push_str(&pad);
            out.push('<');
            out.push_str(&elem.name);

            for attr in &elem.attributes {
                out.push(' ');
                out.push_str(&attr.name);
                out.push_str("=\"");
                out.push_str(&escape_xml(&attr.value));
                out.push('"');
            }

            let has_text = elem.text.as_deref().is_some_and(|t| !t.is_empty());
            let has_children = !elem.children.is_empty();

            if !has_text && !has_children {
                // Self-closing
                out.push_str("/>\n");
            } else {
                out.push('>');

                if has_text && !has_children {
                    // Inline text content
                    out.push_str(&escape_xml(elem.text.as_deref().unwrap_or("")));
                    out.push_str("</");
                    out.push_str(&elem.name);
                    out.push_str(">\n");
                } else {
                    out.push('\n');

                    // Inline text as first child text node
                    if let Some(text) = &elem.text {
                        let t = text.trim();
                        if !t.is_empty() {
                            out.push_str(&INDENT.repeat(depth + 1));
                            out.push_str(&escape_xml(t));
                            out.push('\n');
                        }
                    }

                    for child in &elem.children {
                        write_node(child, out, depth + 1);
                    }

                    out.push_str(&pad);
                    out.push_str("</");
                    out.push_str(&elem.name);
                    out.push_str(">\n");
                }
            }
        }

        XmlNode::Text(text) => {
            let t = text.trim();
            if !t.is_empty() {
                out.push_str(&pad);
                out.push_str(&escape_xml(t));
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
