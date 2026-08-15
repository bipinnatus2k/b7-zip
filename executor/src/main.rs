//! Bit7z executor: a standalone GUI process that runs a job file
//! (extract/compress/test/...) with live progress, cancellation, and a
//! result summary. It is launched by the shell integration, the CLI, or the
//! manager with a job file path as its single argument.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bit7z_rs::{ArchiveEngine, Bit7zEngine};
use clap::Parser;
use gpui::{AppContext, Context, IntoElement, ParentElement, Render, SharedString, Styled, Window, div, px};
use guise::prelude::*;
use guise::theme::Theme;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc::Receiver};
use task::{JobFile, JobSpec, TaskEvent, TaskRunner};

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
    rx: Option<Receiver<TaskEvent>>,
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

        let engine: Arc<dyn ArchiveEngine> = match Bit7zEngine::new(bit7z_rs::locate_dll().as_deref()) {
            Ok(engine) => Arc::new(engine),
            Err(error) => return Self::failed(format!("engine load failed: {error}")),
        };
        let runner = TaskRunner::new(engine);
        let cancel = Arc::new(AtomicBool::new(false));
        let pw = password.as_deref().map(password::Password::new);
        eprintln!("executor: starting job {}", job.id);
        let rx = runner.run_with_cancel(job.spec.clone(), pw.as_ref(), cancel.clone());

        let app = Self {
            job,
            state: ExecutorState::Running,
            progress: None,
            current_file: None,
            cancel,
            rx: Some(rx),
        };
        // Poll the task event stream on the foreground executor.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(50))
                    .await;
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
            job: JobFile::new(
                "failed",
                JobSpec::Test { archive: PathBuf::from("."), password_hint: false },
            ),
            state: ExecutorState::Finished { success: false, message },
            progress: None,
            current_file: None,
            cancel: Arc::new(AtomicBool::new(false)),
            rx: None,
        }
    }

    /// Drain pending task events; returns true when the app should stop
    /// polling (finished).
    fn poll(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(rx) = &self.rx else { return true };
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

fn spec_title(spec: &JobSpec) -> SharedString {
    let title = match spec {
        JobSpec::Extract { archive, .. } => format!("Extracting {}", archive.display()),
        JobSpec::Compress { target, .. } => format!("Compressing to {}", target.display()),
        JobSpec::Test { archive, .. } => format!("Testing {}", archive.display()),
        JobSpec::Add { archive, .. } => format!("Updating {}", archive.display()),
        JobSpec::Delete { archive, .. } => format!("Deleting from {}", archive.display()),
        JobSpec::Rename { archive, .. } => format!("Renaming in {}", archive.display()),
        JobSpec::NewFolder { archive, .. } => format!("Updating {}", archive.display()),
        JobSpec::Checksum { path, .. } => format!("Checksum {}", path.display()),
    };
    title.into()
}

impl Render for ExecutorApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.global::<Theme>();
        let title = spec_title(&self.job.spec);
        let (processed, total) = self.progress.unwrap_or((0, 0));
        let percent = if total > 0 {
            (processed as f64 * 100.0 / total as f64).clamp(0.0, 100.0) as f32
        } else {
            0.0
        };
        let current_file = self.current_file.clone().unwrap_or_default();

        let body = match &self.state {
            ExecutorState::Running => {
                let bar = if total > 0 {
                    Progress::new(percent)
                } else {
                    Progress::new(100.0).color(ColorName::Gray)
                };
                div()
                    .flex()
                    .flex_col()
                    .gap(px(14.0))
                    .child(bar)
                    .child(div().flex().justify_between()
                        .child(Text::new(format!("{} / {}", bit7z_explorer::format_size(processed), bit7z_explorer::format_size(total))).size(Size::Xs).dimmed())
                        .child(Text::new(format!("{percent:.0}%")).size(Size::Xs).dimmed()))
                    .child(Text::new(if current_file.is_empty() { "Working…" } else { &current_file }).size(Size::Sm).dimmed())
                    .child(div().flex().justify_end().child(Button::new("cancel", "Cancel").size(Size::Xs).color(ColorName::Red).variant(Variant::Light).on_click(cx.listener(|this, _, _, _| this.cancel()))))
            }
            ExecutorState::Finished { success, message } => {
                let cancelled = message == "operation cancelled";
                let color = if *success { ColorName::Green } else if cancelled { ColorName::Yellow } else { ColorName::Red };
                let headline = if *success {
                    "Completed successfully"
                } else if cancelled {
                    "Cancelled"
                } else {
                    "Failed"
                };
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(Alert::new(headline).color(color).variant(Variant::Light))
                    .child(Text::new(message.clone()).size(Size::Sm).dimmed())
                    .child(div().flex().justify_end().child(Button::new("exit", "Exit").size(Size::Xs).on_click(|_, _, cx| cx.quit())))
            }
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.body().hsla())
            .text_color(t.text().hsla())
            .child(div().p(px(20.0)).flex().flex_col().gap(px(6.0)).child(Text::new(title).size(Size::Md).bold()).child(body))
            .child(
                StatusBar::new()
                    .left(Text::new(format!("job: {}", self.job.id)).size(Size::Xs))
                    .right(Text::new("bit7z-executor").size(Size::Xs).dimmed()),
            )
    }
}

fn main() {
    let args = Args::parse();
    let platform = gpui_platform::current_platform(false);
    let app = gpui::Application::new_inaccessible(platform);

    app.run(move |cx| {
        Theme::dark().init(cx);
        let job = args.job.clone();
        let password = args.password.clone();
        cx.open_window(
            gpui::WindowOptions {
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some(SharedString::new_static("Bit7z executor")),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_window, cx| cx.new(|cx| ExecutorApp::new(job, password, cx)),
        )
        .expect("failed to open executor window");
    });
}
