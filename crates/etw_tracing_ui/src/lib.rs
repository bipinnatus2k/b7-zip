use std::io::BufReader;
use std::path::PathBuf;
use gpui::{actions, Global, App, AppContext, DismissEvent, SystemNotification, SystemNotificationAction};
use gpui::private::anyhow;
use gpui::private::anyhow::Context;
use etw_tracing::{launch_etw_recording, EtwSession, StatusMessage, Command, recv_json, send_json, EtwSessionHandle};

actions!(
    app,
    [
        /// Starts recording an ETW (Event Tracing for Windows) trace.
        RecordEtwTrace,
        /// Starts recording an ETW (Event Tracing for Windows) trace with heap tracing.
        RecordEtwTraceWithHeapTracing,
        /// Saves an in-progress ETW trace to disk.
        SaveEtwTrace,
        /// Cancels an in-progress ETW trace without saving.
        CancelEtwTrace,
    ]
);

struct EtwNotification;

struct GlobalEtwSession(Option<EtwSessionHandle>);

impl Global for GlobalEtwSession {}

fn has_active_etw_session(cx: &App) -> bool {
    cx.global::<GlobalEtwSession>().0.is_some()
}

fn show_etw_notification(cx: &mut App, message: impl Into<gpui::SharedString>) {
    let message = message.into();
    cx.show_system_notification(SystemNotification{
        tag: Default::default(),
        title: "Bit7zFM: ETW Tracing".into(),
        body: message,
        actions: vec![],
    })
    // show_app_notification(NotificationId::unique::<EtwNotification>(), cx, move |cx| {
    //     cx.new(|cx| MessageNotification::new(message.clone(), cx))
    // });
}

struct MessageNotification {

}

fn show_etw_notification_with_action(
    cx: &mut App,
    message: impl Into<gpui::SharedString>,
    button_label: impl Into<gpui::SharedString>,
    on_click: impl Fn(&mut gpui::Window, &mut gpui::Context<MessageNotification>)
    + Send
    + Sync
    + 'static,
) {
    let message = message.into();
    let button_label = button_label.into();
    let on_click = std::sync::Arc::new(on_click);
    cx.show_system_notification(
        SystemNotification {
            tag: Default::default(),
            title: "Bit7zFM: ETW Tracing".into(),
            body: message,
            actions: vec![
                SystemNotificationAction {
                    id: Default::default(),
                    label: button_label,
                }
            ],
        }
    )
    // show_app_notification(NotificationId::unique::<EtwNotification>(), cx, move |cx| {
    //     let message = message.clone();
    //     let button_label = button_label.clone();
    //     cx.new(|cx| {
    //         MessageNotification::new(message, cx)
    //             .primary_message(button_label)
    //             .primary_on_click_arc(on_click.clone())
    //     })
    // });
}

fn show_etw_status_notification(cx: &mut App, status: anyhow::Result<StatusMessage>, output_path: PathBuf) {
    match status {
        Ok(StatusMessage::Stopped) => {
            let display_path = output_path.display().to_string();
            show_etw_notification_with_action(
                cx,
                format!("ETW trace saved to {display_path}"),
                "Show in File Manager",
                move |_window, cx| {
                    cx.reveal_path(&output_path);
                    // cx.emit(DismissEvent);
                },
            );
        }
        Ok(StatusMessage::TimedOut) => {
            let display_path = output_path.display().to_string();
            show_etw_notification_with_action(
                cx,
                format!("ETW recording timed out. Trace saved to {display_path}"),
                "Show in File Manager",
                move |_window, cx| {
                    cx.reveal_path(&output_path);
                    // cx.emit(DismissEvent);
                },
            );
        }
        Ok(StatusMessage::Cancelled) => {
            show_etw_notification(cx, "ETW recording cancelled");
        }
        Ok(_) => {
            show_etw_notification(cx, "ETW recording ended unexpectedly");
        }
        Err(error) => {
            show_etw_notification(cx, format!("Failed to complete ETW recording: {error:#}"));
        }
    }
}

pub fn init(cx: &mut App) {
    cx.set_global(GlobalEtwSession(None));

    cx.on_action(|_: &RecordEtwTrace, cx: &mut App| {
        start_etw_recording(cx, None);
    });

    cx.on_action(|_: &RecordEtwTraceWithHeapTracing, cx: &mut App| {
        start_etw_recording(cx, Some(std::process::id()));
    });

    cx.on_action(|_: &SaveEtwTrace, cx: &mut App| {
        let session = cx.global_mut::<GlobalEtwSession>().0.as_mut();
        let Some(session) = session else {
            show_etw_notification(cx, "No active ETW recording to stop");
            return;
        };
        match send_json(&mut session.writer, &Command::Save) {
            Ok(()) => {
                show_etw_notification(cx, "Stopping ETW recording...");
            }
            Err(error) => {
                show_etw_notification(cx, format!("Failed to stop ETW recording: {error:#}"));
            }
        }
    });

    cx.on_action(|_: &CancelEtwTrace, cx: &mut App| {
        let session = cx.global_mut::<GlobalEtwSession>().0.as_mut();
        let Some(session) = session else {
            show_etw_notification(cx, "No active ETW recording to cancel");
            return;
        };
        match send_json(&mut session.writer, &Command::Cancel) {
            Ok(()) => {
                show_etw_notification(cx, "Cancelling ETW recording...");
            }
            Err(error) => {
                show_etw_notification(cx, format!("Failed to cancel ETW recording: {error:#}"));
            }
        }
    });
}

fn start_etw_recording(cx: &mut App, heap_pid: Option<u32>) {
    if has_active_etw_session(cx) {
        show_etw_notification(cx, "ETW recording is already in progress");
        return;
    }
    let save_dialog = cx.prompt_for_new_path(&PathBuf::default(), Some("zed-trace.etl"));
    cx.spawn(async move |cx| {
        let output_path = match save_dialog.await {
            Ok(Ok(Some(path))) => path,
            Ok(Ok(None)) => return,
            Ok(Err(error)) => {
                cx.update(|cx| {
                    show_etw_notification(cx, format!("Failed to pick save location: {error:#}"));
                });
                return;
            }
            Err(_) => return,
        };

        let result = cx
            .background_spawn(async move { launch_etw_recording(heap_pid, &output_path) })
            .await;

        let EtwSession {
            output_path,
            stream,
            listener,
            socket_path,
        } = match result {
            Ok(session) => session,
            Err(error) => {
                cx.update(|cx| {
                    show_etw_notification(cx, format!("Failed to start ETW recording: {error:#}"));
                });
                return;
            }
        };

        let (read_half, write_half) = stream.into_inner().into_split();

        cx.spawn(async |cx| {
            let status = cx
                .background_spawn(async move {
                    recv_json(&mut BufReader::new(read_half))
                        .context("Receive status from subprocess")
                })
                .await;
            cx.update(|cx| {
                cx.global_mut::<GlobalEtwSession>().0 = None;
                show_etw_status_notification(cx, status, output_path);
            });
        })
            .detach();

        cx.update(|cx| {
            cx.global_mut::<GlobalEtwSession>().0 = Some(EtwSessionHandle {
                writer: write_half,
                _listener: listener,
                socket_path,
            });
            show_etw_notification(cx, "ETW recording started");
        });
    })
        .detach();
}

pub use etw_tracing::*;

