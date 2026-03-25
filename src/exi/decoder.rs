use anyhow::Result;
use erxi::decoder::decode;
use erxi::xml_serializer::events_to_xml;

use crate::xml::{parse_xml, XmlDocument};

/// Decode EXI binary data to an [`XmlDocument`].
pub fn decode_exi_to_xml(exi_data: &[u8]) -> Result<XmlDocument> {
    let (events, _opts) = decode(exi_data)?;
    let xml_str = events_to_xml(&events)?;
    parse_xml(&xml_str)
}
