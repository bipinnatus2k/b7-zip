//! The WinRAR-style progress page: total progress bar, elapsed / remaining
//! time, throughput, the file being processed, and Pause / Cancel controls.
//!
//! One panel hosts one job (the HeavyJob pattern): the render reads only the
//! status fields, the engine runs on the task crate's own thread, and the
//! poll task just moves bytes into those fields. The pause flag parks the
//! engine inside its progress callback; cancel aborts it there.

use crate::globals;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, hsla, px, App, AppContext, Context, Div, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString, Styled, Task, Window,
};
use gpui_kit::base::dock::PanelEvent;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dock::{BasePanel, Panel};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{ActiveTheme, Disableable as _, Icon, IconName, Sizable};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc::{Receiver, SyncSender}};
use std::time::Instant;
use task::TaskEvent;

/// Poll cadence for the engine's event channel.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);

pub struct ProgressPanel {
    focus_handle: FocusHandle,
    title: SharedString,
    finished: Option<(bool, String)>,
    processed: u64,
    total: Option<u64>,
    current_file: Option<String>,
    paused: bool,
    /// A pending overwrite decision. The engine thread is blocked on the
    /// reply channel until the user answers, so this must always be shown
    /// and always be answered (Cancel answers `skip`).
    conflict: Option<(String, SyncSender<bool>)>,
    started_at: Instant,
    pause: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    rx: Option<Receiver<TaskEvent>>,
    /// Host-side hook (workspace refresh etc.), invoked once on completion.
    on_finished: Option<Box<dyn Fn(&mut App, bool, &str) + 'static>>,
    _poll: Option<Task<()>>,
}

/// Emitted once when the job lands in a terminal state.
#[derive(Debug, Clone)]
pub enum ProgressEvent {
    Finished { success: bool, message: String },
}

impl EventEmitter<ProgressEvent> for ProgressPanel {}

impl EventEmitter<PanelEvent> for ProgressPanel {}

impl Focusable for ProgressPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for ProgressPanel {
    fn panel_name(&self) -> &'static str {
        "progress"
    }

    /// A finished progress page is just a result card; it closes, a running
    /// one must not.
    fn closable(&self, _: &App) -> bool {
        self.finished.is_some()
    }

    fn on_removed(&mut self, _: &mut Window, _: &mut Context<Self>) {
        // Closing the page mid-run cancels the job with it.
        self.cancel.store(true, Ordering::Relaxed);
        self.pause.store(false, Ordering::Relaxed);
    }
}

impl Panel for ProgressPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let label = if self.paused {
            format!("{} (paused)", self.title)
        } else {
            self.title.to_string()
        };
        div()
            .flex()
            .items_center()
            .gap_1p5()
            .child(Icon::new(IconName::Loader).xsmall())
            .child(label)
    }
}

impl ProgressPanel {
    pub fn new(
        title: impl Into<SharedString>,
        rx: Receiver<TaskEvent>,
        cancel: Arc<AtomicBool>,
        pause: Arc<AtomicBool>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            title: title.into(),
            finished: None,
            processed: 0,
            total: None,
            current_file: None,
            paused: false,
            conflict: None,
            started_at: Instant::now(),
            pause,
            cancel,
            rx: Some(rx),
            on_finished: None,
            _poll: None,
        };
        this.start_poll(cx);
        this
    }

    /// Registers the completion hook, called once with (success, message).
    pub fn set_on_finished(
        &mut self,
        cb: impl Fn(&mut App, bool, &str) + 'static,
    ) {
        self.on_finished = Some(Box::new(cb));
    }

    fn start_poll(&mut self, cx: &mut Context<Self>) {
        self._poll = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(POLL_INTERVAL).await;
            let alive = this.update(cx, |this, cx| this.poll(cx));
            if alive.is_err() {
                break;
            }
        }));
    }

    /// Drains pending events; returns false once the panel is gone.
    fn poll(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(rx) = &self.rx else { return false };
        loop {
            match rx.try_recv() {
                Ok(TaskEvent::Progress { processed, total }) => {
                    self.processed = processed;
                    if total > 0 {
                        self.total = Some(total);
                    }
                }
                Ok(TaskEvent::FileStarted { path }) => {
                    self.current_file = Some(path);
                }
                Ok(TaskEvent::OverwriteConflict { path, reply }) => {
                    // The engine thread is parked on this decision; surface
                    // the question until the user answers.
                    self.conflict = Some((path, reply));
                    cx.notify();
                }
                Ok(TaskEvent::Finished { success, message }) => {
                    self.finished = Some((success, message.clone()));
                    self.paused = false;
                    self.pause.store(false, Ordering::Relaxed);
                    self.rx = None;
                    self._poll = None;
                    cx.emit(ProgressEvent::Finished { success, message: message.clone() });
                    if let Some(on_finished) = self.on_finished.take() {
                        on_finished(cx, success, &message);
                    }
                    cx.notify();
                    return true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // Mirror the pause flag for the render.
                    let paused = self.pause.load(Ordering::Relaxed);
                    if paused != self.paused {
                        self.paused = paused;
                        cx.notify();
                    }
                    return true;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.finished =
                        Some((false, "task worker stopped unexpectedly".into()));
                    self.rx = None;
                    self._poll = None;
                    cx.notify();
                    return true;
                }
            }
        }
    }

    fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        if self.finished.is_some() || self.conflict.is_some() {
            return;
        }
        let next = !self.pause.load(Ordering::Relaxed);
        self.pause.store(next, Ordering::Relaxed);
        self.paused = next;
        cx.notify();
    }

    fn cancel_job(&mut self, cx: &mut Context<Self>) {
        self.cancel.store(true, Ordering::Relaxed);
        // Un-park the engine so it can observe the cancel, and answer any
        // pending overwrite question (skip) or the worker would wait forever.
        self.pause.store(false, Ordering::Relaxed);
        if let Some((_, reply)) = self.conflict.take() {
            let _ = reply.send(false);
        }
        self.paused = false;
        cx.notify();
    }

    fn answer_conflict(&mut self, overwrite: bool, cx: &mut Context<Self>) {
        if let Some((_, reply)) = self.conflict.take() {
            let _ = reply.send(overwrite);
        }
        cx.notify();
    }
}

fn format_duration(secs: u64) -> String {
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let secs = secs % 60;
    if hours > 0 {
        format!("{hours}:{mins:02}:{secs:02}")
    } else {
        format!("{mins}:{secs:02}")
    }
}

impl Render for ProgressPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (fg, muted, border, success_color, warning, danger) = {
            let t = cx.theme();
            (
                t.foreground,
                t.muted_foreground,
                t.border,
                t.success,
                t.warning,
                t.danger,
            )
        };

        let close_button = || {
            Button::new("close-progress")
                .label("Close")
                .primary()
                .xsmall()
                .on_click(cx.listener(|this, _, window, cx| {
                    // App-level defer: removing this panel synchronously (or
                    // through Context::defer_in, which wraps view.update)
                    // re-enters the borrow when the dock calls on_removed.
                    let entity = cx.entity();
                    let wh = window.window_handle();
                    cx.defer(move |cx| {
                        let _ = wh.update(cx, |_, window, cx| {
                            globals::close_center_panel(cx, entity, window);
                        });
                    });
                }))
        };

        if let Some((success, message)) = &self.finished {
            let cancelled = message == "operation cancelled";
            let (headline, accent): (SharedString, gpui::Hsla) = if *success {
                ("Completed successfully".into(), success_color)
            } else if cancelled {
                ("Cancelled".into(), warning)
            } else {
                ("Failed".into(), danger)
            };
            return div()
                .id("progress-panel")
                .track_focus(&self.focus_handle)
                .size_full()
                .flex()
                .flex_col()
                .text_color(fg)
                .bg(cx.theme().background)
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_3()
                                .p_6()
                                .min_w(px(320.0))
                                .child(
                                    div()
                                        .text_base()
                                        .text_color(accent)
                                        .child(headline),
                                )
                                .child(
                                    div().text_sm().text_color(muted).child(
                                        message.clone(),
                                    ),
                                )
                                .child(div().flex().justify_end().child(close_button())),
                        ),
                );
        }

        // An overwrite question blocks the engine thread until answered, so
        // it takes over the whole page when present.
        if let Some((path, _)) = &self.conflict {
            return div()
                .id("progress-panel")
                .track_focus(&self.focus_handle)
                .size_full()
                .flex()
                .flex_col()
                .text_color(fg)
                .bg(cx.theme().background)
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_3()
                                .p_6()
                                .max_w(px(520.0))
                                .child(
                                    div()
                                        .text_base()
                                        .text_color(warning)
                                        .child("File already exists"),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(muted)
                                        .truncate()
                                        .child(SharedString::from(path.clone())),
                                )
                                .child(
                                    div().text_xs().text_color(muted).child(
                                        "Overwrite it with the archive copy, or skip this file?",
                                    ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .justify_end()
                                        .gap_2()
                                        .child(
                                            Button::new("conflict-skip")
                                                .label("Skip")
                                                .ghost()
                                                .xsmall()
                                                .on_click(cx.listener(
                                                    |this, _, _, cx| {
                                                        this.answer_conflict(false, cx)
                                                    },
                                                )),
                                        )
                                        .child(
                                            Button::new("conflict-overwrite")
                                                .label("Overwrite")
                                                .primary()
                                                .xsmall()
                                                .on_click(cx.listener(
                                                    |this, _, _, cx| {
                                                        this.answer_conflict(true, cx)
                                                    },
                                                )),
                                        ),
                                ),
                        ),
                );
        }

        let percent = self
            .total
            .filter(|total| *total > 0)
            .map(|total| (self.processed as f64 * 100.0 / total as f64).clamp(0.0, 100.0) as f32);
        let elapsed = self.started_at.elapsed().as_secs();
        let speed = if elapsed > 0 {
            self.processed / elapsed
        } else {
            0
        };
        let remaining = match (self.total, speed) {
            (Some(total), speed) if speed > 0 && total > self.processed => {
                Some((total - self.processed) / speed)
            }
            _ => None,
        };

        let stats = |label: &'static str, value: String| {
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_xs().text_color(muted).child(label))
                .child(div().text_sm().child(value))
        };

        div()
            .id("progress-panel")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .text_color(fg)
            .bg(cx.theme().background)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .p(px(24.0))
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(if self.paused {
                                "Paused".to_string()
                            } else {
                                "Working…".to_string()
                            }),
                    )
                    .child(match percent {
                        Some(percent) => Progress::new("job-bar").value(percent).into_any_element(),
                        None => Progress::new("job-bar").loading(true).into_any_element(),
                    })
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_8()
                            .child(stats(
                                "Processed",
                                format!(
                                    "{} / {}",
                                    ui::util::format_size(self.processed),
                                    self.total
                                        .map(ui::util::format_size)
                                        .unwrap_or_else(|| "?".into())
                                ),
                            ))
                            .child(stats("Elapsed", format_duration(elapsed)))
                            .child(stats(
                                "Remaining",
                                remaining
                                    .map(format_duration)
                                    .unwrap_or_else(|| "--".into()),
                            ))
                            .child(stats("Speed", format!("{}/s", ui::util::format_size(speed)))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_xs().text_color(muted).child("Current file"))
                            .child(
                                div()
                                    .text_sm()
                                    .truncate()
                                    .child(SharedString::from(
                                        self.current_file.clone().unwrap_or_default(),
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("pause-job")
                                    .label(if self.paused { "Resume" } else { "Pause" })
                                    .ghost()
                                    .xsmall()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_pause(cx)
                                    })),
                            )
                            .child(
                                Button::new("background-job")
                                    .label("Background")
                                    .ghost()
                                    .xsmall()
                                    .on_click(|_, window, _| window.minimize_window()),
                            )
                            .child(
                                Button::new("cancel-job")
                                    .label("Cancel")
                                    .danger()
                                    .xsmall()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.cancel_job(cx)
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .px_2()
                    .h(px(24.0))
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(border)
                    .text_xs()
                    .text_color(muted)
                    .child(if self.paused {
                        "The operation is paused; the archive is not being modified"
                    } else {
                        "The operation keeps running while you work in other tabs"
                    }),
            )
    }
}
