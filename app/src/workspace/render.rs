//! Rendering: toolbar, address bar, status bar and the modal dialogs.

use super::Workspace;
use gpui::{
    Context, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, SharedString, Styled,
    div, px,
};
use guise::prelude::*;

impl Workspace {
    // ====================================================================
    // Rendering
    // ====================================================================

    pub(super) fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = cx.global::<Theme>();
        let has_archive = self.explorer.is_some();
        let has_selection = self.selected_count > 0;
        let row = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(8.0))
            .h(px(46.0))
            .border_b_1()
            .border_color(t.border().hsla())
            .bg(t.surface().hsla());

        row
            .child(
                Button::new("tb-open", "Open")
                    .size(Size::Xs)
                    .variant(Variant::Subtle)
                    .left_section(Icon::new(IconName::FolderOpen).size(Size::Xs))
                    .on_click(cx.listener(|this, _, _, cx| this.pick_archive(cx))),
            )
            .child(
                Button::new("tb-add", "Add")
                    .size(Size::Xs)
                    .variant(Variant::Subtle)
                    .left_section(Icon::new(IconName::Plus).size(Size::Xs))
                    .on_click(cx.listener(|this, _, _, cx| this.pick_add_inputs(cx))),
            )
            .child(
                Button::new("tb-extract", "Extract")
                    .size(Size::Xs)
                    .variant(Variant::Subtle)
                    .left_section(Icon::new(IconName::ArchiveRestore).size(Size::Xs))
                    .disabled(!has_archive)
                    .on_click(cx.listener(|this, _, _, cx| this.open_extract_dialog(cx))),
            )
            .child(
                Button::new("tb-test", "Test")
                    .size(Size::Xs)
                    .variant(Variant::Subtle)
                    .left_section(Icon::new(IconName::ShieldCheck).size(Size::Xs))
                    .disabled(!has_archive)
                    .on_click(cx.listener(|this, _, _, cx| this.start_test(cx))),
            )
            .child(div().w(px(8.0)))
            .child(
                Button::new("tb-delete", "Delete")
                    .size(Size::Xs)
                    .variant(Variant::Subtle)
                    .left_section(Icon::new(IconName::Trash2).size(Size::Xs))
                    .disabled(!has_selection)
                    .on_click(cx.listener(|this, _, _, cx| this.request_delete(cx))),
            )
            .child(
                Button::new("tb-rename", "Rename")
                    .size(Size::Xs)
                    .variant(Variant::Subtle)
                    .left_section(Icon::new(IconName::Pencil).size(Size::Xs))
                    .disabled(!has_selection || self.selected_count > 1)
                    .on_click(cx.listener(|this, _, _, cx| this.request_rename(cx))),
            )
            .child(
                Button::new("tb-info", "Info")
                    .size(Size::Xs)
                    .variant(Variant::Subtle)
                    .left_section(Icon::new(IconName::Info).size(Size::Xs))
                    .disabled(!has_archive)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.info_open = true;
                        cx.refresh_windows();
                    })),
            )
    }

    pub(super) fn address_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = cx.global::<Theme>();
        let current = self
            .explorer
            .as_ref()
            .map(|explorer| explorer.read(cx).current_path().to_string())
            .unwrap_or_default();

        let mut crumbs = Breadcrumbs::new();
        if self.explorer.is_some() {
            crumbs = crumbs.link("<root>", cx.listener(|this, _, _, cx| {
                if let Some(explorer) = &this.explorer {
                    explorer.update(cx, |explorer, cx| explorer.navigate("", cx));
                }
                cx.refresh_windows();
            }));
            let mut prefix = String::new();
            for segment in current.split('/').filter(|s| !s.is_empty()) {
                prefix = if prefix.is_empty() {
                    segment.to_string()
                } else {
                    format!("{prefix}/{segment}")
                };
                let target = prefix.clone();
                crumbs = crumbs.link(segment.to_string(), cx.listener(move |this, _, _, cx| {
                    if let Some(explorer) = &this.explorer {
                        explorer.update(cx, |explorer, cx| explorer.navigate(&target, cx));
                    }
                    cx.refresh_windows();
                }));
            }
        } else {
            crumbs = crumbs.item("no archive");
        }

        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(8.0))
            .h(px(38.0))
            .border_b_1()
            .border_color(t.border().hsla())
            .bg(t.body().hsla())
            .child(
                Button::new("tb-up", "")
                    .size(Size::Xs)
                    .variant(Variant::Subtle)
                    .left_section(Icon::new(IconName::ArrowUp).size(Size::Xs))
                    .disabled(self.explorer.is_none())
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(explorer) = &this.explorer {
                            explorer.update(cx, |explorer, cx| explorer.navigate_up(cx));
                        }
                        cx.refresh_windows();
                    })),
            )
            .child(crumbs)
    }

    pub(super) fn status_bar(&self, _cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let archive = self
            .current_archive
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "no archive".to_string());
        StatusBar::new()
            .left(Text::new(format!("{} objects", self.entry_count)).size(Size::Xs).dimmed())
            .center(Text::new(self.status.clone()).size(Size::Xs))
            .right(Text::new(format!("{} selected, {}", self.selected_count, bit7z_explorer::format_size(self.selected_size))).size(Size::Xs))
            .right(Text::new(archive).size(Size::Xs).dimmed())
    }

    fn dialog<M: IntoElement>(&self, cx: &mut Context<Self>, content: M) -> impl IntoElement + use<M> {
        div()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.close_dialogs(cx);
                    cx.stop_propagation();
                }
            }))
            .child(content)
    }

    pub(super) fn extract_modal(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let browse = Button::new("ex-browse", "Browse…")
            .size(Size::Xs)
            .variant(Variant::Default)
            .on_click(cx.listener(|this, _, _, cx| this.pick_extract_folder(cx)));
        self.dialog(
            cx,
            Modal::new()
                .title("Extract")
                .width(520.0)
                .child(self.extract_target.clone())
                .child(div().flex().justify_end().child(browse))
                .child(self.extract_overwrite.clone())
                .child(
                    div().flex().justify_end().gap(px(8.0))
                        .child(Button::new("ex-cancel", "Cancel").size(Size::Xs).variant(Variant::Default).on_click(cx.listener(|this, _, _, cx| {
                            this.extract_open = false;
                            cx.refresh_windows();
                        })))
                        .child(Button::new("ex-ok", "Extract").size(Size::Xs).on_click(cx.listener(|this, _, _, cx| this.submit_extract(cx)))),
                ),
        )
    }

    pub(super) fn add_modal(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let updating = self.current_archive.is_some();
        let files = self.add_inputs.len();

        let name_row: gpui::AnyElement = if updating {
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(Text::new(format!("Add {files} file(s) to the open archive")).size(Size::Sm))
                .into_any_element()
        } else {
            self.add_name.clone().into_any_element()
        };

        let options = if updating {
            div().into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(self.add_format.clone())
                .child(self.add_level.clone())
                .child(Checkbox::new("add-solid").checked(self.add_solid).label("Solid archive").on_change(cx.listener(|this, _, _, cx| {
                    this.add_solid = !this.add_solid;
                    cx.refresh_windows();
                })))
                .child(self.add_volume.clone())
                .child(self.add_threads.clone())
                .into_any_element()
        };

        self.dialog(
            cx,
            Modal::new()
                .title("Add to archive")
                .width(520.0)
                .child(name_row)
                .child(options)
                .child(self.add_password.clone())
                .child(Checkbox::new("add-headers").checked(self.add_encrypt_headers).label("Encrypt file names (header encryption)").on_change(cx.listener(|this, _, _, cx| {
                    this.add_encrypt_headers = !this.add_encrypt_headers;
                    cx.refresh_windows();
                })))
                .child(
                    div().flex().justify_end().gap(px(8.0))
                        .child(Button::new("add-cancel", "Cancel").size(Size::Xs).variant(Variant::Default).on_click(cx.listener(|this, _, _, cx| {
                            this.add_open = false;
                            cx.refresh_windows();
                        })))
                        .child(Button::new("add-ok", if updating { "Add" } else { "Create" }).size(Size::Xs).on_click(cx.listener(|this, _, _, cx| this.submit_add(cx)))),
                ),
        )
    }

    pub(super) fn password_modal(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        self.dialog(
            cx,
            Modal::new()
                .title("Password required")
                .width(420.0)
                .child(self.password_field.clone())
                .child(
                    div().flex().justify_end().gap(px(8.0))
                        .child(Button::new("pw-cancel", "Cancel").size(Size::Xs).variant(Variant::Default).on_click(cx.listener(|this, _, _, cx| this.cancel_password(cx))))
                        .child(Button::new("pw-ok", "OK").size(Size::Xs).on_click(cx.listener(|this, _, _, cx| this.submit_password(cx)))),
                ),
        )
    }

    pub(super) fn rename_modal(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        self.dialog(
            cx,
            Modal::new()
                .title("Rename")
                .width(420.0)
                .child(self.rename_field.clone())
                .child(
                    div().flex().justify_end().gap(px(8.0))
                        .child(Button::new("rn-cancel", "Cancel").size(Size::Xs).variant(Variant::Default).on_click(cx.listener(|this, _, _, cx| {
                            this.rename_open = false;
                            cx.refresh_windows();
                        })))
                        .child(Button::new("rn-ok", "Rename").size(Size::Xs).on_click(cx.listener(|this, _, _, cx| this.submit_rename(cx)))),
                ),
        )
    }

    pub(super) fn delete_confirm(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        self.dialog(
            cx,
            ConfirmModal::new()
                .title("Delete from archive")
                .message(format!("Delete {} selected item(s) from the archive?", self.selected_count))
                .confirm_label("Delete")
                .danger()
                .on_confirm(cx.listener(|this, _, _, cx| this.submit_delete(cx)))
                .on_cancel(cx.listener(|this, _, _, cx| {
                    this.confirm_delete = false;
                    cx.refresh_windows();
                })),
        )
    }

    pub(super) fn info_modal(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let path = self
            .current_archive
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let (count, total, packed) = self.archive_stats().unwrap_or((0, 0, 0));
        let ratio = if total > 0 {
            format!("{:.1}%", 100.0 - packed as f64 * 100.0 / total as f64)
        } else {
            "-".to_string()
        };

        let row_info = self.explorer.as_ref().and_then(|explorer| {
            let selected = explorer.read(cx).selected_rows(cx);
            (selected.len() == 1).then(|| selected[0].clone())
        });

        fn kv(label: &'static str, value: String) -> gpui::Div {
            div().flex().gap(px(8.0)).child(div().w(px(130.0)).flex_none().child(Text::new(label).size(Size::Sm).dimmed())).child(Text::new(value).size(Size::Sm))
        }

        let mut stack = div().flex().flex_col().gap(px(6.0))
            .child(Text::new(path).size(Size::Sm))
            .child(kv("Entries", count.to_string()))
            .child(kv("Total size", bit7z_explorer::format_size(total)))
            .child(kv("Packed size", bit7z_explorer::format_size(packed)))
            .child(kv("Compression ratio", ratio));
        if let Some(row) = row_info {
            stack = stack
                .child(Divider::new())
                .child(Text::new(row.name.clone()).size(Size::Sm).bold())
                .child(kv("Size", bit7z_explorer::format_size(row.size)))
                .child(kv("Packed", bit7z_explorer::format_size(row.packed)))
                .child(kv("Modified", row.modified.clone()))
                .child(kv("CRC", row.crc.map(|crc| format!("{crc:08X}")).unwrap_or_default()))
                .child(kv("Method", row.method.clone()));
        }
        self.dialog(
            cx,
            Modal::new()
                .title("Archive info")
                .width(460.0)
                .child(stack)
                .child(
                    div().flex().justify_end().child(Button::new("info-close", "Close").size(Size::Xs).on_click(cx.listener(|this, _, _, cx| {
                        this.info_open = false;
                        cx.refresh_windows();
                    }))),
                ),
        )
    }

    pub(super) fn task_modal(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let body: gpui::AnyElement = match &self.task {
            None => div().into_any_element(),
            Some(task) => match &task.finished {
                None => {
                    let (processed, total) = task.progress.unwrap_or((0, 0));
                    let percent = if total > 0 {
                        (processed as f64 * 100.0 / total as f64).clamp(0.0, 100.0) as f32
                    } else {
                        0.0
                    };
                    let current = task.current_file.clone().unwrap_or_default();
                    let bar = if total > 0 {
                        Progress::new(percent)
                    } else {
                        Progress::new(100.0).color(ColorName::Gray)
                    };
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.0))
                        .child(bar)
                        .child(
                            div().flex().justify_between()
                                .child(Text::new(format!("{} / {}", bit7z_explorer::format_size(processed), bit7z_explorer::format_size(total))).size(Size::Xs).dimmed())
                                .child(Text::new(format!("{percent:.0}%")).size(Size::Xs).dimmed()),
                        )
                        .child(Text::new(if current.is_empty() { "Working…" } else { &current }).size(Size::Sm).dimmed())
                        .into_any_element()
                }
                Some((success, message)) => {
                    let color = if *success { ColorName::Green } else { ColorName::Red };
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .child(Alert::new(if *success { "Completed successfully" } else { "Failed" }).color(color).variant(Variant::Light))
                        .child(Text::new(message.clone()).size(Size::Sm).dimmed())
                        .into_any_element()
                }
            },
        };

        let (title, footer): (SharedString, gpui::AnyElement) = match &self.task {
            None => (SharedString::default(), div().into_any_element()),
            Some(task) => {
                let title = task.label.clone();
                let footer: gpui::AnyElement = if task.finished.is_some() {
                    Button::new("task-close", "Close")
                        .size(Size::Xs)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.close_task();
                            cx.refresh_windows();
                        }))
                        .into_any_element()
                } else {
                    Button::new("task-cancel", "Cancel")
                        .size(Size::Xs)
                        .color(ColorName::Red)
                        .variant(Variant::Light)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.cancel_task();
                            cx.refresh_windows();
                        }))
                        .into_any_element()
                };
                (title.into(), footer)
            }
        };

        self.dialog(
            cx,
            Modal::new()
                .title(title)
                .width(480.0)
                .child(body)
                .child(div().flex().justify_end().child(footer)),
        )
    }
}
