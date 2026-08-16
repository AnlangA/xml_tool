//! Verification of the deterministic fixture corpus (`AGENTS_PLAN.md` step 0).
//!
//! These tests lock down the sizes and element counts that later acceptance
//! gates depend on, and prove that regeneration is byte-stable.

use xml_tool::fixtures::{
    self, EDIT_MAX_BYTES, EDIT_MAX_ELEMENTS, LARGE_BYTES_ELEMENTS, LARGE_BYTES_TARGET,
    LARGE_NODES_ELEMENTS, OPEN_MAX_BYTES,
};

/// Counts element start tags. Only valid for the large generators' output:
/// they never emit `<` inside text content, CDATA sections, comments,
/// processing instructions, or the internal DTD subset.
fn count_start_tags(bytes: &[u8]) -> usize {
    let mut count = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            match bytes.get(i + 1) {
                Some(b'!') | Some(b'?') | Some(b'/') => {}
                _ => count += 1,
            }
        }
        i += 1;
    }
    count
}

#[test]
fn large_bytes_fixture_is_exactly_20_mib() {
    let xml = fixtures::large_bytes_xml();
    assert_eq!(xml.len(), LARGE_BYTES_TARGET);
    assert_eq!(xml.len(), EDIT_MAX_BYTES);
    assert_eq!(xml.len(), 20 * 1024 * 1024);
}

#[test]
fn large_bytes_fixture_element_count_is_about_50000() {
    let xml = fixtures::large_bytes_xml();
    assert_eq!(count_start_tags(xml.as_bytes()), LARGE_BYTES_ELEMENTS);
    assert!((49_000..=51_000).contains(&LARGE_BYTES_ELEMENTS));
}

#[test]
fn large_bytes_fixture_is_deterministic() {
    assert_eq!(fixtures::large_bytes_xml(), fixtures::large_bytes_xml());
}

#[test]
fn large_nodes_fixture_has_exactly_200000_elements() {
    let xml = fixtures::large_nodes_xml();
    assert_eq!(count_start_tags(xml.as_bytes()), LARGE_NODES_ELEMENTS);
    assert_eq!(LARGE_NODES_ELEMENTS, EDIT_MAX_ELEMENTS);
}

#[test]
fn large_nodes_fixture_stays_under_20_mib() {
    let xml = fixtures::large_nodes_xml();
    assert!(xml.len() <= EDIT_MAX_BYTES, "len = {}", xml.len());
    assert!(xml.len() < OPEN_MAX_BYTES);
}

#[test]
fn large_nodes_fixture_is_deterministic() {
    assert_eq!(fixtures::large_nodes_xml(), fixtures::large_nodes_xml());
}

#[test]
fn fidelity_fixture_names_are_unique() {
    let corpus = fixtures::fidelity_fixtures();
    let mut names: Vec<_> = corpus.iter().map(|(name, _)| *name).collect();
    let total = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), total);
}

#[test]
fn full_fidelity_fixture_contains_every_construct() {
    let (name, bytes) = &fixtures::fidelity_fixtures()[0];
    assert_eq!(*name, "full-fidelity.xml");
    let text = std::str::from_utf8(bytes).unwrap();

    for marker in [
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>",
        "<!DOCTYPE library",
        "<!ENTITY publisher",
        "<!ENTITY licence",
        "&publisher;",
        "&licence;",
        "<?xml-stylesheet",
        "<?refresh",
        "<![CDATA[",
        "<!-- editor notes follow -->",
        "<!-- inline note -->",
        "xmlns=\"urn:example:library\"",
        "xmlns:dc=",
        "xmlns:meta=",
        "<placeholder/>",
        "lang='zh'",
        "中文标题",
    ] {
        assert!(
            text.contains(marker),
            "full-fidelity.xml must contain {marker:?}"
        );
    }
}

#[test]
fn utf16_fixtures_carry_correct_bom_and_decode_round_trip() {
    let corpus = fixtures::fidelity_fixtures();
    let le = corpus
        .iter()
        .find(|(name, _)| *name == "utf16-le-bom.xml")
        .unwrap();
    let be = corpus
        .iter()
        .find(|(name, _)| *name == "utf16-be-bom.xml")
        .unwrap();

    assert_eq!(&le.1[..2], &[0xFF, 0xFE], "UTF-16LE BOM");
    assert_eq!(&be.1[..2], &[0xFE, 0xFF], "UTF-16BE BOM");
    assert!(le.1.len() % 2 == 0 && be.1.len() % 2 == 0);

    let le_units: Vec<u16> = le.1[2..]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let be_units: Vec<u16> = be.1[2..]
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();
    let le_text = String::from_utf16(&le_units).unwrap();
    let be_text = String::from_utf16(&be_units).unwrap();
    assert_eq!(le_text, be_text, "LE and BE encode the same document");
    assert!(le_text.contains("encoding=\"UTF-16\""));
    assert!(le_text.contains("中文 Ünïcødé &amp; Δ"));
}

#[test]
fn crlf_fixture_uses_windows_line_endings_exclusively() {
    let corpus = fixtures::fidelity_fixtures();
    let (_, bytes) = corpus.iter().find(|(name, _)| *name == "crlf.xml").unwrap();
    let text = std::str::from_utf8(bytes).unwrap();

    assert!(text.contains("\r\n"));
    // Every LF must be preceded by CR: no bare Unix line endings.
    for (idx, ch) in text.bytes().enumerate() {
        if ch == b'\n' {
            assert!(
                idx > 0 && text.as_bytes()[idx - 1] == b'\r',
                "bare \\n at {idx}"
            );
        }
    }
}

#[test]
fn namespaces_fixture_declares_and_redefines_namespaces() {
    let corpus = fixtures::fidelity_fixtures();
    let (_, bytes) = corpus
        .iter()
        .find(|(name, _)| *name == "namespaces.xml")
        .unwrap();
    let text = std::str::from_utf8(bytes).unwrap();

    assert!(text.contains("xmlns=\"urn:example:cfg\""));
    assert!(text.contains("xmlns:a=\"urn:example:a\""));
    assert!(text.contains("xmlns:b=\"urn:example:b\""));
    assert!(text.contains("b:visible=\"true\""));
    assert!(text.contains("a:key=\"one\""));
    // Default namespace is redefined on a nested element.
    assert!(text.contains("<item xmlns=\"urn:example:other\""));
}

#[test]
fn mixed_content_fixture_interleaves_all_node_kinds() {
    let corpus = fixtures::fidelity_fixtures();
    let (_, bytes) = corpus
        .iter()
        .find(|(name, _)| *name == "mixed-content.xml")
        .unwrap();
    let text = std::str::from_utf8(bytes).unwrap();

    // Text before, between, and after child elements, plus comment, CDATA, PI.
    let para = text
        .split_once("<mixed:para")
        .and_then(|(_, rest)| rest.split_once("</mixed:para>"))
        .map(|(inner, _)| inner)
        .unwrap();
    assert!(para.starts_with(" xmlns:mixed=\"urn:example:mixed\">plain start "));
    assert!(para.contains("<b>bold</b>"));
    assert!(para.contains(" middle "));
    assert!(para.contains("<!-- note -->"));
    assert!(para.contains("<![CDATA[ raw <text> ]]>"));
    assert!(para.contains("<?refresh hint=\"x\"?>"));
    assert!(para.ends_with(" final"));
}

#[test]
fn write_all_fixtures_materializes_the_whole_corpus() {
    let tmp = tempfile::tempdir().unwrap();
    fixtures::write_all_fixtures(tmp.path()).unwrap();

    let large_bytes = std::fs::read(tmp.path().join("generated/large-bytes.xml")).unwrap();
    assert_eq!(large_bytes.len(), LARGE_BYTES_TARGET);
    let large_nodes = std::fs::read(tmp.path().join("generated/large-nodes.xml")).unwrap();
    assert!(large_nodes.len() <= EDIT_MAX_BYTES);

    let committed = fixtures::fidelity_fixtures();
    assert!(committed.len() >= 6);
    for (name, bytes) in committed {
        let on_disk = std::fs::read(tmp.path().join("fidelity").join(name)).unwrap();
        assert_eq!(on_disk, bytes, "{name} differs after regeneration");
    }
}
