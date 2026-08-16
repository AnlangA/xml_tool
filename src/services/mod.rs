//! Background services: task scheduling, safe document I/O, workspace
//! sessions, crash recovery, and external file-change monitoring.
//!
//! Nothing in this module touches the UI; the egui shell polls these from
//! its frame loop. Heavy work (file reads, parsing, serialization, EXI
//! codecs, image decoding) runs on worker threads and results are delivered
//! only when the `JobId + SessionId + Revision` triple still matches the
//! live document, so stale completions can never clobber newer edits.

pub mod document_io;
pub mod recovery;
pub mod task_manager;
pub mod watcher;
pub mod workspace;

pub use document_io::{OpenOutcome, save_bytes_atomically};
pub use recovery::{RecoverySnapshot, RecoveryStore};
pub use task_manager::SessionId;
pub use task_manager::{CancelFlag, JobId, JobOutcome, TaskManager, TaskSpec};
pub use workspace::{DocumentMode, DocumentSession, FileType, WorkspaceState};
