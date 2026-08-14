
//! Component gallery: renders every ui_kit component so styles and
//! interactions can be inspected while iterating on the kit.

use gpui::{Context, Div, IntoElement, ParentElement, Render, Styled, Window, div, px};
use ui_kit::{Button, ButtonStyle, Checkbox, IconButton, TextField};

pub struct Gallery {
    pub text_value: SharedString,
}

impl Gallery {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            text_value: "".into(),
        }
    }

    fn section(title: impl Into<SharedString>) -> Div {
        div()
            .flex_col()
            .gap(px(8.0))
            .child(div().text_size(px(14.0)).text_color(ui_kit::tokens::TEXT_SECONDARY).child(title.into()))
    }
}

impl Render for Gallery {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui::{App, SharedString};
        let _ = &mut self.text_value;
        let tf1 = TextField::new("tf-1", cx);
        tf1.update(cx, |field, _| {
            field.set_placeholder("Type here...");
            field.set_on_change(|value, _cx| {
                // The gallery stores its own copy; a real app
                // would route this into shared state.
                let _ = value;
            });
        });
        let tf2 = TextField::new("tf-2", cx);
        tf2.update(cx, |field, _| {
            field.set_placeholder("disabled");
            field.set_enabled(false);
        });
        div()
            .flex_col()
            .gap(px(16.0))
            .p(px(24.0))
            .bg(ui_kit::tokens::BACKGROUND)
            .size_full()
            .child(Self::section("Buttons"))
            .child(
                div().flex_row().gap(px(8.0))
                    .child(Button::new("btn-default", "Default").on_click(|_cx| {}))
                    .child(Button::new("btn-primary", "Primary").style(ButtonStyle::Primary).on_click(|_cx| {}))
                    .child(Button::new("btn-danger", "Danger").style(ButtonStyle::Danger).on_click(|_cx| {}))
                    .child(Button::new("btn-disabled", "Disabled").disabled()),
            )
            .child(Self::section("Icon buttons"))
            .child(
                div().flex_row().gap(px(8.0))
                    .child(IconButton::new("icon-1", "\u{2606}").on_click(|_cx| {}))
                    .child(IconButton::new("icon-2", "\u{2699}").on_click(|_cx| {}))
                    .child(IconButton::new("icon-3", "\u{21BB}").on_click(|_cx| {})),
            )
            .child(Self::section("Checkboxes"))
            .child(
                div().flex_col().gap(px(4.0))
                    .child(Checkbox::new("cb-1").checked(true).label("checked").on_toggle(|_, _| {}))
                    .child(Checkbox::new("cb-2").label("unchecked").on_toggle(|_, _| {}))
                    .child(Checkbox::new("cb-3").label("disabled").disabled()),
            )
            .child(Self::section("Text field"))
            .child(
                div().flex_col().gap(px(8.0)).w(px(320.0))
                    .child(tf1)
                    .child(tf2),
            )
            .child(Self::section("Colors"))
            .child(
                div().flex_row().gap(px(8.0))
                    .child(color_swatch("accent", ui_kit::tokens::ACCENT))
                    .child(color_swatch("danger", ui_kit::tokens::DANGER))
                    .child(color_swatch("success", ui_kit::tokens::SUCCESS))
                    .child(color_swatch("warning", ui_kit::tokens::WARNING)),
            )
    }
}

fn color_swatch(name: impl Into<SharedString>, color: gpui::Rgba) -> Div {
    div()
        .size(px(32.0))
        .rounded(px(6.0))
        .bg(color)
        .child(div().text_size(px(9.0)).text_color(ui_kit::tokens::TEXT_SECONDARY).child(name.into()))
}

use gpui::SharedString;
