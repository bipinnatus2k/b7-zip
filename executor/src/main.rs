//! Bit7z executor: a standalone GUI process that runs a job file
//! (extract/compress/test/...) with progress, cancellation, and a result
//! summary. It is launched by the shell integration, the CLI, or the
//! manager with a job file path as its single argument.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bit7z_rs::{ArchiveEngine, Bit7zEngine};
use clap::Parser;
use gpui::{
    App, AppContext, Context, IntoElement, ParentElement, Render, SharedString, Styled, Window,
    div, px,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc::Receiver};
use task::{JobFile, TaskEvent, TaskRunner};
use ui_kit::{Button, ButtonStyle, ProgressBar, StatusBar};

/// Command-line arguments.
#[derive(Parser, Debug)]
#[command(name = "bit7z-executor", max_term_width = 100)]
struct Args {
    /// Job file to execute.
    job: PathBuf,
    /// Optional password (never written to job files).
    #[arg(long)]
    password: Option<String>,
}

/// Executor state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ExecutorState {
    Running,
    Finished { success: bool, message: String },
}

/// The executor window view.
pub struct ExecutorApp {
    job: JobFile,
    state: ExecutorState,
    progress: Option<(u64, u64)>,
    current_file: Option<String>,
    cancel: Arc<AtomicBool>,
    _rx: Option<Receiver<TaskEvent>>,
}

impl ExecutorApp {
    pub fn new(job_path: PathBuf, password: Option<String>, cx: &mut Context<Self>) -> Self {
        let json = match std::fs::read_to_string(&job_path) {
            Ok(json) => json,
            Err(error) => {
                return Self::failed(format!("cannot read job file {}: {error}", job_path.display()));
            }
        };
        let job = match JobFile::from_json(&json) {
            Ok(job) => job,
            Err(error) => {
                return Self::failed(format!("invalid job file: {error}"));
            }
        };

        let engine: Arc<dyn ArchiveEngine> = match Bit7zEngine::new(find_dll().as_deref()) {
            Ok(engine) => Arc::new(engine),
            Err(error) => return Self::failed(format!("engine load failed: {error}")),
        };
        let runner = TaskRunner::new(engine);
        let cancel = Arc::new(AtomicBool::new(false));
        let pw = password.as_deref().map(password::Password::new);
        eprintln!("executor: starting job {}", job.id);
        let rx = runner.run(job.spec.clone(), pw.as_ref());
        eprintln!("executor: task spawned");

        let mut app = Self {
            job,
            state: ExecutorState::Running,
            progress: None,
            current_file: None,
            cancel,
            _rx: Some(rx),
        };
        // Poll the task event stream on the foreground executor.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(50)).await;
                let done = cx.update(|cx| {
                    let Some(app) = this.upgrade() else {
                        return true;
                    };
                    app.update(cx, |app, cx| app.poll(cx))
                });
                if done {
                    break;
                }
            }
        })
        .detach();
        app
    }

    fn failed(message: String) -> Self {
        Self {
            job: JobFile::new("failed", task::JobSpec::Test {
                archive: PathBuf::from("."),
                password_hint: false,
            }),
            state: ExecutorState::Finished { success: false, message },
            progress: None,
            current_file: None,
            cancel: Arc::new(AtomicBool::new(false)),
            _rx: None,
        }
    }

    /// Drain pending task events; returns true when the app should stop
    /// polling (finished).
    fn poll(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(rx) = &self._rx else {
            return true;
        };
        let mut done = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                TaskEvent::Progress { processed, total } => {
                    self.progress = Some((processed, total));
                }
                TaskEvent::FileStarted { path } => {
                    self.current_file = Some(path);
                }
                TaskEvent::Finished { success, message } => {
                    eprintln!("executor: finished success={success} message={message}");
                    self.state = ExecutorState::Finished { success, message };
                    done = true;
                }
            }
        }
        cx.notify();
        done
    }

    fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Render for ExecutorApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = match &self.job.spec {
            task::JobSpec::Extract { archive, .. } => format!("Extracting {}", archive.display()),
            task::JobSpec::Compress { target, .. } => format!("Compressing to {}", target.display()),
            task::JobSpec::Test { archive, .. } => format!("Testing {}", archive.display()),
            other => format!("Running {other:?}"),
        };

        let progress = self.progress;
        let current_file = self.current_file.clone();
        let state = self.state.clone();
        let weak = cx.weak_entity();

        let mut body = div()
            .flex_col()
            .gap(px(16.0))
            .p(px(24.0))
            .bg(ui_kit::tokens::BACKGROUND)
            .child(div().text_size(px(16.0)).text_color(ui_kit::tokens::TEXT_PRIMARY).child(title));

        match state {
            ExecutorState::Running => {
                let (processed, total) = progress.unwrap_or((0, 0));
                let fraction = if total > 0 {
                    processed as f32 / total as f32
                } else {
                    0.0
                };
                let bar = if total > 0 {
                    ProgressBar::new().progress(fraction)
                } else {
                    ProgressBar::new().indeterminate()
                };
                body = body
                    .child(div().w(px(420.0)).child(bar))
                    .child(
                        div()
                            .text_color(ui_kit::tokens::TEXT_SECONDARY)
                            .child(
                                current_file.unwrap_or_else(|| "Working...".into()),
                            ),
                    )
                    .child(
                        div().flex_row().justify_end().gap(px(8.0)).child(
                            Button::new("cancel", "Cancel")
                                .style(ButtonStyle::Danger)
                                .on_click({
                                    let weak = weak.clone();
                                    move |cx| {
                                        if let Some(app) = weak.upgrade() {
                                            app.update(cx, |app, _| app.cancel());
                                        }
                                    }
                                }),
                        ),
                    );
            }
            ExecutorState::Finished { success, message } => {
                let color = if success {
                    ui_kit::tokens::SUCCESS
                } else {
                    ui_kit::tokens::DANGER
                };
                body = body
                    .child(
                        div()
                            .text_color(color)
                            .text_size(px(14.0))
                            .child(if success { "Completed successfully" } else { "Failed" }),
                    )
                    .child(
                        div()
                            .text_color(ui_kit::tokens::TEXT_SECONDARY)
                            .child(message),
                    )
                    .child(
                        div().flex_row().justify_end().gap(px(8.0)).child(
                            Button::new("exit", "Exit")
                                .style(ButtonStyle::Primary)
                                .on_click(|cx| cx.quit()),
                        ),
                    );
            }
        }

        body.child(
            StatusBar::new()
                .left(format!("job: {}", self.job.id))
                .right("bit7z-executor"),
        )
    }
}

/// Locate the 7-Zip DLL: next to the executable, then VCPKG_ROOT.
fn find_dll() -> Option<String> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    for name in ["7z.dll", "7zip.dll"] {
        let candidate = exe_dir.join(name);
        if candidate.exists() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    if let Ok(vcpkg) = std::env::var("VCPKG_ROOT") {
        for name in ["7zip.dll", "7z.dll"] {
            let candidate = PathBuf::from(&vcpkg)
                .join("installed/x64-windows/bin")
                .join(name);
            if candidate.exists() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }
    None
}

fn main() {
    let args = Args::parse();
    let platform = gpui_platform::current_platform(false);
    let app = gpui::Application::new_inaccessible(platform);

    app.run(move |cx| {
        let job = args.job.clone();
        let password = args.password.clone();
        cx.open_window(gpui::WindowOptions::default(), |_window, cx| {
            cx.new(|cx| ExecutorApp::new(job, password, cx))
        })
        .expect("failed to open executor window");
    });
}
