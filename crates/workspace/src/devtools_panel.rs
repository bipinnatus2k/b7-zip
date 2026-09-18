//! Developer tools panel (debug builds): application/session status, the
//! telemetry toggle, and quick actions. Registered only when
//! `debug_assertions` is on; release builds never surface it.

use crate::globals;
use gpui::{div, px, App, AppContext, Context, Div, EventEmitter, FocusHandle, Focusable,
           InteractiveElement, IntoElement, ParentElement, Render, SharedString,
           StatefulInteractiveElement, Styled, UniformListScrollHandle, Window};
use gpui_kit::base::dock::PanelEvent;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dock::{BasePanel, Panel};
use gpui_kit::component::{ActiveTheme, Disableable as _, Icon, IconName, Sizable};
use settings::SettingsStore;

pub struct DevToolsPanel {
    focus_handle: FocusHandle,
}

impl EventEmitter<PanelEvent> for DevToolsPanel {}

impl Focusable for DevToolsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for DevToolsPanel {
    fn panel_name(&self) -> &'static str {
        "devtools"
    }
}

impl Panel for DevToolsPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_1p5()
            .child(Icon::new(IconName::SquareTerminal).xsmall())
            .child("DevTools")
    }
}

impl DevToolsPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }
}

pub(crate) fn info_row(label: &'static str, value: String, cx: &App) -> Div {
    div()
        .flex()
        .flex_row()
        .gap_3()
        .child(
            div()
                .w(px(160.0))
                .flex_shrink_0()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(div().text_xs().child(value))
}

impl Render for DevToolsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border, success, warning) =
            (theme.muted_foreground, theme.border, theme.success, theme.warning);
        let engine = globals::engine(cx).is_some();
        let session_count = globals::sessions(cx).all().len();
        let telemetry = settings::global(cx).telemetry_enabled;
        let dsn = settings::global(cx)
            .telemetry_dsn
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty();
        let version = env!("CARGO_PKG_VERSION");
        let profile = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };

        let mut panel = div()
            .id("devtools-panel")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .text_color(cx.theme().foreground)
            .bg(cx.theme().background)
            .overflow_y_scroll()
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("APPLICATION"),
            )
            .child(info_row("Version", format!("{version} ({profile})"), cx))
            .child(info_row(
                "7-Zip engine",
                if engine {
                    "loaded".into()
                } else {
                    "NOT loaded".into()
                },
                cx,
            ))
            .child(info_row("Open sessions", session_count.to_string(), cx))
            .child(div().text_xs().text_color(muted).child("TELEMETRY"))
            .child(info_row(
                "Enabled",
                if telemetry {
                    "yes".into()
                } else {
                    "no (opt-in)".to_string()
                },
                cx,
            ))
            .child(info_row(
                "Sentry DSN",
                if dsn {
                    "not configured".into()
                } else {
                    "configured".into()
                },
                cx,
            ))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        Button::new("toggle-telemetry")
                            .label(if telemetry {
                                "Disable telemetry"
                            } else {
                                "Enable telemetry"
                            })
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(|_, _, _, cx| {
                                settings::update(cx, |settings| {
                                    settings.telemetry_enabled = !settings.telemetry_enabled;
                                });
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("send-crash-now")
                            .label("Send pending reports")
                            .ghost()
                            .xsmall()
                            .disabled(!telemetry || dsn)
                            .on_click(cx.listener(|_, _, _, cx| {
                                let Some(dsn) = settings::global(cx)
                                    .telemetry_dsn
                                    .clone()
                                    .filter(|dsn| !dsn.trim().is_empty())
                                else {
                                    return;
                                };
                                let logs_dir = paths::logs_dir().clone();
                                let version = env!("CARGO_PKG_VERSION").to_string();
                                cx.spawn(async move |_, cx| {
                                    let sent = cx
                                        .background_executor()
                                        .spawn(async move {
                                            crashes::report_pending(
                                                &logs_dir, &dsn, &version, "Dev",
                                            )
                                        })
                                        .await;
                                    if sent > 0 {
                                        log::info!("devtools: uploaded {sent} crash report(s)");
                                    }
                                })
                                .detach();
                            })),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("Crash reports pending upload stay on disk until sent or discarded."),
            );

        let _ = (warning, success);
        panel = panel.child(div().flex_1().min_h_0());
        panel
    }
}
