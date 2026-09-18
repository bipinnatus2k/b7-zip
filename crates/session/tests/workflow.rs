//! Full-workflow integration tests: open archive -> extract entry to temp
//! working dir -> edit the file -> watcher picks it up -> overlay marks it
//! dirty -> changeset -> commit back -> reopen and verify.

use bit7z_rs::{ArchiveEngine, Bit7zEngine, CompressOptions};
use fs_watcher::{FsEvent, FsEventKind};
use session::{ArchiveSession, SessionStore};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

/// The bit7z C++ layer is single-threaded across *all* engine instances,
/// and `cargo test --workspace` runs the integration-test binaries of
/// different crates as separate processes. Hold a cross-process file lock
/// (plus the in-process mutex for same-binary parallelism) for the whole
/// body of every engine-using test.
fn engine_lock() -> Arc<Mutex<()>> {
    static LOCK: OnceLock<Arc<Mutex<()>>> = OnceLock::new();
    LOCK.get_or_init(|| Arc::new(Mutex::new(()))).clone()
}

fn engine_process_lock() -> std::fs::File {
    let path = std::env::temp_dir().join("bit7z-engine-tests.lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .expect("create engine lock file");
    file.lock().expect("lock engine file");
    file
}

fn engine() -> Option<Arc<dyn ArchiveEngine>> {
    let dll = bit7z_rs::locate_dll()?;
    Some(Arc::new(Bit7zEngine::new(Some(dll.as_path())).ok()?))
}

fn make_archive(dir: &std::path::Path, engine: &Arc<dyn ArchiveEngine>) -> PathBuf {
    let src = dir.join("doc.txt");
    std::fs::write(&src, "original content\n").unwrap();
    let archive = dir.join("work.7z");
    engine
        .compress(&[src], &archive, &CompressOptions::default())
        .expect("compress");
    archive
}

#[test]
fn edit_and_commit_workflow() {
    let _engine_guard = engine_lock();
    let _process_guard = engine_process_lock();
    let Some(engine) = engine() else {
        eprintln!("skipped: 7zip.dll not found");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    temp::register_temp_root(dir.path().join("temp")).unwrap();
    let archive = make_archive(dir.path(), &engine);

    // 1. Open the archive: overlay contains the entry.
    let mut session = ArchiveSession::open(101, engine.clone(), &archive, None).expect("open");
    assert_eq!(session.overlay().base().len(), 2); // root + doc.txt
    assert!(!session.has_changes());

    // 2. Lazily extract the entry into the temp working dir.
    let entry_index = session
        .overlay()
        .base()
        .nodes()
        .values()
        .find(|n| n.name == "doc.txt")
        .unwrap()
        .archive_index()
        .unwrap();
    let extracted = session.extract_to_workdir(entry_index).expect("extract to workdir");
    assert!(extracted.exists());
    assert_eq!(std::fs::read_to_string(&extracted).unwrap(), "original content\n");

    // 3. The user edits the file and saves.
    std::fs::write(&extracted, "EDITED CONTENT\n").unwrap();

    // 4. A watcher/fs event arrives; the overlay marks the node dirty.
    session.apply_event(&FsEvent {
        kind: FsEventKind::Modify,
        path: extracted.clone(),
    });
    assert!(session.has_changes());
    let changeset = session.changeset();
    assert_eq!(changeset.modifications.len(), 1);
    assert_eq!(changeset.modifications[0].archive_index, entry_index);
    assert_eq!(changeset.modifications[0].archive_path, "doc.txt");

    // 5. Commit the change back to the archive.
    session.commit().expect("commit");
    assert!(!session.has_changes());

    // 6. Reopen and verify the new content.
    let fresh = ArchiveSession::open(102, engine.clone(), &archive, None).expect("reopen");
    let fresh_index = fresh
        .overlay()
        .base()
        .nodes()
        .values()
        .find(|n| n.name == "doc.txt")
        .unwrap()
        .archive_index()
        .unwrap();
    let bytes = engine.extract_to_buffer(&archive, fresh_index, None).expect("buffer");
    assert_eq!(String::from_utf8_lossy(&bytes), "EDITED CONTENT\n");
}

#[test]
fn add_and_delete_via_session() {
    let _engine_guard = engine_lock();
    let _process_guard = engine_process_lock();
    let Some(engine) = engine() else {
        eprintln!("skipped: 7zip.dll not found");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    temp::register_temp_root(dir.path().join("temp2")).unwrap();
    let archive = make_archive(dir.path(), &engine);
    let mut session = ArchiveSession::open(201, engine.clone(), &archive, None).expect("open");

    // Add a new file into the working dir and sync it into the overlay.
    let new_file = session.work_dir().join("new.txt");
    std::fs::write(&new_file, "brand new").unwrap();
    session.apply_event(&FsEvent {
        kind: FsEventKind::Create,
        path: new_file.clone(),
    });
    let changeset = session.changeset();
    assert_eq!(changeset.additions.len(), 1);
    assert_eq!(changeset.additions[0].archive_path, "new.txt");

    // Delete the existing entry.
    let doc_index = session
        .overlay()
        .base()
        .nodes()
        .values()
        .find(|n| n.name == "doc.txt")
        .unwrap()
        .archive_index()
        .unwrap();
    session.overlay_mut().remove_path("doc.txt");
    let changeset = session.changeset();
    assert_eq!(changeset.deletions.len(), 1);
    assert_eq!(changeset.deletions[0].archive_index, doc_index);

    session.commit().expect("commit");

    let entries = engine.list(&archive, None).expect("list");
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["new.txt"], "names: {names:?}");
}

#[test]
fn staged_partial_commit_only_writes_selected_entries() {
    let _engine_guard = engine_lock();
    let _process_guard = engine_process_lock();
    let Some(engine) = engine() else {
        eprintln!("skipped: 7zip.dll not found");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    temp::register_temp_root(dir.path().join("temp3")).unwrap();
    let archive = make_archive(dir.path(), &engine);
    let mut session = ArchiveSession::open(301, engine.clone(), &archive, None).expect("open");

    // Two edits: an added file (staged) and a modified file (unstaged).
    let added = session.work_dir().join("added.txt");
    std::fs::write(&added, "added body").unwrap();
    session.apply_event(&FsEvent {
        kind: FsEventKind::Create,
        path: added.clone(),
    });

    let doc_index = session
        .overlay()
        .base()
        .nodes()
        .values()
        .find(|n| n.name == "doc.txt")
        .unwrap()
        .archive_index()
        .unwrap();
    let extracted = session.extract_to_workdir(doc_index).expect("extract");
    std::fs::write(&extracted, "UNSTAGED EDIT\n").unwrap();
    session.apply_event(&FsEvent {
        kind: FsEventKind::Modify,
        path: extracted.clone(),
    });

    // Stage only the addition.
    let rows = session.changes();
    assert_eq!(rows.len(), 2, "rows: {rows:?}");
    let added_row = rows.iter().find(|r| r.path == "added.txt").expect("added row");
    assert_eq!(added_row.state, vfs::DirtyState::Added);
    assert!(!added_row.staged);
    session.stage([added_row.id]);

    let staged = session.staged_changeset();
    assert_eq!(staged.additions.len(), 1);
    assert!(staged.modifications.is_empty());
    assert_eq!(session.unstaged_changeset().modifications.len(), 1);

    // Commit the staged subset; the archive gains added.txt but keeps the
    // original doc.txt content.
    let committed = session.commit_staged().expect("commit staged");
    assert_eq!(committed.additions.len(), 1);
    assert!(session.unstaged_changeset().modifications.len() == 1, "unstaged edit survives the partial commit");

    let entries = engine.list(&archive, None).expect("list");
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"added.txt"), "names: {names:?}");
    assert!(names.contains(&"doc.txt"), "names: {names:?}");
    let doc_entry = entries.iter().find(|e| e.name == "doc.txt").unwrap();
    let bytes = engine
        .extract_to_buffer(&archive, doc_entry.index, None)
        .expect("buffer");
    assert_eq!(
        String::from_utf8_lossy(&bytes),
        "original content\n",
        "unstaged edit must not be written"
    );

    // The unstaged modification is still dirty and still commits correctly
    // afterwards (the base node kept its archive_index mapping).
    session.stage_all();
    session.commit_staged().expect("commit rest");
    let entries = engine.list(&archive, None).expect("list");
    let doc_entry = entries.iter().find(|e| e.name == "doc.txt").unwrap();
    let bytes = engine
        .extract_to_buffer(&archive, doc_entry.index, None)
        .expect("buffer");
    assert_eq!(String::from_utf8_lossy(&bytes), "UNSTAGED EDIT\n");
    assert!(!session.has_changes());
}

#[test]
fn discard_unstaged_keeps_staged_state() {
    let _engine_guard = engine_lock();
    let _process_guard = engine_process_lock();
    let Some(engine) = engine() else {
        eprintln!("skipped: 7zip.dll not found");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    temp::register_temp_root(dir.path().join("temp4")).unwrap();
    let archive = make_archive(dir.path(), &engine);
    let mut session = ArchiveSession::open(401, engine.clone(), &archive, None).expect("open");

    // Added file (unstaged) + modified file (staged).
    let added = session.work_dir().join("junk.txt");
    std::fs::write(&added, "junk").unwrap();
    session.apply_event(&FsEvent {
        kind: FsEventKind::Create,
        path: added.clone(),
    });
    let doc_index = session
        .overlay()
        .base()
        .nodes()
        .values()
        .find(|n| n.name == "doc.txt")
        .unwrap()
        .archive_index()
        .unwrap();
    let extracted = session.extract_to_workdir(doc_index).expect("extract");
    std::fs::write(&extracted, "KEEP ME\n").unwrap();
    session.apply_event(&FsEvent {
        kind: FsEventKind::Modify,
        path: extracted.clone(),
    });
    let mod_row = session
        .changes()
        .into_iter()
        .find(|r| r.path == "doc.txt")
        .expect("modified row");
    session.stage([mod_row.id]);

    session.discard_unstaged();
    let rows = session.changes();
    assert_eq!(rows.len(), 1, "rows: {rows:?}");
    assert_eq!(rows[0].path, "doc.txt");
    assert!(rows[0].staged);
    assert!(session.overlay().working().resolve_path("doc.txt").is_some());
    assert!(session.overlay().working().resolve_path("junk.txt").is_none());

    // The staged edit still commits cleanly after the discard.
    session.commit_staged().expect("commit");
    let entries = engine.list(&archive, None).expect("list");
    let doc_entry = entries.iter().find(|e| e.name == "doc.txt").unwrap();
    let bytes = engine
        .extract_to_buffer(&archive, doc_entry.index, None)
        .expect("buffer");
    assert_eq!(String::from_utf8_lossy(&bytes), "KEEP ME\n");
}

#[test]
fn session_store_roundtrip() {
    let _engine_guard = engine_lock();
    let _process_guard = engine_process_lock();
    let store = SessionStore::new();
    let dir = tempfile::tempdir().unwrap();
    // A store works without a real archive for lifecycle tests: insert/remove.
    // We can't construct a session without an engine, so test the store's
    // bookkeeping with the real engine path guarded by dll availability.
    let Some(engine) = engine() else {
        eprintln!("skipped: 7zip.dll not found");
        return;
    };
    let archive = make_archive(dir.path(), &engine);
    let session = ArchiveSession::open(501, engine.clone(), &archive, None).expect("open");
    let handle = store.insert(session).expect("insert");
    assert!(store.contains(501));
    assert_eq!(store.all(), vec![501]);
    let got = store.get(501).expect("get");
    assert_eq!(got.lock().unwrap().id(), 501);
    let _ = handle;
    store.remove(501).expect("remove");
    assert!(!store.contains(501));
}
