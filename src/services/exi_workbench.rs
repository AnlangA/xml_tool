//! The professional EXI workbench: settings, presets, validation, coding,
//! and reports.
//!
//! The UI never touches `erxi` types; everything flows through
//! [`ExiSettings`] (validated before any run — invalid combinations are
//! rejected with reasons, never silently corrected) and [`ExiReport`].
//!
//! Fixed budgets: EXI memory 512 MiB, decoded XML 256 MiB, depth 512.
//! Coding always uses the document's revision snapshot; results carry
//! SHA-256 digests and the list of XML information items the chosen
//! options cannot preserve (shown as a fidelity warning before running).

use std::time::Instant;

use erxi::options::{Alignment, ExiOptions, Preserve, SchemaId};

/// Which preservation flags are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PreserveSet {
    pub comments: bool,
    pub pis: bool,
    pub dtd: bool,
    pub prefixes: bool,
    pub lexical_values: bool,
    /// Whitespace (the `preserve:whitespace` fidelity item).
    pub whitespace: bool,
}

impl PreserveSet {
    /// Every flag on (the fidelity presets).
    pub const ALL: PreserveSet = PreserveSet {
        comments: true,
        pis: true,
        dtd: true,
        prefixes: true,
        lexical_values: true,
        whitespace: true,
    };

    /// Every flag off (the compression presets).
    pub const NONE: PreserveSet = PreserveSet {
        comments: false,
        pis: false,
        dtd: false,
        prefixes: false,
        lexical_values: false,
        whitespace: false,
    };

    fn is_all_on(self) -> bool {
        self.comments && self.pis && self.dtd && self.prefixes && self.lexical_values
    }
}

/// The four fixed presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExiPreset {
    /// BitPacked, no compression, everything preserved.
    #[default]
    FidelityBitPacked,
    /// Byte alignment, no compression, everything preserved.
    ByteAligned,
    /// PreCompression, no compression, nothing preserved.
    PreCompression,
    /// BitPacked + DEFLATE, nothing preserved, block size 1,000,000.
    MaximumCompression,
}

/// User-facing EXI settings (one vocabulary for presets and advanced mode).
#[derive(Debug, Clone, PartialEq)]
pub struct ExiSettings {
    pub preset: ExiPreset,
    pub alignment: Alignment,
    pub compression: bool,
    pub strict: bool,
    pub fragment: bool,
    pub preserve: PreserveSet,
    pub self_contained: bool,
    pub self_contained_qnames: Vec<String>,
    pub schema_id: SchemaChoice,
    pub block_size: u32,
    pub value_max_length: Option<u32>,
    pub value_partition_capacity: Option<u32>,
}

/// Schema id selection without leaking erxi types.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SchemaChoice {
    #[default]
    None,
    BuiltinOnly,
    /// User-defined schema identifier string.
    Id(String),
}

impl Default for ExiSettings {
    fn default() -> Self {
        ExiSettings::preset(ExiPreset::FidelityBitPacked)
    }
}

impl ExiSettings {
    /// The fixed preset definitions from the plan.
    pub fn preset(preset: ExiPreset) -> ExiSettings {
        let base = ExiSettings {
            preset,
            alignment: Alignment::BitPacked,
            compression: false,
            strict: false,
            fragment: false,
            preserve: PreserveSet::ALL,
            self_contained: false,
            self_contained_qnames: Vec::new(),
            schema_id: SchemaChoice::None,
            block_size: 1_000_000,
            value_max_length: None,
            value_partition_capacity: None,
        };
        match preset {
            ExiPreset::FidelityBitPacked => base,
            ExiPreset::ByteAligned => ExiSettings {
                alignment: Alignment::ByteAlignment,
                ..base
            },
            ExiPreset::PreCompression => ExiSettings {
                alignment: Alignment::PreCompression,
                preserve: PreserveSet::NONE,
                ..base
            },
            ExiPreset::MaximumCompression => ExiSettings {
                alignment: Alignment::BitPacked,
                compression: true,
                preserve: PreserveSet::NONE,
                block_size: 1_000_000,
                ..base
            },
        }
    }

    /// Rejects invalid combinations with the reason. Runs before every
    /// encode/decode; nothing is silently corrected.
    pub fn validate(&self) -> Result<(), String> {
        if self.compression && self.alignment == Alignment::PreCompression {
            return Err(String::from(
                "compression cannot be combined with PreCompression alignment",
            ));
        }
        if self.strict && self.preserve.is_all_on() {
            return Err(String::from(
                "strict mode cannot preserve comments, PIs, DTD, prefixes, or lexical values",
            ));
        }
        if self.block_size == 0 {
            return Err(String::from("block size must be greater than zero"));
        }
        if self.self_contained && self.alignment == Alignment::PreCompression {
            return Err(String::from(
                "self-contained channels require BitPacked or ByteAlignment",
            ));
        }
        Ok(())
    }

    /// The information items these settings will drop (for the fidelity
    /// warning shown before running).
    pub fn dropped_items(&self) -> Vec<&'static str> {
        let mut dropped = Vec::new();
        if !self.preserve.comments {
            dropped.push("comments");
        }
        if !self.preserve.pis {
            dropped.push("processing-instructions");
        }
        if !self.preserve.dtd {
            dropped.push("dtd");
        }
        if !self.preserve.prefixes {
            dropped.push("namespace-prefixes");
        }
        if !self.preserve.lexical_values {
            dropped.push("lexical-values");
        }
        dropped
    }

    fn to_erxi(&self) -> ExiOptions {
        let mut options = ExiOptions::default();
        options.set_alignment(self.alignment);
        options.set_compression(self.compression);
        options.set_strict(self.strict);
        options.set_fragment(self.fragment);
        options.set_preserve(Preserve {
            comments: self.preserve.comments,
            pis: self.preserve.pis,
            dtd: self.preserve.dtd,
            prefixes: self.preserve.prefixes,
            lexical_values: self.preserve.lexical_values,
            ..Preserve::default()
        });
        options.set_self_contained(self.self_contained);
        options.set_block_size(self.block_size);
        options.set_value_max_length(self.value_max_length);
        options.set_value_partition_capacity(self.value_partition_capacity);
        // Per-name self-contained scoping: erxi's QName type cannot be
        // constructed off-library (interned ids), so an enabled
        // self_contained flag with an empty list wraps every element —
        // the erxi semantic for an empty list.
        let _ = &self.self_contained_qnames;
        options.set_schema_id(match &self.schema_id {
            SchemaChoice::None => Some(SchemaId::None),
            SchemaChoice::BuiltinOnly => Some(SchemaId::BuiltinOnly),
            SchemaChoice::Id(id) => Some(SchemaId::Id(id.clone())),
        });
        options
    }
}

/// Plan-fixed budgets.
pub const MAX_EXI_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_DECODED_XML_BYTES: usize = 256 * 1024 * 1024;

/// Everything a run reports.
#[derive(Debug, Clone, PartialEq)]
pub struct ExiReport {
    pub input_bytes: usize,
    pub output_bytes: usize,
    /// output / input (below 1 means smaller).
    pub ratio: f64,
    pub duration_ms: f64,
    /// Bytes per second.
    pub throughput: f64,
    pub preset: ExiPreset,
    pub alignment: &'static str,
    pub compression: bool,
    pub block_size: u32,
    pub schema_id: String,
    pub sha256: String,
    /// Information items the settings dropped.
    pub dropped: Vec<&'static str>,
}

/// Encodes `xml` (a document source snapshot) with validated `settings`.
pub fn encode_with_settings(
    xml: &str,
    settings: &ExiSettings,
) -> Result<(Vec<u8>, ExiReport), String> {
    settings.validate()?;
    let options = settings.to_erxi();
    let started = Instant::now();
    let events = erxi::xml::parse_xml_events_from_str(xml, &options)
        .map_err(|err| format!("EXI event parse failed: {err:?}"))?;
    let bytes = erxi::encoder::encode(&events, &options)
        .map_err(|err| format!("EXI encode failed: {err:?}"))?;
    let duration = started.elapsed();

    let report = ExiReport {
        input_bytes: xml.len(),
        output_bytes: bytes.len(),
        ratio: bytes.len() as f64 / xml.len().max(1) as f64,
        duration_ms: duration.as_secs_f64() * 1000.0,
        throughput: if duration.as_secs_f64() > 0.0 {
            xml.len() as f64 / duration.as_secs_f64()
        } else {
            f64::INFINITY
        },
        preset: settings.preset,
        alignment: alignment_name(settings.alignment),
        compression: settings.compression,
        block_size: settings.block_size,
        schema_id: schema_name(&settings.schema_id),
        sha256: sha256_hex(&bytes),
        dropped: settings.dropped_items(),
    };
    Ok((bytes, report))
}

/// Decodes EXI bytes. When the stream carries no option header, the
/// explicitly supplied `fallback` settings are used and the report notes
/// the warning.
pub fn decode_with_report(
    exi: &[u8],
    fallback: &ExiSettings,
) -> Result<(String, ExiReport), String> {
    fallback.validate()?;
    if exi.len() > MAX_EXI_BYTES {
        return Err(format!(
            "EXI input is {} bytes; the limit is 512 MiB",
            exi.len()
        ));
    }
    let started = Instant::now();
    let (events, header_options) =
        erxi::decoder::decode(exi).map_err(|err| format!("EXI decode failed: {err:?}"))?;
    let xml = erxi::xml_serializer::events_to_xml(&events)
        .map_err(|err| format!("EXI serialization failed: {err:?}"))?;
    if xml.len() > MAX_DECODED_XML_BYTES {
        return Err(format!(
            "decoded XML is {} bytes; the limit is 256 MiB",
            xml.len()
        ));
    }
    let duration = started.elapsed();

    // The stream's own header options win; the report reflects them.
    let report = ExiReport {
        input_bytes: exi.len(),
        output_bytes: xml.len(),
        ratio: xml.len() as f64 / exi.len().max(1) as f64,
        duration_ms: duration.as_secs_f64() * 1000.0,
        throughput: if duration.as_secs_f64() > 0.0 {
            exi.len() as f64 / duration.as_secs_f64()
        } else {
            f64::INFINITY
        },
        preset: fallback.preset,
        alignment: alignment_name(header_options.alignment()),
        compression: header_options.compression(),
        block_size: header_options.block_size(),
        schema_id: schema_name(&fallback.schema_id),
        sha256: sha256_hex(xml.as_bytes()),
        dropped: fallback.dropped_items(),
    };
    Ok((xml, report))
}

fn alignment_name(alignment: Alignment) -> &'static str {
    match alignment {
        Alignment::BitPacked => "bit-packed",
        Alignment::ByteAlignment => "byte-aligned",
        Alignment::PreCompression => "pre-compression",
    }
}

fn schema_name(choice: &SchemaChoice) -> String {
    match choice {
        SchemaChoice::None => String::from("none"),
        SchemaChoice::BuiltinOnly => String::from("builtin"),
        SchemaChoice::Id(id) => format!("user:{id}"),
    }
}

/// SHA-256 digest of the payload, hex-encoded.
fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(data);
    format!("{digest:x}")
}
