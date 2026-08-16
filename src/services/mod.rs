//! Background services: task scheduling, safe document I/O, workspace
//! sessions, crash recovery, and external file-change monitoring.
//!
//! Nothing in this module touches the UI; the egui shell polls these from
//! its frame loop. Heavy work (file reads, parsing, serialization, EXI
//! codecs, image decoding) runs on worker threads and results are delivered
//! only when the `JobId + SessionId + Revision` triple still matches the
//! live document, so stale completions can never clobber newer edits.

pub mod diff;
pub mod document_io;
pub mod exi_workbench;
pub mod frame_observer;
pub mod large_file;
pub mod outline;
pub mod recovery;
pub mod replace;
pub mod search;
pub mod session_cache;
pub mod source_buffer;
pub mod source_editor;
pub mod task_manager;
pub mod validation;
pub mod watcher;
pub mod workspace;
pub mod xpath;

pub use document_io::{OpenOutcome, save_bytes_atomically};
pub use recovery::{RecoverySnapshot, RecoveryStore};
pub use task_manager::SessionId;
pub use task_manager::{CancelFlag, JobId, JobOutcome, TaskManager, TaskSpec};
pub use workspace::{DocumentMode, DocumentSession, FileType, WorkspaceState};
