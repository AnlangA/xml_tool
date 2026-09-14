//! XML parsing, serialization, and editable tree-model primitives.
//!
//! Two layers live here:
//!
//! - [`engine`] is the isolation layer over the pinned `uppsala` backend:
//!   byte-level parsing with encoding detection, fidelity-preserving
//!   serialization, source ranges, and the plan's security limits. New code
//!   (and everything built in later plan steps) uses this.
//! - [`parser`] and [`tree`] are the legacy compatibility facade: the
//!   editable tree model with `anyhow`-based signatures kept for existing
//!   callers.
//!
//! [`encoding`] and [`error`] provide the shared encoding detection and the
//! stable error-code vocabulary used by both layers.

pub mod encoding;
pub mod engine;
pub mod error;
pub mod parser;
pub mod tree;

pub use encoding::SourceEncoding;
pub use engine::{
    EngineDocument, ParseOptions, SecurityLimits, SerializeOptions, UppsalaXmlEngine, XmlEngine,
    parse_xml_bytes, serialize_xml_bytes, serialize_xml_with_options,
};
pub use error::{SourceLocation, XmlError, XmlErrorCode};
pub use parser::{parse_xml, parse_xml_file, serialize_xml};
pub use tree::{XmlAttribute, XmlDocument, XmlElement, XmlNode, truncate_str};
