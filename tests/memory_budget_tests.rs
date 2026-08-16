//! Memory-budget acceptance tests for `AGENTS_PLAN.md` step 4.
//!
//! These live in their own test binary and share a mutex because RSS is a
//! per-process measurement: parallel tests allocating multi-hundred-megabyte
//! fixtures would otherwise pollute each other's deltas.

use std::collections::HashSet;
use std::sync::{Mutex, MutexGuard, OnceLock};

use xml_tool::core::document::{NodeId, XmlDocument};
use xml_tool::core::{Command, NodeContent};
use xml_tool::fixtures;
use xml_tool::services::outline::FlatTree;
use xml_tool::services::search::SearchIndex;

fn rss_kib() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status");
    for line in status.lines() {
        if let Some(value) = line.strip_prefix("VmRSS:") {
            return value
                .trim()
                .trim_end_matches(" kB")
                .parse()
                .unwrap_or(u64::MAX);
        }
    }
    u64::MAX
}

/// Serializes the two RSS measurements inside this binary.
fn measurement_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn twenty_mib_document_stays_within_320_mib_rss() {
    let _guard = measurement_lock();
    let before = rss_kib();
    let bytes = fixtures::large_bytes_xml();
    let doc = XmlDocument::parse(bytes.as_bytes()).unwrap();
    let mut expanded = HashSet::new();
    expanded.insert(doc.root_element().unwrap());
    let tree = FlatTree::build(&doc, &expanded);
    let order = doc.document_order().to_vec();
    let mut index = SearchIndex::build(&doc);
    let hits = index.search("Record 12000 title", false, &order);
    assert!(!hits.is_empty());
    assert!(!tree.is_empty());
    let after = rss_kib();
    // 20 MiB source + snapshot copies + DOM + index + flattened rows must
    // stay under 320 MiB of incremental RSS.
    assert!(
        after.saturating_sub(before) <= 320 * 1024,
        "RSS grew by {} KiB",
        after.saturating_sub(before)
    );
}

#[test]
fn hundred_edits_stay_within_64_mib_rss_growth() {
    let _guard = measurement_lock();
    let bytes = fixtures::large_bytes_xml();
    let mut doc = XmlDocument::parse(bytes.as_bytes()).unwrap();
    let mut history = xml_tool::core::History::new();
    let root = doc.root_element().unwrap();
    let records: Vec<NodeId> = doc
        .children(root)
        .into_iter()
        .filter(|id| doc.kind(*id) == Some(xml_tool::core::XmlNodeKind::Element))
        .take(100)
        .collect();
    let before = rss_kib();
    for (round, record) in records.iter().enumerate() {
        let title = doc
            .children(*record)
            .into_iter()
            .find(|id| doc.kind(*id) == Some(xml_tool::core::XmlNodeKind::Element))
            .expect("record has a title element");
        let text = doc.children(title)[0];
        history
            .commit(
                &mut doc,
                Command::SetNodeContent {
                    node: text,
                    content: NodeContent::Text(format!("edited-{round}")),
                },
            )
            .expect("edits apply");
    }
    let after = rss_kib();
    assert!(
        after.saturating_sub(before) <= 64 * 1024,
        "100 edits grew RSS by {} KiB",
        after.saturating_sub(before)
    );
}
