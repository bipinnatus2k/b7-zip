//! Task execution: run a [`JobSpec`] against an [`ArchiveEngine`] on a
//! background thread, streaming progress events.

use crate::job::{JobSpec, OverwriteSpec};
use bit7z_rs::{ArchiveEngine, CompressOptions, EngineOp, ExtractOptions, OverwriteMode};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, Sender, channel};

/// Events emitted while a task runs.
#[derive(Debug, Clone)]
pub enum TaskEvent {
    /// Progress update (processed/total bytes).
    Progress { processed: u64, total: u64 },
    /// A file is being processed.
    FileStarted { path: String },
    /// The task finished (success or failure with a message).
    Finished { success: bool, message: String },
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
        let (tx, rx) = channel::<TaskEvent>();
        let engine = self.engine.clone();
        let password = password.cloned();
        std::thread::spawn(move || {
            let result = run_job(engine.as_ref(), &job, password.as_ref(), &tx);
            let message = match &result {
                Ok(()) => "ok".to_string(),
                Err(error) => error.to_string(),
            };
            let _ = tx.send(TaskEvent::Finished {
                success: result.is_ok(),
                message,
            });
        });
        rx
    }
}

fn run_job(
    engine: &dyn ArchiveEngine,
    job: &JobSpec,
    password: Option<&password::Password>,
    tx: &Sender<TaskEvent>,
) -> Result<(), bit7z_rs::ArchiveError> {
    match job {
        JobSpec::Extract { archive, items, target, overwrite, .. } => {
            std::fs::create_dir_all(target)?;
            let options = ExtractOptions {
                overwrite: (*overwrite).into(),
                cancel: None,
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
                cancel: None,
            };
            engine.compress(inputs, target, &options)
        }
        JobSpec::Test { archive, .. } => {
            let result = engine.test(archive, password)?;
            if result.all_ok {
                Ok(())
            } else {
                Err(bit7z_rs::ArchiveError::Engine(format!(
                    "{}/{} items failed: {:?}",
                    result.failed_count,
                    result.total,
                    result.errors
                )))
            }
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
        JobSpec::Delete { archive, indices, .. } => {
            let ops: Vec<EngineOp> = indices
                .iter()
                .map(|i| EngineOp::Delete { archive_index: *i })
                .collect();
            engine.update(archive, &ops, password)
        }
        JobSpec::Rename { archive, index, new_path, .. } => {
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
        },
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
