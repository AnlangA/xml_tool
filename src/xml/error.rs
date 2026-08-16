//! Stable error codes and source locations for the XML engine.
//!
//! Every engine failure carries a machine-readable [`XmlErrorCode`], a
//! human-readable message, and — whenever the underlying parser can report
//! it — a 1-based [`SourceLocation`] in the original input. Callers must
//! never have to string-match to distinguish failure classes.

use std::fmt;

/// Machine-readable failure classes for the XML engine.
///
/// Codes are stable across releases: diagnostics and tests may persist them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum XmlErrorCode {
    /// Input exceeds the configured maximum open size.
    InputTooLarge,
    /// Bytes do not decode under the detected encoding.
    EncodingUndecodable,
    /// The declared encoding is one this tool does not support.
    EncodingUnsupported,
    /// The detected byte encoding contradicts the XML declaration.
    EncodingMismatch,
    /// Malformed markup (broken tags, bad characters, truncation).
    Syntax,
    /// A well-formedness or namespace constraint violation.
    WellFormedness,
    /// A namespace-specific constraint violation.
    Namespace,
    /// Entity expansion exceeded the configured byte budget.
    EntityBudgetExceeded,
    /// Element nesting exceeded the configured maximum depth.
    NestingTooDeep,
    /// A reference to an entity that was never declared.
    EntityUndefined,
    /// Filesystem failure while reading or writing a document.
    Io,
}

impl XmlErrorCode {
    /// Stable snake_case identifier for the code.
    pub fn as_str(self) -> &'static str {
        match self {
            XmlErrorCode::InputTooLarge => "input_too_large",
            XmlErrorCode::EncodingUndecodable => "encoding_undecodable",
            XmlErrorCode::EncodingUnsupported => "encoding_unsupported",
            XmlErrorCode::EncodingMismatch => "encoding_mismatch",
            XmlErrorCode::Syntax => "syntax",
            XmlErrorCode::WellFormedness => "well_formedness",
            XmlErrorCode::Namespace => "namespace",
            XmlErrorCode::EntityBudgetExceeded => "entity_budget_exceeded",
            XmlErrorCode::NestingTooDeep => "nesting_too_deep",
            XmlErrorCode::EntityUndefined => "entity_undefined",
            XmlErrorCode::Io => "io",
        }
    }
}

impl fmt::Display for XmlErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A 1-based line/column position in the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceLocation {
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number, counted in characters.
    pub column: usize,
}

/// An XML engine failure with a stable code and optional source location.
#[derive(Debug, Clone)]
pub struct XmlError {
    code: XmlErrorCode,
    message: String,
    location: Option<SourceLocation>,
}

impl XmlError {
    /// Creates an error without a source location.
    pub fn new(code: XmlErrorCode, message: impl Into<String>) -> Self {
        XmlError {
            code,
            message: message.into(),
            location: None,
        }
    }

    /// Creates an error located at a 1-based line/column pair.
    pub fn at(code: XmlErrorCode, message: impl Into<String>, location: SourceLocation) -> Self {
        XmlError {
            code,
            message: message.into(),
            location: Some(location),
        }
    }

    /// Stable failure class.
    pub fn code(&self) -> XmlErrorCode {
        self.code
    }

    /// Human-readable detail. Never empty.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// 1-based source location, when the parser reported one.
    pub fn location(&self) -> Option<SourceLocation> {
        self.location
    }

    /// Attaches a source location, keeping any existing one.
    pub(crate) fn with_location(mut self, location: SourceLocation) -> Self {
        if self.location.is_none() {
            self.location = Some(location);
        }
        self
    }

    /// Converts an `uppsala` parse failure, preserving line/column and
    /// classifying budget/depth/entity failures into their dedicated codes.
    pub(crate) fn from_uppsala(err: uppsala::error::XmlError) -> Self {
        use uppsala::error::XmlError as U;
        let (code, message, line, column) = match &err {
            U::Parse(e) => (
                classify_message(&e.message, XmlErrorCode::Syntax),
                e.message.clone(),
                Some(e.line),
                Some(e.column),
            ),
            U::WellFormedness(e) => (
                classify_message(&e.message, XmlErrorCode::WellFormedness),
                e.message.clone(),
                Some(e.line),
                Some(e.column),
            ),
            U::Namespace(e) => (
                classify_message(&e.message, XmlErrorCode::Namespace),
                e.message.clone(),
                Some(e.line),
                Some(e.column),
            ),
            U::XPath(e) => (XmlErrorCode::Syntax, e.message.clone(), None, None),
            U::Validation(e) => (
                XmlErrorCode::WellFormedness,
                e.message.clone(),
                e.line,
                e.column,
            ),
            U::UnexpectedEof => (
                XmlErrorCode::Syntax,
                String::from("unexpected end of input"),
                None,
                None,
            ),
        };
        let error = XmlError::new(code, message);
        match (line, column) {
            (Some(line), Some(column)) if line > 0 || column > 0 => {
                error.with_location(SourceLocation { line, column })
            }
            _ => error,
        }
    }
}

/// Upgrades generic parse errors to dedicated codes when the message
/// identifies a budget, depth, or unknown-entity failure.
fn classify_message(message: &str, fallback: XmlErrorCode) -> XmlErrorCode {
    if message.contains("Entity expansion exceeds") {
        XmlErrorCode::EntityBudgetExceeded
    } else if message.contains("Unknown entity reference") {
        XmlErrorCode::EntityUndefined
    } else if message.contains("nesting") && message.contains("depth") {
        XmlErrorCode::NestingTooDeep
    } else {
        fallback
    }
}

impl fmt::Display for XmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)?;
        if let Some(loc) = self.location {
            write!(f, " (at line {}, column {})", loc.line, loc.column)?;
        }
        Ok(())
    }
}

impl std::error::Error for XmlError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_includes_code_and_location() {
        let err = XmlError::at(
            XmlErrorCode::Syntax,
            "mismatched end tag",
            SourceLocation { line: 3, column: 7 },
        );
        assert_eq!(
            err.to_string(),
            "[syntax] mismatched end tag (at line 3, column 7)"
        );
        assert_eq!(err.code(), XmlErrorCode::Syntax);
        assert_eq!(err.location(), Some(SourceLocation { line: 3, column: 7 }));
    }

    #[test]
    fn code_identifiers_are_stable() {
        assert_eq!(XmlErrorCode::InputTooLarge.as_str(), "input_too_large");
        assert_eq!(
            XmlErrorCode::EntityBudgetExceeded.as_str(),
            "entity_budget_exceeded"
        );
        assert_eq!(XmlErrorCode::NestingTooDeep.as_str(), "nesting_too_deep");
    }

    #[test]
    fn with_location_keeps_existing_location() {
        let first = SourceLocation { line: 1, column: 1 };
        let second = SourceLocation { line: 9, column: 9 };
        let err = XmlError::new(XmlErrorCode::Io, "boom")
            .with_location(first)
            .with_location(second);
        assert_eq!(err.location(), Some(first));
    }

    #[test]
    fn uppsala_budget_error_maps_to_dedicated_code() {
        let inner = uppsala::error::XmlError::parse(
            "Entity expansion exceeds configured limit (0 bytes remaining)",
            4,
            12,
        );
        let err = XmlError::from_uppsala(inner);
        assert_eq!(err.code(), XmlErrorCode::EntityBudgetExceeded);
        assert_eq!(
            err.location(),
            Some(SourceLocation {
                line: 4,
                column: 12
            })
        );
    }

    #[test]
    fn uppsala_unknown_entity_maps_to_dedicated_code() {
        let inner =
            uppsala::error::XmlError::well_formedness("Unknown entity reference: &missing;", 2, 5);
        let err = XmlError::from_uppsala(inner);
        assert_eq!(err.code(), XmlErrorCode::EntityUndefined);
    }
}
