// //! Task orchestration: background task types and the Workspace task methods.
//
// use ::task::{JobSpec, TaskEvent, TaskRunner};
// use gpui::{App, Context};
// use guise::prelude::*;
// use std::path::PathBuf;
// use std::sync::Arc;
// use std::sync::atomic::{AtomicBool, Ordering};
// use std::sync::mpsc::{Receiver, SyncSender, TryRecvError};
// use crate::workspace::Workspace;
//
// /// What kind of long-running operation the progress dialog is showing.
// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
// pub(super) enum TaskKind {
//     Extract,
//     Compress,
//     Test,
//     Update,
// }
//
// impl TaskKind {
//     fn verb(self) -> &'static str {
//         match self {
//             TaskKind::Extract => "Extracting",
//             TaskKind::Compress => "Creating",
//             TaskKind::Test => "Testing",
//             TaskKind::Update => "Updating",
//         }
//     }
//
//     pub(super) fn supports_cancel(self) -> bool {
//         matches!(self, TaskKind::Extract | TaskKind::Compress)
//     }
// }
//
// /// A running (or just finished) background operation.
// pub(super) struct RunningTask {
//     pub(super) kind: TaskKind,
//     pub(super) label: String,
//     job: JobSpec,
//     cancel: Arc<AtomicBool>,
//     rx: Receiver<TaskEvent>,
//     pub(super) progress: Option<(u64, u64)>,
//     pub(super) current_file: Option<String>,
//     pub(super) finished: Option<(bool, String)>,
//     pub(super) overwrite_query: Option<(String, SyncSender<bool>)>,
// }
//
// /// An operation deferred until the user supplies a password.
// #[derive(Clone)]
// pub(super) enum PendingOp {
//     Open { path: PathBuf },
//     Job(JobSpec),
// }
//
// impl Workspace {
//     // ====================================================================
//     // Task execution
//     // ====================================================================
//
//     pub(super) fn start_task(&mut self, kind: TaskKind, label: String, job: JobSpec, cx: &mut App) {
//         let cancel = Arc::new(AtomicBool::new(false));
//         let runner = TaskRunner::new(self.engine.clone());
//         let password = self.password.clone();
//         let rx = runner.run_with_cancel(job.clone(), password.as_ref(), cancel.clone());
//         self.task = Some(RunningTask {
//             kind,
//             label,
//             job,
//             cancel,
//             rx,
//             progress: None,
//             current_file: None,
//             finished: None,
//             overwrite_query: None,
//         });
//         let this = self.self_entity.clone();
//         cx.spawn(async move |cx| {
//             loop {
//                 cx.background_executor()
//                     .timer(std::time::Duration::from_millis(50))
//                     .await;
//                 let done = this.update(cx, |ws, cx| ws.poll_task(cx)).unwrap_or(true);
//                 if done {
//                     break;
//                 }
//             }
//         })
//         .detach();
//         cx.refresh_windows();
//     }
//
//     /// Drain task events; returns true once the task finished.
//     fn poll_task(&mut self, cx: &mut Context<Self>) -> bool {
//         let Some(task) = &mut self.task else {
//             return true;
//         };
//         let mut done = false;
//         loop {
//             match task.rx.try_recv() {
//                 Ok(TaskEvent::Progress { processed, total }) => {
//                     task.progress = Some((processed, total));
//                     cx.notify();
//                 }
//                 Ok(TaskEvent::FileStarted { path }) => {
//                     task.current_file = Some(path);
//                     cx.notify();
//                 }
//                 Ok(TaskEvent::OverwriteConflict { path, reply }) => {
//                     task.overwrite_query = Some((path, reply));
//                     cx.notify();
//                 }
//                 Ok(TaskEvent::Finished { success, message }) => {
//                     task.finished = Some((success, message));
//                     done = true;
//                 }
//                 Err(TryRecvError::Empty) => break,
//                 Err(TryRecvError::Disconnected) => {
//                     task.finished = Some((false, "task worker stopped unexpectedly".to_string()));
//                     done = true;
//                 }
//             }
//         }
//         if done {
//             let (success, message) = task.finished.clone().expect("just set");
//             let kind = task.kind;
//             let verb = task.kind.verb();
//             let label = task.label.clone();
//             self.handle_task_finished(kind, success, message, verb, label, cx);
//         }
//         done
//     }
//
//     fn handle_task_finished(
//         &mut self,
//         kind: TaskKind,
//         success: bool,
//         message: String,
//         verb: &str,
//         label: String,
//         cx: &mut Context<Self>,
//     ) {
//         if message == "wrong password" {
//             // Re-run the same job once the user supplies a password.
//             let job = self.task.as_ref().map(|task| task.job.clone());
//             if let Some(job) = job {
//                 self.task = None;
//                 self.pending_op = Some(PendingOp::Job(job));
//                 self.open_password_dialog(cx);
//             }
//             return;
//         }
//         if success {
//             self.toast_titled(
//                 "Done",
//                 format!("{verb} finished successfully"),
//                 ColorName::Green,
//                 cx,
//             );
//             if kind == TaskKind::Update {
//                 self.reload_session(cx);
//             }
//         } else if message == "operation cancelled" {
//             self.toast_titled("Cancelled", label, ColorName::Yellow, cx);
//         } else {
//             self.toast_titled("Error", message, ColorName::Red, cx);
//         }
//     }
//
//     pub(super) fn cancel_task(&mut self) {
//         if let Some(task) = &mut self.task {
//             task.cancel.store(true, Ordering::Relaxed);
//             if let Some((_, reply)) = task.overwrite_query.take() {
//                 let _ = reply.send(false);
//             }
//         }
//     }
//
//     pub(super) fn answer_overwrite(&mut self, overwrite: bool) {
//         if let Some(task) = &mut self.task
//             && let Some((_, reply)) = task.overwrite_query.take()
//         {
//             let _ = reply.send(overwrite);
//         }
//     }
//
//     pub(super) fn close_task(&mut self) {
//         self.task = None;
//     }
// }
//
// /// A label and kind for a job (used for password retries).
// pub(super) fn task_meta(job: &JobSpec) -> (TaskKind, String, Option<PathBuf>) {
//     match job {
//         JobSpec::Extract { archive, .. } => (
//             TaskKind::Extract,
//             format!("Extracting {}", archive.display()),
//             Some(archive.clone()),
//         ),
//         JobSpec::Compress { target, .. } => (
//             TaskKind::Compress,
//             format!("Creating {}", target.display()),
//             None,
//         ),
//         JobSpec::Test { archive, .. } => (
//             TaskKind::Test,
//             format!("Testing {}", archive.display()),
//             Some(archive.clone()),
//         ),
//         JobSpec::Add { archive, .. } => (
//             TaskKind::Update,
//             format!("Updating {}", archive.display()),
//             Some(archive.clone()),
//         ),
//         JobSpec::Delete { archive, .. } => (
//             TaskKind::Update,
//             format!("Deleting from {}", archive.display()),
//             Some(archive.clone()),
//         ),
//         JobSpec::Rename { archive, .. } => (
//             TaskKind::Update,
//             format!("Renaming in {}", archive.display()),
//             Some(archive.clone()),
//         ),
//         JobSpec::NewFolder { archive, .. } => (
//             TaskKind::Update,
//             format!("Updating {}", archive.display()),
//             Some(archive.clone()),
//         ),
//         JobSpec::Checksum { path, .. } => {
//             (TaskKind::Test, format!("Checksum {}", path.display()), None)
//         }
//     }
// }
