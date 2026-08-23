//! Acceptance tests for `AGENTS_PLAN.md` step 3: workspace sessions,
//! background tasks with staleness filtering, atomic saves, crash
//! recovery, and external file-change monitoring.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use xml_tool::core::document::XmlDocument;
use xml_tool::core::{Command, NodeContent, Revision};
use xml_tool::fixtures;
use xml_tool::services::document_io::{
    OpenMode, SaveHooks, classify_bytes, document_bytes, save_bytes_atomically,
    save_bytes_with_hooks,
};
use xml_tool::services::recovery::{RecoverySnapshot, RecoveryStore};
use xml_tool::services::task_manager::{SessionId, TaskManager};
use xml_tool::services::watcher::{DEBOUNCE, FileChange, FileWatcher};
use xml_tool::services::workspace::{DocumentMode, FileType, WorkspaceState};

fn temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp dir")
}

// ---------------------------------------------------------------------------
// TaskManager: staleness filtering + cancellation
// ---------------------------------------------------------------------------

#[test]
fn stale_task_results_never_override_newer_revision() {
    let manager = TaskManager::new();
    let session = SessionId(7);
    let started_at = Revision(3);

    manager.spawn(session, started_at, |_| {
        Box::new(String::from("computed-on-rev-3"))
    });
    // The document moved on before the job finished.
    let stale = manager.wait_for_outcome(session, Revision(4));
    assert!(
        stale.is_none(),
        "a result for revision 3 must not surface against revision 4"
    );

    // The correct revision still receives it (take the next job).
    manager.spawn(session, Revision(4), |_| {
        Box::new(String::from("computed-on-rev-4"))
    });
    let outcome = manager
        .wait_for_outcome(session, Revision(4))
        .expect("fresh result");
    let payload = *outcome.result.downcast::<String>().expect("string payload");
    assert_eq!(payload, "computed-on-rev-4");
}

#[test]
fn results_belonging_to_other_sessions_are_dropped() {
    let manager = TaskManager::new();
    manager.spawn(SessionId(1), Revision(1), |_| {
        Box::new(String::from("session-1"))
    });
    assert!(
        manager
            .wait_for_outcome(SessionId(2), Revision(1))
            .is_none()
    );
}

#[test]
fn cancelled_jobs_have_their_results_discarded() {
    let manager = TaskManager::new();
    let session = SessionId(9);
    let (job, _flag) = manager.spawn(session, Revision(1), |_| Box::new(String::from("late")));
    manager.cancel(job);
    assert!(manager.wait_for_outcome(session, Revision(1)).is_none());
}

#[test]
fn cancel_session_invalidates_every_job_of_that_session() {
    let manager = TaskManager::new();
    let session = SessionId(4);
    let (first, _) = manager.spawn(session, Revision(1), |_| Box::new(String::from("a")));
    let (second, _) = manager.spawn(session, Revision(1), |_| Box::new(String::from("b")));
    manager.cancel_session(session);
    assert!(manager.wait_for_outcome(session, Revision(1)).is_none());
    assert_ne!(first, second);
}

#[test]
fn parallel_jobs_all_complete() {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let manager = TaskManager::with_parallelism(4);
    let session = SessionId(1);
    for i in 0..8 {
        manager.spawn(session, Revision(1), move |_| {
            std::thread::sleep(Duration::from_millis(10));
            COUNTER.fetch_add(1, Ordering::SeqCst);
            Box::new(format!("job-{i}"))
        });
    }
    let mut received = 0;
    while received < 8 {
        if manager.wait_for_outcome(session, Revision(1)).is_some() {
            received += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Mode classification
// ---------------------------------------------------------------------------

#[test]
fn threshold_documents_classify_correctly() {
    let editable = classify_bytes(b"<r><a/></r>").unwrap();
    assert_eq!(editable.mode, OpenMode::Editable);
    assert_eq!(editable.elements, 2);

    // 200,000-element fixture: exactly on the element ceiling.
    let nodes = fixtures::large_nodes_xml();
    let outcome = classify_bytes(nodes.as_bytes()).unwrap();
    assert_eq!(outcome.mode, OpenMode::Editable);
    assert_eq!(outcome.elements, 200_000);

    // 20 MiB fixture: on the byte ceiling.
    let bytes = fixtures::large_bytes_xml();
    let outcome = classify_bytes(bytes.as_bytes()).unwrap();
    assert_eq!(outcome.mode, OpenMode::Editable);
    assert_eq!(outcome.elements, 50_001);

    // Above the element ceiling but under the open limit: read-only.
    let mut heavy = String::from("<r>");
    for i in 0..200_001 {
        heavy.push_str(&format!("<i{i}/>"));
    }
    heavy.push_str("</r>");
    let outcome = classify_bytes(heavy.as_bytes()).unwrap();
    assert_eq!(outcome.mode, OpenMode::LargeReadOnly);
}

#[test]
fn oversize_input_is_refused_before_any_dom_work() {
    let mut oversized = vec![b' '; 256 * 1024 * 1024 + 1];
    oversized[..5].copy_from_slice(b"<r/>x");
    let err = classify_bytes(&oversized).unwrap_err();
    assert_eq!(err.code(), xml_tool::xml::XmlErrorCode::InputTooLarge);
}

// ---------------------------------------------------------------------------
// Atomic save
// ----------------------------------------------------------------++

struct FailBeforeRename;

impl SaveHooks for FailBeforeRename {
    fn before_rename(&mut self, _temp: &std::path::Path) -> std::io::Result<()> {
        Err(std::io::Error::other("simulated crash before rename"))
    }
}

#[test]
fn interrupted_save_leaves_original_file_untouched() {
    let dir = temp_dir();
    let path = dir.path().join("doc.xml");
    std::fs::write(&path, b"<original/>").unwrap();
    let checksum_before = sha256(&std::fs::read(&path).unwrap());

    let result = save_bytes_with_hooks(&path, b"<half-written/>", &mut FailBeforeRename);
    assert!(result.is_err(), "injected failure must propagate");

    let after = std::fs::read(&path).unwrap();
    assert_eq!(
        sha256(&after),
        checksum_before,
        "original bytes must survive"
    );
    // No temp litter.
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temp files must be cleaned up");
}

#[test]
fn successful_save_replaces_content_atomically() {
    let dir = temp_dir();
    let path = dir.path().join("doc.xml");
    save_bytes_atomically(&path, b"<saved/>").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"<saved/>");
}

#[test]
fn document_bytes_replay_originals_and_reencode_edits() {
    // Unedited: byte replay including BOM.
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"<r><a x=\"1\"/></r>");
    let doc = XmlDocument::parse(&bytes).unwrap();
    assert_eq!(document_bytes(&doc), bytes);

    // Edited UTF-16 document keeps its encoding on save.
    let text = "<?xml version=\"1.0\" encoding=\"UTF-16\"?><r>\u{4e2d}\u{6587}</r>";
    let utf16 =
        xml_tool::xml::encoding::encode_xml_text(text, xml_tool::xml::SourceEncoding::Utf16LeBom);
    let mut doc = XmlDocument::parse(&utf16).unwrap();
    let r = doc.root_element().unwrap();
    let text_node = doc
        .children(r)
        .into_iter()
        .find(|id| doc.kind(*id) == Some(xml_tool::core::XmlNodeKind::Text))
        .unwrap();
    doc_mark_edited(&mut doc, text_node);
    let saved = document_bytes(&doc);
    assert_eq!(
        &saved[..2],
        &[0xFF, 0xFE],
        "UTF-16LE BOM must survive edits"
    );
}

fn doc_mark_edited(doc: &mut XmlDocument, text_node: xml_tool::core::NodeId) {
    let mut history = xml_tool::core::History::new();
    history
        .commit(
            doc,
            Command::SetNodeContent {
                node: text_node,
                content: NodeContent::Text("\u{4e2d}\u{6587}\u{65b0}".into()),
            },
        )
        .expect("edit applies");
}

// ---------------------------------------------------------------------------
// Workspace: multi-tab independence
// ---------------------------------------------------------------------------

fn open_session(workspace: &mut WorkspaceState, dir: &std::path::Path, index: usize) -> SessionId {
    let path = dir.join(format!("doc{index}.xml"));
    std::fs::write(
        &path,
        format!("<doc{index}><a>text{index}</a></doc{index}>"),
    )
    .unwrap();
    let document = XmlDocument::parse(std::fs::read(&path).unwrap().as_slice()).unwrap();
    workspace.add_opened(path, FileType::Xml, DocumentMode::Editable, document)
}

#[test]
fn ten_sessions_keep_tab_state_independent() {
    let dir = temp_dir();
    let mut workspace = WorkspaceState::new();
    let mut ids = Vec::new();
    for index in 0..10 {
        ids.push(open_session(&mut workspace, dir.path(), index));
    }
    assert_eq!(workspace.sessions().len(), 10);

    // Edit session 3 only.
    workspace.select(3);
    let session = workspace.active_mut().unwrap();
    let root = session.document.root_element().unwrap();
    let inner = session.document.children(root)[0];
    let text_node = session
        .document
        .children(inner)
        .into_iter()
        .find(|id| session.document.kind(*id) == Some(xml_tool::core::XmlNodeKind::Text))
        .unwrap();
    session
        .history
        .commit(
            &mut session.document,
            Command::SetNodeContent {
                node: text_node,
                content: NodeContent::Text("edited".into()),
            },
        )
        .unwrap();
    session.cursor = 42;
    session.selection = Some(root);

    // Only session 3 is dirty; every other tab is untouched.
    assert_eq!(workspace.dirty_sessions(), vec![ids[3]]);
    for (index, id) in ids.iter().enumerate() {
        let session = workspace
            .sessions()
            .iter()
            .find(|session| session.id == *id)
            .unwrap();
        let expected_dirty = index == 3;
        assert_eq!(session.is_dirty(), expected_dirty, "session {index}");
        if !expected_dirty {
            assert_eq!(session.cursor, 0);
            assert_eq!(session.selection, None);
        }
    }

    // Close a different tab; session 3 keeps its state.
    workspace.close(0);
    let survivor = workspace
        .sessions()
        .iter()
        .find(|session| session.id == ids[3])
        .unwrap();
    assert!(survivor.is_dirty());
    assert_eq!(survivor.cursor, 42);
}

#[test]
fn reopening_the_same_path_focuses_instead_of_duplicating() {
    let dir = temp_dir();
    let path = dir.path().join("same.xml");
    std::fs::write(&path, "<r/>").unwrap();
    let document = XmlDocument::parse(std::fs::read(&path).unwrap().as_slice()).unwrap();

    let mut workspace = WorkspaceState::new();
    workspace.add_opened(
        path.clone(),
        FileType::Xml,
        DocumentMode::Editable,
        document,
    );
    workspace.add_untitled();

    let focused = workspace
        .focus_existing(&path)
        .expect("existing session found");
    assert_eq!(focused, 0);
    assert_eq!(workspace.active_id().unwrap(), SessionId(1));
    assert_eq!(workspace.sessions().len(), 2, "no duplicate tab");
}

#[test]
fn untitled_sessions_get_increasing_names_and_paths_on_save() {
    let mut workspace = WorkspaceState::new();
    let first = workspace.add_untitled();
    let second = workspace.add_untitled();
    let names: Vec<String> = workspace
        .sessions()
        .iter()
        .map(|session| session.display_name())
        .collect();
    assert_eq!(names[0], "Untitled-1");
    assert_eq!(names[1], "Untitled-2");

    workspace.assign_path(second, PathBuf::from("/tmp/saved.xml"));
    let session = workspace
        .sessions()
        .iter()
        .find(|session| session.id == second)
        .unwrap();
    assert_eq!(session.display_name(), "saved");
    assert_eq!(
        session.path.as_deref(),
        Some(std::path::Path::new("/tmp/saved.xml"))
    );
    assert_ne!(first, second);
}

// ---------------------------------------------------------------------------
// Recovery snapshots
// ---------------------------------------------------------------------------

#[test]
fn recovery_snapshot_round_trips_everything() {
    let dir = temp_dir();
    let store = RecoveryStore::new(dir.path().join("recovery"));

    store
        .write(
            5,
            &RecoverySnapshot {
                title: "doc.xml".into(),
                path: Some(PathBuf::from("/tmp/doc.xml")),
                source: "<r><a>编辑内容</a></r>".into(),
                dirty: true,
                cursor: 123,
                selection_path: Some("/r/a[1]".into()),
                written_at: 0,
            },
        )
        .unwrap();

    let all = store.load_all();
    assert_eq!(all.len(), 1);
    let (session, snapshot) = &all[0];
    assert_eq!(*session, 5);
    assert_eq!(snapshot.source, "<r><a>编辑内容</a></r>");
    assert_eq!(snapshot.path, Some(PathBuf::from("/tmp/doc.xml")));
    assert_eq!(snapshot.cursor, 123);
    assert_eq!(snapshot.selection_path.as_deref(), Some("/r/a[1]"));
    assert!(snapshot.dirty);
    assert!(snapshot.written_at > 0);

    store.remove(5);
    assert!(store.load_all().is_empty());
}

#[test]
fn recovery_survives_rewrites_and_ignores_foreign_files() {
    let dir = temp_dir();
    let store = RecoveryStore::new(dir.path());
    for round in 0..3 {
        store
            .write(
                1,
                &RecoverySnapshot {
                    title: "t".into(),
                    path: None,
                    source: format!("<r v=\"{round}\"/>"),
                    dirty: true,
                    cursor: 0,
                    selection_path: None,
                    written_at: 0,
                },
            )
            .unwrap();
    }
    std::fs::write(dir.path().join("not-a-snapshot.json"), "garbage").unwrap();
    std::fs::write(dir.path().join("session-99.json"), "garbage").unwrap();

    let all = store.load_all();
    assert_eq!(all.len(), 1, "only the valid snapshot loads");
    assert_eq!(all[0].1.source, "<r v=\"2\"/>");
}

// ---------------------------------------------------------------------------
// External file changes (three branches drive the UI banners; here we
// verify the watcher reports the change kind correctly)
// ---------------------------------------------------------------------------

#[test]
fn external_modification_and_removal_are_detected() {
    let dir = temp_dir();
    let path = dir.path().join("watched.xml");
    std::fs::write(&path, "<r/>").unwrap();

    let mut watcher = FileWatcher::new().expect("watcher starts");
    watcher.watch(&path).unwrap();

    // Branch 1: clean document, file modified on disk.
    let canonical = path.canonicalize().unwrap();
    std::fs::write(&path, "<r changed=\"yes\"/>").unwrap();
    let change = wait_for_change(&mut watcher, &path, || {
        std::fs::write(&path, "<r changed=\"yes\"/>").ok()
    });
    assert_eq!(change, FileChange::Modified(canonical.clone()));

    // Branch 3: file removed.
    std::fs::remove_file(&path).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut removal = None;
    while std::time::Instant::now() < deadline {
        for change in watcher.poll_changes() {
            if change.path().ends_with("watched.xml") {
                removal = Some(change);
            }
        }
        if removal.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        removal.expect("removal detected"),
        FileChange::Removed(canonical)
    );
}

fn wait_for_change(
    watcher: &mut FileWatcher,
    path: &std::path::Path,
    retrigger: impl Fn() -> Option<()>,
) -> FileChange {
    let target = path.canonicalize().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut last_touch = std::time::Instant::now() - DEBOUNCE;
    while std::time::Instant::now() < deadline {
        if last_touch.elapsed() > Duration::from_millis(1500) {
            retrigger();
            last_touch = std::time::Instant::now();
        }
        for change in watcher.poll_changes() {
            if *change.path() == target {
                return change;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("no debounced change arrived for {}", target.display());
}

/// Minimal SHA-256 for checksum assertions (no extra dependency).
fn sha256(data: &[u8]) -> String {
    // A checksum only needs to detect change; use a stable 64-bit FNV
    // digest rendered as hex (test-only, not cryptographic).
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in data {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
