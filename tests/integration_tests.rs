use xml_tool::xml::{parse_xml, serialize_xml, XmlDocument};
use xml_tool::export::export_to_json;

#[test]
fn test_parse_simple_xml() {
    let xml = r#"<?xml version="1.0"?>
<root>
    <child>text</child>
</root>"#;
    
    let result = parse_xml(xml);
    assert!(result.is_ok());
    
    let doc = result.unwrap();
    assert!(doc.root.as_element().is_some());
}

#[test]
fn test_parse_with_attributes() {
    let xml = r#"<root id="1" name="test">
    <child attr="value">content</child>
</root>"#;
    
    let doc = parse_xml(xml).unwrap();
    let root = doc.root.as_element().unwrap();
    
    assert_eq!(root.attributes.len(), 2);
    assert_eq!(root.attributes[0].name, "id");
    assert_eq!(root.attributes[0].value, "1");
}

#[test]
fn test_parse_nested_structure() {
    let xml = r#"<root>
    <level1>
        <level2>
            <level3>deep content</level3>
        </level2>
    </level1>
</root>"#;
    
    let doc = parse_xml(xml).unwrap();
    let root = doc.root.as_element().unwrap();
    
    assert_eq!(root.children.len(), 1);
}

#[test]
fn test_serialize_round_trip() {
    let xml = r#"<root>
    <child attr="value">text</child>
</root>"#;
    
    let doc = parse_xml(xml).unwrap();
    let serialized = serialize_xml(&doc).unwrap();
    let doc2 = parse_xml(&serialized).unwrap();
    
    // Should be able to parse the serialized output
    assert!(doc2.root.as_element().is_some());
}

#[test]
fn test_empty_elements() {
    let xml = r#"<root>
    <empty/>
    <also-empty></also-empty>
</root>"#;
    
    let doc = parse_xml(xml).unwrap();
    let root = doc.root.as_element().unwrap();
    
    assert_eq!(root.children.len(), 2);
}

#[test]
fn test_comments() {
    let xml = r#"<root>
    <!-- This is a comment -->
    <child>text</child>
</root>"#;
    
    let doc = parse_xml(xml).unwrap();
    let root = doc.root.as_element().unwrap();
    
    // Should have comment and child
    assert_eq!(root.children.len(), 2);
}

#[test]
fn test_text_content() {
    let xml = r#"<root>
    <child>This is text content</child>
</root>"#;
    
    let doc = parse_xml(xml).unwrap();
    let root = doc.root.as_element().unwrap();
    let child = root.children[0].as_element().unwrap();
    
    assert!(child.text.is_some());
    assert_eq!(child.text.as_ref().unwrap(), "This is text content");
}

#[test]
fn test_mixed_content() {
    let xml = r#"<root>
    Text before
    <child>child text</child>
    Text after
</root>"#;
    
    let doc = parse_xml(xml).unwrap();
    assert!(doc.root.as_element().is_some());
}

#[test]
fn test_special_characters() {
    let xml = r#"<root attr="&lt;&gt;&amp;&quot;&apos;">
    &lt;escaped&gt;
</root>"#;
    
    let doc = parse_xml(xml).unwrap();
    let root = doc.root.as_element().unwrap();
    
    // Attributes should be unescaped
    assert!(root.attributes[0].value.contains('<'));
    assert!(root.attributes[0].value.contains('>'));
}

#[test]
fn test_unicode_content() {
    let xml = r#"<root>
    <chinese>你好世界</chinese>
    <emoji>🚀✨🎨</emoji>
    <mixed>Hello 世界 🌍</mixed>
</root>"#;
    
    let doc = parse_xml(xml).unwrap();
    let root = doc.root.as_element().unwrap();
    
    assert_eq!(root.children.len(), 3);
}

#[test]
fn test_large_document() {
    let mut xml = String::from("<root>");
    
    // Generate 1000 child elements
    for i in 0..1000 {
        xml.push_str(&format!(r#"<item id="{}">{}</item>"#, i, i * 2));
    }
    
    xml.push_str("</root>");
    
    let start = std::time::Instant::now();
    let doc = parse_xml(&xml).unwrap();
    let duration = start.elapsed();
    
    let root = doc.root.as_element().unwrap();
    assert_eq!(root.children.len(), 1000);
    
    // Should parse in reasonable time (< 100ms)
    assert!(duration.as_millis() < 100, "Parsing took too long: {:?}", duration);
}

#[test]
fn test_export_to_json() {
    let xml = r#"<root>
    <child attr="value">text</child>
</root>"#;
    
    let doc = parse_xml(xml).unwrap();
    let json = export_to_json(&doc).unwrap();
    
    assert!(json.contains("child"));
    assert!(json.contains("@attributes"));
    assert!(json.contains("attr"));
}

#[test]
fn test_json_export_array() {
    let xml = r#"<root>
    <item>1</item>
    <item>2</item>
    <item>3</item>
</root>"#;
    
    let doc = parse_xml(xml).unwrap();
    let json = export_to_json(&doc).unwrap();
    
    // Multiple items with same name should become array
    assert!(json.contains('['));
}

#[test]
fn test_malformed_xml() {
    let xml = r#"<root>
    <unclosed>
</root>"#;
    
    let result = parse_xml(xml);
    assert!(result.is_err());
}

#[test]
fn test_no_root_element() {
    let xml = r#"<?xml version="1.0"?>"#;
    
    let result = parse_xml(xml);
    assert!(result.is_err());
}

#[test]
fn test_document_versioning() {
    let xml = r#"<root><child>text</child></root>"#;
    let mut doc = parse_xml(xml).unwrap();
    
    let version1 = doc.version();
    doc.increment_version();
    let version2 = doc.version();
    
    assert_eq!(version2, version1 + 1);
}

#[test]
fn test_node_id_uniqueness() {
    let xml = r#"<root>
    <child1/>
    <child2/>
    <child3/>
</root>"#;
    
    let doc = parse_xml(xml).unwrap();
    let root = doc.root.as_element().unwrap();
    
    let id1 = root.children[0].as_element().unwrap().id;
    let id2 = root.children[1].as_element().unwrap().id;
    let id3 = root.children[2].as_element().unwrap().id;
    
    // All IDs should be unique
    assert_ne!(id1, id2);
    assert_ne!(id2, id3);
    assert_ne!(id1, id3);
}

#[test]
fn test_serialize_preserves_structure() {
    let xml = r#"<root>
    <parent>
        <child attr="test">content</child>
    </parent>
</root>"#;
    
    let doc = parse_xml(xml).unwrap();
    let serialized = serialize_xml(&doc).unwrap();
    
    // Should contain all elements
    assert!(serialized.contains("<root>"));
    assert!(serialized.contains("<parent>"));
    assert!(serialized.contains("<child"));
    assert!(serialized.contains("attr=\"test\""));
    assert!(serialized.contains("content"));
}

#[test]
fn test_real_world_xml_file() {
    let xml = std::fs::read_to_string("test_files/complex-nested.xml");
    
    if let Ok(content) = xml {
        let doc = parse_xml(&content).unwrap();
        let root = doc.root.as_element().unwrap();
        
        assert_eq!(root.name, "company");
        assert!(!root.attributes.is_empty());
        assert!(!root.children.is_empty());
    }
}
