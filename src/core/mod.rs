//! Core document model: the authoritative XML document, editing commands,
//! incremental history, and diagnostics.
//!
//! Everything here is backend-agnostic from the caller's perspective: the
//! engine DOM, the current source text, per-node source ranges, indexes,
//! and the revision counter live behind [`document::XmlDocument`]. Mutations
//! happen exclusively through [`command::Command`], which validates fully
//! before committing, returns the reverse command plus a [`command::ChangedSet`],
//! and never leaves the document half-edited on failure.

pub mod command;
pub mod diagnostic;
pub mod document;
pub mod history;

pub use command::{ChangedSet, Command, CommandError, InsertPosition, NewNode, NodeContent};
pub use diagnostic::{Diagnostic, Severity};
pub use document::{NodeId, QName, QNameSpec, Revision, SourceRange, XmlDocument, XmlNodeKind};
pub use history::{History, HistoryLimits};
