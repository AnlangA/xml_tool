pub mod parser;
pub mod tree;

pub use parser::{parse_xml, parse_xml_file, serialize_xml};
pub use tree::{XmlAttribute, XmlDocument, XmlElement, XmlNode, truncate_str};
