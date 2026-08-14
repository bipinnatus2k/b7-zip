
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
            .child(Self::section("Progress"))
            .child(div().w(px(300.0)).child(ui_kit::ProgressBar::new().progress(0.65)))
            .child(div().w(px(300.0)).child(ui_kit::ProgressBar::new().indeterminate()))
            .child(Self::section("Badges"))
            .child(
                div().flex_row().gap(px(8.0))
                    .child(ui_kit::Badge::new("info"))
                    .child(ui_kit::Badge::new("ok").success())
                    .child(ui_kit::Badge::new("error").danger())
                    .child(ui_kit::Badge::new("accent").accent()),
            )
            .child(Self::section("Breadcrumb"))
            .child(ui_kit::Breadcrumb::new().push("C:").push("work").push("docs"))
            .child(Self::section("Tabs"))
            .child(ui_kit::Tabs::new().tab("tab-a", "Files").tab("tab-b", "Preview").select("tab-a"))
            .child(Self::section("List"))
            .child(
                ui_kit::List::new()
                    .row("l-1", "first entry")
                    .row("l-2", "second entry")
                    .row("l-3", "third entry")
                    .select("l-2")
                    .max_height(120.0),
            )
            .child(Self::section("Table"))
            .child(
                ui_kit::Table::new()
                    .column("name", "Name", 160.0)
                    .column("size", "Size", 90.0).sortable("size")
                    .row("r-1", vec!["a.txt".into(), "1.2 KB".into()])
                    .row("r-2", vec!["b.txt".into(), "3.4 KB".into()])
                    .select("r-1"),
            )
            .child(Self::section("Tree"))
            .child(
                ui_kit::TreeView::new().root(ui_kit::TreeNode {
                    id: "t-root".into(),
                    label: "archive.7z".into(),
                    expanded: true,
                    selected: false,
                    is_directory: true,
                    children: vec![
                        ui_kit::TreeNode {
                            id: "t-dir".into(),
                            label: "docs".into(),
                            expanded: true,
                            selected: false,
                            is_directory: true,
                            children: vec![ui_kit::TreeNode {
                                id: "t-file".into(),
                                label: "readme.md".into(),
                                expanded: false,
                                selected: true,
                                is_directory: false,
                                children: vec![],
                                on_toggle: None,
                                on_click: None,
                            }],
                            on_toggle: None,
                            on_click: None,
                        },
                        ui_kit::TreeNode {
                            id: "t-other".into(),
                            label: "notes.txt".into(),
                            expanded: false,
                            selected: false,
                            is_directory: false,
                            children: vec![],
                            on_toggle: None,
                            on_click: None,
                        },
                    ],
                    on_toggle: None,
                    on_click: None,
                }),
            )
            .child(Self::section("Combo box"))
            .child(ui_kit::ComboBox::new("combo-1").option("7z", "7z").option("zip", "zip").option("tar", "tar").selected("zip").open())
            .child(Self::section("Toolbar"))
            .child(
                ui_kit::Toolbar::new()
                    .item(ui_kit::Button::new("tb-open", "Open").on_click(|_| {}))
                    .item(ui_kit::Button::new("tb-extract", "Extract").on_click(|_| {}))
                    .item(ui_kit::IconButton::new("tb-cog", "\u{2699}").on_click(|_| {})),
            )
            .child(Self::section("Dialog"))
            .child(
                div().w(px(460.0)).h(px(260.0)).child(
                    ui_kit::Dialog::new("Example dialog")
                        .child(ui_kit::Label::new("This is a modal dialog with content."))
                        .footer_child(ui_kit::Button::new("dlg-ok", "OK").style(ButtonStyle::Primary).on_click(|_| {}))
                        .footer_child(ui_kit::Button::new("dlg-cancel", "Cancel").on_click(|_| {})),
                ),
            )
            .child(Self::section("Toast"))
            .child(ui_kit::Toast::new("Operation completed").severity(ui_kit::ToastSeverity::Success))
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
