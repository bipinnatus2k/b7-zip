//! Full-workflow integration tests: open archive -> extract entry to temp
//! working dir -> edit the file -> watcher picks it up -> overlay marks it
//! dirty -> changeset -> commit back -> reopen and verify.

use bit7z_rs::{ArchiveEngine, Bit7zEngine, CompressOptions};
use fs_watcher::{FsEvent, FsEventKind};
use session::{ArchiveSession, SessionStore};
use std::path::PathBuf;
use std::sync::Arc;

fn find_dll() -> Option<PathBuf> {
    let vcpkg = std::env::var("VCPKG_ROOT").ok()?;
    let candidates = [
        format!("{vcpkg}/installed/x64-windows/bin/7zip.dll"),
        format!("{vcpkg}/installed/x64-windows/bin/7z.dll"),
    ];
    candidates.iter().map(PathBuf::from).find(|p| p.exists())
}

fn engine() -> Option<Arc<dyn ArchiveEngine>> {
    let dll = find_dll()?;
    Some(Arc::new(Bit7zEngine::new(Some(dll.to_str()?)).ok()?))
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
    let Some(engine) = engine() else {
        eprintln!("skipped: 7zip.dll not found");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    temp::register_temp_root(dir.path().join("temp")).unwrap();
    let archive = make_archive(dir.path(), &engine);

    // 1. Open the archive: overlay contains the entry.
    let mut session = ArchiveSession::open(1, engine.clone(), &archive, None).expect("open");
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
    let fresh = ArchiveSession::open(2, engine.clone(), &archive, None).expect("reopen");
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
    let Some(engine) = engine() else {
        eprintln!("skipped: 7zip.dll not found");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    temp::register_temp_root(dir.path().join("temp2")).unwrap();
    let archive = make_archive(dir.path(), &engine);
    let mut session = ArchiveSession::open(1, engine.clone(), &archive, None).expect("open");

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
fn session_store_roundtrip() {
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
    let session = ArchiveSession::open(7, engine.clone(), &archive, None).expect("open");
    let handle = store.insert(session).expect("insert");
    assert!(store.contains(7));
    assert_eq!(store.all(), vec![7]);
    let got = store.get(7).expect("get");
    assert_eq!(got.lock().unwrap().id(), 7);
    let _ = handle;
    store.remove(7).expect("remove");
    assert!(!store.contains(7));
}
