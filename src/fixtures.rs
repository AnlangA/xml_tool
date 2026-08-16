//! Deterministic fixture corpus for functional and performance verification.
//!
//! Every fixture is a pure function of its constants: generating twice yields
//! byte-identical output, so any failure can be reproduced exactly. The
//! [`large_bytes_xml`] and [`large_nodes_xml`] generators back the
//! 20 MiB / 200,000-element editing-threshold acceptance tests, while
//! [`fidelity_fixtures`] covers the constructs a fidelity-preserving editor
//! must never lose: declarations, UTF-16, CRLF line endings, namespaces,
//! CDATA sections, processing instructions, DOCTYPE declarations, internal
//! entities, and mixed content.
//!
//! Use `cargo run --example gen_fixtures` to materialize the corpus on disk.

use std::path::Path;

/// One mebibyte, the unit all size budgets in the plan are expressed in.
pub const MIB: usize = 1024 * 1024;

/// Inputs at or below this size stay editable when the element count is also
/// at or below [`EDIT_MAX_ELEMENTS`].
pub const EDIT_MAX_BYTES: usize = 20 * MIB;

/// Element-node ceiling for the fully editable mode.
pub const EDIT_MAX_ELEMENTS: usize = 200_000;

/// Inputs above this size are refused outright, before any DOM is built.
pub const OPEN_MAX_BYTES: usize = 256 * MIB;

/// `large-bytes.xml` is exactly 20 MiB so it sits on the editable boundary.
pub const LARGE_BYTES_TARGET: usize = EDIT_MAX_BYTES;

/// Twelve thousand records plus the root give 50,001 elements (~50,000).
pub const LARGE_BYTES_RECORDS: usize = 12_500;

/// Total element count of `large-bytes.xml`: root + 4 elements per record.
pub const LARGE_BYTES_ELEMENTS: usize = 1 + LARGE_BYTES_RECORDS * 4;

/// `large-nodes.xml` has exactly 200,000 elements: root + 199,999 items.
pub const LARGE_NODES_ELEMENTS: usize = EDIT_MAX_ELEMENTS;

/// Builds `large-bytes.xml`: exactly [`LARGE_BYTES_TARGET`] bytes of UTF-8
/// containing [`LARGE_BYTES_ELEMENTS`] elements. All content is ASCII and the
/// summary text length is computed so the final byte count is exact.
pub fn large_bytes_xml() -> String {
    let header = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<!-- large-bytes.xml: deterministic fixture, exactly 20 MiB -->\n",
        "<catalog>\n"
    );
    let footer = "</catalog>\n";

    // Sum the record skeletons with empty summaries, then distribute the
    // remaining byte budget evenly over the summary texts. Every character is
    // ASCII, so byte lengths and character counts coincide.
    let skeleton: usize = (0..LARGE_BYTES_RECORDS).map(|i| record(i, 0).len()).sum();
    let budget = LARGE_BYTES_TARGET
        .checked_sub(header.len() + footer.len() + skeleton)
        .expect("record skeletons must fit inside the 20 MiB target");
    let base_summary = budget / LARGE_BYTES_RECORDS;
    let bump = budget % LARGE_BYTES_RECORDS;

    let mut out = String::with_capacity(LARGE_BYTES_TARGET);
    out.push_str(header);
    for i in 0..LARGE_BYTES_RECORDS {
        let summary_len = base_summary + usize::from(i < bump);
        out.push_str(&record(i, summary_len));
    }
    out.push_str(footer);
    debug_assert_eq!(out.len(), LARGE_BYTES_TARGET);
    out
}

/// Builds `large-nodes.xml`: exactly [`LARGE_NODES_ELEMENTS`] elements packed
/// into roughly 8 MiB, well below the 20 MiB editable ceiling.
pub fn large_nodes_xml() -> String {
    let header = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<nodes>\n";
    let footer = "</nodes>\n";
    let items = LARGE_NODES_ELEMENTS - 1; // the root element makes up the rest

    let mut out = String::with_capacity(9 * MIB);
    out.push_str(header);
    for i in 0..items {
        out.push_str(&format!(
            "  <item id=\"{i}\" g=\"{}\">{}</item>\n",
            i % 7,
            i * 11
        ));
    }
    out.push_str(footer);
    out
}

/// One catalog record: `record`, `title`, `summary`, `value` — four elements.
fn record(i: usize, summary_len: usize) -> String {
    let status = match i % 3 {
        0 => "active",
        1 => "archived",
        _ => "pending",
    };
    format!(
        "  <record id=\"{i}\" category=\"cat-{}\" status=\"{status}\">\n    \
         <title>Record {i} title</title>\n    \
         <summary>{}</summary>\n    \
         <value index=\"{i}\">{}</value>\n  </record>\n",
        i % 8,
        filler(summary_len, i),
        (i * 37) % 1_000_000,
    )
}

/// Deterministic ASCII filler of exactly `len` bytes, seeded by `seed`.
/// The corpus never places `<` inside text content.
fn filler(len: usize, seed: usize) -> String {
    const WORDS: [&str; 8] = [
        "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
    ];
    if len == 0 {
        return String::new();
    }
    let mut text = String::with_capacity(len + WORDS[0].len());
    let mut word = seed;
    while text.len() < len {
        text.push_str(WORDS[word % WORDS.len()]);
        text.push(' ');
        word += 3;
    }
    text.truncate(len);
    text
}

const FULL_FIDELITY: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n",
    "<!-- full-fidelity.xml: exercises every construct the editor must preserve -->\n",
    "<!DOCTYPE library [\n",
    "<!ENTITY publisher \"Mason &amp; Sons\">\n",
    "<!ENTITY licence \"MIT\">\n",
    "]>\n",
    "<?xml-stylesheet type=\"text/xsl\" href=\"catalog.xsl\"?>\n",
    "<library xmlns=\"urn:example:library\" xmlns:dc=\"urn:example:dc\" xml:lang=\"en\">\n",
    "  <book id=\"b-001\" dc:issued=\"2024-03-01\" draft=\"false\">\n",
    "    <title>Efficient XML</title>\n",
    "    <summary>Published by &publisher; under &licence;; 5 &lt; 6 &amp; 7 &gt; 2.</summary>\n",
    "    <excerpt><![CDATA[if (a < b && c > d) { emit(\"raw <text>\"); }]]></excerpt>\n",
    "    <!-- editor notes follow -->\n",
    "    <notes type=\"mixed\">Lead &publisher; middle <em>emphasised</em> tail<!-- inline note --><?refresh hint=\"daily\"?> end</notes>\n",
    "    <meta:review xmlns:meta=\"urn:example:meta\" score=\"5\">compact</meta:review>\n",
    "    <placeholder/>\n",
    "  </book>\n",
    "  <book id=\"b-002\" dc:issued=\"2025-11-30\">\n",
    "    <title lang='zh'>中文标题 Ünïcødé</title>\n",
    "    <summary>CDATA with <b>markup-ish</b> text: <![CDATA[x < y]]></summary>\n",
    "  </book>\n",
    "</library>\n",
);

const UTF16_DOC: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n",
    "<!-- utf16 fixture: BOM plus multi-script text must survive round trips -->\n",
    "<unicode dir=\"ltr\">中文 Ünïcødé &amp; Δ<!-- comment --><![CDATA[a < b]]></unicode>\n",
);

const CRLF_DOC: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n",
    "<!-- crlf.xml: Windows line endings must survive load and save -->\r\n",
    "<invoice currency=\"EUR\" version=\"1\">\r\n",
    "  <line no=\"1\">\r\n",
    "    <qty>2</qty>\r\n",
    "    <price>19.90</price>\r\n",
    "  </line>\r\n",
    "  <line no=\"2\">\r\n",
    "    <qty>1</qty>\r\n",
    "    <price>5.00</price>\r\n",
    "  </line>\r\n",
    "  <total>44.80</total>\r\n",
    "</invoice>\r\n",
);

const NAMESPACES_DOC: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
    "<cfg xmlns=\"urn:example:cfg\" xmlns:a=\"urn:example:a\" xmlns:b=\"urn:example:b\">\n",
    "  <a:section b:visible=\"true\">\n",
    "    <b:item a:key=\"one\">first</b:item>\n",
    "    <item xmlns=\"urn:example:other\" a:key=\"two\">second</item>\n",
    "    <a:section>\n",
    "      <deep/>\n",
    "    </a:section>\n",
    "  </a:section>\n",
    "</cfg>\n",
);

const MIXED_CONTENT_DOC: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
    "<mixed:para xmlns:mixed=\"urn:example:mixed\">plain start <b>bold</b> middle ",
    "<i>italic <u>underline</u> tail</i><!-- note -->",
    "<![CDATA[ raw <text> ]]><?refresh hint=\"x\"?> final</mixed:para>\n",
);

/// Returns the small fidelity corpus as `(file name, bytes)` pairs in a
/// stable order. `utf16-le-bom.xml` and `utf16-be-bom.xml` carry a BOM.
pub fn fidelity_fixtures() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("full-fidelity.xml", FULL_FIDELITY.as_bytes().to_vec()),
        ("utf16-le-bom.xml", encode_utf16(UTF16_DOC, true)),
        ("utf16-be-bom.xml", encode_utf16(UTF16_DOC, false)),
        ("crlf.xml", CRLF_DOC.as_bytes().to_vec()),
        ("namespaces.xml", NAMESPACES_DOC.as_bytes().to_vec()),
        ("mixed-content.xml", MIXED_CONTENT_DOC.as_bytes().to_vec()),
    ]
}

/// Encodes `text` as UTF-16 with a leading BOM (`FF FE` for LE, `FE FF` BE).
fn encode_utf16(text: &str, little_endian: bool) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(2 + text.len() * 2);
    bytes.extend_from_slice(if little_endian {
        &[0xFF, 0xFE]
    } else {
        &[0xFE, 0xFF]
    });
    for unit in text.encode_utf16() {
        if little_endian {
            bytes.extend_from_slice(&unit.to_le_bytes());
        } else {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
    }
    bytes
}

/// Writes the whole corpus under `dir`:
///
/// - `generated/large-bytes.xml` and `generated/large-nodes.xml` (git-ignored)
/// - `fidelity/*.xml` (committed)
pub fn write_all_fixtures(dir: &Path) -> std::io::Result<()> {
    let generated = dir.join("generated");
    std::fs::create_dir_all(&generated)?;
    std::fs::write(generated.join("large-bytes.xml"), large_bytes_xml())?;
    std::fs::write(generated.join("large-nodes.xml"), large_nodes_xml())?;

    let fidelity = dir.join("fidelity");
    std::fs::create_dir_all(&fidelity)?;
    for (name, bytes) in fidelity_fixtures() {
        std::fs::write(fidelity.join(name), bytes)?;
    }
    Ok(())
}
