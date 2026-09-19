//! Task execution: run a [`JobSpec`] against an [`ArchiveEngine`] on a
//! background thread, streaming progress events.

use crate::job::{JobSpec, OverwriteSpec};
use bit7z_rs::{ArchiveEngine, CompressOptions, EngineOp, ExtractOptions, OverwriteMode};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};

/// Refuses a compress job whose target file is one of its own inputs: the
/// writer truncates the target before reading the inputs, which would
/// destroy that input. Callers usually prevent this (the shell verbs append
/// `_new`, the CLI rejects explicitly); this is the last line of defense.
fn ensure_compress_target_not_input(inputs: &[std::path::PathBuf], target: &Path) -> Result<(), bit7z_rs::ArchiveError> {
    let Ok(target) = target.canonicalize() else {
        // A missing target cannot clobber anything yet.
        return Ok(());
    };
    for input in inputs {
        if let Ok(input) = input.canonicalize()
            && input == target
        {
            return Err(bit7z_rs::ArchiveError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "compress target would overwrite its own input",
            )));
        }
    }
    Ok(())
}

/// Events emitted while a task runs.
#[derive(Debug, Clone)]
pub enum TaskEvent {
    /// Progress update (processed/total bytes).
    Progress { processed: u64, total: u64 },
    /// A file is being processed.
    FileStarted { path: String },
    /// An extraction found an existing file and needs an overwrite decision.
    /// The receiver is answered with `true` (overwrite) or `false` (skip).
    OverwriteConflict {
        path: String,
        reply: SyncSender<bool>,
    },
    /// The task finished. `error` classifies a failure by kind so consumers
    /// never have to string-match the human-readable `message`.
    Finished {
        success: bool,
        message: String,
        error: Option<TaskErrorKind>,
    },
}

/// Coarse classification of a failed job, derived from the engine error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskErrorKind {
    /// The supplied (or dialog-entered) password did not unlock the archive.
    WrongPassword,
    /// The archive could not be opened: bad header, or a header-encrypted
    /// archive queried without the right password.
    OpenFailed,
    /// An integrity test found failing entries.
    Corrupted,
    /// The user cancelled the operation.
    Cancelled,
    /// A filesystem-level failure.
    Io,
    /// Any other engine-level failure.
    Engine,
    /// Anything not covered above.
    Other,
}

impl From<&bit7z_rs::ArchiveError> for TaskErrorKind {
    fn from(error: &bit7z_rs::ArchiveError) -> Self {
        use bit7z_rs::ArchiveError;
        match error {
            ArchiveError::WrongPassword => TaskErrorKind::WrongPassword,
            ArchiveError::OpenFailed(_) => TaskErrorKind::OpenFailed,
            ArchiveError::Corrupted(_) => TaskErrorKind::Corrupted,
            ArchiveError::Cancelled => TaskErrorKind::Cancelled,
            ArchiveError::Io(_) => TaskErrorKind::Io,
            ArchiveError::Engine(_) => TaskErrorKind::Engine,
            _ => TaskErrorKind::Other,
        }
    }
}

/// Executes archive tasks against an engine.
pub struct TaskRunner {
    engine: Arc<dyn ArchiveEngine>,
}

impl TaskRunner {
    pub fn new(engine: Arc<dyn ArchiveEngine>) -> Self {
        Self { engine }
    }

    pub fn engine(&self) -> &Arc<dyn ArchiveEngine> {
        &self.engine
    }

    /// Run a job on a background thread; returns the event stream.
    pub fn run(&self, job: JobSpec, password: Option<&password::Password>) -> Receiver<TaskEvent> {
        self.run_with_cancel(job, password, Arc::new(AtomicBool::new(false)))
    }

    /// Like [`run`](Self::run) but with a cancellation flag the caller can
    /// flip to abort the operation.
    pub fn run_with_cancel(
        &self,
        job: JobSpec,
        password: Option<&password::Password>,
        cancel: Arc<AtomicBool>,
    ) -> Receiver<TaskEvent> {
        self.run_with_controls(job, password, cancel, Arc::new(AtomicBool::new(false)))
    }

    /// Like [`run_with_cancel`](Self::run_with_cancel) but with an
    /// additional pause flag: while it is set the operation blocks inside
    /// its progress callback (a true pause — the engine thread is parked,
    /// the event stream simply stops flowing).
    pub fn run_with_controls(
        &self,
        job: JobSpec,
        password: Option<&password::Password>,
        cancel: Arc<AtomicBool>,
        pause: Arc<AtomicBool>,
    ) -> Receiver<TaskEvent> {
        let (tx, rx) = channel::<TaskEvent>();
        let engine = self.engine.clone();
        let password = password.cloned();
        std::thread::spawn(move || {
            let result = run_job_with_pause(
                engine.as_ref(),
                &job,
                password.as_ref(),
                &tx,
                &cancel,
                &pause,
            );
            let message = match &result {
                Ok(()) => "ok".to_string(),
                Err(error) => error.to_string(),
            };
            let error = result.as_ref().err().map(TaskErrorKind::from);
            let _ = tx.send(TaskEvent::Finished {
                success: result.is_ok(),
                message,
                error,
            });
        });
        rx
    }
}

fn run_job_with_pause(
    engine: &dyn ArchiveEngine,
    job: &JobSpec,
    password: Option<&password::Password>,
    tx: &Sender<TaskEvent>,
    cancel: &Arc<AtomicBool>,
    pause: &Arc<AtomicBool>,
) -> Result<(), bit7z_rs::ArchiveError> {
    match job {
        JobSpec::Extract {
            archive,
            items,
            target,
            overwrite,
            ..
        } => {
            std::fs::create_dir_all(target)?;
            let progress = {
                let tx = tx.clone();
                Arc::new(move |processed: u64, total: u64| {
                    let _ = tx.send(TaskEvent::Progress { processed, total });
                }) as Arc<dyn Fn(u64, u64) + Send + Sync>
            };
            let file = {
                let tx = tx.clone();
                Arc::new(move |path: &str| {
                    let _ = tx.send(TaskEvent::FileStarted {
                        path: path.to_string(),
                    });
                }) as Arc<dyn Fn(&str) + Send + Sync>
            };
            let on_conflict = if *overwrite == OverwriteSpec::Ask {
                let tx = tx.clone();
                Some(Arc::new(move |path: &str| -> bool {
                    let (reply, answer) = sync_channel(1);
                    let _ = tx.send(TaskEvent::OverwriteConflict {
                        path: path.to_string(),
                        reply,
                    });
                    answer.recv().unwrap_or(false)
                })
                    as Arc<dyn Fn(&str) -> bool + Send + Sync>)
            } else {
                None
            };
            let options = ExtractOptions {
                overwrite: (*overwrite).into(),
                cancel: Some(cancel.clone()),
                pause: Some(pause.clone()),
                progress: Some(progress),
                file: Some(file),
                on_conflict,
            };
            // An empty item list means "extract everything"; resolve it to the
            // full index set (an empty slice would be UB at the FFI boundary).
            let indices: Vec<u32> = if items.is_empty() {
                engine
                    .list(archive, password)?
                    .iter()
                    .map(|entry| entry.index)
                    .collect()
            } else {
                items.clone()
            };
            engine.extract(archive, &indices, target, password, &options)
        }
        JobSpec::Compress {
            inputs,
            target,
            format,
            level,
            solid,
            volume,
            threads,
            encrypt_headers,
            ..
        } => {
            ensure_compress_target_not_input(inputs, target)?;
            let progress = {
                let tx = tx.clone();
                Arc::new(move |processed: u64, total: u64| {
                    let _ = tx.send(TaskEvent::Progress { processed, total });
                }) as Arc<dyn Fn(u64, u64) + Send + Sync>
            };
            let file = {
                let tx = tx.clone();
                Arc::new(move |path: &str| {
                    let _ = tx.send(TaskEvent::FileStarted {
                        path: path.to_string(),
                    });
                }) as Arc<dyn Fn(&str) + Send + Sync>
            };
            let options = CompressOptions {
                format: (*format).into(),
                level: (*level).into(),
                method: None,
                dictionary_size: None,
                word_size: None,
                solid: *solid,
                volume_size: *volume,
                threads: threads.unwrap_or(0),
                password: password.map(|p| p.as_str().to_string()),
                encrypt_headers: *encrypt_headers,
                cancel: Some(cancel.clone()),
                pause: Some(pause.clone()),
                progress: Some(progress),
                file: Some(file),
            };
            engine.compress(inputs, target, &options)
        }
        JobSpec::Test { archive, .. } => {
            let progress = {
                let tx = tx.clone();
                Arc::new(move |processed: u64, total: u64| {
                    let _ = tx.send(TaskEvent::Progress { processed, total });
                }) as Arc<dyn Fn(u64, u64) + Send + Sync>
            };
            let file = {
                let tx = tx.clone();
                Arc::new(move |path: &str| {
                    let _ = tx.send(TaskEvent::FileStarted {
                        path: path.to_string(),
                    });
                }) as Arc<dyn Fn(&str) + Send + Sync>
            };
            let options = bit7z_rs::TestOptions {
                cancel: Some(cancel.clone()),
                pause: Some(pause.clone()),
                progress: Some(progress),
                file: Some(file),
            };
            let result = engine.test_with_options(archive, password, &options)?;
            // A passing `Ok` only means the test ran; integrity failures come
            // back as a TestResult, so the job must fail the same way.
            if !result.all_ok {
                return Err(bit7z_rs::ArchiveError::Corrupted(format!(
                    "{} of {} items failed the integrity test",
                    result.failed_count, result.total
                )));
            }
            Ok(())
        }
        JobSpec::Add { archive, items, .. } => {
            let ops: Vec<EngineOp> = items
                .iter()
                .map(|item| EngineOp::Add {
                    fs_path: item.fs_path.clone(),
                    archive_path: item.archive_path.clone(),
                })
                .collect();
            engine.update(archive, &ops, password)
        }
        JobSpec::Delete {
            archive, indices, ..
        } => {
            let ops: Vec<EngineOp> = indices
                .iter()
                .map(|i| EngineOp::Delete { archive_index: *i })
                .collect();
            engine.update(archive, &ops, password)
        }
        JobSpec::Rename {
            archive,
            index,
            new_path,
            ..
        } => {
            let ops = vec![EngineOp::Rename {
                archive_index: *index,
                new_path: new_path.clone(),
            }];
            engine.update(archive, &ops, password)
        }
        JobSpec::NewFolder { .. } => {
            // Creating empty folders is not representable as a file op yet;
            // the manager synthesizes it by adding a placeholder directory
            // through the engine once supported.
            Err(bit7z_rs::ArchiveError::UnsupportedOperation(
                "creating empty folders is not supported yet".into(),
            ))
        }
        JobSpec::Checksum { path, algorithm } => {
            let result = checksum::checksum_file(path, *algorithm)?;
            let _ = tx.send(TaskEvent::FileStarted { path: result.path });
            let _ = tx.send(TaskEvent::Progress {
                processed: result.size,
                total: result.size,
            });
            Ok(())
        }
    }
}

impl From<OverwriteSpec> for OverwriteMode {
    fn from(value: OverwriteSpec) -> Self {
        match value {
            OverwriteSpec::Ask => OverwriteMode::Ask,
            OverwriteSpec::Overwrite => OverwriteMode::Overwrite,
            OverwriteSpec::Skip => OverwriteMode::Skip,
            OverwriteSpec::AutoRename => OverwriteMode::AutoRename,
        }
    }
}

/// Map a VFS changeset to engine operations (commit path).
pub fn changeset_to_ops(changeset: &vfs::Changeset) -> Vec<EngineOp> {
    let mut ops = Vec::new();
    ops.extend(changeset.additions.iter().map(|op| EngineOp::Add {
        fs_path: op.fs_path.clone(),
        archive_path: op.archive_path.clone(),
    }));
    ops.extend(changeset.modifications.iter().map(|op| EngineOp::Modify {
        archive_index: op.archive_index,
        archive_path: op.archive_path.clone(),
        fs_path: op.fs_path.clone(),
    }));
    ops.extend(changeset.deletions.iter().map(|op| EngineOp::Delete {
        archive_index: op.archive_index,
    }));
    ops.extend(changeset.renames.iter().map(|op| EngineOp::Rename {
        archive_index: op.archive_index,
        new_path: op.new_path.clone(),
    }));
    ops
}

/// A shared cancellation flag for long-running jobs.
pub type CancelFlag = Arc<AtomicBool>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "b7zfm-runner-test-{}-{}",
            tag,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn compress_target_matching_an_input_is_rejected() {
        let dir = scratch_dir("self-overwrite");
        let input = dir.join("foo.7z");
        fs::write(&input, b"payload").unwrap();

        let err = ensure_compress_target_not_input(&[input.clone()], &input);
        assert!(err.is_err());

        // Same file spelled with different casing must still be caught.
        let dotted = dir.join("FOO.7z");
        assert!(ensure_compress_target_not_input(&[input.clone()], &dotted).is_err());

        // A distinct target is fine.
        let other = dir.join("bar.7z");
        assert!(ensure_compress_target_not_input(&[input], &other).is_ok());

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn compress_target_that_does_not_exist_yet_is_allowed() {
        let dir = scratch_dir("missing-target");
        let input = dir.join("foo.txt");
        fs::write(&input, b"payload").unwrap();
        assert!(
            ensure_compress_target_not_input(&[input], &dir.join("new.7z")).is_ok()
        );
        fs::remove_dir_all(dir).ok();
    }
}
