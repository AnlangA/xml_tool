/// Export XML documents to various formats
use anyhow::Result;
use serde_json::{json, Value};

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
        
        for child in &elem.children {
            if let XmlNode::Element(child_elem) = child {
                children_map
                    .entry(child_elem.name.clone())
                    .or_default()
                    .push(element_to_json(child_elem));
            }
        }
        
        // Convert children map to JSON
        for (name, values) in children_map {
            let value = if values.len() == 1 {
                values.into_iter().next().unwrap()
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
}
