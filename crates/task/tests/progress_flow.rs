//! Verifies that extract actually streams Progress events (the manager's
//! progress page depends on them).

use bit7z_rs::{ArchiveEngine, Bit7zEngine, CompressOptions};
use password::Password;
use std::sync::{Arc, Mutex, atomic::AtomicBool};
use task::{JobSpec, OverwriteSpec, TaskEvent, TaskRunner};

fn engine() -> Option<Arc<dyn ArchiveEngine>> {
    let dll = bit7z_rs::locate_dll()?;
    Some(Arc::new(Bit7zEngine::new(Some(dll.as_path())).ok()?))
}

fn engine_locks() -> (Arc<Mutex<()>>, std::fs::File) {
    use std::sync::{Mutex, OnceLock};
    static LOCK: OnceLock<Arc<Mutex<()>>> = OnceLock::new();
    let lock = LOCK.get_or_init(|| Arc::new(Mutex::new(()))).clone();
    let path = std::env::temp_dir().join("bit7z-engine-tests.lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .expect("create engine lock file");
    file.lock().expect("lock engine file");
    (lock, file)
}

/// The bit7z C++ layer is single-threaded across *all* engine instances,
/// and other crates' integration-test binaries run concurrently with this
/// one under `cargo test --workspace`. Hold a cross-process file lock (plus
/// the in-process mutex) for the whole body of every engine-using test.
#[test]
fn extract_emits_progress_events() {
    let (_engine_guard, _process_guard) = engine_locks();
    let Some(engine) = engine() else {
        eprintln!("skipped: 7zip.dll not found");
        return;
    };
    let dir = tempfile::tempdir().unwrap();

    // ~12 MiB of incompressible-ish data across two files so the progress
    // callback has room to fire multiple times.
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let mut state: u64 = 0x243F6A8885A308D3;
    let mut next_byte = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state as u8
    };
    for name in ["big1.bin", "big2.bin"] {
        let bytes: Vec<u8> = (0..6 * 1024 * 1024).map(|_| next_byte()).collect();
        std::fs::write(src.join(name), &bytes).unwrap();
    }
    let archive = dir.path().join("big.7z");
    let mut options = CompressOptions::default();
    options.password = Some("pw".into());
    options.encrypt_headers = true;
    engine
        .compress(&[src.clone()], &archive, &options)
        .expect("compress");

    let progress_events = Mutex::new(Vec::new());
    let out = dir.path().join("out");
    let spec = JobSpec::Extract {
        archive,
        items: vec![],
        target: out.clone(),
        overwrite: OverwriteSpec::Ask,
        password_hint: true,
    };
    let runner = TaskRunner::new(engine);
    let mut rx = runner.run_with_controls(
        spec,
        Some(&Password::new("pw")),
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    loop {
        match rx.recv().expect("event") {
            TaskEvent::Progress { processed, total } => {
                progress_events.lock().unwrap().push((processed, total));
            }
            TaskEvent::FileStarted { .. } => {}
            TaskEvent::OverwriteConflict { .. } => panic!("no conflict expected"),
            TaskEvent::Finished { success, message } => {
                assert!(success, "{message}");
                break;
            }
        }
    }
    let events = progress_events.into_inner().unwrap();
    eprintln!("progress events received: {}", events.len());
    let non_zero = events.iter().filter(|(p, _)| *p > 0).count();
    eprintln!("non-zero processed events: {non_zero}");
    if let Some((p, t)) = events.last() {
        eprintln!("last event: processed={p} total={t}");
    }
    assert!(
        non_zero > 0 || events.last().is_some_and(|(p, _)| *p > 0),
        "extract must report non-zero progress at some point (file-level or final)"
    );
}

#[test]
fn test_emits_progress_events() {
    let (_engine_guard, _process_guard) = engine_locks();
    let Some(engine) = engine() else {
        eprintln!("skipped: 7zip.dll not found");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let mut state: u64 = 0x243F6A8885A308D3;
    let mut next_byte = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state as u8
    };
    for name in ["big1.bin", "big2.bin"] {
        let bytes: Vec<u8> = (0..6 * 1024 * 1024).map(|_| next_byte()).collect();
        std::fs::write(src.join(name), &bytes).unwrap();
    }
    let archive = dir.path().join("big.7z");
    let mut options = CompressOptions::default();
    options.password = Some("pw".into());
    options.encrypt_headers = true;
    engine
        .compress(&[src.clone()], &archive, &options)
        .expect("compress");

    let progress_events = Mutex::new(Vec::new());
    let spec = JobSpec::Test {
        archive,
        password_hint: true,
    };
    let runner = TaskRunner::new(engine);
    let mut rx = runner.run_with_controls(
        spec,
        Some(&Password::new("pw")),
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    loop {
        match rx.recv().expect("event") {
            TaskEvent::Progress { processed, total } => {
                progress_events.lock().unwrap().push((processed, total));
            }
            TaskEvent::FileStarted { .. } => {}
            TaskEvent::OverwriteConflict { .. } => panic!("no conflict expected"),
            TaskEvent::Finished { success, message } => {
                assert!(success, "{message}");
                break;
            }
        }
    }
    let events = progress_events.into_inner().unwrap();
    eprintln!("test progress events received: {}", events.len());
    let non_zero = events.iter().filter(|(p, _)| *p > 0).count();
    eprintln!("non-zero processed events: {non_zero}");
    if let Some((p, t)) = events.last() {
        eprintln!("last event: processed={p} total={t}");
    }
    assert!(
        non_zero > 0 || events.last().is_some_and(|(p, _)| *p > 0),
        "test must report non-zero progress at some point (file-level or final)"
    );
}

/// Regression: a Test job must surface integrity failures as a failed task.
/// The engine returns Ok(TestResult { all_ok: false, .. }) for a corrupt
/// archive; the runner used to drop the result and report success.
#[test]
fn test_job_fails_on_corrupted_archive() {
    let (_engine_guard, _process_guard) = engine_locks();
    let Some(engine) = engine() else {
        eprintln!("skipped: 7zip.dll not found");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    // Incompressible payload so the corrupted bytes land in stored data.
    let mut state: u64 = 0x243F6A8885A308D3;
    let mut next_byte = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state as u8
    };
    let bytes: Vec<u8> = (0..256 * 1024).map(|_| next_byte()).collect();
    std::fs::write(src.join("payload.bin"), &bytes).unwrap();
    let archive = dir.path().join("corrupt.7z");
    engine
        .compress(&[src.clone()], &archive, &CompressOptions::default())
        .expect("compress");

    // Flip a byte in the middle of the file: past the header CRC region,
    // inside the solid compressed data stream.
    let mut raw = std::fs::read(&archive).unwrap();
    let mid = raw.len() / 2;
    raw[mid] ^= 0xFF;
    std::fs::write(&archive, &raw).unwrap();

    let spec = JobSpec::Test {
        archive,
        password_hint: false,
    };
    let runner = TaskRunner::new(engine);
    let rx = runner.run_with_controls(
        spec,
        None,
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let mut finished = None;
    while let Ok(event) = rx.recv() {
        if let TaskEvent::Finished { success, message } = event {
            finished = Some((success, message));
            break;
        }
    }
    let (success, message) = finished.expect("job must finish");
    assert!(
        !success,
        "corrupted archive must not report success (message: {message})"
    );
    assert!(
        message.contains("failed the integrity test"),
        "failure message must name the integrity failures, got: {message}"
    );
}
