use anyhow::{Context, Result};
use erxi::decoder::decode;
use erxi::xml_serializer::events_to_xml;

use crate::xml::{XmlDocument, parse_xml};

/// Decode EXI binary data to an [`XmlDocument`].
pub fn decode_exi_to_xml(exi_data: &[u8]) -> Result<XmlDocument> {
    let (events, _opts) = decode(exi_data).context("failed to decode EXI stream")?;
    let xml_str = events_to_xml(&events).context("failed to serialise EXI events to XML")?;
    parse_xml(&xml_str).context("decoded EXI payload produced XML the parser could not model")
}
