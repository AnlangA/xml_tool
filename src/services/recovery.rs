//! Crash-recovery snapshots for dirty documents.
//!
//! Every 30 seconds the shell snapshots each *dirty* session into a
//! per-session file under the recovery directory: the full source text,
//! the path (when the document has one), cursor offset, selected-node
//! path, and a timestamp. Snapshots are deleted on successful save or
//! explicit discard. At startup, any surviving snapshot shows the recovery
//! selection page — never an automatic overwrite of the on-disk file.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Serialized snapshot of one dirty session at a save point.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecoverySnapshot {
    /// Session tab title at snapshot time.
    pub title: String,
    /// On-disk path, when the document had one.
    pub path: Option<PathBuf>,
    /// Full decoded source text.
    pub source: String,
    /// Whether the tab showed unsaved changes.
    pub dirty: bool,
    /// Byte offset of the source cursor.
    pub cursor: usize,
    /// XPath-like path of the selected node (root/child[2]/...), when any.
    pub selection_path: Option<String>,
    /// Unix-epoch seconds when the snapshot was written.
    pub written_at: u64,
}

/// Directory of per-session snapshot files.
pub struct RecoveryStore {
    root: PathBuf,
}

impl RecoveryStore {
    /// A store rooted at `root` (created on first write).
    pub fn new(root: impl Into<PathBuf>) -> RecoveryStore {
        RecoveryStore { root: root.into() }
    }

    /// Platform-default location (`~/.local/share/xml_tool/recovery` on
    /// Linux, platform dirs elsewhere via `dirs`-free fallback).
    pub fn default_root() -> PathBuf {
        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".local/share/xml_tool/recovery")
        } else {
            PathBuf::from(".xml_tool/recovery")
        }
    }

    fn snapshot_path(&self, session: u64) -> PathBuf {
        self.root.join(format!("session-{session}.json"))
    }

    /// Persists `snapshot` for `session`, replacing any previous one.
    pub fn write(&self, session: u64, snapshot: &RecoverySnapshot) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        let mut resolved = snapshot.clone();
        resolved.written_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since| since.as_secs())
            .unwrap_or(0);
        let json = serde_json::to_string_pretty(&resolved)?;
        // Write-then-rename so a crash mid-write cannot corrupt a snapshot.
        let final_path = self.snapshot_path(session);
        let temp_path = self.root.join(format!("session-{session}.json.tmp"));
        std::fs::write(&temp_path, json.as_bytes())?;
        std::fs::rename(&temp_path, &final_path)
    }

    /// Removes the snapshot for `session` (after save or discard).
    pub fn remove(&self, session: u64) {
        let _ = std::fs::remove_file(self.snapshot_path(session));
    }

    /// Loads every surviving snapshot, oldest first.
    pub fn load_all(&self) -> Vec<(u64, RecoverySnapshot)> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut snapshots = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let Some(session) = stem
                .strip_prefix("session-")
                .and_then(|digits| digits.parse::<u64>().ok())
            else {
                continue;
            };
            if let Ok(text) = std::fs::read_to_string(&path)
                && let Ok(snapshot) = serde_json::from_str::<RecoverySnapshot>(&text)
            {
                snapshots.push((session, snapshot));
            }
        }
        snapshots.sort_by_key(|(_, snapshot)| snapshot.written_at);
        snapshots
    }

    /// The store's directory (for tests and diagnostics).
    pub fn root(&self) -> &Path {
        &self.root
    }
}
