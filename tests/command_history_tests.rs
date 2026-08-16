//! Acceptance tests for `AGENTS_PLAN.md` step 2: the rebuilt document model,
//! editing commands, and incremental history.
//!
//! Every command gets success / validation-failure / undo / redo coverage.
//! Cross-cutting gates: failed commands change nothing, leaf edits leave all
//! other bytes untouched, history stays within budget, undo/redo round-trips
//! CDATA/PI/comments/mixed content and attribute renames.

use std::time::Duration;

use xml_tool::core::{
    Command, History, HistoryLimits, InsertPosition, NewNode, NodeContent, NodeId, XmlDocument,
    XmlNodeKind,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse(xml: &str) -> XmlDocument {
    XmlDocument::parse(xml.as_bytes()).expect("test document must parse")
}

fn root_of(doc: &XmlDocument) -> NodeId {
    doc.root_element().expect("root element")
}

fn child_named(doc: &XmlDocument, parent: NodeId, name: &str) -> NodeId {
    doc.children(parent)
        .into_iter()
        .find(|id| doc.qname(*id).is_some_and(|q| q.local_name() == name))
        .unwrap_or_else(|| panic!("no child named {name}"))
}

fn first_text_child(doc: &XmlDocument, parent: NodeId) -> NodeId {
    doc.children(parent)
        .into_iter()
        .find(|id| doc.kind(*id) == Some(XmlNodeKind::Text))
        .expect("text child")
}

fn first_kind(doc: &XmlDocument, parent: NodeId, kind: XmlNodeKind) -> NodeId {
    doc.children(parent)
        .into_iter()
        .find(|id| doc.kind(*id) == Some(kind))
        .unwrap_or_else(|| panic!("child of kind {kind:?}"))
}

fn commit(doc: &mut XmlDocument, history: &mut History, command: Command) {
    history
        .commit(doc, command)
        .expect("command must apply in tests");
}

/// The document source must always re-parse to the same structure as the
/// live DOM: the splice machinery can never desynchronize the two.
fn assert_source_matches_dom(doc: &XmlDocument) {
    let reparsed = XmlDocument::parse(doc.source().as_bytes())
        .unwrap_or_else(|err| panic!("source no longer parses: {err}\n{}", doc.source()));
    let rendered = render(doc);
    let reparsed_rendered = render(&reparsed);
    assert_eq!(
        rendered,
        reparsed_rendered,
        "DOM and source diverged\nsource:\n{}\nlive:\n{rendered}\nreparsed:\n{reparsed_rendered}",
        doc.source()
    );
}

fn render(doc: &XmlDocument) -> String {
    doc.dom()
        .to_xml_with_options(&uppsala::dom::XmlWriteOptions::compact().with_doctype(true))
}

// ---------------------------------------------------------------------------
// RenameElement
// ---------------------------------------------------------------------------

#[test]
fn rename_element_success_fail_undo_redo() {
    let mut doc = parse("<catalog><old id=\"1\"/><old/></catalog>");
    let mut history = History::new();
    let catalog = root_of(&doc);
    let first = child_named(&doc, catalog, "old");
    let before_source = doc.source().to_string();

    // Validation failure: bad name.
    let err = Command::RenameElement {
        node: first,
        new_name: "1bad".into(),
    }
    .apply(&mut doc)
    .unwrap_err();
    assert_eq!(err.code, "invalid_name");
    assert_eq!(doc.source(), before_source, "failed command changed bytes");
    assert_eq!(doc.revision().0, 0, "failed command bumped revision");

    // Success.
    commit(
        &mut doc,
        &mut history,
        Command::RenameElement {
            node: first,
            new_name: "fresh".into(),
        },
    );
    assert_eq!(doc.qname(first).unwrap().local_name(), "fresh");
    assert!(doc.source().contains("<fresh id=\"1\"/>"));
    assert_source_matches_dom(&doc);

    // Undo / redo.
    history.undo(&mut doc).expect("undo");
    assert_eq!(doc.qname(first).unwrap().local_name(), "old");
    assert_source_matches_dom(&doc);
    history.redo(&mut doc).expect("redo");
    assert_eq!(doc.qname(first).unwrap().local_name(), "fresh");
    assert_source_matches_dom(&doc);
}

// ---------------------------------------------------------------------------
// Attribute commands
// ---------------------------------------------------------------------------

#[test]
fn add_attribute_success_fail_undo_redo() {
    let mut doc = parse("<item a=\"1\"/>");
    let mut history = History::new();
    let item = root_of(&doc);

    // Failure: duplicate.
    let err = Command::AddAttribute {
        element: item,
        name: "a".into(),
        value: "2".into(),
    }
    .apply(&mut doc)
    .unwrap_err();
    assert_eq!(err.code, "duplicate_attribute");

    commit(
        &mut doc,
        &mut history,
        Command::AddAttribute {
            element: item,
            name: "b".into(),
            value: "hello & bye".into(),
        },
    );
    assert!(doc.source().contains("b=\"hello &amp; bye\""));
    assert_source_matches_dom(&doc);

    history.undo(&mut doc).expect("undo");
    assert!(!doc.attributes(item).iter().any(|(name, _)| name == "b"));
    assert_source_matches_dom(&doc);
    history.redo(&mut doc).expect("redo");
    assert!(doc.attributes(item).iter().any(|(name, _)| name == "b"));
}

#[test]
fn rename_attribute_success_fail_undo_redo() {
    let mut doc = parse("<item old=\"v\"/>");
    let mut history = History::new();
    let item = root_of(&doc);

    let err = Command::RenameAttribute {
        element: item,
        old_name: "missing".into(),
        new_name: "new".into(),
    }
    .apply(&mut doc)
    .unwrap_err();
    assert_eq!(err.code, "unknown_attribute");

    commit(
        &mut doc,
        &mut history,
        Command::RenameAttribute {
            element: item,
            old_name: "old".into(),
            new_name: "new".into(),
        },
    );
    assert!(doc.source().contains("new=\"v\""), "{}", doc.source());

    history.undo(&mut doc).expect("undo");
    assert!(doc.source().contains("old=\"v\""));
    history.redo(&mut doc).expect("redo");
    assert!(doc.source().contains("new=\"v\""));
}

#[test]
fn set_attribute_value_success_fail_undo_redo() {
    let mut doc = parse("<item a=\"1\"/>");
    let mut history = History::new();
    let item = root_of(&doc);

    let err = Command::SetAttributeValue {
        element: item,
        name: "nope".into(),
        value: "x".into(),
    }
    .apply(&mut doc)
    .unwrap_err();
    assert_eq!(err.code, "unknown_attribute");

    commit(
        &mut doc,
        &mut history,
        Command::SetAttributeValue {
            element: item,
            name: "a".into(),
            value: "42".into(),
        },
    );
    assert!(doc.source().contains("a=\"42\""));

    history.undo(&mut doc).expect("undo");
    assert!(doc.source().contains("a=\"1\""));
    history.redo(&mut doc).expect("redo");
    assert!(doc.source().contains("a=\"42\""));
}

#[test]
fn remove_attribute_success_fail_undo_redo() {
    let mut doc = parse("<item a=\"1\" b=\"2\"/>");
    let mut history = History::new();
    let item = root_of(&doc);

    let err = Command::RemoveAttribute {
        element: item,
        name: "c".into(),
    }
    .apply(&mut doc)
    .unwrap_err();
    assert_eq!(err.code, "unknown_attribute");

    commit(
        &mut doc,
        &mut history,
        Command::RemoveAttribute {
            element: item,
            name: "a".into(),
        },
    );
    assert!(!doc.source().contains("a=\"1\""));

    history.undo(&mut doc).expect("undo");
    assert!(doc.source().contains("a=\"1\""));
    history.redo(&mut doc).expect("redo");
    assert!(!doc.source().contains("a=\"1\""));
}

// ---------------------------------------------------------------------------
// SetNodeContent across node kinds
// ---------------------------------------------------------------------------

#[test]
fn set_text_content_success_fail_undo_redo() {
    let mut doc = parse("<a><t>old</t></a>");
    let mut history = History::new();
    let a = root_of(&doc);
    let t = first_text_child(&doc, child_named(&doc, a, "t"));

    // Kind mismatch: comment content into a text node.
    let err = Command::SetNodeContent {
        node: t,
        content: NodeContent::Comment("no".into()),
    }
    .apply(&mut doc)
    .unwrap_err();
    assert_eq!(err.code, "wrong_node_kind");

    // Illegal character.
    let err = Command::SetNodeContent {
        node: t,
        content: NodeContent::Text("a < b".into()),
    }
    .apply(&mut doc)
    .unwrap_err();
    assert_eq!(err.code, "invalid_text");

    commit(
        &mut doc,
        &mut history,
        Command::SetNodeContent {
            node: t,
            content: NodeContent::Text("new & fresh".into()),
        },
    );
    assert!(doc.source().contains("new &amp; fresh"), "{}", doc.source());
    assert_source_matches_dom(&doc);

    history.undo(&mut doc).expect("undo");
    assert!(doc.source().contains(">old<"));
    history.redo(&mut doc).expect("redo");
    assert!(doc.source().contains("new &amp; fresh"));
}

#[test]
fn set_cdata_comment_pi_content_undo_redo() {
    let xml = "<r><c><![CDATA[old]]></c><!--note--><?proc data?></r>";
    let mut doc = parse(xml);
    let mut history = History::new();
    let r = root_of(&doc);
    let cdata = first_kind(&doc, child_named(&doc, r, "c"), XmlNodeKind::CData);
    let comment = first_kind(&doc, r, XmlNodeKind::Comment);
    let pi = first_kind(&doc, r, XmlNodeKind::ProcessingInstruction);

    commit(
        &mut doc,
        &mut history,
        Command::SetNodeContent {
            node: cdata,
            content: NodeContent::CData("a < b && c > d".into()),
        },
    );
    assert!(doc.source().contains("<![CDATA[a < b && c > d]]>"));

    commit(
        &mut doc,
        &mut history,
        Command::SetNodeContent {
            node: comment,
            content: NodeContent::Comment("revised".into()),
        },
    );
    assert!(doc.source().contains("<!--revised-->"));

    commit(
        &mut doc,
        &mut history,
        Command::SetNodeContent {
            node: pi,
            content: NodeContent::ProcessingInstruction {
                target: "proc2".into(),
                data: Some("other".into()),
            },
        },
    );
    assert!(doc.source().contains("<?proc2 other?>"), "{}", doc.source());
    assert_source_matches_dom(&doc);

    for expected in ["<?proc data?>", "<!--note-->", "<![CDATA[old]]>"] {
        history.undo(&mut doc).expect("undo");
        assert!(doc.source().contains(expected), "missing {expected}");
    }
    assert_source_matches_dom(&doc);
    for _ in 0..3 {
        history.redo(&mut doc).expect("redo");
    }
    assert!(doc.source().contains("<?proc2 other?>"));
    assert_source_matches_dom(&doc);
}

// ---------------------------------------------------------------------------
// InsertNode / DeleteNode / MoveNode / DuplicateSubtree
// ---------------------------------------------------------------------------

#[test]
fn insert_node_into_populated_parent_undo_redo() {
    let mut doc = parse("<r><a/><z/></r>");
    let mut history = History::new();
    let r = root_of(&doc);
    let a = child_named(&doc, r, "a");
    let z = child_named(&doc, r, "z");

    // Failure: bad name.
    let err = Command::InsertNode {
        parent: r,
        position: InsertPosition::Last,
        node: NewNode::Element {
            name: "bad name".into(),
        },
    }
    .apply(&mut doc)
    .unwrap_err();
    assert_eq!(err.code, "invalid_name");

    // Insert in the middle.
    commit(
        &mut doc,
        &mut history,
        Command::InsertNode {
            parent: r,
            position: InsertPosition::After(a),
            node: NewNode::Element { name: "m".into() },
        },
    );
    let m = child_named(&doc, r, "m");
    assert_eq!(
        doc.children(r)
            .into_iter()
            .map(|id| doc.qname(id).unwrap().local_name().to_string())
            .collect::<Vec<_>>(),
        vec!["a", "m", "z"]
    );
    let (m_start, _) = doc
        .source_range(m)
        .map(|r| (r.start_byte, r.end_byte))
        .unwrap();
    let (a_end, z_start) = (
        doc.source_range(a).unwrap().end_byte,
        doc.source_range(z).unwrap().start_byte,
    );
    assert!(
        a_end <= m_start && m_start <= z_start,
        "m must sit between a and z"
    );
    assert_source_matches_dom(&doc);

    history.undo(&mut doc).expect("undo");
    assert_eq!(doc.children(r).len(), 2);
    assert_source_matches_dom(&doc);
    history.redo(&mut doc).expect("redo");
    assert_eq!(doc.children(r).len(), 3);
    assert_source_matches_dom(&doc);
}

#[test]
fn insert_node_into_childless_parent_expands_empty_tag() {
    let mut doc = parse("<r><e/></r>");
    let mut history = History::new();
    let r = root_of(&doc);
    let e = child_named(&doc, r, "e");

    commit(
        &mut doc,
        &mut history,
        Command::InsertNode {
            parent: e,
            position: InsertPosition::Last,
            node: NewNode::Text { text: "hi".into() },
        },
    );
    assert!(doc.source().contains("<e>hi</e>"), "{}", doc.source());
    assert_source_matches_dom(&doc);

    history.undo(&mut doc).expect("undo");
    assert!(doc.source().contains("<e/>"), "{}", doc.source());
    assert_source_matches_dom(&doc);
    history.redo(&mut doc).expect("redo");
    assert!(doc.source().contains("<e>hi</e>"));
}

#[test]
fn insert_comment_cdata_pi_nodes() {
    let mut doc = parse("<r/>");
    let mut history = History::new();
    let r = root_of(&doc);

    for node in [
        NewNode::Comment {
            text: "note".into(),
        },
        NewNode::CData {
            text: "raw <".into(),
        },
        NewNode::ProcessingInstruction {
            target: "hint".into(),
            data: Some("x".into()),
        },
    ] {
        commit(
            &mut doc,
            &mut history,
            Command::InsertNode {
                parent: r,
                position: InsertPosition::Last,
                node,
            },
        );
    }
    let source = doc.source();
    assert!(source.contains("<!--note-->"), "{source}");
    assert!(source.contains("<![CDATA[raw <]]>"), "{source}");
    assert!(source.contains("<?hint x?>"), "{source}");
    assert_source_matches_dom(&doc);
}

#[test]
fn delete_node_success_fail_undo_redo() {
    let mut doc = parse("<r keep=\"1\"><a/><b>bye</b></r>");
    let mut history = History::new();
    let r = root_of(&doc);

    // Failure: cannot delete the root.
    let err = Command::DeleteNode { node: r }.apply(&mut doc).unwrap_err();
    assert_eq!(err.code, "cannot_delete_root");

    let b = child_named(&doc, r, "b");
    let before = doc.source().to_string();
    commit(&mut doc, &mut history, Command::DeleteNode { node: b });
    assert_eq!(doc.source(), before.replace("<b>bye</b>", ""));
    assert_source_matches_dom(&doc);

    history.undo(&mut doc).expect("undo");
    assert!(doc.source().contains("<b>bye</b>"), "{}", doc.source());
    assert_source_matches_dom(&doc);
    history.redo(&mut doc).expect("redo");
    assert!(!doc.source().contains("<b>bye</b>"));
    assert_source_matches_dom(&doc);
}

#[test]
fn delete_top_level_comment_undoes_cleanly() {
    // Top-level nodes restore under DOCUMENT, not an element parent —
    // this used to fail undo with `not_an_element` and drop the entry.
    let mut doc = parse("<!--top--><r><a/></r><!--tail-->");
    let mut history = History::new();
    let top = doc
        .children(NodeId::DOCUMENT)
        .into_iter()
        .find(|id| doc.kind(*id) == Some(XmlNodeKind::Comment))
        .expect("top-level comment");

    commit(&mut doc, &mut history, Command::DeleteNode { node: top });
    assert_eq!(doc.source(), "<r><a/></r><!--tail-->");
    assert_source_matches_dom(&doc);

    history.undo(&mut doc).expect("undo top-level restore");
    assert_eq!(doc.source(), "<!--top--><r><a/></r><!--tail-->");
    assert_source_matches_dom(&doc);

    history.redo(&mut doc).expect("redo");
    assert_eq!(doc.source(), "<r><a/></r><!--tail-->");
    assert_source_matches_dom(&doc);
}

#[test]
fn delete_subtree_removes_descendants_too() {
    let mut doc = parse("<r><parent><x>1</x><y>2</y></parent><tail/></r>");
    let mut history = History::new();
    let r = root_of(&doc);
    let parent = child_named(&doc, r, "parent");
    let tail = child_named(&doc, r, "tail");

    commit(&mut doc, &mut history, Command::DeleteNode { node: parent });
    assert!(doc.source().contains("<tail/>"));
    assert!(!doc.source().contains("<parent>"));
    assert_eq!(doc.source_range(tail).unwrap().start_byte, "<r>".len());

    history.undo(&mut doc).expect("undo");
    assert!(doc.source().contains("<y>2</y>"));
    // Descendant ranges survive byte-exact restore.
    let y = child_named(&doc, child_named(&doc, r, "parent"), "y");
    assert!(doc.source_range(y).is_some());
}

#[test]
fn move_node_success_fail_undo_redo() {
    let mut doc = parse("<r><p1><x/></p1><p2><y/></p2></r>");
    let mut history = History::new();
    let r = root_of(&doc);
    let p1 = child_named(&doc, r, "p1");
    let p2 = child_named(&doc, r, "p2");
    let x = child_named(&doc, p1, "x");
    let y = child_named(&doc, p2, "y");

    // Failure: moving into own subtree.
    let err = Command::MoveNode {
        node: p1,
        new_parent: p1,
        position: InsertPosition::Last,
    }
    .apply(&mut doc)
    .unwrap_err();
    assert_eq!(err.code, "invalid_move_target");

    // Failure: the position sibling belongs to another parent.
    // p1 is a child of r, not of p2, so anchoring on it must be rejected.
    let err = Command::MoveNode {
        node: x,
        new_parent: p2,
        position: InsertPosition::After(p1),
    }
    .apply(&mut doc)
    .unwrap_err();
    assert_eq!(err.code, "position_not_child_of_parent");

    commit(
        &mut doc,
        &mut history,
        Command::MoveNode {
            node: x,
            new_parent: p2,
            position: InsertPosition::Before(y),
        },
    );
    assert!(
        doc.source().contains("<p1/><p2><x/><y/></p2>"),
        "{}",
        doc.source()
    );
    assert_source_matches_dom(&doc);

    history.undo(&mut doc).expect("undo");
    assert!(doc.source().contains("<p1><x/></p1>"), "{}", doc.source());
    assert_source_matches_dom(&doc);
    history.redo(&mut doc).expect("redo");
    assert!(doc.source().contains("<p2><x/><y/></p2>"));
}

#[test]
fn duplicate_subtree_success_fail_undo_redo() {
    let xml = "<r><item id=\"1\"><name>x</name></item></r>";
    let mut doc = parse(xml);
    let mut history = History::new();
    let r = root_of(&doc);
    let item = child_named(&doc, r, "item");

    let err = Command::DuplicateSubtree { node: r }
        .apply(&mut doc)
        .unwrap_err();
    assert_eq!(err.code, "cannot_duplicate_root");

    commit(
        &mut doc,
        &mut history,
        Command::DuplicateSubtree { node: item },
    );
    let items = doc.elements_named("item");
    assert_eq!(items.len(), 2);
    assert!(
        doc.source()
            .contains("<item id=\"1\"><name>x</name></item><item id=\"1\"><name>x</name></item>"),
        "{}",
        doc.source()
    );
    assert_source_matches_dom(&doc);

    history.undo(&mut doc).expect("undo");
    assert_eq!(doc.elements_named("item").len(), 1);
    assert_source_matches_dom(&doc);
    history.redo(&mut doc).expect("redo");
    assert_eq!(doc.elements_named("item").len(), 2);
}

// ---------------------------------------------------------------------------
// ReplaceWholeSource / BatchReplace / FormatDocument
// ---------------------------------------------------------------------------

#[test]
fn replace_whole_source_success_fail_undo_redo() {
    let mut doc = parse("<r><a/></r>");
    let mut history = History::new();

    let err = Command::ReplaceWholeSource {
        new_source: "<broken>".into(),
    }
    .apply(&mut doc)
    .unwrap_err();
    assert_eq!(err.code, "invalid_source");
    assert_eq!(doc.source(), "<r><a/></r>");

    commit(
        &mut doc,
        &mut history,
        Command::ReplaceWholeSource {
            new_source: "<fresh><b>hi</b><!--c--></fresh>".into(),
        },
    );
    assert!(doc.source().contains("<fresh>"));
    assert_source_matches_dom(&doc);

    history.undo(&mut doc).expect("undo");
    assert!(doc.source().contains("<r><a/></r>"));
    history.redo(&mut doc).expect("redo");
    assert!(doc.source().contains("<fresh>"));
}

#[test]
fn batch_replace_validates_whole_batch_before_applying() {
    let mut doc = parse("<r><a>one</a><b>two</b></r>");
    let mut history = History::new();
    let r = root_of(&doc);
    let a_text = first_text_child(&doc, child_named(&doc, r, "a"));
    let b_text = first_text_child(&doc, child_named(&doc, r, "b"));
    let before = doc.source().to_string();

    // One bad op (illegal '<' in text) rejects the whole batch.
    let err = Command::BatchReplace {
        ops: vec![
            replace_text(a_text, "one", "uno"),
            replace_text(b_text, "two", "bad <"),
        ],
    }
    .apply(&mut doc)
    .unwrap_err();
    assert_eq!(err.code, "invalid_text");
    assert_eq!(doc.source(), before, "rejected batch must change nothing");

    // A valid batch applies as one undoable step.
    commit(
        &mut doc,
        &mut history,
        Command::BatchReplace {
            ops: vec![
                replace_text(a_text, "one", "uno"),
                replace_text(b_text, "two", "dos"),
            ],
        },
    );
    assert!(doc.source().contains("uno"));
    assert!(doc.source().contains("dos"));
    assert_eq!(history.undo_depth(), 1);

    history.undo(&mut doc).expect("undo");
    assert!(doc.source().contains(">one<"));
    assert!(doc.source().contains(">two<"));
    history.redo(&mut doc).expect("redo");
    assert!(doc.source().contains("uno"));
}

fn replace_text(node: NodeId, old: &str, new: &str) -> xml_tool::core::command::ReplaceOp {
    xml_tool::core::command::ReplaceOp {
        node,
        target: xml_tool::core::command::ReplaceTarget::NodeText,
        old_value: old.into(),
        new_value: new.into(),
    }
}

#[test]
fn format_document_is_one_undoable_command() {
    let mut doc = parse("<r><a x=\"1\"/><b>txt</b></r>");
    let mut history = History::new();

    commit(
        &mut doc,
        &mut history,
        Command::FormatDocument {
            indent: "  ".into(),
        },
    );
    let formatted = doc.source().to_string();
    assert!(formatted.contains("\n  <a x=\"1\"/>"), "{formatted}");
    // Second format is byte-stable.
    commit(
        &mut doc,
        &mut history,
        Command::FormatDocument {
            indent: "  ".into(),
        },
    );
    assert_eq!(doc.source(), formatted, "second format must be byte-stable");

    // One undo returns to the pre-format state (both formats collapse via
    // their reverses; undo the second, then the first).
    history.undo(&mut doc).expect("undo");
    assert_eq!(doc.source(), formatted);
    history.undo(&mut doc).expect("undo");
    assert!(doc.source().contains("<r><a x=\"1\"/><b>txt</b></r>"));
}

// ---------------------------------------------------------------------------
// Cross-cutting gates
// ---------------------------------------------------------------------------

#[test]
fn failed_commands_leave_bytes_cursor_revision_untouched() {
    let mut doc = parse("<r><a v=\"1\">t</a><b/></r>");
    let history = History::new();
    let r = root_of(&doc);
    let a = child_named(&doc, r, "a");
    let text = first_text_child(&doc, a);
    let snapshot = doc.source().to_string();

    let failures = vec![
        Command::SetAttributeValue {
            element: a,
            name: "missing".into(),
            value: "x".into(),
        },
        Command::AddAttribute {
            element: a,
            name: "v".into(),
            value: "dup".into(),
        },
        Command::RenameElement {
            node: a,
            new_name: "a:b:c".into(),
        },
        Command::SetNodeContent {
            node: text,
            content: NodeContent::CData("kind mismatch".into()),
        },
        Command::DeleteNode { node: r },
        Command::MoveNode {
            node: a,
            new_parent: a,
            position: InsertPosition::Last,
        },
        Command::ReplaceWholeSource {
            new_source: "</bad>".into(),
        },
    ];
    for command in failures {
        let revision = doc.revision();
        let depth = history.undo_depth();
        assert!(command.apply(&mut doc).is_err(), "{command:?} should fail");
        assert_eq!(doc.source(), snapshot);
        assert_eq!(doc.revision(), revision);
        assert_eq!(history.undo_depth(), depth);
    }
}

#[test]
fn leaf_edit_leaves_all_other_bytes_untouched() {
    let xml = "<r>\n  <head keep=\"1\"/>\n  <leaf>old</leaf>\n  <tail/>\n</r>\n";
    let mut doc = parse(xml);
    let mut history = History::new();
    let r = root_of(&doc);
    let leaf = child_named(&doc, r, "leaf");
    let text = first_text_child(&doc, leaf);

    let range = doc.source_range(text).expect("text range");
    commit(
        &mut doc,
        &mut history,
        Command::SetNodeContent {
            node: text,
            content: NodeContent::Text("brand new value".into()),
        },
    );
    let after = doc.source();
    assert_eq!(
        &after[..range.start_byte],
        &xml[..range.start_byte],
        "bytes before the leaf changed"
    );
    assert_eq!(
        &after[after.len() - (xml.len() - range.end_byte)..],
        &xml[range.end_byte..],
        "bytes after the leaf changed"
    );
}

#[test]
fn undo_redo_preserves_namespaced_rename_and_mixed_content() {
    let xml = concat!(
        "<r xmlns:p=\"urn:p\">\n",
        "  <p:child>lead <b>bold</b> tail<!--c--></p:child>\n",
        "</r>\n"
    );
    let mut doc = parse(xml);
    let mut history = History::new();
    let r = root_of(&doc);
    let child = child_named(&doc, r, "child");
    assert_eq!(doc.qname(child).unwrap().prefix(), Some("p"));

    // Rename with explicit namespace rebinding, then undo/redo.
    commit(
        &mut doc,
        &mut history,
        Command::RenameElement {
            node: child,
            new_name: xml_tool::core::QNameSpec {
                name: "q:other".into(),
                namespace_uri: Some("urn:q".into()),
            },
        },
    );
    let renamed = doc.qname(child).unwrap();
    assert_eq!(renamed.prefix(), Some("q"));
    assert_eq!(renamed.namespace_uri(), Some("urn:q"));
    assert!(doc.source().contains("q:other"), "{}", doc.source());
    assert_source_matches_dom(&doc);

    history.undo(&mut doc).expect("undo");
    let restored = doc.qname(child).unwrap();
    assert_eq!(restored.prefix(), Some("p"));
    assert_eq!(restored.namespace_uri(), Some("urn:p"));
    // Mixed content survived the rename round-trip.
    assert!(doc.source().contains("lead <b>bold</b> tail<!--c-->"));
    assert_source_matches_dom(&doc);
    history.redo(&mut doc).expect("redo");
    assert!(doc.source().contains("q:other"));
}

#[test]
fn hundred_leaf_edits_stay_within_history_budget() {
    let mut doc = parse("<r><i>0</i></r>");
    let limits = HistoryLimits {
        coalesce_window: Duration::from_millis(0), // never merge: worst case
        ..HistoryLimits::default()
    };
    let mut history = History::with_limits(limits);
    let r = root_of(&doc);
    let text = first_text_child(&doc, child_named(&doc, r, "i"));

    for value in 0..100 {
        history
            .commit(
                &mut doc,
                Command::SetNodeContent {
                    node: text,
                    content: NodeContent::Text(value.to_string()),
                },
            )
            .expect("edit applies");
    }
    assert_eq!(history.undo_depth(), 100);
    assert!(
        history.memory_bytes() <= 64 * 1024 * 1024,
        "history memory {} exceeds budget",
        history.memory_bytes()
    );
    // All 100 undos work and every intermediate state parses.
    for _ in 0..100 {
        history.undo(&mut doc).expect("undo");
    }
    assert_source_matches_dom(&doc);
}

#[test]
fn history_enforces_entry_and_memory_budgets() {
    let mut doc = parse("<r><i>0</i></r>");
    let limits = HistoryLimits {
        max_entries: 5,
        coalesce_window: Duration::from_millis(0),
        ..HistoryLimits::default()
    };
    let mut history = History::with_limits(limits);
    let r = root_of(&doc);
    let text = first_text_child(&doc, child_named(&doc, r, "i"));

    for value in 0..10 {
        history
            .commit(
                &mut doc,
                Command::SetNodeContent {
                    node: text,
                    content: NodeContent::Text(value.to_string()),
                },
            )
            .expect("edit applies");
    }
    assert_eq!(history.undo_depth(), 5, "entry budget must evict oldest");
}

#[test]
fn coalescing_merges_same_field_bursts() {
    let mut doc = parse("<r><i>0</i></r>");
    let mut history = History::new();
    let r = root_of(&doc);
    let text = first_text_child(&doc, child_named(&doc, r, "i"));

    for chunk in ["1", "12", "123"] {
        history
            .commit(
                &mut doc,
                Command::SetNodeContent {
                    node: text,
                    content: NodeContent::Text(chunk.into()),
                },
            )
            .expect("edit applies");
    }
    assert_eq!(history.undo_depth(), 1, "burst must merge into one entry");

    // One undo restores the pre-burst value, one redo the final value.
    history.undo(&mut doc).expect("undo");
    assert!(doc.source().contains(">0<"), "{}", doc.source());
    history.redo(&mut doc).expect("redo");
    assert!(doc.source().contains(">123<"), "{}", doc.source());
}

#[test]
fn dirty_flag_tracks_saved_cursor_both_ways() {
    let mut doc = parse("<r><a>1</a></r>");
    let mut history = History::new();
    let r = root_of(&doc);
    let text = first_text_child(&doc, child_named(&doc, r, "a"));

    assert!(!history.is_dirty());
    history.mark_saved();

    commit(
        &mut doc,
        &mut history,
        Command::SetNodeContent {
            node: text,
            content: NodeContent::Text("2".into()),
        },
    );
    assert!(history.is_dirty());

    history.undo(&mut doc).expect("undo");
    assert!(!history.is_dirty(), "undo back to save point must clean");
    history.redo(&mut doc).expect("redo");
    assert!(history.is_dirty());

    history.mark_saved();
    assert!(!history.is_dirty());
}

#[test]
fn revision_advances_exactly_once_per_command() {
    let mut doc = parse("<r><a>1</a></r>");
    let mut history = History::new();
    let r = root_of(&doc);
    let text = first_text_child(&doc, child_named(&doc, r, "a"));

    assert_eq!(doc.revision().0, 0);
    commit(
        &mut doc,
        &mut history,
        Command::SetNodeContent {
            node: text,
            content: NodeContent::Text("2".into()),
        },
    );
    assert_eq!(doc.revision().0, 1);
    history.undo(&mut doc).expect("undo");
    assert_eq!(doc.revision().0, 2);
    history.redo(&mut doc).expect("redo");
    assert_eq!(doc.revision().0, 3);
}

#[test]
fn edit_paths_avoid_full_document_snapshots() {
    // Structural gate: the core edit path must not clone whole documents or
    // use Arc::make_mut (the step-1 audit's memory complaint).
    let mut found_snapshot = Vec::new();
    if let Ok(entries) = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src/core")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "rs") {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                if text.contains("Arc::make_mut") || text.contains(".clone()\n    // full-document")
                {
                    found_snapshot.push(path.display().to_string());
                }
            }
        }
    }
    assert!(
        found_snapshot.is_empty(),
        "core modules must not use Arc::make_mut: {found_snapshot:?}"
    );
}
