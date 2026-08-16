//! Incremental undo/redo history with a memory budget and input coalescing.
//!
//! History stores command pairs (forward + reverse), never document
//! snapshots. Consecutive edits to the same field within the coalescing
//! window (750 ms) merge into one entry: the entry keeps the first forward
//! command and the latest reverse command, so one undo rolls the whole
//! typing burst back and one redo replays it.
//!
//! The dirty flag compares the undo-stack depth against the saved cursor:
//! undoing back to the save point makes the document clean again.

use std::time::{Duration, Instant};

use super::command::{ChangedSet, Command};
use super::document::XmlDocument;

/// Budget knobs; the production values are fixed by the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryLimits {
    /// Maximum number of undo entries.
    pub max_entries: usize,
    /// Maximum total estimated memory of undo entries (bytes).
    pub max_memory_bytes: usize,
    /// Editing bursts within this window merge into one entry.
    pub coalesce_window: Duration,
}

impl Default for HistoryLimits {
    fn default() -> Self {
        HistoryLimits {
            max_entries: 1_000,
            max_memory_bytes: 64 * 1024 * 1024,
            coalesce_window: Duration::from_millis(750),
        }
    }
}

/// Clock injection point so coalescing tests do not need to sleep.
pub(crate) struct HistoryClock {
    pub now: fn() -> Instant,
}

impl Default for HistoryClock {
    fn default() -> Self {
        HistoryClock { now: Instant::now }
    }
}

struct HistoryEntry {
    /// Command as first applied (redo target after undo).
    forward: Command,
    /// Command restoring the pre-apply state (undo target).
    reverse: Command,
    /// Coalescing key of the *latest* merged command, if any.
    coalesce_key: Option<super::command::CoalesceKey>,
    /// When the entry was last touched.
    committed_at: Instant,
    /// Rough memory estimate for budget enforcement.
    estimated_bytes: usize,
}

/// Undo/redo stacks plus the save cursor. One history per document.
pub struct History {
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    /// Number of applied commands at the last `mark_saved`; `None` until
    /// the first save.
    saved_cursor: Option<usize>,
    memory_bytes: usize,
    limits: HistoryLimits,
    pub(crate) clock: HistoryClock,
}

impl History {
    /// A history with the production limits.
    pub fn new() -> History {
        History::with_limits(HistoryLimits::default())
    }

    /// A history with explicit limits.
    pub fn with_limits(limits: HistoryLimits) -> History {
        History {
            undo: Vec::new(),
            redo: Vec::new(),
            saved_cursor: None,
            memory_bytes: 0,
            limits,
            clock: HistoryClock::default(),
        }
    }

    /// Number of undoable commands.
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// Number of redoable commands.
    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// Estimated memory held by history entries.
    pub fn memory_bytes(&self) -> usize {
        self.memory_bytes
    }

    /// Whether the document differs from its last saved state.
    pub fn is_dirty(&self) -> bool {
        match self.saved_cursor {
            Some(cursor) => self.undo.len() != cursor,
            None => !self.undo.is_empty(),
        }
    }

    /// Records that the current state is on disk.
    pub fn mark_saved(&mut self) {
        self.saved_cursor = Some(self.undo.len());
    }

    /// Applies `command` and records it. Coalesces with the previous entry
    /// when both edit the same field inside the window.
    pub fn commit(
        &mut self,
        document: &mut XmlDocument,
        command: Command,
    ) -> Result<ChangedSet, super::command::CommandError> {
        let key = command.coalesce_key();
        let (reverse, changed) = command.apply(document)?;
        let now = (self.clock.now)();

        let should_coalesce = match (&key, self.undo.last()) {
            (Some(key), Some(last)) => {
                last.coalesce_key.as_ref() == Some(key)
                    && now.duration_since(last.committed_at) <= self.limits.coalesce_window
            }
            _ => false,
        };

        if should_coalesce {
            // Merge the burst: keep the entry's reverse (it restores the
            // pre-burst state) and adopt this command as the forward (it
            // reproduces the cumulative value on redo).
            let last = self.undo.last_mut().expect("checked above");
            let reverse_bytes = estimate_command_bytes(&last.reverse);
            last.forward = command;
            last.coalesce_key = key;
            last.committed_at = now;
            let new_estimate = estimate_command_bytes(&last.forward) + reverse_bytes;
            self.memory_bytes += new_estimate.saturating_sub(last.estimated_bytes);
            last.estimated_bytes = new_estimate;
        } else {
            let estimate = estimate_command_bytes(&command) + estimate_command_bytes(&reverse);
            self.memory_bytes += estimate;
            self.undo.push(HistoryEntry {
                forward: command,
                reverse,
                coalesce_key: key,
                committed_at: now,
                estimated_bytes: estimate,
            });
        }

        // A new commit invalidates every redo.
        for entry in self.redo.drain(..) {
            self.memory_bytes = self.memory_bytes.saturating_sub(entry.estimated_bytes);
        }
        self.evict_over_budget();
        Ok(changed)
    }

    /// Undoes the most recent command. Returns the change set of the
    /// reversal, or `None` when there is nothing to undo. A failed reversal
    /// keeps the entry on the stack instead of silently dropping history.
    pub fn undo(&mut self, document: &mut XmlDocument) -> Option<ChangedSet> {
        let entry = self.undo.pop()?;
        match entry.reverse.apply(document) {
            Ok((reverse_of_reverse, changed)) => {
                self.redo.push(HistoryEntry {
                    forward: entry.forward,
                    reverse: reverse_of_reverse,
                    coalesce_key: entry.coalesce_key,
                    committed_at: entry.committed_at,
                    estimated_bytes: entry.estimated_bytes,
                });
                Some(changed)
            }
            Err(_) => {
                self.undo.push(entry);
                None
            }
        }
    }

    /// Redoes the most recently undone command.
    pub fn redo(&mut self, document: &mut XmlDocument) -> Option<ChangedSet> {
        let entry = self.redo.pop()?;
        match entry.forward.apply(document) {
            Ok((reverse_of_forward, changed)) => {
                self.undo.push(HistoryEntry {
                    forward: entry.forward,
                    reverse: reverse_of_forward,
                    coalesce_key: entry.coalesce_key,
                    committed_at: (self.clock.now)(),
                    estimated_bytes: entry.estimated_bytes,
                });
                Some(changed)
            }
            Err(_) => {
                self.redo.push(entry);
                None
            }
        }
    }

    /// Drops all history (used by reload/discard flows).
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.memory_bytes = 0;
        self.saved_cursor = None;
    }

    /// Evicts oldest entries until both budgets hold. The saved cursor
    /// shifts with the undo stack so a document that was clean stays clean.
    fn evict_over_budget(&mut self) {
        while self.undo.len() > self.limits.max_entries
            || self.memory_bytes > self.limits.max_memory_bytes && !self.undo.is_empty()
        {
            let evicted = self.undo.remove(0);
            self.memory_bytes = self.memory_bytes.saturating_sub(evicted.estimated_bytes);
            if let Some(cursor) = self.saved_cursor.as_mut()
                && *cursor > 0
            {
                *cursor -= 1;
            }
        }
    }
}

impl Default for History {
    fn default() -> Self {
        History::new()
    }
}

/// Rough memory estimate: fragment strings dominate; the enum shells are a
/// small constant each.
fn estimate_command_bytes(command: &Command) -> usize {
    const COMMAND_OVERHEAD: usize = 96;
    let payload = match command {
        Command::ReplaceWholeSource { new_source }
        | Command::RestoreNode {
            bytes: new_source, ..
        } => new_source.len(),
        Command::BatchReplace { ops } => ops
            .iter()
            .map(|op| op.old_value.len() + op.new_value.len() + 64)
            .sum(),
        Command::SetAttributeValue { value, .. } | Command::AddAttribute { value, .. } => {
            value.len()
        }
        Command::SetNodeContent { content, .. } => match content {
            super::command::NodeContent::Text(text)
            | super::command::NodeContent::CData(text)
            | super::command::NodeContent::Comment(text) => text.len(),
            super::command::NodeContent::ProcessingInstruction { target, data } => {
                target.len() + data.as_deref().map_or(0, str::len)
            }
        },
        Command::InsertNode { node, .. } => match node {
            super::command::NewNode::Element { name }
            | super::command::NewNode::ProcessingInstruction { target: name, .. } => name.len(),
            super::command::NewNode::Text { text }
            | super::command::NewNode::CData { text }
            | super::command::NewNode::Comment { text } => text.len(),
        },
        _ => 0,
    };
    COMMAND_OVERHEAD + payload
}
