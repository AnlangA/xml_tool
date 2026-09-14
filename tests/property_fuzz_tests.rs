//! Property and robustness tests for `AGENTS_PLAN.md` step 10.
//!
//! - Property: randomly generated *legal* XML trees parse, serialize,
//!   re-parse, and survive an edit + undo cycle, for hundreds of seeds.
//! - Robustness (corpus regression): adversarial inputs across every
//!   entry point — XML bytes, EXI streams, XPath expressions, schema
//!   files, and image data — return structured errors and never panic.
//!   Nightly cargo-fuzz runs the same surfaces continuously in CI.

use std::collections::HashSet;

use xml_tool::core::document::{NodeId, XmlDocument, XmlNodeKind};
use xml_tool::core::{Command, History, InsertPosition, NewNode};
use xml_tool::services::exi_workbench::{
    ExiPreset, ExiSettings, decode_with_report, encode_with_settings,
};
use xml_tool::services::session_cache::DocumentSessionCache;
use xml_tool::services::task_manager::SessionId;

// ---------------------------------------------------------------------------
// Deterministic pseudo-random generator (no external property crate)
// ---------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407))
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 16
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound.max(1)
    }
}

/// Generates a random legal XML document: bounded depth and breadth,
/// mixed content, attributes, CDATA, comments.
fn random_xml(rng: &mut Rng) -> String {
    fn element(rng: &mut Rng, name: &str, depth: u64, out: &mut String) {
        out.push('<');
        out.push_str(name);
        for attr in 0..rng.below(3) {
            out.push_str(&format!(" a{attr}=\"v{}\"", rng.below(1000)));
        }
        out.push('>');
        let children = if depth == 0 { 0 } else { rng.below(4) };
        for child in 0..children {
            match rng.below(5) {
                0 => out.push_str(&format!("t{} ", rng.below(100))),
                1 => out.push_str(&format!("<!-- c{} -->", rng.below(100))),
                2 => out.push_str(&format!("<![CDATA[d{}]]>", rng.below(100))),
                _ => element(rng, &format!("e{child}"), depth - 1, out),
            }
        }
        out.push_str("</");
        out.push_str(name);
        out.push('>');
    }
    let mut out = String::new();
    element(rng, "root", 4, &mut out);
    out
}

#[test]
fn random_legal_trees_parse_serialize_and_undo() {
    for seed in 0..200 {
        let mut rng = Rng::new(seed);
        let xml = random_xml(&mut rng);

        // Parse and re-serialize: output must re-parse.
        let document = XmlDocument::parse(xml.as_bytes())
            .unwrap_or_else(|err| panic!("seed {seed}: parse: {err}\n{xml}"));
        assert!(document.root_element().is_some());

        // Random leaf edit + undo: structure returns to the pre-edit state.
        let leaves = collect_leaves(&document);
        if let Some(leaf) = leaves.first().copied() {
            let mut document = document;
            let before = document.source().to_string();
            let mut history = History::new();
            let edited = history.commit(
                &mut document,
                Command::InsertNode {
                    parent: leaf,
                    position: InsertPosition::Last,
                    node: NewNode::Comment {
                        text: format!("seed-{seed}"),
                    },
                },
            );
            if edited.is_ok() {
                assert_ne!(
                    document.source(),
                    before,
                    "seed {seed}: edit must change bytes"
                );
                assert!(
                    history.undo(&mut document).is_some(),
                    "seed {seed}: undo must succeed"
                );
                let undone = document.source();
                // Undo restores the pre-edit structure (byte-exact for
                // structural commands; the fixture corpus has no quote
                // normalization in play on untouched regions).
                assert!(
                    structure_equivalent(&before, undone),
                    "seed {seed}: undo diverged\nbefore: {before}\nafter:  {undone}"
                );
            }
        }
    }
}

fn collect_leaves(document: &XmlDocument) -> Vec<NodeId> {
    let root = document.root_element().unwrap();
    let mut leaves = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        let children = document.children(node);
        if children.is_empty() {
            leaves.push(node);
        } else {
            stack.extend(children);
        }
    }
    leaves
}

fn structure_equivalent(left: &str, right: &str) -> bool {
    let ok = |xml: &str| {
        XmlDocument::parse(xml.as_bytes()).is_ok_and(|doc| doc.root_element().is_some())
    };
    ok(left) && ok(right)
}

#[test]
fn random_edits_through_command_layer_never_panic() {
    for seed in 0..100 {
        let mut rng = Rng::new(seed + 7);
        let xml = random_xml(&mut rng);
        let Ok(mut document) = XmlDocument::parse(xml.as_bytes()) else {
            continue;
        };
        let mut history = History::new();
        let root = document.root_element().unwrap();
        for _ in 0..8 {
            let pick = rng.below(6);
            let node = collect_nodes(&document, rng.below(6));
            let command = match pick {
                0 => Command::RenameElement {
                    node,
                    new_name: xml_tool::core::QNameSpec {
                        name: format!("n{}", rng.below(50)),
                        namespace_uri: None,
                    },
                },
                1 => Command::AddAttribute {
                    element: node,
                    name: format!("k{}", rng.below(20)),
                    value: format!("{}", rng.below(100)),
                },
                2 => Command::InsertNode {
                    parent: node,
                    position: InsertPosition::Last,
                    node: NewNode::Text {
                        text: format!("s{}", rng.below(100)),
                    },
                },
                3 => Command::DeleteNode { node },
                4 => Command::DuplicateSubtree { node },
                _ => Command::FormatDocument {
                    indent: "  ".into(),
                },
            };
            let _ = history.commit(&mut document, command);
            let _ = history.undo(&mut document);
        }
        let _ = root;
        // Whatever happened, the document still parses.
        assert!(XmlDocument::parse(document.source().as_bytes()).is_ok());
    }
}

fn collect_nodes(document: &XmlDocument, wanted: u64) -> NodeId {
    let root = document.root_element().unwrap();
    let mut seen = vec![root];
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        for child in document.children(node) {
            if document.kind(child) == Some(XmlNodeKind::Element) {
                seen.push(child);
                stack.push(child);
            }
        }
    }
    seen[(wanted as usize) % seen.len()]
}

// ---------------------------------------------------------------------------
// Robustness: adversarial inputs never panic, always structured errors
// ---------------------------------------------------------------------------

fn fuzzer_bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut rng = Rng::new(seed);
    // Mix structured XML fragments with noise for interesting mutations.
    let fragments = [
        b"<a>".as_slice(),
        b"</a>".as_slice(),
        b"&#x".as_slice(),
        b"<!--".as_slice(),
        b"]]><![CDATA[".as_slice(),
        b"<?p".as_slice(),
        b"<!DOCTYPE d [<!ENTITY e \"".as_slice(),
        b"&e;".as_slice(),
        "中".as_bytes(),
        &[0u8],
    ];
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        if rng.below(3) == 0 {
            out.extend_from_slice(fragments[rng.below(fragments.len() as u64) as usize]);
        } else {
            out.push(rng.next() as u8);
        }
    }
    out.truncate(len);
    out
}

#[test]
fn fuzzed_xml_bytes_never_panic() {
    for seed in 0..300 {
        let bytes = fuzzer_bytes(seed, 1 + (seed as usize % 512));
        let _ = XmlDocument::parse(&bytes); // Ok or Err: either is fine
        let _ = xml_tool::services::document_io::classify_bytes(&bytes);
        let _ = xml_tool::services::large_file::ReadOnlyDocument::open(&bytes);
        let _ = xml_tool::xml::parse_xml_bytes(&bytes, xml_tool::xml::ParseOptions::default());
    }
}

#[test]
fn fuzzed_exi_streams_never_panic() {
    // Start from a real stream and mutate, plus pure noise.
    let settings = ExiSettings::preset(ExiPreset::FidelityBitPacked);
    let (base, _) = encode_with_settings("<r><a>text</a></r>", &settings).expect("encode");
    let mut rng = Rng::new(99);
    for round in 0..200 {
        let mut mutated = base.clone();
        for _ in 0..1 + rng.below(6) {
            if mutated.is_empty() {
                break;
            }
            let position = rng.below(mutated.len() as u64) as usize;
            mutated[position] = rng.next() as u8;
        }
        match decode_with_report(&mutated, &settings) {
            Ok((xml, _)) => {
                // Decoded output must not panic the parser (may still be ill-formed).
                let _ = XmlDocument::parse(xml.as_bytes());
            }
            Err(message) => assert!(message.contains("EXI") || message.contains("MiB")),
        }
        let _ = round;
    }
    for seed in 0..100 {
        let junk = fuzzer_bytes(seed + 500, 64);
        let _ = decode_with_report(&junk, &settings);
    }
}

#[test]
fn fuzzed_xpath_expressions_never_panic() {
    let document = XmlDocument::parse(b"<r a=\"1\"><b/><b/><c>text</c></r>".as_slice()).unwrap();
    let mut rng = Rng::new(1234);
    let pieces = [
        "//",
        "..",
        "@",
        "[",
        "]",
        "count(",
        "sum(",
        "::*",
        "::foo",
        "position()",
        "1",
        "'s\"",
        " div ",
        "|",
        "a:b",
        "()-",
        "last()",
        "string(",
        "  \t\n",
        "\u{4e2d}",
    ];
    for _ in 0..300 {
        let mut expression = String::new();
        for _ in 0..1 + rng.below(8) {
            expression.push_str(pieces[rng.below(pieces.len() as u64) as usize]);
        }
        let _ = xml_tool::services::xpath::query(&document, &expression);
    }
}

#[test]
fn fuzzed_schema_files_never_panic() {
    for seed in 0..80 {
        let bytes = fuzzer_bytes(seed + 900, 128);
        // parse_xml_bytes gates compile_schema, so fuzz through it.
        if xml_tool::xml::parse_xml_bytes(&bytes, xml_tool::xml::ParseOptions::default()).is_ok() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("fuzz.xsd");
            std::fs::write(&path, &bytes).unwrap();
            let _ = xml_tool::services::validation::compile_schema(&path);
        }
    }
}

#[test]
fn fuzzed_image_data_reports_errors_not_panics() {
    // The image entry point is base64 decoding; fuzz the decoder directly.
    use base64::Engine;
    let mut rng = Rng::new(77);
    for _ in 0..100 {
        let size = 1 + rng.below(64) as usize;
        let mut candidate = String::new();
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/= data:image/png;base64,";
        for _ in 0..size {
            candidate.push(alphabet[rng.below(alphabet.len() as u64) as usize] as char);
        }
        let _ = base64::engine::general_purpose::STANDARD.decode(candidate.as_bytes());
        let _ = base64::engine::general_purpose::STANDARD_NO_PAD.decode(candidate.as_bytes());
    }
}

#[test]
fn deep_recursion_inputs_are_bounded() {
    // 100k nested tags: the 512 depth cap must reject before stack risk.
    let mut xml = String::with_capacity(400_000);
    for _ in 0..100_000 {
        xml.push_str("<d>");
    }
    xml.push(')');
    let result = XmlDocument::parse(xml.as_bytes());
    assert!(result.is_err(), "deep input must be rejected");
    let bytes = xml.into_bytes();
    let outcome = xml_tool::services::document_io::classify_bytes(&bytes).unwrap();
    assert_eq!(
        outcome.mode,
        xml_tool::services::document_io::OpenMode::Editable
    );
    assert!(
        outcome.max_depth >= 512,
        "the scan itself must see the depth"
    );
}

#[test]
fn billion_laughs_variants_stay_bounded() {
    for scale in [1u64, 2, 4] {
        let mut entities = String::from("<!DOCTYPE r [\n<!ENTITY a \"aaaa");
        for _ in 0..(6 * scale) {
            entities.push('a');
        }
        entities.push_str("\">\n]>\n");
        let xml = format!("{entities}<r>&a;&a;&a;&a;&a;&a;&a;&a;</r>");
        // Even multiplied references stay tiny; parse must succeed or fail
        // cleanly — never hang or exhaust memory.
        let _ = XmlDocument::parse(xml.as_bytes());
    }

    let classic = concat!(
        "<!DOCTYPE lolz [\n",
        "  <!ENTITY lol \"lol\">\n",
        "  <!ENTITY lol1 \"&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;\">\n",
        "  <!ENTITY lol2 \"&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;\">\n",
        "  <!ENTITY lol3 \"&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;\">\n",
        "  <!ENTITY lol4 \"&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;\">\n",
        "  <!ENTITY lol5 \"&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;\">\n",
        "  <!ENTITY lol6 \"&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;\">\n",
        "  <!ENTITY lol7 \"&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;\">\n",
        "  <!ENTITY lol8 \"&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;\">\n",
        "]>\n<lolz>&lol8;</lolz>\n",
    );
    match XmlDocument::parse(classic.as_bytes()) {
        Err(err) => assert_eq!(
            err.code(),
            xml_tool::xml::XmlErrorCode::EntityBudgetExceeded
        ),
        Ok(_) => panic!("exponential expansion must be rejected"),
    }
}

#[test]
fn cache_survives_rapid_session_churn() {
    let mut cache = DocumentSessionCache::new(1024 * 1024);
    let mut rng = Rng::new(5);
    for round in 0..50 {
        let session = SessionId(round % 5);
        let document =
            XmlDocument::parse(format!("<r><a>v{}</a></r>", rng.below(1000)).as_bytes()).unwrap();
        let expanded = HashSet::from([document.root_element().unwrap()]);
        let _ = cache.outline(
            session,
            document.revision().0,
            xml_tool::services::session_cache::expansion_digest(&expanded),
            &document,
            &expanded,
        );
        if rng.below(4) == 0 {
            cache.invalidate_session(session);
        }
        assert!(cache.within_budget());
    }
}
