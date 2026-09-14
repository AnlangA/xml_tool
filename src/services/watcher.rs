//! External file-change monitoring with a fixed 500 ms debounce.
//!
//! The shell watches every open session's file. Events are debounced per
//! path and surfaced as [`FileChange`] decisions; the UI turns them into
//! the plan's banners: clean documents get Reload/Ignore, dirty documents
//! get Compare/Reload/Keep (Reload requiring explicit confirmation).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::Watcher;

/// Debounce window fixed by the plan.
pub const DEBOUNCE: Duration = Duration::from_millis(500);

/// A debounced, interpretable change for one watched path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileChange {
    /// File content changed on disk.
    Modified(PathBuf),
    /// File was removed or renamed away.
    Removed(PathBuf),
}

impl FileChange {
    /// The watched path this change refers to.
    pub fn path(&self) -> &Path {
        match self {
            FileChange::Modified(path) | FileChange::Removed(path) => path,
        }
    }
}

/// Shared event queue between the notify callback thread and the shell.
type SharedEvents = Arc<Mutex<Vec<(PathBuf, Instant)>>>;

/// A debouncing wrapper over notify's recommended watcher.
pub struct FileWatcher {
    watcher: notify::RecommendedWatcher,
    events: SharedEvents,
    watched: Vec<PathBuf>,
}

impl FileWatcher {
    /// Creates a watcher; events land in an internal queue until polled.
    pub fn new() -> notify::Result<FileWatcher> {
        let events: SharedEvents = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let watcher =
            notify::recommended_watcher(move |event: Result<notify::Event, notify::Error>| {
                if let Ok(event) = event {
                    let mut queue = sink.lock().expect("watcher queue lock");
                    for path in event.paths {
                        queue.push((path, Instant::now()));
                    }
                }
            })?;
        Ok(FileWatcher {
            watcher,
            events,
            watched: Vec::new(),
        })
    }

    /// Starts watching `path` (idempotent).
    pub fn watch(&mut self, path: &Path) -> notify::Result<()> {
        let absolute = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !self.watched.contains(&absolute) {
            self.watcher
                .watch(&absolute, notify::RecursiveMode::NonRecursive)?;
            self.watched.push(absolute);
        }
        Ok(())
    }

    /// Stops watching `path`.
    pub fn unwatch(&mut self, path: &Path) {
        let absolute = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if self.watched.contains(&absolute) {
            let _ = self.watcher.unwatch(&absolute);
            self.watched.retain(|watched| *watched != absolute);
        }
    }

    /// Collects debounced changes: events for one path collapse when they
    /// arrive within [`DEBOUNCE`] of each other; only paths whose last
    /// event is older than the window are reported. Removal is detected by
    /// the path no longer existing.
    pub fn poll_changes(&mut self) -> Vec<FileChange> {
        let now = Instant::now();
        let mut latest: HashMap<PathBuf, Instant> = HashMap::new();
        let mut reported: Vec<PathBuf> = Vec::new();
        {
            let mut queue = self.events.lock().expect("watcher queue lock");
            // Events stay queued until they are old enough to report (or age
            // out entirely); draining them early would lose every event that
            // arrived between polls.
            queue.retain(|(_, at)| now.duration_since(*at) < DEBOUNCE * 8);
            for (path, at) in queue.iter() {
                latest
                    .entry(path.clone())
                    .and_modify(|kept| {
                        if *kept < *at {
                            *kept = *at;
                        }
                    })
                    .or_insert(*at);
            }
            for (path, at) in latest {
                if now.duration_since(at) >= DEBOUNCE {
                    reported.push(path);
                }
            }
            if !reported.is_empty() {
                queue.retain(|(path, _)| !reported.contains(path));
            }
        }
        let mut changes: Vec<FileChange> = reported
            .into_iter()
            .map(|path| {
                if path.exists() {
                    FileChange::Modified(path)
                } else {
                    FileChange::Removed(path)
                }
            })
            .collect();
        changes.sort_by(|a, b| a.path().cmp(b.path()));
        changes
    }
}
