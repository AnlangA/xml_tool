//! Acceptance tests for `AGENTS_PLAN.md` step 1: the XML engine swap.
//!
//! Gates covered here:
//! - UTF-8 / UTF-16LE / UTF-16BE fixtures all open through the byte API.
//! - Unedited saves replay the original bytes exactly, BOM and line
//!   endings included.
//! - Declaration, CDATA, comments, PIs, DOCTYPE, internal entities,
//!   namespace prefixes, and mixed content survive parsing — and survive an
//!   edit + re-render round trip.
//! - External entities fail closed; no dependency may add network code;
//!   `quick-xml` is gone from the direct dependency list.

use uppsala::dom::NodeKind;
use xml_tool::fixtures;
use xml_tool::xml::{
    ParseOptions, SourceEncoding, XmlErrorCode, parse_xml_bytes, serialize_xml_bytes,
};

fn fixture(name: &str) -> Vec<u8> {
    fixtures::fidelity_fixtures()
        .into_iter()
        .find(|(fixture_name, _)| *fixture_name == name)
        .map(|(_, bytes)| bytes)
        .unwrap_or_else(|| panic!("fixture {name} not found"))
}

fn parse(name: &str) -> xml_tool::xml::EngineDocument {
    parse_xml_bytes(&fixture(name), ParseOptions::default()).expect("fixture must parse")
}

// ---------------------------------------------------------------------------
// Gate: UTF-8, UTF-16LE, UTF-16BE fixtures all open
// ---------------------------------------------------------------------------

#[test]
fn all_fidelity_fixtures_open_through_the_byte_api() {
    for (name, _) in fixtures::fidelity_fixtures() {
        let doc = parse_xml_bytes(&fixture(name), ParseOptions::default())
            .unwrap_or_else(|err| panic!("{name} must parse: {err}"));
        assert!(
            doc.document().document_element().is_some(),
            "{name} needs a root element"
        );
    }
}

#[test]
fn utf16_fixtures_report_their_detected_encoding() {
    let le = parse("utf16-le-bom.xml");
    let be = parse("utf16-be-bom.xml");
    assert_eq!(le.encoding(), SourceEncoding::Utf16LeBom);
    assert_eq!(be.encoding(), SourceEncoding::Utf16BeBom);
    // Multi-script text made it through the decode.
    let text = le.decoded_text().unwrap();
    assert!(text.contains("中文 Ünïcødé &amp; Δ"));
}

// ---------------------------------------------------------------------------
// Gate: unedited saves are byte-identical, BOM and line endings included
// ---------------------------------------------------------------------------

#[test]
fn unedited_save_replays_original_bytes_for_every_fixture() {
    for (name, _) in fixtures::fidelity_fixtures() {
        let doc = parse(name);
        let saved = serialize_xml_bytes(&doc).expect("serialize");
        assert_eq!(
            saved,
            fixture(name),
            "unedited save of {name} must be byte-identical"
        );
    }
}

#[test]
fn crlf_line_endings_and_bom_survive_load_and_save() {
    let doc = parse("crlf.xml");
    let saved = serialize_xml_bytes(&doc).unwrap();
    assert!(std::str::from_utf8(&saved).unwrap().contains("\r\n"));
    assert!(!doc.is_dirty());
}

// ---------------------------------------------------------------------------
// Gate: no construct silently disappears
// ---------------------------------------------------------------------------

#[test]
fn declaration_doctype_and_internal_entities_survive() {
    let doc = parse("full-fidelity.xml");
    let dom = doc.document();

    let decl = dom.xml_declaration.as_ref().expect("declaration kept");
    assert_eq!(decl.version, "1.0");
    assert_eq!(decl.encoding.as_deref(), Some("UTF-8"));
    assert_eq!(decl.standalone, Some(false));

    let doctype = dom.doctype.as_deref().expect("doctype kept");
    assert!(doctype.contains("<!ENTITY publisher"), "doctype: {doctype}");
    assert!(doctype.contains("<!ENTITY licence"), "doctype: {doctype}");

    // The internal entities were expanded into text, not dropped.
    let summary = dom
        .get_elements_by_tag_name("summary")
        .first()
        .copied()
        .expect("summary element");
    let text = dom.text_content_deep(summary);
    assert!(text.contains("Mason & Sons"), "expanded text: {text}");
    assert!(text.contains("MIT"), "expanded text: {text}");
}

#[test]
fn cdata_comments_and_pis_survive_in_the_dom() {
    let doc = parse("full-fidelity.xml");
    let dom = doc.document();

    let root = dom.document_element().unwrap();
    let mut saw_cdata = false;
    let mut saw_comment = false;
    let mut saw_pi = false;
    for id in dom.descendants(root) {
        match dom.node_kind(id) {
            Some(NodeKind::CData(_)) => saw_cdata = true,
            Some(NodeKind::Comment(_)) => saw_comment = true,
            Some(NodeKind::ProcessingInstruction(_)) => saw_pi = true,
            _ => {}
        }
    }
    assert!(saw_cdata, "CDATA sections must be modeled");
    assert!(saw_comment, "comments must be modeled");
    assert!(saw_pi, "processing instructions must be modeled");

    // Top-level PI (before the root element) is kept as a document child.
    let top_level: Vec<_> = dom.children(dom.root()).into_iter().collect();
    assert!(
        top_level
            .iter()
            .any(|&id| matches!(dom.node_kind(id), Some(NodeKind::ProcessingInstruction(_)))),
        "xml-stylesheet PI before the root must survive"
    );
}

#[test]
fn namespace_prefixes_are_modeled() {
    let doc = parse("namespaces.xml");
    let dom = doc.document();

    let root = dom.document_element().unwrap();
    let root_name = &dom.element(root).unwrap().name;
    assert_eq!(root_name.prefix.as_deref(), None);
    assert_eq!(root_name.local_name, "cfg");
    assert_eq!(
        root_name.namespace_uri.as_deref(),
        Some("urn:example:cfg"),
        "default namespace must be resolved"
    );

    let section = dom
        .get_elements_by_tag_name("section")
        .first()
        .copied()
        .expect("a:section");
    let section_name = &dom.element(section).unwrap().name;
    assert_eq!(section_name.prefix.as_deref(), Some("a"));
    assert_eq!(section_name.namespace_uri.as_deref(), Some("urn:example:a"));

    // Nested default-namespace redefinition resolves per-scope.
    let items = dom.get_elements_by_tag_name("item");
    assert_eq!(items.len(), 2, "prefixed and redefined items both match");
    let uris: Vec<_> = items
        .iter()
        .map(|&id| dom.element(id).unwrap().name.namespace_uri.clone())
        .collect();
    assert!(uris.contains(&Some("urn:example:b".into())));
    assert!(uris.contains(&Some("urn:example:other".into())));
}

#[test]
fn mixed_content_child_order_is_preserved() {
    let doc = parse("mixed-content.xml");
    let dom = doc.document();

    let root = dom.document_element().unwrap();
    let kinds: Vec<&NodeKind> = dom
        .children(root)
        .iter()
        .map(|id| dom.node_kind(*id).unwrap())
        .collect();

    // Text / element / text / element / text / comment / cdata / pi / text
    let kinds_debug: Vec<String> = kinds
        .iter()
        .map(|k| match k {
            NodeKind::Text(_) => "text".into(),
            NodeKind::Element(e) => format!("elem:{}", e.name.local_name),
            NodeKind::Comment(_) => "comment".into(),
            NodeKind::CData(_) => "cdata".into(),
            NodeKind::ProcessingInstruction(pi) => format!("pi:{}", pi.target),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(
        kinds_debug,
        vec![
            "text",
            "elem:b",
            "text",
            "elem:i",
            "comment",
            "cdata",
            "pi:refresh",
            "text",
        ],
        "mixed content order must be exact"
    );
}

// ---------------------------------------------------------------------------
// Gate: constructs survive an edit + re-render + reparse round trip
// ---------------------------------------------------------------------------

#[test]
fn edited_documents_keep_every_construct_after_rerender() {
    let mut doc = parse("full-fidelity.xml");
    let root = doc.document().document_element().unwrap();
    {
        let dom = doc.document_mut();
        let library = dom.element_mut(root).unwrap();
        library.set_attribute(uppsala::dom::QName::local("edited"), "true".into());
    }
    assert!(doc.is_dirty());

    let saved = serialize_xml_bytes(&doc).unwrap();
    assert_ne!(saved, fixture("full-fidelity.xml"), "edit must show up");

    let reparsed = parse_xml_bytes(&saved, ParseOptions::default()).expect("re-render parses");
    let dom = reparsed.document();
    let text = String::from_utf8_lossy(&saved);

    assert!(text.contains("edited=\"true\""));
    assert!(
        dom.xml_declaration.is_some(),
        "declaration survives re-render"
    );
    assert!(
        dom.doctype
            .as_deref()
            .unwrap_or("")
            .contains("!ENTITY publisher"),
        "doctype survives re-render"
    );
    let root2 = dom.document_element().unwrap();
    assert!(
        dom.descendants(root2)
            .iter()
            .any(|&id| matches!(dom.node_kind(id), Some(NodeKind::CData(_)))),
        "CDATA survives re-render"
    );
    assert!(
        dom.descendants(root2)
            .iter()
            .any(|&id| matches!(dom.node_kind(id), Some(NodeKind::Comment(_)))),
        "comments survive re-render"
    );
    assert!(
        dom.descendants(root2)
            .iter()
            .any(|&id| matches!(dom.node_kind(id), Some(NodeKind::ProcessingInstruction(_)))),
        "PIs survive re-render"
    );
}

// ---------------------------------------------------------------------------
// Gate: external entities fail closed; no network surface exists
// ---------------------------------------------------------------------------

#[test]
fn external_file_entities_are_never_fetched() {
    // A SYSTEM entity declaration is legal, but referencing it must error
    // instead of touching the filesystem or network.
    let xml = concat!(
        "<!DOCTYPE doc [\n",
        "  <!ENTITY leak SYSTEM \"file:///etc/passwd\">",
        "]>\n",
        "<doc>&leak;</doc>\n",
    );
    let err = parse_xml_bytes(xml.as_bytes(), ParseOptions::default()).unwrap_err();
    assert_eq!(err.code(), XmlErrorCode::EntityUndefined);
}

#[test]
fn direct_dependencies_have_no_network_surface() {
    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    for networked in [
        "reqwest",
        "ureq",
        "curl",
        "hyper",
        "isahc",
        "surf",
        "attohttpc",
        "tokio-tungstenite",
    ] {
        assert!(
            !manifest.contains(networked),
            "network dependency {networked} must not sneak in"
        );
    }
}

#[test]
fn quick_xml_is_no_longer_a_direct_dependency() {
    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    assert!(!manifest.contains("quick-xml"));
}

#[test]
fn uppsala_is_pinned_exactly() {
    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    assert!(
        manifest.contains("uppsala = \"=0.9.0\""),
        "uppsala must stay pinned to exactly 0.9.0"
    );
}
