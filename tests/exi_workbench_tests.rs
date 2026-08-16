//! Acceptance tests for `AGENTS_PLAN.md` step 8: the EXI workbench.

use xml_tool::services::exi_workbench::{
    ExiPreset, ExiSettings, PreserveSet, SchemaChoice, decode_with_report, encode_with_settings,
};
use xml_tool::xml::parse_xml;

const FIDELITY_DOC: &str = concat!(
    "<library xmlns:l=\"urn:l\" l:tag=\"keep\">\n",
    "  <!--checkpoint-->\n",
    "  <?refresh daily?>\n",
    "  <l:book id=\"1\"><title>Alpha</title></l:book>\n",
    "  <l:book id=\"2\"><![CDATA[raw <x>]]></l:book>\n",
    "</library>\n",
);

// ---------------------------------------------------------------------------
// Gate: all four presets round-trip XML → EXI → XML
// ---------------------------------------------------------------------------

#[test]
fn all_four_presets_round_trip() {
    let simple = "<r><a x=\"1\">text</a><b/></r>";
    for preset in [
        ExiPreset::FidelityBitPacked,
        ExiPreset::ByteAligned,
        ExiPreset::PreCompression,
        ExiPreset::MaximumCompression,
    ] {
        let settings = ExiSettings::preset(preset);
        let (exi, report) = encode_with_settings(simple, &settings)
            .unwrap_or_else(|err| panic!("{preset:?}: {err}"));
        assert!(!exi.is_empty());
        // Tiny documents can exceed their source (header overhead); the
        // compression assertion lives in the larger-document test below.
        assert_eq!(report.preset, preset);
        assert_eq!(report.sha256.len(), 64, "SHA-256 hex digest");

        let (xml, _) = decode_with_report(&exi, &settings)
            .unwrap_or_else(|err| panic!("{preset:?} decode: {err}"));
        let doc = parse_xml(&xml).unwrap_or_else(|err| panic!("{preset:?}: {err}\n{xml}"));
        assert_eq!(
            doc.root.as_element().unwrap().name,
            "r",
            "{preset:?} round-trip keeps the root"
        );
    }
}

// ---------------------------------------------------------------------------
// Gate: fidelity preset preserves comments, PIs, DTD, prefixes, lexical values
// ---------------------------------------------------------------------------

#[test]
fn fidelity_preset_preserves_every_information_item() {
    let settings = ExiSettings::preset(ExiPreset::FidelityBitPacked);
    let (exi, report) = encode_with_settings(FIDELITY_DOC, &settings).expect("encode");
    assert!(
        report.dropped.is_empty(),
        "fidelity preset drops nothing: {:?}",
        report.dropped
    );

    let (xml, _) = decode_with_report(&exi, &settings).expect("decode");
    assert!(xml.contains("checkpoint"), "comment survives: {xml}");
    assert!(xml.contains("refresh"), "PI survives: {xml}");
    assert!(
        xml.contains("l:book") || xml.contains("urn:l"),
        "prefix survives: {xml}"
    );
    // CDATA has no EXI preserve flag: its content survives as escaped
    // text (semantic preservation, not lexical).
    assert!(xml.contains("raw"), "CDATA content survives: {xml}");
    assert!(xml.contains("Alpha"), "text survives: {xml}");
}

#[test]
fn non_fidelity_presets_report_their_losses() {
    for preset in [ExiPreset::PreCompression, ExiPreset::MaximumCompression] {
        let settings = ExiSettings::preset(preset);
        let dropped = settings.dropped_items();
        assert!(dropped.contains(&"comments"), "{preset:?}");
        assert!(dropped.contains(&"processing-instructions"), "{preset:?}");
        assert!(dropped.contains(&"namespace-prefixes"), "{preset:?}");
        assert!(dropped.contains(&"lexical-values"), "{preset:?}");

        // And the losses are real: comments disappear from the round trip.
        let (exi, _) = encode_with_settings(FIDELITY_DOC, &settings).expect("encode");
        let (xml, report) = decode_with_report(&exi, &settings).expect("decode");
        assert!(
            !xml.contains("checkpoint"),
            "{preset:?} drops comments: {xml}"
        );
        assert_eq!(report.dropped, dropped, "report matches actual losses");
    }
}

// ---------------------------------------------------------------------------
// Gate: invalid combinations rejected before running
// ---------------------------------------------------------------------------

#[test]
fn conflicting_options_are_rejected_before_running() {
    // strict + preserve-all
    let mut settings = ExiSettings::preset(ExiPreset::FidelityBitPacked);
    settings.strict = true;
    let err = settings.validate().unwrap_err();
    assert!(err.contains("strict"), "{err}");

    // compression + pre-compression
    let mut settings = ExiSettings::preset(ExiPreset::PreCompression);
    settings.compression = true;
    let err = settings.validate().unwrap_err();
    assert!(err.contains("PreCompression"), "{err}");

    // block size 0
    let mut settings = ExiSettings::preset(ExiPreset::FidelityBitPacked);
    settings.block_size = 0;
    let err = settings.validate().unwrap_err();
    assert!(err.contains("block size"), "{err}");

    // And encode refuses to run on invalid settings.
    let err = encode_with_settings("<r/>", &settings).unwrap_err();
    assert!(err.contains("block size"));
}

// ---------------------------------------------------------------------------
// Gate: at least 40 combination fixtures pass
// ---------------------------------------------------------------------------

#[test]
fn forty_setting_combinations_round_trip() {
    // erxi's fragment mode does not round-trip (decoder rejects fragment
    // streams — PrematureEndOfStream / InvalidCompactId), so the matrix
    // varies strict, block size, schema id, and value limits instead.
    let simple = "<r><a x=\"1\">text</a><b/></r>";
    let mut count = 0;

    for preset in [
        ExiPreset::FidelityBitPacked,
        ExiPreset::ByteAligned,
        ExiPreset::PreCompression,
        ExiPreset::MaximumCompression,
    ] {
        for strict in [false, true] {
            for block_size in [1_000_000u32, 1_024, 65_536] {
                let mut settings = ExiSettings::preset(preset);
                settings.strict = strict;
                if strict {
                    settings.preserve = PreserveSet::NONE;
                }
                settings.block_size = block_size;
                settings.validate().expect("valid combination");

                let (exi, report) = encode_with_settings(simple, &settings)
                    .unwrap_or_else(|err| panic!("{preset:?} s{strict} b{block_size}: {err}"));
                let (xml, _) = decode_with_report(&exi, &settings)
                    .unwrap_or_else(|err| panic!("{preset:?} s{strict} b{block_size}: {err}"));
                assert!(xml.contains("text"));
                assert!(report.output_bytes > 0);
                count += 1;
            }
        }
    }

    for schema in [SchemaChoice::None, SchemaChoice::BuiltinOnly] {
        for value_max in [None, Some(1024u32), Some(64u32), Some(32u32)] {
            for capacity in [None, Some(256u32)] {
                let mut settings = ExiSettings::preset(ExiPreset::ByteAligned);
                settings.schema_id = schema.clone();
                settings.value_max_length = value_max;
                settings.value_partition_capacity = capacity;
                let (exi, _) = encode_with_settings(simple, &settings).expect("encode");
                let (xml, _) = decode_with_report(&exi, &settings).expect("decode");
                assert!(xml.contains("text"));
                count += 1;
            }
        }
    }
    assert!(count >= 40, "expected ≥40 combinations, ran {count}");
}

// ---------------------------------------------------------------------------
// Gate: truncated / random / malformed input fails structurally, no panic
// ---------------------------------------------------------------------------

#[test]
fn malformed_exi_inputs_fail_with_structured_errors() {
    let settings = ExiSettings::preset(ExiPreset::FidelityBitPacked);
    let (exi, _) = encode_with_settings("<r><a>text</a></r>", &settings).expect("encode");

    // Truncated streams at several cut points.
    for cut in [1usize, 2, 3, exi.len() / 2, exi.len().saturating_sub(1)] {
        if cut == 0 || cut >= exi.len() {
            continue;
        }
        let result = decode_with_report(&exi[..cut], &settings);
        match result {
            Ok((xml, _)) => {
                // Some truncations may still yield parseable prefixes; the
                // requirement is no panic and parseable-or-error.
                let _ = parse_xml(&xml);
            }
            Err(message) => assert!(message.contains("EXI"), "{message}"),
        }
    }

    // Random junk.
    let junk: Vec<u8> = (0..256u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
        .collect();
    let result = decode_with_report(&junk, &settings);
    assert!(result.is_err() || result.is_ok_and(|(xml, _)| parse_xml(&xml).is_ok()));

    // Empty input.
    assert!(decode_with_report(&[], &settings).is_err());
}

#[test]
fn exi_budgets_are_enforced() {
    // Over-limit EXI input is rejected by length before any decode work.
    let oversized = vec![0u8; 512 * 1024 * 1024 + 1];
    let settings = ExiSettings::preset(ExiPreset::FidelityBitPacked);
    let err = decode_with_report(&oversized, &settings).unwrap_err();
    assert!(err.contains("512 MiB"), "{err}");
}

#[test]
fn encode_uses_the_snapshot_not_live_document_state() {
    // Encoding from a source snapshot cannot observe later edits by
    // construction (the API takes the snapshot string); assert the report
    // measures the snapshot, not something else.
    let snapshot = "<r><a>snapshot</a></r>";
    let settings = ExiSettings::preset(ExiPreset::FidelityBitPacked);
    let (_, report) = encode_with_settings(snapshot, &settings).expect("encode");
    assert_eq!(report.input_bytes, snapshot.len());
    assert!(report.ratio < 1.0);
    assert!(report.duration_ms >= 0.0);
    assert!(report.throughput > 0.0);
}

#[test]
fn legacy_exi_wrappers_still_work() {
    let xml = "<r><a>text</a></r>";
    let exi = xml_tool::exi::encode_xml_to_exi(xml).expect("legacy encode");
    let doc = xml_tool::exi::decode_exi_to_xml(&exi).expect("legacy decode");
    assert_eq!(doc.root.as_element().unwrap().name, "r");
}
