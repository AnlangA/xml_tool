use anyhow::Result;
use erxi::ExiOptions;
use erxi::encoder::encode;

/// Encode an XML string to EXI binary format.
pub fn encode_xml_to_exi(xml_content: &str) -> Result<Vec<u8>> {
    let opts = ExiOptions::default();
    let events = erxi::xml::parse_xml_events_from_str(xml_content, &opts)?;
    let bytes = encode(&events, &opts)?;
    Ok(bytes)
}
