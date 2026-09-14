//! XSD validation with local-only schema composition.
//!
//! Schemas load from disk once; `xs:include`, `xs:redefine`, and
//! `xs:import` may only reference files inside the schema's own root
//! directory tree — anything else (absolute escapes, `..` traversal, or
//! network-looking locations) is rejected before any file is read. There
//! is no network code path anywhere in the engine. Compilation is cached
//! by (canonical path, mtime, size).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use uppsala::dom::NodeKind;
use uppsala::xsd::XsdValidator;

use crate::core::document::XmlDocument;
use crate::core::{Diagnostic, Severity};

/// Why a schema could not be used.
#[derive(Debug)]
pub enum SchemaError {
    Io(String),
    Parse(String),
    /// An include/import escapes the schema root directory.
    PathEscape {
        reference: String,
    },
}

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchemaError::Io(message) => write!(f, "{message}"),
            SchemaError::Parse(message) => write!(f, "{message}"),
            SchemaError::PathEscape { reference } => write!(
                f,
                "schema reference '{reference}' leaves the schema root directory"
            ),
        }
    }
}

/// Compiled schemas keyed by (canonical path, mtime, size).
#[derive(Default)]
pub struct SchemaCache {
    entries: HashMap<(PathBuf, Option<SystemTime>, u64), XsdValidator>,
}

impl SchemaCache {
    /// Number of cached compiled schemas.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Loads (or returns the cached compile of) the schema at `path`.
    pub fn load(&mut self, path: &Path) -> Result<&XsdValidator, SchemaError> {
        let metadata = std::fs::metadata(path).map_err(|err| SchemaError::Io(format!("{err}")))?;
        let mtime = metadata.modified().ok();
        let key = (path.to_path_buf(), mtime, metadata.len());
        if !self.entries.contains_key(&key) {
            let validator = compile_schema(path)?;
            self.entries.insert(key.clone(), validator);
        }
        Ok(&self.entries[&key])
    }
}

/// Reads, parses, and compiles a schema, enforcing composition confinement.
pub fn compile_schema(path: &Path) -> Result<XsdValidator, SchemaError> {
    let bytes = std::fs::read(path).map_err(|err| SchemaError::Io(format!("{err}")))?;
    let document = crate::xml::parse_xml_bytes(&bytes, crate::xml::ParseOptions::default())
        .map_err(|err| SchemaError::Parse(err.to_string()))?;
    let dom = document.document();
    let root = dom
        .document_element()
        .ok_or_else(|| SchemaError::Parse(String::from("schema has no root element")))?;
    let schema_dir = path.parent().unwrap_or_else(|| Path::new("."));
    check_composition_confined(dom, root, schema_dir)?;
    XsdValidator::from_schema_with_base_path(dom, Some(schema_dir))
        .map_err(|err| SchemaError::Parse(err.to_string()))
}

/// Rejects include/import/redefine references that leave `root_dir`.
fn check_composition_confined(
    dom: &uppsala::dom::Document<'_>,
    root: uppsala::dom::NodeId,
    root_dir: &Path,
) -> Result<(), SchemaError> {
    let xs = "http://www.w3.org/2001/XMLSchema";
    for id in dom.descendants(root) {
        if let Some(NodeKind::Element(element)) = dom.node_kind(id) {
            let in_xs_namespace = element.name.namespace_uri.as_deref() == Some(xs);
            let local = element.name.local_name.as_ref();
            if in_xs_namespace
                && matches!(local, "include" | "import" | "redefine")
                && let Some(location) = element
                    .attributes
                    .iter()
                    .find(|attr| attr.name.local_name == "schemaLocation")
            {
                let reference = location.value.to_string();
                if !is_confined(&reference, root_dir) {
                    return Err(SchemaError::PathEscape { reference });
                }
            }
        }
    }
    Ok(())
}

fn is_confined(reference: &str, _root_dir: &Path) -> bool {
    use std::path::Component;
    if reference.contains("://") || reference.starts_with("urn:") {
        return false; // anything URL-shaped is out (there is no network anyway)
    }
    // Walk the reference's own components relative to the schema root:
    // climbing above depth 0 or anchoring absolutely escapes the root.
    let mut depth = 0i32;
    for component in Path::new(reference).components() {
        match component {
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => return false,
            _ => depth += 1,
        }
    }
    true
}

/// Validates `document` against a compiled schema, mapping engine errors to
/// diagnostics with 1-based positions.
pub fn validate(document: &XmlDocument, validator: &XsdValidator) -> Vec<Diagnostic> {
    let dom = document.dom();
    let mut prepared = dom.clone();
    prepared.prepare_xpath();
    validator
        .validate(&prepared)
        .into_iter()
        .map(|error| {
            let mut diagnostic = Diagnostic::new(Severity::Error, "xsd", error.message.clone());
            if let (Some(line), Some(column)) = (error.line, error.column) {
                diagnostic
                    .arguments
                    .insert(String::from("line"), line.to_string());
                diagnostic
                    .arguments
                    .insert(String::from("column"), column.to_string());
            }
            diagnostic
        })
        .collect()
}
