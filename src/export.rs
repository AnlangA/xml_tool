/// Export XML documents to other formats.
///
/// JSON export uses `@attributes` for attributes, `@text` for leaf text,
/// and `@content` for ordered mixed-content children when text/comments and
/// elements are interleaved.
use anyhow::Result;
use serde_json::{Value, json};

use crate::xml::{XmlDocument, XmlElement, XmlNode};

/// Export XML document to JSON format
pub fn export_to_json(doc: &XmlDocument) -> Result<String> {
    let json_value = node_to_json(&doc.root);
    Ok(serde_json::to_string_pretty(&json_value)?)
}

/// Convert an XML node to JSON value
fn node_to_json(node: &XmlNode) -> Value {
    match node {
        XmlNode::Element(elem) => element_to_json(elem),
        XmlNode::Text(text) => json!(text.trim()),
        XmlNode::Comment(_) => Value::Null, // Skip comments in JSON
    }
}

/// Convert an XML element to JSON object
fn element_to_json(elem: &XmlElement) -> Value {
    let mut obj = serde_json::Map::new();

    // Add attributes with @ prefix
    if !elem.attributes.is_empty() {
        let mut attrs = serde_json::Map::new();
        for attr in &elem.attributes {
            attrs.insert(attr.name.clone(), json!(attr.value));
        }
        obj.insert("@attributes".to_string(), Value::Object(attrs));
    }

    // Add text content
    if let Some(text) = &elem.text {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            obj.insert("@text".to_string(), json!(trimmed));
        }
    }

    // Add children
    if !elem.children.is_empty() {
        let mut children_map: std::collections::HashMap<String, Vec<Value>> =
            std::collections::HashMap::new();
        let mut ordered_content = Vec::new();
        let has_mixed_content = elem
            .children
            .iter()
            .any(|child| !matches!(child, XmlNode::Element(_)));

        for child in &elem.children {
            match child {
                XmlNode::Element(child_elem) => {
                    let child_value = element_to_json(child_elem);
                    children_map
                        .entry(child_elem.name.clone())
                        .or_default()
                        .push(child_value.clone());

                    if has_mixed_content {
                        let mut child_entry = serde_json::Map::new();
                        child_entry.insert(child_elem.name.clone(), child_value);
                        ordered_content.push(Value::Object(child_entry));
                    }
                }
                XmlNode::Text(text) => {
                    if !text.trim().is_empty() {
                        ordered_content.push(json!({ "@text": text }));
                    }
                }
                XmlNode::Comment(comment) => {
                    if has_mixed_content {
                        ordered_content.push(json!({ "@comment": comment }));
                    }
                }
            }
        }

        if has_mixed_content && !ordered_content.is_empty() {
            obj.insert("@content".to_string(), Value::Array(ordered_content));
        }

        // Convert children map to JSON
        for (name, values) in children_map {
            let value = if values.len() == 1 {
                values.into_iter().next().expect("single child value")
            } else {
                Value::Array(values)
            };
            obj.insert(name, value);
        }
    }

    // If element has no attributes, text, or children, return empty object
    if obj.is_empty() {
        return Value::Null;
    }

    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::parse_xml;

    #[test]
    fn test_export_simple_xml() {
        let xml = r#"<root><child>text</child></root>"#;
        let doc = parse_xml(xml).unwrap();
        let json = export_to_json(&doc).unwrap();
        assert!(json.contains("child"));
        assert!(json.contains("text"));
    }

    #[test]
    fn test_export_with_attributes() {
        let xml = r#"<root id="1"><child attr="value">text</child></root>"#;
        let doc = parse_xml(xml).unwrap();
        let json = export_to_json(&doc).unwrap();
        assert!(json.contains("@attributes"));
        assert!(json.contains("attr"));
    }

    #[test]
    fn test_export_mixed_content_preserves_ordered_text_and_elements() {
        let xml =
            r#"<root>Hello <child attr="value">world</child><!--note--> &amp; friends</root>"#;
        let doc = parse_xml(xml).unwrap();
        let json: Value = serde_json::from_str(&export_to_json(&doc).unwrap()).unwrap();

        let content = json
            .get("@content")
            .and_then(Value::as_array)
            .expect("ordered mixed content");

        assert_eq!(content[0]["@text"], Value::String("Hello ".to_string()));
        assert_eq!(
            content[1]["child"]["@attributes"]["attr"],
            Value::String("value".to_string())
        );
        assert_eq!(
            content[1]["child"]["@text"],
            Value::String("world".to_string())
        );
        assert_eq!(content[2]["@comment"], Value::String("note".to_string()));
        assert_eq!(content[3]["@text"], Value::String(" & friends".to_string()));
    }
}
