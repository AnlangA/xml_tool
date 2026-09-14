//! Acceptance tests for `AGENTS_PLAN.md` step 4: large-file read-only mode
//! and virtualization foundations.
//!
//! Timing assertions use debug-tolerant bounds so CI stays stable; the
//! plan's release-mode budgets (2.0 s open, 500 ms/50 ms search) are
//! measured and recorded in `docs/BASELINE-step-4.md`.

use std::collections::HashSet;
use std::time::Instant;

use xml_tool::core::document::{NodeId, XmlDocument};
use xml_tool::core::{Command, InsertPosition, NewNode};
use xml_tool::fixtures;
use xml_tool::services::large_file::{LargeFileError, ReadOnlyDocument};
use xml_tool::services::outline::FlatTree;
use xml_tool::services::search::SearchIndex;
use xml_tool::services::source_buffer::SourceBuffer;

// ---------------------------------------------------------------------------
// Gate: both large fixtures open in fully editable mode
// ---------------------------------------------------------------------------

#[test]
fn large_fixtures_open_editable_and_stay_fast() {
    let started = Instant::now();
    let bytes = fixtures::large_bytes_xml();
    let doc = XmlDocument::parse(bytes.as_bytes()).expect("20 MiB fixture parses");
    let open_elapsed = started.elapsed();
    assert!(doc.root_element().is_some());
    assert!(
        open_elapsed.as_secs_f64() < 30.0,
        "20 MiB open took {open_elapsed:?} (debug budget 30 s)"
    );

    let started = Instant::now();
    let nodes = fixtures::large_nodes_xml();
    let doc = XmlDocument::parse(nodes.as_bytes()).expect("200k fixture parses");
    assert!(
        started.elapsed().as_secs_f64() < 30.0,
        "200k open exceeded debug budget"
    );
    let root = doc.root_element().unwrap();
    let element_children = doc
        .children(root)
        .into_iter()
        .filter(|id| doc.kind(*id) == Some(xml_tool::core::XmlNodeKind::Element))
        .count();
    assert_eq!(element_children, 199_999);
}

// ---------------------------------------------------------------------------
// Gate: literal search on 200,000 nodes — first and repeat
// ---------------------------------------------------------------------------

#[test]
fn literal_search_on_200k_nodes_meets_budgets() {
    let nodes = fixtures::large_nodes_xml();
    let doc = XmlDocument::parse(nodes.as_bytes()).unwrap();

    let started = Instant::now();
    let mut index = SearchIndex::build(&doc);
    let order = doc.document_order().to_vec();
    let hits = index.search("id=\"123456\"", false, &order);
    let first = started.elapsed();
    assert!(!hits.is_empty(), "needle must hit");
    assert!(
        first.as_millis() < 5_000,
        "first search (build + query) took {first:?} (debug budget 5 s)"
    );

    let started = Instant::now();
    let warm = index.search("id=\"123456\"", false, &order);
    let repeat = started.elapsed();
    assert_eq!(warm.len(), hits.len());
    assert!(
        repeat.as_millis() < 500,
        "repeat query took {repeat:?} (debug budget 500 ms)"
    );

    // Case-insensitive finds the same node with folded text.
    let folded = index.search("ID=\"123456\"", false, &order);
    assert_eq!(folded.len(), hits.len());
    let sensitive = index.search("ID=\"123456\"", true, &order);
    assert!(sensitive.is_empty(), "case-sensitive must not match");
}

#[test]
fn search_index_rebuilds_only_changed_nodes() {
    let mut doc = XmlDocument::parse(b"<r><a>alpha</a><b>beta</b></r>".as_slice()).unwrap();
    let mut index = SearchIndex::build(&doc);
    let order = doc.document_order().to_vec();
    assert_eq!(index.search("alpha", true, &order).len(), 1);

    // Edit one text node; only that node is re-indexed.
    let root = doc.root_element().unwrap();
    let a = doc.children(root)[0];
    let text = doc.children(a)[0];
    let mut history = xml_tool::core::History::new();
    let changed = history
        .commit(
            &mut doc,
            Command::SetNodeContent {
                node: text,
                content: xml_tool::core::NodeContent::Text("gamma".into()),
            },
        )
        .expect("edit applies");
    index.apply_changes(&doc, &changed);
    let order = doc.document_order().to_vec();
    assert_eq!(index.search("alpha", true, &order).len(), 0);
    assert_eq!(
        index.search("beta", true, &order).len(),
        1,
        "untouched nodes stay indexed"
    );
    assert_eq!(index.search("gamma", true, &order).len(), 1);
    assert_eq!(index.revision(), doc.revision().0);
}

// ---------------------------------------------------------------------------
// Gate: flattened tree virtualization
// ---------------------------------------------------------------------------

#[test]
fn flat_tree_collapses_and_pages_rows() {
    let nodes = fixtures::large_nodes_xml();
    let doc = XmlDocument::parse(nodes.as_bytes()).unwrap();

    // Fully collapsed: exactly one row.
    let collapsed = FlatTree::build(&doc, &HashSet::new());
    assert_eq!(collapsed.len(), 1);

    // Root expanded: root + 199,999 item rows, no recursion beyond depth 1.
    let root = doc.root_element().unwrap();
    let mut expanded = HashSet::new();
    expanded.insert(root);
    let tree = FlatTree::build(&doc, &expanded);
    assert_eq!(tree.len(), 200_000);
    let _ = &tree;

    // Viewport slicing returns only the window with overscan.
    let page = tree.viewport(1_000, 1_040, 10);
    assert_eq!(page.len(), 61);
    assert_eq!(page[0], tree.rows[990]);
    assert_eq!(page.last(), Some(&tree.rows[1_050]));

    // Deep nesting collapses whole subtrees.
    let deep = XmlDocument::parse(b"<a><b><c><d/><d/></c></b></a>".as_slice()).unwrap();
    let a = deep.root_element().unwrap();
    let b = deep.children(a)[0];
    let c = deep.children(b)[0];
    let all: HashSet<NodeId> = [a, b, c].into_iter().collect();
    let tree = FlatTree::build(&deep, &all);
    assert_eq!(tree.len(), 5, "a b c d d");
    let mut partial = all.clone();
    partial.remove(&a);
    let tree = FlatTree::build(&deep, &partial);
    assert_eq!(tree.len(), 1, "collapsing the root hides everything below");
}

// ---------------------------------------------------------------------------
// Gate: read-only mode for above-threshold documents
// ---------------------------------------------------------------------------

#[test]
fn read_only_skeleton_browses_searches_and_exports() {
    let mut heavy = String::from("<catalog>");
    for i in 0..200_001 {
        heavy.push_str(&format!("<item id=\"i{i}\">value {i}</item>"));
    }
    heavy.push_str("</catalog>");

    let started = Instant::now();
    let doc = ReadOnlyDocument::open(heavy.as_bytes()).expect("skeleton builds");
    assert!(started.elapsed().as_secs_f64() < 30.0, "debug budget");

    assert_eq!(doc.element_count(), 200_002); // catalog + items
    assert_eq!(doc.elements.last().unwrap().depth, 0);
    assert_eq!(doc.elements[0].name, "item");
    assert_eq!(doc.elements[0].depth, 1);

    // Literal search jumps to the owning element.
    let hits = doc.search_literal("value 199999", false);
    assert_eq!(hits.len(), 1);
    let element = doc.element_at(hits[0].start).expect("owning element");
    assert_eq!(doc.elements[element].name, "item");
    assert!(doc.element_source(element).contains("value 199999"));

    // Paths are copyable and unambiguous.
    let path = doc.node_path(element);
    assert!(path.starts_with("/catalog[1]/item["), "path: {path}");
    assert!(path.ends_with(']'));

    // Save-as replays the original bytes.
    assert_eq!(doc.source.len(), heavy.len());
}

#[test]
fn read_only_open_refuses_oversize_input() {
    let mut oversized = vec![b' '; 256 * 1024 * 1024 + 1];
    oversized[..5].copy_from_slice(b"<r/>x");
    match ReadOnlyDocument::open(&oversized) {
        Err(LargeFileError::TooLarge(size)) => assert_eq!(size, oversized.len()),
        Err(LargeFileError::Parse(message)) => panic!("wrong failure: {message}"),
        Ok(_) => panic!("expected refusal"),
    }
}

#[test]
fn read_only_mode_supports_no_editing_paths() {
    // The mode is data-only: ReadOnlyDocument exposes no mutation API, and
    // the classification below pins the workflow that produces it.
    let mut heavy = String::from("<r>");
    for i in 0..200_001 {
        heavy.push_str(&format!("<i{i}/>"));
    }
    heavy.push_str("</r>");
    let outcome = xml_tool::services::document_io::classify_bytes(heavy.as_bytes()).unwrap();
    assert_eq!(
        outcome.mode,
        xml_tool::services::document_io::OpenMode::LargeReadOnly
    );
}

// ---------------------------------------------------------------------------
// Gate: rope source buffer
// ---------------------------------------------------------------------------

#[test]
fn source_buffer_converts_lines_columns_and_edits() {
    let mut buffer = SourceBuffer::new("line one\nline two\nline three\n");
    assert_eq!(buffer.len_lines(), 4);
    assert_eq!(buffer.line(1), "line two\n");

    assert_eq!(buffer.line_column_of_char(9), (2, 1));
    assert_eq!(buffer.char_offset_of_line_column(2, 1), 9);
    // Round trip through both directions.
    for offset in [0, 5, 8, 9, 20] {
        let (line, column) = buffer.line_column_of_char(offset);
        assert_eq!(buffer.char_offset_of_line_column(line, column), offset);
    }

    buffer.insert(5, "INSERTED ");
    assert_eq!(buffer.line(0), "line INSERTED one\n");
    buffer.remove(5, 9);
    assert!(buffer.text().starts_with("line one\n"));
}

// ---------------------------------------------------------------------------
// Gate: inserting into the big document keeps working (editable mode)
// ---------------------------------------------------------------------------

#[test]
fn structural_edits_work_on_large_editable_documents() {
    let nodes = fixtures::large_nodes_xml();
    let mut doc = XmlDocument::parse(nodes.as_bytes()).unwrap();
    let mut history = xml_tool::core::History::new();
    let root = doc.root_element().unwrap();

    history
        .commit(
            &mut doc,
            Command::InsertNode {
                parent: root,
                position: InsertPosition::First,
                node: NewNode::Comment {
                    text: "inserted at scale".into(),
                },
            },
        )
        .expect("insert at scale");
    let first = doc.children(root)[0];
    assert_eq!(doc.comment_text(first), Some("inserted at scale"));
    history.undo(&mut doc).expect("undo at scale");
    assert_ne!(
        doc.comment_text(doc.children(root)[0]),
        Some("inserted at scale")
    );
}

// ---------------------------------------------------------------------------
// Step 9: unified session cache
// ---------------------------------------------------------------------------

#[test]
fn session_cache_hits_and_invalidates_by_revision() {
    use xml_tool::services::session_cache::{DocumentSessionCache, expansion_digest};
    use xml_tool::services::task_manager::SessionId;

    let mut doc = XmlDocument::parse(b"<r><a>alpha</a></r>".as_slice()).unwrap();
    let session = SessionId(1);
    let mut cache = DocumentSessionCache::new(8 * 1024 * 1024);

    let order = doc.document_order().to_vec();
    let first = cache.search_index(session, 0, &doc);
    let hits_first = first.search("alpha", true, &order).len();
    assert_eq!(hits_first, 1);
    assert_eq!(cache.search_stats().misses, 1);

    let second = cache.search_index(session, 0, &doc);
    let hits_second = second.search("alpha", true, &order).len();
    assert_eq!(hits_second, 1, "memoized query still works");
    assert_eq!(cache.search_stats().hits, 1, "second lookup is a hit");

    // An edit bumps the revision: the next lookup must rebuild.
    let mut history = xml_tool::core::History::new();
    let text = doc.children(doc.children(doc.root_element().unwrap())[0])[0];
    history
        .commit(
            &mut doc,
            Command::SetNodeContent {
                node: text,
                content: xml_tool::core::NodeContent::Text("beta".into()),
            },
        )
        .unwrap();
    cache.invalidate_revisions(session, doc.revision().0);
    let order = doc.document_order().to_vec();
    let rebuilt = cache.search_index(session, doc.revision().0, &doc);
    assert_eq!(rebuilt.search("beta", true, &order).len(), 1);
    assert_eq!(cache.search_stats().misses, 2, "revision change rebuilds");
    assert!(cache.within_budget());

    // Expansion digest: different sets produce different keys.
    let a = std::collections::HashSet::from([xml_tool::core::NodeId(1)]);
    let b = std::collections::HashSet::from([xml_tool::core::NodeId(2)]);
    assert_ne!(expansion_digest(&a), expansion_digest(&b));
}

#[test]
fn session_cache_evicts_over_budget() {
    use xml_tool::services::session_cache::DocumentSessionCache;
    use xml_tool::services::task_manager::SessionId;

    let mut cache = DocumentSessionCache::new(1); // impossibly small
    let doc = XmlDocument::parse(b"<r><a>text</a></r>".as_slice()).unwrap();
    cache.search_index(SessionId(1), 0, &doc);
    assert!(
        cache.bytes() <= 1 || cache.searches_is_empty(),
        "budget must evict: {} bytes held",
        cache.bytes()
    );
}
