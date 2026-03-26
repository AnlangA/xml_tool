use anyhow::{Context, Result};
use erxi::encoder::encode;
use erxi::options::{ExiOptions, Preserve};

/// Encode an XML string to EXI binary format.
pub fn encode_xml_to_exi(xml_content: &str) -> Result<Vec<u8>> {
    let opts = editor_exi_options();
    let events = erxi::xml::parse_xml_events_from_str(xml_content, &opts)
        .context("failed to parse XML into EXI events")?;
    let bytes = encode(&events, &opts).context("failed to encode EXI stream")?;
    Ok(bytes)
}

fn editor_exi_options() -> ExiOptions {
    let mut opts = ExiOptions::default();
    opts.set_preserve(Preserve {
        comments: true,
        prefixes: true,
        ..Preserve::default()
    });
    opts
}
