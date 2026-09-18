//! Bit7z executor: a standalone GUI process that runs a job file
//! (extract/compress/test/...) with live progress, cancellation, and a
//! result summary. It is launched by the shell integration, the CLI, or the
//! manager with a job file path as its single argument.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bit7z_rs::{ArchiveEngine, Bit7zEngine};
use clap::Parser;
use gpui::{AppContext, Context, Entity, FontWeight, IntoElement, ParentElement, Render, SharedString, Styled, WeakEntity, Window, div, px};
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{Disableable, Sizable};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError};
use task::{JobFile, JobSpec, TaskEvent, TaskRunner};
use ui::util::format_size;

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
    overwrite_query: Option<(String, SyncSender<bool>)>,
    engine: Option<Arc<dyn ArchiveEngine>>,
    self_entity: WeakEntity<Self>,
    password_state: Entity<InputState>,
    password_required: bool,
}

impl ExecutorApp {
    pub fn new(job_path: PathBuf, password: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let json = match std::fs::read_to_string(&job_path) {
            Ok(json) => json,
            Err(error) => {
                return Self::failed(
                    format!("cannot read job file {}: {error}", job_path.display()),
                    window,
                    cx,
                );
            }
        };
        // The executor has read the whole job into memory; delete the file so
        // completed shell/CLI jobs do not accumulate in the temp jobs dir.
        let _ = std::fs::remove_file(&job_path);
        let job = match JobFile::from_json(&json) {
            Ok(job) => job,
            Err(error) => {
                return Self::failed(format!("invalid job file: {error}"), window, cx);
            }
        };

        let engine: Arc<dyn ArchiveEngine> =
            match Bit7zEngine::new(bit7z_rs::locate_dll().as_deref()) {
                Ok(engine) => Arc::new(engine),
                Err(error) => {
                    return Self::failed(format!("engine load failed: {error}"), window, cx)
                }
            };
        let runner = TaskRunner::new(engine.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        let pw = password.as_deref().map(password::Password::new);
        eprintln!("executor: starting job {}", job.id);
        let rx = runner.run_with_cancel(job.spec.clone(), pw.as_ref(), cancel.clone());

        let password_state = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Password")
                .masked(true)
        });
        let app = Self {
            job,
            state: ExecutorState::Running,
            progress: None,
            current_file: None,
            cancel,
            rx: Some(rx),
            overwrite_query: None,
            engine: Some(engine),
            self_entity: cx.weak_entity(),
            password_state,
            password_required: false,
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

    fn failed(message: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let password_state = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Password")
                .masked(true)
        });
        Self {
            job: JobFile::new(
                "failed",
                JobSpec::Test {
                    archive: PathBuf::from("."),
                    password_hint: false,
                },
            ),
            state: ExecutorState::Finished {
                success: false,
                message,
            },
            progress: None,
            current_file: None,
            cancel: Arc::new(AtomicBool::new(false)),
            rx: None,
            overwrite_query: None,
            engine: None,
            self_entity: cx.weak_entity(),
            password_state,
            password_required: false,
        }
    }

    /// Drain pending task events; returns true when the app should stop
    /// polling (finished).
    fn poll(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(rx) = &self.rx else { return true };
        let mut done = false;
        let mut wrong_password = false;
        loop {
            match rx.try_recv() {
                Ok(TaskEvent::Progress { processed, total }) => {
                    self.progress = Some((processed, total));
                }
                Ok(TaskEvent::FileStarted { path }) => {
                    self.current_file = Some(path);
                }
                Ok(TaskEvent::OverwriteConflict { path, reply }) => {
                    self.overwrite_query = Some((path, reply));
                }
                Ok(TaskEvent::Finished { success, message }) => {
                    eprintln!("executor: finished success={success} message={message}");
                    if !success && message == "wrong password" {
                        self.password_required = true;
                        self.state = ExecutorState::Running;
                        self.overwrite_query = None;
                        wrong_password = true;
                    } else {
                        self.state = ExecutorState::Finished { success, message };
                    }
                    done = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    eprintln!("executor: task channel disconnected unexpectedly");
                    self.state = ExecutorState::Finished {
                        success: false,
                        message: "task worker stopped unexpectedly".into(),
                    };
                    done = true;
                }
            }
        }
        if wrong_password {
            self.rx = None;
        }
        cx.notify();
        done
    }

    fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some((_, reply)) = self.overwrite_query.take() {
            let _ = reply.send(false);
        }
    }

    fn answer_overwrite(&mut self, overwrite: bool) {
        if let Some((_, reply)) = self.overwrite_query.take() {
            let _ = reply.send(overwrite);
        }
    }

    fn submit_password(&mut self, cx: &mut Context<Self>) {
        let password = self.password_state.read(cx).value().to_string();
        if password.is_empty() {
            return;
        }
        self.password_required = false;
        self.start_job(Some(password), cx);
    }

    fn cancel_password(&mut self, cx: &mut Context<Self>) {
        self.password_required = false;
        self.state = ExecutorState::Finished {
            success: false,
            message: "operation cancelled".into(),
        };
        cx.notify();
    }

    fn start_job(&mut self, password: Option<String>, cx: &mut Context<Self>) {
        let Some(engine) = self.engine.clone() else {
            self.state = ExecutorState::Finished {
                success: false,
                message: "7-Zip engine unavailable".into(),
            };
            cx.notify();
            return;
        };
        let runner = TaskRunner::new(engine.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        let pw = password.as_deref().map(password::Password::new);
        self.cancel = cancel.clone();
        self.progress = None;
        self.current_file = None;
        self.overwrite_query = None;
        self.state = ExecutorState::Running;
        self.rx = Some(runner.run_with_cancel(self.job.spec.clone(), pw.as_ref(), cancel));
        let weak = self.self_entity.clone();
        cx.spawn(async move |_this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(50))
                    .await;
                let done = weak.update(cx, |app, cx| app.poll(cx)).unwrap_or(true);
                if done {
                    break;
                }
            }
        })
        .detach();
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
        let (background, foreground, muted) = {
            let theme = cx.theme();
            (theme.background, theme.foreground, theme.muted_foreground)
        };
        let title = spec_title(&self.job.spec);
        let (processed, total) = self.progress.unwrap_or((0, 0));
        let percent = if total > 0 {
            (processed as f64 * 100.0 / total as f64).clamp(0.0, 100.0) as f32
        } else {
            0.0
        };
        let current_file = self.current_file.clone().unwrap_or_default();
        let conflict_path = self.overwrite_query.as_ref().map(|(path, _)| path.clone());
        let supports_cancel = matches!(
            &self.job.spec,
            JobSpec::Extract { .. } | JobSpec::Compress { .. }
        );

        let body = if self.password_required {
            div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(Input::new(&self.password_state))
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(8.0))
                        .child(
                            Button::new("pw-cancel")
                                .label("Cancel")
                                .ghost()
                                .xsmall()
                                .on_click(cx.listener(|this, _, _, cx| this.cancel_password(cx))),
                        )
                        .child(
                            Button::new("pw-retry")
                                .label("Retry")
                                .primary()
                                .xsmall()
                                .on_click(cx.listener(|this, _, _, cx| this.submit_password(cx))),
                        ),
                )
                .into_any_element()
        } else {
            (match &self.state {
                ExecutorState::Running if conflict_path.is_some() => {
                    let path = conflict_path.clone().unwrap_or_default();
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.0))
                        .child(div().text_sm().child(format!("{path} already exists.")))
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap(px(8.0))
                                .child(
                                    Button::new("overwrite")
                                        .label("Overwrite")
                                        .primary()
                                        .xsmall()
                                        .on_click(
                                            cx.listener(|this, _, _, _| {
                                                this.answer_overwrite(true)
                                            }),
                                        ),
                                )
                                .child(
                                    Button::new("skip")
                                        .label("Skip")
                                        .ghost()
                                        .xsmall()
                                        .on_click(cx.listener(|this, _, _, _| {
                                            this.answer_overwrite(false)
                                        })),
                                ),
                        )
                }
                ExecutorState::Running => {
                    let bar = if total > 0 {
                        Progress::new("job-progress").value(percent)
                    } else {
                        Progress::new("job-progress").loading(true)
                    };
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(14.0))
                        .child(bar)
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .text_color(muted)
                                .text_xs()
                                .child(format!("{} / {}", format_size(processed), format_size(total)))
                                .child(div().child(format!("{percent:.0}%"))),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child(if current_file.is_empty() {
                                    "Working…".to_string()
                                } else {
                                    current_file.clone()
                                }),
                        )
                        .child(
                            div().flex().justify_end().child(
                                Button::new("cancel")
                                    .label(if supports_cancel {
                                        "Cancel"
                                    } else {
                                        "Cancel unavailable"
                                    })
                                    .danger()
                                    .xsmall()
                                    .disabled(!supports_cancel)
                                    .on_click(cx.listener(|this, _, _, _| this.cancel())),
                            ),
                        )
                }
                ExecutorState::Finished { success, message } => {
                    let cancelled = message == "operation cancelled";
                    let (headline, accent) = if *success {
                        ("Completed successfully", cx.theme().success)
                    } else if cancelled {
                        ("Cancelled", cx.theme().warning)
                    } else {
                        ("Failed", cx.theme().danger)
                    };
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.0))
                        .child(
                            div()
                                .rounded_sm()
                                .border_1()
                                .border_color(accent)
                                .px_3()
                                .py_2()
                                .text_sm()
                                .text_color(accent)
                                .child(headline),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child(message.clone()),
                        )
                        .child(
                            div().flex().justify_end().child(
                                Button::new("exit")
                                    .label("Exit")
                                    .primary()
                                    .xsmall()
                                    .on_click(|_, _, cx| cx.quit()),
                            ),
                        )
                }
            })
            .into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(background)
            .text_color(foreground)
            .child(
                div()
                    .p(px(20.0))
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(div().text_base().font_weight(FontWeight::BOLD).child(title))
                    .child(body),
            )
            .child(
                StatusBar::new()
                    .left(div().text_xs().child(format!("job: {}", self.job.id)))
                    .right(div().text_xs().text_color(muted).child("bit7z-executor")),
            )
    }
}

fn main() {
    let args = Args::parse();
    let platform = gpui_platform::current_platform(false);
    let app = gpui::Application::new_inaccessible(platform);

    app.run(move |cx| {
        gpui_kit::init(cx);
        gpui_kit::component::theme::Theme::change(
            gpui_kit::component::theme::ThemeMode::Dark,
            None,
            cx,
        );
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
            |window, cx| cx.new(|cx| ExecutorApp::new(job, password, window, cx)),
        )
        .expect("failed to open executor window");
    });
}
