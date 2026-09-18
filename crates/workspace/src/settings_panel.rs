//! The settings panel: telemetry opt-in/out, the Sentry DSN, and close
//! confirmation preference. Edits persist immediately to settings.json.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, App, AppContext, Context, Div, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Window,
};
use gpui_kit::base::dock::PanelEvent;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dock::{BasePanel, Panel};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Disableable as _, Icon, IconName, Sizable};
use crate::devtools_panel::info_row;
use settings::SettingsStore;

pub struct SettingsPanel {
    focus_handle: FocusHandle,
    dsn_entry: Option<gpui::Entity<InputState>>,
}

impl EventEmitter<PanelEvent> for SettingsPanel {}

impl Focusable for SettingsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for SettingsPanel {
    fn panel_name(&self) -> &'static str {
        "settings"
    }
}

impl Panel for SettingsPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_1p5()
            .child(Icon::new(IconName::Settings).xsmall())
            .child("Settings")
    }
}

impl SettingsPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let dsn_entry = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("https://<key>@<host>/<project>")
        });
        // Seed the field with the stored DSN (plain text; it is a URL, not a
        // secret beyond the project key).
        let stored = settings::global(cx).telemetry_dsn.clone();
        if let Some(stored) = stored {
            dsn_entry.update(cx, |state, cx| state.set_value(stored, window, cx));
        }
        Self {
            focus_handle: cx.focus_handle(),
            dsn_entry: Some(dsn_entry),
        }
    }
}

fn setting_row(cx: &App, label: &'static str, description: &str) -> Div {
    let description: SharedString = description.to_string().into();
    div()
        .flex()
        .flex_col()
        .gap_0p5()
        .child(div().text_sm().child(label))
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(description),
        )
}

fn toggle_button(
    id: &'static str,
    enabled: bool,
    cx: &Context<SettingsPanel>,
    on_click: impl Fn(&mut SettingsPanel, &mut Context<SettingsPanel>) + 'static,
) -> Button {
    Button::new(id)
        .label(if enabled { "Enabled" } else { "Disabled" })
        .ghost()
        .xsmall()
        .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)))
}

impl Render for SettingsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border, success, warning) =
            (theme.muted_foreground, theme.border, theme.success, theme.warning);
        let telemetry = settings::global(cx).telemetry_enabled;
        let confirm_close = settings::global(cx).confirm_close_with_changes;
        let dsn_configured = settings::global(cx)
            .telemetry_dsn
            .as_deref()
            .is_some_and(|dsn| !dsn.trim().is_empty());
        let settings_file = settings::settings_file();

        let dsn_entry = self.dsn_entry.clone();

        div()
            .id("settings-panel")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .gap_4()
            .p_3()
            .overflow_y_scroll()
            .text_color(cx.theme().foreground)
            .bg(cx.theme().background)
            .child(div().text_xs().text_color(muted).child("TELEMETRY"))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        setting_row(
                            cx,
                            "Crash reporting",
                            "Upload pending crash reports to Sentry on the launch after a crash.",
                        ),
                    )
                    .child(toggle_button(
                        "toggle-telemetry",
                        telemetry,
                        cx,
                        |this, cx| {
                            settings::update(cx, |settings| {
                                settings.telemetry_enabled = !settings.telemetry_enabled;
                            });
                            cx.notify();
                        },
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(
                        setting_row(
                            cx,
                            "Sentry DSN",
                            "Where crash reports go. Empty disables uploads even when reporting is enabled.",
                        ),
                    )
                    .when_some(dsn_entry.clone(), |row, entry| {
                        row.child(div().w(px(420.0)).child(Input::new(&entry)))
                    })
                    .child(
                        div().flex().flex_row().items_center().gap_2().child(
                            Button::new("save-dsn")
                                .label("Save DSN")
                                .ghost()
                                .xsmall()
                                .when_some(dsn_entry.clone(), |btn, entry| {
                                    btn.on_click(cx.listener(move |this, _, _, cx| {
                                        let value = entry.read(cx).value().trim().to_string();
                                        let value =
                                            if value.is_empty() { None } else { Some(value) };
                                        settings::update(cx, |settings| {
                                            settings.telemetry_dsn = value;
                                        });
                                        cx.notify();
                                    }))
                                }),
                        ),
                    ),
            )
            .child(div().text_xs().text_color(muted).child("WORKSPACE"))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(setting_row(
                        cx,
                        "Confirm close with changes",
                        "Ask before closing a workspace that has uncommitted edits.",
                    ))
                    .child(toggle_button(
                        "toggle-confirm-close",
                        confirm_close,
                        cx,
                        |this, cx| {
                            settings::update(cx, |settings| {
                                settings.confirm_close_with_changes =
                                    !settings.confirm_close_with_changes;
                            });
                            cx.notify();
                        },
                    )),
            )
            .child(div().text_xs().text_color(muted).child("ABOUT"))
            .child(info_row("Settings file", settings_file.display().to_string(), cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .child(
                        div()
                            .child(if dsn_configured {
                                SharedString::from("Sentry DSN: configured")
                            } else {
                                SharedString::from("Sentry DSN: not configured")
                            }),
                    )
                    .child(
                        div()
                            .text_color(if dsn_configured { success } else { warning }),
                        // marker only
                    ),
            )
    }
}
