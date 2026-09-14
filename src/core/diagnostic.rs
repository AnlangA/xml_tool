//! Unified diagnostic structure for problems surfaced by the engine,
//! commands, validation, and diff services.

use std::collections::BTreeMap;

use super::document::{NodeId, SourceRange};

/// How severe a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Severity {
    /// Informational note.
    Info,
    /// Recoverable or stylistic problem.
    Warning,
    /// Operation failed or document invalid.
    Error,
}

/// A single problem attached to a document.
///
/// `message_key` + `arguments` carry a localization-ready message (step 5);
/// `source_range` and `node` anchor the problem in the document.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub message_key: String,
    pub arguments: BTreeMap<String, String>,
    pub source_range: Option<SourceRange>,
    pub node: Option<NodeId>,
}

impl Diagnostic {
    /// Builds a diagnostic with no location.
    pub fn new(
        severity: Severity,
        code: impl Into<String>,
        message_key: impl Into<String>,
    ) -> Self {
        Diagnostic {
            severity,
            code: code.into(),
            message_key: message_key.into(),
            arguments: BTreeMap::new(),
            source_range: None,
            node: None,
        }
    }

    /// Attaches a node (and, when known, its source range).
    pub fn at_node(mut self, node: NodeId, range: Option<SourceRange>) -> Self {
        self.node = Some(node);
        self.source_range = range;
        self
    }

    /// Adds one message argument.
    pub fn with_argument(mut self, key: &str, value: impl Into<String>) -> Self {
        self.arguments.insert(key.to_string(), value.into());
        self
    }
}
