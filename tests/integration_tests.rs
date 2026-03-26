use xml_tool::exi::{decode_exi_to_xml, encode_xml_to_exi};
use xml_tool::export::export_to_json;
use xml_tool::xml::{XmlNode, parse_xml, serialize_xml};

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
    let root = doc.root.as_element().unwrap();

    assert!(root.text.is_none());
    assert_eq!(root.children.len(), 3);

    assert!(matches!(&root.children[0], XmlNode::Text(text) if text == "Text before"));

    let child = root.children[1].as_element().unwrap();
    assert_eq!(child.name, "child");
    assert_eq!(child.text.as_deref(), Some("child text"));

    assert!(matches!(&root.children[2], XmlNode::Text(text) if text == "Text after"));
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
    assert_eq!(root.text.as_deref(), Some("<escaped>"));
}

#[test]
fn test_cdata_content() {
    let xml = r#"<root><![CDATA[<raw> & text]]></root>"#;

    let doc = parse_xml(xml).unwrap();
    let root = doc.root.as_element().unwrap();

    assert_eq!(root.text.as_deref(), Some("<raw> & text"));
}

#[test]
fn test_inline_mixed_content_spacing() {
    let xml = r#"<root>Hello <child>world</child> &amp; friends</root>"#;

    let doc = parse_xml(xml).unwrap();
    let root = doc.root.as_element().unwrap();

    assert!(root.text.is_none());
    assert_eq!(root.children.len(), 3);
    assert!(matches!(&root.children[0], XmlNode::Text(text) if text == "Hello "));
    assert_eq!(root.children[1].as_element().unwrap().name, "child");
    assert!(matches!(&root.children[2], XmlNode::Text(text) if text == " & friends"));
}

#[test]
fn test_doctype_and_processing_instructions_are_tolerated() {
    let xml = r#"<?xml version="1.0"?>
<!DOCTYPE root>
<root>
    <?process ignored?>
    <child>text</child>
</root>"#;

    let doc = parse_xml(xml).unwrap();
    let root = doc.root.as_element().unwrap();

    assert_eq!(root.name, "root");
    assert_eq!(root.children.len(), 1);
}

#[test]
fn test_prefixed_names_are_preserved() {
    let xml = r#"<ns:root xmlns:ns="urn:test"><ns:child ns:attr="value"/></ns:root>"#;

    let doc = parse_xml(xml).unwrap();
    let root = doc.root.as_element().unwrap();
    let child = root.children[0].as_element().unwrap();

    assert_eq!(root.name, "ns:root");
    assert_eq!(child.name, "ns:child");
    assert_eq!(child.attributes[0].name, "ns:attr");
}

#[test]
fn test_editable_document_mutations_round_trip() {
    let mut doc = parse_xml(r#"<root><child status="old">text</child></root>"#).unwrap();
    let child_id = doc.root.as_element().unwrap().children[0]
        .as_element()
        .unwrap()
        .id
        .0;

    assert!(doc.rename_element(child_id, "renamed".to_string()).unwrap());
    assert!(doc.set_attribute_value(child_id, "status", "new".to_string()));
    assert!(
        doc.set_text_content(child_id, Some("updated".to_string()))
            .unwrap()
    );

    let serialized = serialize_xml(&doc).unwrap();
    assert!(serialized.contains(r#"<renamed status="new">updated</renamed>"#));

    let reparsed = parse_xml(&serialized).unwrap();
    let root = reparsed.root.as_element().unwrap();
    let child = root.children[0].as_element().unwrap();
    assert_eq!(child.name, "renamed");
    assert_eq!(child.attributes[0].value, "new");
    assert_eq!(child.text.as_deref(), Some("updated"));
}

#[test]
fn test_structural_document_mutations_round_trip() {
    let mut doc = parse_xml(r#"<root><parent>hello</parent></root>"#).unwrap();
    let parent_id = doc.root.as_element().unwrap().children[0]
        .as_element()
        .unwrap()
        .id
        .0;

    assert!(
        doc.add_attribute(parent_id, "lang".to_string(), "en".to_string())
            .unwrap()
    );
    let child_id = doc
        .append_child_element(parent_id, "child".to_string())
        .unwrap()
        .unwrap();

    let parent = doc.find_element(parent_id).unwrap();
    assert!(parent.text.is_none());
    assert!(matches!(&parent.children[0], XmlNode::Text(text) if text == "hello"));
    assert_eq!(parent.children[1].as_element().unwrap().id.0, child_id);

    assert!(doc.remove_attribute(parent_id, "lang"));
    assert_eq!(doc.remove_element(child_id).unwrap(), Some(parent_id));

    let serialized = serialize_xml(&doc).unwrap();
    assert!(serialized.contains("<parent>hello</parent>"));
    assert!(!serialized.contains("lang=\"en\""));
}

#[test]
fn test_reordered_siblings_round_trip() {
    let mut doc = parse_xml(r#"<root><alpha/><beta/></root>"#).unwrap();
    let alpha_id = doc.root.as_element().unwrap().children[0]
        .as_element()
        .unwrap()
        .id
        .0;
    let beta_id = doc.root.as_element().unwrap().children[1]
        .as_element()
        .unwrap()
        .id
        .0;

    let gamma_id = doc
        .insert_sibling_element_after(alpha_id, "gamma".to_string())
        .unwrap()
        .unwrap();
    assert!(doc.move_element_up(beta_id).unwrap());

    let serialized = serialize_xml(&doc).unwrap();
    let reparsed = parse_xml(&serialized).unwrap();
    let root = reparsed.root.as_element().unwrap();
    let child_names: Vec<&str> = root
        .children
        .iter()
        .map(|child| child.as_element().unwrap().name.as_str())
        .collect();

    assert_eq!(child_names, vec!["alpha", "beta", "gamma"]);
    assert!(doc.find_element(gamma_id).is_some());
}

#[test]
fn test_reordering_mixed_content_is_rejected() {
    let mut doc = parse_xml(r#"<root>Hello <first/><second/></root>"#).unwrap();
    let first_id = doc.root.as_element().unwrap().children[1]
        .as_element()
        .unwrap()
        .id
        .0;

    let err = doc.move_element_down(first_id).unwrap_err();
    assert!(err.to_string().contains("only element children"));
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
    assert!(
        duration.as_millis() < 100,
        "Parsing took too long: {:?}",
        duration
    );
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
fn test_export_to_json_preserves_mixed_content_order() {
    let xml = r#"<root>Hello <child>world</child><!--note--> &amp; friends</root>"#;

    let doc = parse_xml(xml).unwrap();
    let json: serde_json::Value = serde_json::from_str(&export_to_json(&doc).unwrap()).unwrap();
    let content = json
        .get("@content")
        .and_then(serde_json::Value::as_array)
        .expect("mixed content array");

    assert_eq!(content[0]["@text"], "Hello ");
    assert_eq!(content[1]["child"]["@text"], "world");
    assert_eq!(content[2]["@comment"], "note");
    assert_eq!(content[3]["@text"], " & friends");
}

#[test]
fn test_exi_round_trip_preserves_comments_and_prefixes() {
    let xml = r#"<ns:root xmlns:ns="urn:test"><!--note--><ns:child ns:attr="value">text</ns:child></ns:root>"#;

    let exi = encode_xml_to_exi(xml).unwrap();
    let decoded = decode_exi_to_xml(&exi).unwrap();
    let root = decoded.root.as_element().unwrap();

    assert_eq!(root.name, "ns:root");
    assert!(root.attributes.iter().any(|attr| attr.name == "xmlns:ns"));
    assert!(matches!(&root.children[0], XmlNode::Comment(comment) if comment == "note"));

    let child = root.children[1].as_element().unwrap();
    assert_eq!(child.name, "ns:child");
    assert_eq!(child.attributes[0].name, "ns:attr");
    assert_eq!(child.attributes[0].value, "value");
    assert_eq!(child.text.as_deref(), Some("text"));
}

#[test]
fn test_exi_round_trip_preserves_document_structure() {
    let xml = r#"<root><parent><!--note--><child attr="value">text</child></parent></root>"#;

    let exi = encode_xml_to_exi(xml).unwrap();
    let decoded = decode_exi_to_xml(&exi).unwrap();
    let serialized = serialize_xml(&decoded).unwrap();

    assert!(serialized.contains("<!-- note -->"));
    assert!(serialized.contains(r#"<child attr="value">text</child>"#));
}

#[test]
fn test_invalid_exi_is_reported() {
    let err = decode_exi_to_xml(b"not exi").unwrap_err();
    let message = err.to_string();

    assert!(message.contains("failed to decode EXI stream"));
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
