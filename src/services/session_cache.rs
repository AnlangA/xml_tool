//! Unified per-session caches with a byte budget and hit statistics.
//!
//! One [`DocumentSessionCache`] owns every derived-from-document cache
//! (search index, flattened outline). Keys always include
//! `SessionId + Revision` (+ operation parameters like the expansion set
//! digest), so stale results are unreachable by construction. Entries are
//! evicted LRU-first against the 128 MiB budget.

use std::collections::HashMap;

use crate::core::document::{NodeId, XmlDocument};
use crate::services::outline::FlatTree;
use crate::services::search::SearchIndex;
use crate::services::task_manager::SessionId;

/// Total byte budget for all document caches (the plan's 128 MiB).
pub const CACHE_BUDGET_BYTES: usize = 128 * 1024 * 1024;
/// Budget headroom tolerated before hard eviction (the plan's 110% rule).
pub const CACHE_BUDGET_TOLERANCE: f64 = 1.10;

/// Per-kind hit/miss counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
}

/// One cached flattened outline.
pub struct OutlineEntry {
    pub tree: FlatTree,
    pub elements: usize,
}

struct Slot {
    bytes: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Search {
        session: SessionId,
        revision: u64,
    },
    Outline {
        session: SessionId,
        revision: u64,
        expansion: u64,
    },
}

/// The unified cache. Not internally synchronized: one per shell, driven
/// from the UI thread.
pub struct DocumentSessionCache {
    slots: HashMap<Kind, Slot>,
    /// LRU order: most recently used last.
    order: Vec<Kind>,
    searches: HashMap<(SessionId, u64), SearchIndex>,
    outlines: HashMap<(SessionId, u64, u64), OutlineEntry>,
    budget: usize,
    bytes: usize,
    stats_search: CacheStats,
    stats_outline: CacheStats,
}

impl Default for DocumentSessionCache {
    fn default() -> Self {
        Self::new(CACHE_BUDGET_BYTES)
    }
}

impl DocumentSessionCache {
    /// A cache with an explicit byte budget.
    pub fn new(budget: usize) -> DocumentSessionCache {
        DocumentSessionCache {
            slots: HashMap::new(),
            order: Vec::new(),
            searches: HashMap::new(),
            outlines: HashMap::new(),
            budget,
            bytes: 0,
            stats_search: CacheStats::default(),
            stats_outline: CacheStats::default(),
        }
    }

    /// Search index for (session, revision), built on miss.
    pub fn search_index(
        &mut self,
        session: SessionId,
        revision: u64,
        document: &XmlDocument,
    ) -> &mut SearchIndex {
        if !self.searches.contains_key(&(session, revision)) {
            self.stats_search.misses += 1;
            let index = SearchIndex::build(document);
            let bytes = estimate_search_bytes(&index);
            self.insert(Kind::Search { session, revision }, bytes);
            self.searches.insert((session, revision), index);
        } else {
            self.stats_search.hits += 1;
            self.touch(Kind::Search { session, revision });
        }
        self.searches
            .get_mut(&(session, revision))
            .expect("just inserted")
    }

    /// Flattened outline for (session, revision, expansion digest).
    pub fn outline(
        &mut self,
        session: SessionId,
        revision: u64,
        expansion: u64,
        document: &XmlDocument,
        expanded: &std::collections::HashSet<NodeId>,
    ) -> &OutlineEntry {
        if !self.outlines.contains_key(&(session, revision, expansion)) {
            self.stats_outline.misses += 1;
            let tree = FlatTree::build(document, expanded);
            let bytes = tree.len() * std::mem::size_of::<crate::services::outline::Row>() + 64;
            self.insert(
                Kind::Outline {
                    session,
                    revision,
                    expansion,
                },
                bytes,
            );
            self.outlines.insert(
                (session, revision, expansion),
                OutlineEntry {
                    tree,
                    elements: document.document_order().len().saturating_sub(1),
                },
            );
        } else {
            self.stats_outline.hits += 1;
            self.touch(Kind::Outline {
                session,
                revision,
                expansion,
            });
        }
        self.outlines
            .get(&(session, revision, expansion))
            .expect("just inserted")
    }

    /// Drops every entry for `session` (tab closed or replaced).
    pub fn invalidate_session(&mut self, session: SessionId) {
        self.retain(|kind| !matches!(kind, Kind::Search { session: s, .. } | Kind::Outline { session: s, .. } if *s == session));
    }

    /// Drops entries whose revision differs from the current one.
    pub fn invalidate_revisions(&mut self, session: SessionId, current: u64) {
        self.retain(|kind| match kind {
            Kind::Search {
                session: s,
                revision,
            }
            | Kind::Outline {
                session: s,
                revision,
                ..
            } => !(*s == session && *revision != current),
        });
    }

    /// Search cache statistics.
    pub fn search_stats(&self) -> CacheStats {
        self.stats_search
    }

    /// Outline cache statistics.
    pub fn outline_stats(&self) -> CacheStats {
        self.stats_outline
    }

    /// Current estimated bytes held.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Whether every entry has been evicted (test hook for tiny budgets).
    pub fn searches_is_empty(&self) -> bool {
        self.searches.is_empty()
    }

    /// Whether the cache respects its budget (within the 110% tolerance).
    pub fn within_budget(&self) -> bool {
        self.bytes as f64 <= self.budget as f64 * CACHE_BUDGET_TOLERANCE
    }

    fn insert(&mut self, kind: Kind, bytes: usize) {
        if let Some(old) = self.slots.insert(kind, Slot { bytes }) {
            self.bytes = self.bytes.saturating_sub(old.bytes);
        }
        self.bytes += bytes;
        self.order.retain(|key| *key != kind);
        self.order.push(kind);
        self.evict_over_budget();
    }

    fn touch(&mut self, kind: Kind) {
        self.order.retain(|key| *key != kind);
        self.order.push(kind);
    }

    fn evict_over_budget(&mut self) {
        while self.bytes > self.budget && !self.order.is_empty() {
            let victim = self.order.remove(0);
            self.remove_kind(victim);
        }
    }

    fn remove_kind(&mut self, kind: Kind) {
        if let Some(slot) = self.slots.remove(&kind) {
            self.bytes = self.bytes.saturating_sub(slot.bytes);
            match kind {
                Kind::Search { session, revision } => {
                    self.searches.remove(&(session, revision));
                }
                Kind::Outline {
                    session,
                    revision,
                    expansion,
                } => {
                    self.outlines.remove(&(session, revision, expansion));
                }
            }
        }
    }

    fn retain(&mut self, keep: impl Fn(&Kind) -> bool) {
        let victims: Vec<Kind> = self
            .order
            .iter()
            .copied()
            .filter(|kind| !keep(kind))
            .collect();
        for victim in victims {
            self.order.retain(|kind| *kind != victim);
            self.remove_kind(victim);
        }
    }
}

fn estimate_search_bytes(index: &SearchIndex) -> usize {
    // Two strings (original + folded) per entry, plus map overhead.
    index.estimated_bytes()
}

/// Stable digest of an expansion set (for outline cache keys).
pub fn expansion_digest(expanded: &std::collections::HashSet<NodeId>) -> u64 {
    let mut ids: Vec<u64> = expanded.iter().map(|node| node.0).collect();
    ids.sort_unstable();
    let mut hash: u64 = 0xcbf29ce484222325;
    for id in ids {
        hash ^= id;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
