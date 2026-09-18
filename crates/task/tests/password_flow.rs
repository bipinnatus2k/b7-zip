//! Password-protected job flow: compress with a password (encrypted
//! headers), then extract via the TaskRunner the way the manager does.

use bit7z_rs::{ArchiveEngine, Bit7zEngine, CompressOptions};
use password::Password;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use task::{JobSpec, OverwriteSpec, TaskEvent, TaskRunner};

fn engine() -> Option<Arc<dyn bit7z_rs::ArchiveEngine>> {
    let dll = bit7z_rs::locate_dll()?;
    Some(Arc::new(Bit7zEngine::new(Some(dll.as_path())).ok()?))
}

/// Compresses `src` with `password` and encrypted headers via the engine
/// directly (the open/probe path the manager uses), then extracts through
/// TaskRunner with the session-style password reference.
#[test]
fn task_runner_extracts_header_encrypted_archive_with_password() {
    let Some(engine) = engine() else {
        eprintln!("skipped: 7zip.dll not found");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("secret.txt");
    std::fs::write(&src, "classified content\n").unwrap();
    let archive = dir.path().join("enc.7z");

    // Encrypt headers (file names) like the manager's password flow expects.
    let mut options = CompressOptions::default();
    options.password = Some("pw123".into());
    options.encrypt_headers = true;
    engine.compress(&[src.clone()], &archive, &options).expect("compress");

    // Without the password even the listing must fail.
    assert!(engine.list(&archive, None).is_err(), "list must fail without password");

    // The manager's extract job: TaskRunner with the session password.
    let runner = TaskRunner::new(engine.clone());
    let pw = Password::new("pw123");
    let out = dir.path().join("out");
    let spec = JobSpec::Extract {
        archive: archive.clone(),
        items: vec![],
        target: out.clone(),
        overwrite: OverwriteSpec::Ask,
        password_hint: true,
    };
    let mut rx = runner.run_with_controls(
        spec,
        Some(&pw),
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    loop {
        match rx.recv().expect("event") {
            TaskEvent::Progress { .. } | TaskEvent::FileStarted { .. } => {}
            TaskEvent::OverwriteConflict { .. } => panic!("no conflict expected"),
            TaskEvent::Finished { success, message } => {
                assert!(success, "extract failed: {message}");
                break;
            }
        }
    }
    let extracted = find_file(&out, "secret.txt");
    assert_eq!(std::fs::read_to_string(extracted).unwrap(), "classified content\n");
}

/// A wrong password on a header-encrypted archive fails at open time with
/// the generic open error (the header cannot be decrypted, so 7-Zip reports
/// an open failure rather than a password error). The manager keys its
/// retry prompt on this exact string when the job carried a password.
#[test]
fn task_runner_wrong_password_reports_wrong_password() {
    let Some(engine) = engine() else {
        eprintln!("skipped: 7zip.dll not found");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("secret.txt");
    std::fs::write(&src, "classified content\n").unwrap();
    let archive = dir.path().join("enc2.7z");

    let mut options = CompressOptions::default();
    options.password = Some("right".into());
    options.encrypt_headers = true;
    engine.compress(&[src], &archive, &options).expect("compress");

    let runner = TaskRunner::new(engine.clone());
    let out = dir.path().join("out2");
    let spec = JobSpec::Extract {
        archive: archive.clone(),
        items: vec![],
        target: out,
        overwrite: OverwriteSpec::Ask,
        password_hint: true,
    };
    let mut rx = runner.run_with_controls(
        spec,
        Some(&Password::new("WRONG")),
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    loop {
        match rx.recv().expect("event") {
            TaskEvent::Progress { .. } | TaskEvent::FileStarted { .. } => {}
            TaskEvent::OverwriteConflict { .. } => panic!("no conflict expected"),
            TaskEvent::Finished { success, message } => {
                assert!(!success);
                assert!(
                    message == "wrong password"
                        || message.contains("Failed to open archive"),
                    "message: {message}"
                );
                break;
            }
        }
    }
}

/// Overwrite conflicts must reach the UI as an answerable question and the
/// engine thread must not advance until answered.
#[test]
fn task_runner_conflict_waits_for_reply() {
    let Some(engine) = engine() else {
        eprintln!("skipped: 7zip.dll not found");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("dup.txt");
    std::fs::write(&src, "payload\n").unwrap();
    let archive = dir.path().join("dup.7z");
    let mut options = CompressOptions::default();
    options.password = Some("pw".into());
    options.encrypt_headers = true;
    engine.compress(&[src], &archive, &options).expect("compress");

    // First extraction populates the target.
    let out = dir.path().join("out");
    let spec = JobSpec::Extract {
        archive: archive.clone(),
        items: vec![],
        target: out.clone(),
        overwrite: OverwriteSpec::Ask,
        password_hint: true,
    };
    let runner = TaskRunner::new(engine.clone());
    let mut rx = runner.run_with_controls(
        spec,
        Some(&Password::new("pw")),
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    loop {
        match rx.recv().expect("event") {
            TaskEvent::Finished { success, message } => {
                assert!(success, "{message}");
                break;
            }
            _ => {}
        }
    }

    // Second extraction to the same target must raise a conflict.
    let spec = JobSpec::Extract {
        archive,
        items: vec![],
        target: out,
        overwrite: OverwriteSpec::Ask,
        password_hint: true,
    };
    let mut rx = runner.run_with_controls(
        spec,
        Some(&Password::new("pw")),
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let mut answered = false;
    loop {
        match rx.recv().expect("event") {
            TaskEvent::OverwriteConflict { reply, .. } => {
                let _ = reply.send(false); // skip
                answered = true;
            }
            TaskEvent::Finished { success, message } => {
                assert!(success, "{message}");
                assert!(answered, "a conflict must have been raised");
                break;
            }
            _ => {}
        }
    }
}

fn find_file(root: &std::path::Path, name: &str) -> PathBuf {
    fn walk(dir: &std::path::Path, name: &str, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, name, out);
            } else if path.file_name().is_some_and(|n| n == name) {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, name, &mut out);
    out.into_iter().next().expect("file not found in output tree")
}
