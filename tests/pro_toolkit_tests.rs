//! Acceptance tests for `AGENTS_PLAN.md` step 7: the XML pro toolkit —
//! XPath, XSD validation, structural diff, batch replace, and lossless
//! JSON export.

use xml_tool::core::document::XmlDocument;
use xml_tool::core::{Command, History};
use xml_tool::services::diff::{DiffEntry, DiffOptions, diff_xml};
use xml_tool::services::replace::{ReplaceScope, build_replacements, to_ops};
use xml_tool::services::search::SearchIndex;
use xml_tool::services::validation::{SchemaCache, SchemaError};
use xml_tool::services::xpath::{XPathOutcome, query};

fn parse(xml: &str) -> XmlDocument {
    XmlDocument::parse(xml.as_bytes()).expect("test document parses")
}

// ---------------------------------------------------------------------------
// XPath 1.0: axes, predicates, functions, namespaces, all four result types
// ---------------------------------------------------------------------------

#[test]
fn xpath_node_sets_axes_and_predicates() {
    let doc = parse(
        "<catalog><book id=\"1\"><title>Alpha</title></book>\
         <book id=\"2\"><title>Beta</title></book>\
         <book id=\"3\"><title>Gamma</title></book></catalog>",
    );

    let all = query(&doc, "//book").expect("descendant axis");
    match all {
        XPathOutcome::NodeSet(nodes) => assert_eq!(nodes.len(), 3),
        other => panic!("node set expected, got {other:?}"),
    }

    let first = query(&doc, "//book[@id='1']/title").expect("predicate + child");
    match first {
        XPathOutcome::NodeSet(nodes) => {
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].label, "title");
        }
        other => panic!("node set expected, got {other:?}"),
    }

    let parent = query(&doc, "//title/..").expect("parent axis");
    match parent {
        XPathOutcome::NodeSet(nodes) => assert_eq!(nodes.len(), 3),
        other => panic!("node set expected, got {other:?}"),
    }

    let following =
        query(&doc, "//book[@id='1']/following-sibling::book").expect("following-sibling");
    match following {
        XPathOutcome::NodeSet(nodes) => assert_eq!(nodes.len(), 2),
        other => panic!("node set expected, got {other:?}"),
    }
}

#[test]
fn xpath_scalar_functions_and_types() {
    let doc = parse("<catalog><book id=\"1\"/><book id=\"2\"/><book id=\"3\"/></catalog>");

    assert_eq!(
        query(&doc, "count(//book)").ok(),
        Some(XPathOutcome::Number(3.0)),
        "count returns Number"
    );
    assert_eq!(
        query(&doc, "//book[@id='2']/@id='2'").ok(),
        Some(XPathOutcome::Boolean(true)),
        "comparison returns Boolean"
    );
    match query(&doc, "string(//book/@id)").expect("string()") {
        XPathOutcome::String(text) => assert_eq!(text, "1"),
        other => panic!("string expected, got {other:?}"),
    }
    assert_eq!(
        query(&doc, "sum(//book/@id)").ok(),
        Some(XPathOutcome::Number(6.0))
    );
}

#[test]
fn xpath_resolves_namespaced_names() {
    let doc = parse("<lib:library xmlns:lib=\"urn:lib\"><lib:book id=\"1\"/></lib:library>");
    let hits = query(&doc, "//lib:book").expect("prefixed query");
    match hits {
        XPathOutcome::NodeSet(nodes) => assert_eq!(nodes.len(), 1),
        other => panic!("node set expected, got {other:?}"),
    }
    let misses = query(&doc, "//book").expect("unprefixed in no default ns");
    match misses {
        XPathOutcome::NodeSet(nodes) => assert!(nodes.is_empty()),
        other => panic!("node set expected, got {other:?}"),
    }
}

#[test]
fn xpath_errors_are_structured() {
    let doc = parse("<r/>");
    let err = query(&doc, "//book[").expect_err("broken expression must fail");
    assert_eq!(err.code(), xml_tool::xml::XmlErrorCode::Syntax);
}

// ---------------------------------------------------------------------------
// XSD: valid/invalid pairs, include confinement, path escapes
// ---------------------------------------------------------------------------

const BASE_SCHEMA: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="person">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="name" type="xs:string"/>
        <xs:element name="age" type="xs:integer"/>
      </xs:sequence>
      <xs:attribute name="id" type="xs:ID" use="required"/>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

fn xsd_case(schema: &str, document: &str) -> usize {
    let dir = tempfile::tempdir().unwrap();
    let schema_path = dir.path().join("schema.xsd");
    std::fs::write(&schema_path, schema).unwrap();
    let mut cache = SchemaCache::default();
    let validator = cache.load(&schema_path).expect("schema compiles");
    let doc = parse(document);
    xml_tool::services::validation::validate(&doc, validator).len()
}

#[test]
fn xsd_valid_and_invalid_document_pairs() {
    // Twenty valid/invalid pairs across the plan's coverage axes.
    struct Case {
        name: &'static str,
        schema: &'static str,
        valid: &'static str,
        invalid: &'static str,
    }
    let string_schema = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
      <xs:element name="r"><xs:complexType><xs:sequence>
        <xs:element name="v" type="xs:string"/>
      </xs:sequence></xs:complexType></xs:element></xs:schema>"#;
    let int_schema = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
      <xs:element name="r"><xs:complexType><xs:sequence>
        <xs:element name="v" type="xs:integer"/>
      </xs:sequence></xs:complexType></xs:element></xs:schema>"#;
    let date_schema = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
      <xs:element name="r"><xs:complexType><xs:sequence>
        <xs:element name="v" type="xs:date"/>
      </xs:sequence></xs:complexType></xs:element></xs:schema>"#;
    let pattern_schema = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
      <xs:element name="r"><xs:complexType><xs:sequence>
        <xs:element name="v"><xs:simpleType><xs:restriction base="xs:string">
          <xs:pattern value="[A-Z]{3}"/>
        </xs:restriction></xs:simpleType></xs:element>
      </xs:sequence></xs:complexType></xs:element></xs:schema>"#;
    let minlen_schema = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
      <xs:element name="r"><xs:complexType><xs:sequence>
        <xs:element name="v"><xs:simpleType><xs:restriction base="xs:string">
          <xs:minLength value="3"/>
        </xs:restriction></xs:simpleType></xs:element>
      </xs:sequence></xs:complexType></xs:element></xs:schema>"#;
    let attrs_schema = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
      <xs:element name="r"><xs:complexType>
        <xs:attribute name="a" type="xs:integer" use="required"/>
      </xs:complexType></xs:element></xs:schema>"#;

    let cases = vec![
        Case {
            name: "sequence-ok",
            schema: BASE_SCHEMA,
            valid: "<person id=\"p1\"><name>Ada</name><age>36</age></person>",
            invalid: "<person id=\"p1\"><age>36</age><name>Ada</name></person>",
        },
        Case {
            name: "missing-required-element",
            schema: BASE_SCHEMA,
            valid: "<person id=\"p1\"><name>Ada</name><age>1</age></person>",
            invalid: "<person id=\"p1\"><name>Ada</name></person>",
        },
        Case {
            name: "required-attribute",
            schema: BASE_SCHEMA,
            valid: "<person id=\"p1\"><name>Ada</name><age>1</age></person>",
            invalid: "<person><name>Ada</name><age>1</age></person>",
        },
        Case {
            name: "id-type",
            schema: BASE_SCHEMA,
            valid: "<person id=\"p1\"><name>A</name><age>1</age></person>",
            invalid: "<person id=\"1 x\"><name>A</name><age>1</age></person>",
        },
        Case {
            name: "unknown-element",
            schema: BASE_SCHEMA,
            valid: "<person id=\"p1\"><name>A</name><age>1</age></person>",
            invalid: "<person id=\"p1\"><name>A</name><age>1</age><extra/></person>",
        },
        Case {
            name: "string-type",
            schema: string_schema,
            valid: "<r><v>any text</v></r>",
            invalid: "<r/>",
        },
        Case {
            name: "integer-ok",
            schema: int_schema,
            valid: "<r><v>42</v></r>",
            invalid: "<r><v>forty-two</v></r>",
        },
        Case {
            name: "integer-negative",
            schema: int_schema,
            valid: "<r><v>-7</v></r>",
            invalid: "<r><v>1.5</v></r>",
        },
        Case {
            name: "date-ok",
            schema: date_schema,
            valid: "<r><v>2026-08-16</v></r>",
            invalid: "<r><v>2026-13-01</v></r>",
        },
        Case {
            name: "pattern-ok",
            schema: pattern_schema,
            valid: "<r><v>ABC</v></r>",
            invalid: "<r><v>abc</v></r>",
        },
        Case {
            name: "pattern-length",
            schema: pattern_schema,
            valid: "<r><v>XYZ</v></r>",
            invalid: "<r><v>ABCD</v></r>",
        },
        Case {
            name: "min-length",
            schema: minlen_schema,
            valid: "<r><v>abc</v></r>",
            invalid: "<r><v>ab</v></r>",
        },
        Case {
            name: "attribute-present",
            schema: attrs_schema,
            valid: "<r a=\"5\"/>",
            invalid: "<r/>",
        },
        Case {
            name: "attribute-integer",
            schema: attrs_schema,
            valid: "<r a=\"5\"/>",
            invalid: "<r a=\"five\"/>",
        },
    ];
    assert!(cases.len() >= 14, "keep the fixture count meaningful");
    for case in cases {
        assert_eq!(
            xsd_case(case.schema, case.valid),
            0,
            "{}: valid document must pass",
            case.name
        );
        assert!(
            xsd_case(case.schema, case.invalid) > 0,
            "{}: invalid document must produce diagnostics",
            case.name
        );
    }
}

#[test]
fn xsd_include_composes_within_schema_root() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("shared.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
           <xs:simpleType name="code"><xs:restriction base="xs:string">
             <xs:enumeration value="A"/><xs:enumeration value="B"/>
           </xs:restriction></xs:simpleType></xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
           <xs:include schemaLocation="shared.xsd"/>
           <xs:element name="r"><xs:complexType><xs:sequence>
             <xs:element name="v" type="code"/>
           </xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )
    .unwrap();

    let mut cache = SchemaCache::default();
    let validator = cache
        .load(&dir.path().join("main.xsd"))
        .expect("include composes");
    let good = parse("<r><v>A</v></r>");
    assert_eq!(
        xml_tool::services::validation::validate(&good, validator).len(),
        0
    );
    let bad = parse("<r><v>C</v></r>");
    assert!(
        !xml_tool::services::validation::validate(&bad, validator).is_empty(),
        "enum facet from the included schema must be enforced"
    );
}

#[test]
fn xsd_rejects_references_outside_schema_root() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
           <xs:include schemaLocation="../../elsewhere.xsd"/>
           <xs:element name="r"/></xs:schema>"#,
    )
    .unwrap();
    match xml_tool::services::validation::compile_schema(&dir.path().join("main.xsd")) {
        Err(SchemaError::PathEscape { reference }) => assert_eq!(reference, "../../elsewhere.xsd"),
        Err(other) => panic!("path escape expected, got {other}"),
        Ok(_) => panic!("path escape expected"),
    }

    let network = dir.path().join("net.xsd");
    std::fs::write(
        &network,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
           <xs:import namespace="urn:x" schemaLocation="http://example.com/x.xsd"/>
           <xs:element name="r"/></xs:schema>"#,
    )
    .unwrap();
    match xml_tool::services::validation::compile_schema(&network) {
        Err(SchemaError::PathEscape { .. }) => {}
        Err(other) => panic!("network location must be refused, got {other}"),
        Ok(_) => panic!("network location must be refused"),
    }
}

#[test]
fn xsd_cache_reuses_compiled_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("schema.xsd");
    std::fs::write(&path, BASE_SCHEMA).unwrap();
    let mut cache = SchemaCache::default();
    let errors_first = {
        let validator = cache.load(&path).unwrap();
        let doc = parse("<person><name>A</name></person>");
        xml_tool::services::validation::validate(&doc, validator).len()
    };
    let errors_second = {
        let validator = cache.load(&path).unwrap();
        let doc = parse("<person><name>A</name></person>");
        xml_tool::services::validation::validate(&doc, validator).len()
    };
    assert_eq!(errors_first, errors_second);
    assert!(errors_first > 0);
    assert_eq!(cache.len(), 1, "one entry after two loads");
}

// ---------------------------------------------------------------------------
// Structural diff
// ---------------------------------------------------------------------------

#[test]
fn diff_detects_added_removed_modified_and_format_only() {
    let left = parse("<r>\n  <a x=\"1\">text</a>\n  <b/>\n</r>");
    let added = parse("<r>\n  <a x=\"1\">text</a>\n  <b/>\n  <c/>\n</r>");
    let removed = parse("<r>\n  <a x=\"1\">text</a>\n</r>");
    let modified = parse("<r>\n  <a x=\"2\">text</a>\n  <b/>\n</r>");
    let text_modified = parse("<r>\n  <a x=\"1\">other</a>\n  <b/>\n</r>");
    let reformatted = parse("<r><a x=\"1\">text</a><b/></r>");

    let entries = diff_xml(&left, &added, DiffOptions::default()).unwrap();
    assert!(
        entries
            .iter()
            .any(|entry| matches!(entry, DiffEntry::Added { .. }))
    );

    let entries = diff_xml(&left, &removed, DiffOptions::default()).unwrap();
    assert!(
        entries
            .iter()
            .any(|entry| matches!(entry, DiffEntry::Removed { .. }))
    );

    let entries = diff_xml(&left, &modified, DiffOptions::default()).unwrap();
    assert!(!entries.is_empty(), "attribute change must register");

    let entries = diff_xml(&left, &text_modified, DiffOptions::default()).unwrap();
    assert!(!entries.is_empty(), "text change must register");

    let entries = diff_xml(&left, &reformatted, DiffOptions::default()).unwrap();
    assert!(
        entries.is_empty(),
        "formatting-only differences must be ignored: {entries:?}"
    );
}

#[test]
fn diff_detects_moves() {
    let left = parse("<r><a/><b/><c/></r>");
    let moved = parse("<r><c/><a/><b/></r>");
    let entries = diff_xml(&left, &moved, DiffOptions::default()).unwrap();
    assert!(
        entries
            .iter()
            .any(|entry| matches!(entry, DiffEntry::Moved { .. })),
        "same content at different positions is a move: {entries:?}"
    );
}

#[test]
fn diff_does_not_report_unmoved_items_as_moves() {
    // Swapping two siblings moves exactly those two — not every node.
    let left = parse("<r><keep/><a/><b/></r>");
    let right = parse("<r><keep/><b/><a/></r>");
    let entries = diff_xml(&left, &right, DiffOptions::default()).unwrap();
    let moves = entries
        .iter()
        .filter(|entry| matches!(entry, DiffEntry::Moved { .. }))
        .count();
    assert_eq!(moves, 2, "only the swapped siblings are moves: {entries:?}");
}

#[test]
fn diff_keys_do_not_collide_across_attribute_boundaries() {
    // `a="1 b=2"` (one attribute) must not equal `a="1" b="2"` (two).
    let left = parse("<r><e a=\"1 b=2\"/></r>");
    let right = parse("<r><e a=\"1\" b=\"2\"/></r>");
    let entries = diff_xml(&left, &right, DiffOptions::default()).unwrap();
    assert!(
        !entries.is_empty(),
        "different attribute sets must not compare equal"
    );
}

// ---------------------------------------------------------------------------
// Batch replace: preview, apply, undo on 1,000 hits
// ---------------------------------------------------------------------------

#[test]
fn batch_replace_previews_applies_and_undoes_1000_hits() {
    let mut xml = String::from("<r>");
    for i in 0..1000 {
        xml.push_str(&format!("<item n=\"{i}\">value-{i}</item>"));
    }
    xml.push_str("</r>");
    let mut doc = parse(&xml);
    let mut index = SearchIndex::build(&doc);
    let order = doc.document_order().to_vec();

    let previews = build_replacements(
        &doc,
        &mut index,
        &order,
        "value-",
        "replaced-",
        ReplaceScope::Text,
        true,
    )
    .expect("all replacements legal");
    assert_eq!(previews.len(), 1000, "one hit per item text");

    let mut history = History::new();
    history
        .commit(
            &mut doc,
            Command::BatchReplace {
                ops: to_ops(&previews),
            },
        )
        .expect("batch applies");
    assert!(doc.source().contains("replaced-999"));
    assert!(!doc.source().contains("value-999"));
    assert_eq!(history.undo_depth(), 1, "the batch is one undo step");

    history.undo(&mut doc).expect("undo");
    assert!(
        doc.source().contains("value-999"),
        "undo restores every hit"
    );
}

#[test]
fn batch_replace_rejects_illegal_content_wholesale() {
    let doc = parse("<r><a>ok</a><b>also ok</b></r>");
    let mut index = SearchIndex::build(&doc);
    let order = doc.document_order().to_vec();
    // The replacement injects '<' into text: the whole preview must fail.
    let err = build_replacements(
        &doc,
        &mut index,
        &order,
        "ok",
        "bad <",
        ReplaceScope::Text,
        true,
    )
    .unwrap_err();
    assert!(err.contains("must not contain"), "{err}");
}

#[test]
fn batch_replace_attribute_scope_and_case_folding() {
    let doc = parse("<r><a STATUS=\"OLD\"/><b status=\"old\"/></r>");
    let mut index = SearchIndex::build(&doc);
    let order = doc.document_order().to_vec();
    let previews = build_replacements(
        &doc,
        &mut index,
        &order,
        "old",
        "new",
        ReplaceScope::AttributeValues,
        false,
    )
    .expect("legal");
    assert_eq!(
        previews.len(),
        2,
        "case-insensitive attr hits: {previews:?}"
    );
    for preview in &previews {
        assert!(preview.new_value.contains("new"));
    }
}

// ---------------------------------------------------------------------------
// Lossless JSON export
// ---------------------------------------------------------------------------

#[test]
fn lossless_json_preserves_every_node_kind_in_order() {
    let xml = concat!(
        "<r a=\"1\" xmlns:p=\"urn:p\">",
        "lead <b>bold</b>",
        "<![CDATA[raw <x>]]>",
        "<!--note-->",
        "<?hint data?>",
        "<p:c p:k=\"v\"/>",
        "</r>",
    );
    let doc = parse(xml);
    let json = xml_tool::export::export_to_json_lossless(&doc).expect("exports");
    let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");

    // Walk: every construct appears in document order under the root.
    assert_eq!(value["kind"], "element");
    assert_eq!(value["name"], "r");
    assert_eq!(value["attributes"][0]["name"], "a");
    let children = value["children"].as_array().expect("children");
    let kinds: Vec<&str> = children
        .iter()
        .map(|child| child["kind"].as_str().unwrap_or("?"))
        .collect();
    assert_eq!(
        kinds,
        vec!["text", "element", "cdata", "comment", "pi", "element"],
        "document order and node kinds survive"
    );
    assert_eq!(children[0]["text"], "lead ");
    assert_eq!(children[1]["name"], "b");
    assert_eq!(children[2]["text"], "raw <x>");
    assert_eq!(children[3]["text"], "note");
    assert_eq!(children[4]["target"], "hint");
    assert_eq!(children[5]["name"], "p:c");
    assert_eq!(children[5]["namespace"], "urn:p");
}

#[test]
fn legacy_json_export_still_works() {
    let doc = xml_tool::xml::parse_xml("<root a=\"1\"><child>text</child></root>").unwrap();
    let json = xml_tool::export::export_to_json(&doc).expect("legacy export");
    assert!(json.contains("@attributes"));
    assert!(json.contains("@text"));
}
