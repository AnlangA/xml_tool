pub mod parser;
pub mod tree;

pub use tree::{truncate_str, XmlAttribute, XmlDocument, XmlElement, XmlNode};
pub use parser::{parse_xml, parse_xml_file, serialize_xml};
