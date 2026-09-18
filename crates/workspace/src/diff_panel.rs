//! The diff panel: an entry list (added / removed / modified) plus a
//! side-by-side content view for the selected entry. The panel renders a
//! snapshot — entries and content are computed when the panel is opened, so
//! it stays valid even if the source workspace changes underneath.

use compare::{compare_bytes, BlobInfo, ContentDiff, DiffEntry, DiffKind, DiffLine, DiffReport, LineKind};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, hsla, px, uniform_list, App, AppContext, Context, Div, EventEmitter, FocusHandle,
    Focusable, Hsla, InteractiveElement, IntoElement, ParentElement, Render,
    SharedString, Stateful, StatefulInteractiveElement, Styled, UniformListScrollHandle, Window,
};
use gpui_kit::base::dock::PanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable};
use std::sync::Arc;

/// Content larger than this is shown as a digest card instead of lines.
const MAX_DIFF_HINT: &str = "(oversized or unavailable contents are summarized)";

pub struct DiffPanel {
    focus_handle: FocusHandle,
    title: SharedString,
    entries: Vec<DiffEntry>,
    /// Content diff per selected path, computed lazily through the provider.
    provider: Arc<dyn DiffContentProvider>,
    selected: Option<usize>,
    /// The rendered content view for `selected` once loaded.
    content: Option<ContentView>,
    entries_scroll: UniformListScrollHandle,
    rows_scroll: UniformListScrollHandle,
}

/// Supplies the raw bytes of one entry, per side. Implemented by the host
/// (workspace) so this panel stays decoupled from the engine.
pub trait DiffContentProvider: Send + Sync {
    fn content(&self, path: &str, side: Side) -> Option<Vec<u8>>;
}

/// The no-content provider behind placeholder panels restored from a saved
/// layout: they have no snapshot to compare.
pub struct EmptyProvider;

impl DiffContentProvider for EmptyProvider {
    fn content(&self, _: &str, _: Side) -> Option<Vec<u8>> {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Base,
    Working,
}

/// The loaded content view for the selected entry.
enum ContentView {
    Text { rows: Vec<DiffLine> },
    Binary { base: BlobInfo, working: BlobInfo },
    Unavailable(String),
}

impl EventEmitter<PanelEvent> for DiffPanel {}

impl Focusable for DiffPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for DiffPanel {
    fn panel_name(&self) -> &'static str {
        "diff"
    }
}

impl Panel for DiffPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_1p5()
            .child(Icon::new(IconName::FileText).xsmall())
            .child(self.title.clone())
    }
}

impl DiffPanel {
    /// Creates a panel from a precomputed report and a content provider.
    pub fn new(
        title: impl Into<SharedString>,
        report: DiffReport,
        provider: Arc<dyn DiffContentProvider>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            title: title.into(),
            entries: report.entries,
            provider,
            selected: None,
            content: None,
            entries_scroll: UniformListScrollHandle::new(),
            rows_scroll: UniformListScrollHandle::new(),
        }
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(entry) = self.entries.get(ix).cloned() else {
            return;
        };
        self.selected = Some(ix);
        self.content = None;
        cx.notify();
        let provider = self.provider.clone();
        let provider_for_working = provider.clone();
        cx.spawn(async move |this, cx| {
            let path = entry.path.clone();
            let base = cx
                .background_executor()
                .spawn(async move { provider.content(&path, Side::Base) })
                .await;
            // Bail out if the selection moved on while we were loading.
            let still_current = this
                .read_with(cx, |this, _| this.selected.map(|ix| this.entries[ix].path.clone()))
                .ok()
                .flatten()
                .is_some_and(|selected| selected == entry.path);
            if !still_current {
                return;
            }
            let working_path = entry.path.clone();
            let working = cx
                .background_executor()
                .spawn(async move { provider_for_working.content(&working_path, Side::Working) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.content = Some(build_content_view(&entry, base, working));
                this.rows_scroll = UniformListScrollHandle::new();
                cx.notify();
            });
        })
        .detach();
    }
}

fn build_content_view(
    entry: &DiffEntry,
    base: Option<Vec<u8>>,
    working: Option<Vec<u8>>,
) -> ContentView {
    let Some(base) = base else {
        return ContentView::Unavailable("base content unavailable".into());
    };
    let Some(working) = working else {
        return ContentView::Unavailable("working content unavailable".into());
    };
    match compare_bytes(&base, &working) {
        ContentDiff::Same => ContentView::Unavailable("contents are identical".into()),
        ContentDiff::Text(rows) => ContentView::Text { rows },
        ContentDiff::Binary { base, working } => ContentView::Binary { base, working },
    }
}

impl Render for DiffPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (fg, muted, border, secondary, primary, success, danger) = {
            let t = cx.theme();
            (
                t.foreground,
                t.muted_foreground,
                t.border,
                t.secondary,
                t.primary,
                t.success,
                t.danger,
            )
        };
        let entity = cx.entity().clone();
        let weak = entity.downgrade();
        let entry_count = self.entries.len();
        let selected = self.selected;

        let list = div().flex_1().min_h_0().child(
            uniform_list("diff-entries", entry_count, {
                let weak = weak.clone();
                move |range, _window, cx| {
                    let Some(this) = weak.upgrade() else {
                        return Vec::new();
                    };
                    let this = this.read(cx);
                    range
                        .clone()
                        .filter_map(|ix| {
                            let entry = this.entries.get(ix)?;
                            let row = entry_row(
                                ix,
                                entry.clone(),
                                this.selected == Some(ix),
                                muted,
                                secondary,
                                primary,
                            );
                            let weak = weak.clone();
                            Some(row.on_click(move |_, _, cx| {
                                weak.update(cx, |this, cx| this.select(ix, cx)).ok();
                            }))
                        })
                        .collect()
                }
            })
            .track_scroll(&self.entries_scroll)
            .h_full()
            .w_full(),
        );

        let content: Div = match &self.content {
            None => placeholder(if selected.is_some() {
                "Loading contents…"
            } else {
                "Select an entry to compare"
            }, muted),
            Some(ContentView::Unavailable(reason)) => placeholder(reason, muted),
            Some(ContentView::Binary { base, working }) => {
                binary_card(base, working, muted, border, success, danger)
            }
            Some(ContentView::Text { rows }) => {
                let rows = rows.clone();
                text_rows(&rows, fg, muted, border, success, danger, &self.rows_scroll)
            }
        };

        div()
            .id("diff-panel")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .text_color(fg)
            .bg(cx.theme().background)
            .child(
                div()
                    .id("diff-split")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .child(
                        div()
                            .w(px(320.0))
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .border_r_1()
                            .border_color(border)
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .text_xs()
                                    .text_color(muted)
                                    .border_b_1()
                                    .border_color(border)
                                    .child(format!("{entry_count} differing entries")),
                            )
                            .child(list),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .child(content)
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .text_xs()
                                    .text_color(hsla(muted.h, muted.s, muted.l, 0.6))
                                    .child(MAX_DIFF_HINT),
                            ),
                    ),
            )
    }
}

fn entry_row(
    ix: usize,
    entry: DiffEntry,
    selected: bool,
    muted: Hsla,
    secondary: Hsla,
    primary: Hsla,
) -> Stateful<Div> {
    let letter = match entry.kind {
        DiffKind::Added => "A",
        DiffKind::Removed => "R",
        DiffKind::Modified => "M",
    };
    let letter_color = match entry.kind {
        DiffKind::Added => hsla(0.33, 0.7, 0.45, 1.0),
        DiffKind::Removed => hsla(0.0, 0.7, 0.5, 1.0),
        DiffKind::Modified => hsla(0.11, 0.85, 0.5, 1.0),
    };
    let selected_bg = hsla(primary.h, primary.s, primary.l, 0.25);
    let summary = match (entry.base_size, entry.working_size) {
        (Some(a), Some(b)) => format!("{a} → {b} B"),
        (Some(a), None) => format!("{a} B"),
        (None, Some(b)) => format!("{b} B"),
        _ => String::new(),
    };

    div()
        .id(("diff-entry", ix))
        .h(px(28.0))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_2()
        .when(selected, |el| el.bg(selected_bg))
        .when(!selected, |el| el.hover(|el| el.bg(secondary)))
        .child(
            div()
                .w(px(10.0))
                .text_xs()
                .text_color(letter_color)
                .child(letter),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_xs()
                .child(SharedString::from(entry.path)),
        )
        .child(div().text_xs().text_color(muted).child(summary))
}

fn placeholder(text: &str, muted: Hsla) -> Div {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(muted)
        .child(text.to_string())
}

fn binary_card(
    base: &BlobInfo,
    working: &BlobInfo,
    muted: Hsla,
    border: Hsla,
    success: Hsla,
    danger: Hsla,
) -> Div {
    let row = |label: &'static str, info: &BlobInfo| {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_xs().text_color(muted).child(label))
            .child(div().text_xs().child(format!("size: {} bytes", info.size)))
            .child(div().text_xs().child(format!("md5: {}", info.md5)))
    };
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
                .p_4()
                .border_1()
                .border_color(border)
                .child(div().text_sm().child("Binary contents differ"))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_6()
                        .child(row("Base", base))
                        .child(row("Working", working)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(if base.md5 == working.md5 {
                            success
                        } else {
                            danger
                        })
                        .child(if base.md5 == working.md5 {
                            "MD5 identical"
                        } else {
                            "MD5 differs"
                        }),
                ),
        )
}

fn text_rows(
    rows: &[DiffLine],
    fg: Hsla,
    muted: Hsla,
    border: Hsla,
    success: Hsla,
    danger: Hsla,
    scroll: &UniformListScrollHandle,
) -> Div {
    let row_count = rows.len();
    let snapshot: Vec<DiffLine> = rows.to_vec();
    let added_bg = hsla(success.h, success.s, success.l, 0.12);
    let removed_bg = hsla(danger.h, danger.s, danger.l, 0.12);
    let changed_bg = hsla(0.11, 0.85, 0.5, 0.10);

    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(
            div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(muted)
                .border_b_1()
                .border_color(border)
                .child("base    working"),
        )
        .child(
            uniform_list("diff-rows", row_count, move |range, _window, _cx| {
                let start = range.start;
                snapshot[range]
                    .iter()
                    .enumerate()
                    .map(|(offset, row)| {
                        diff_row(
                            start + offset,
                            row,
                            added_bg,
                            removed_bg,
                            changed_bg,
                            fg,
                            muted,
                        )
                    })
                    .collect()
            })
            .track_scroll(scroll)
            .flex_1()
            .min_h_0()
            .w_full(),
        )
}

fn diff_row(
    ix: usize,
    row: &DiffLine,
    added_bg: Hsla,
    removed_bg: Hsla,
    changed_bg: Hsla,
    fg: Hsla,
    muted: Hsla,
) -> Stateful<Div> {
    let (bg, left_color, right_color) = match row.kind {
        LineKind::Context => (None, fg, fg),
        LineKind::Added => (Some(added_bg), muted, fg),
        LineKind::Removed => (Some(removed_bg), fg, muted),
        LineKind::Changed => (Some(changed_bg), fg, fg),
    };
    let line_no = |cell: &Option<(u32, String)>, width_of_other: bool| {
        div()
            .w(px(36.0))
            .flex_shrink_0()
            .text_right()
            .pr_1()
            .text_color(muted)
            .child(
                cell.as_ref()
                    .map(|(n, _)| n.to_string())
                    .unwrap_or_default(),
            )
    };
    let text_of = |cell: &Option<(u32, String)>| {
        cell.as_ref()
            .map(|(_, t)| t.trim_end_matches('\n').to_string())
            .unwrap_or_default()
    };
    let left_text = text_of(&row.left);
    let right_text = text_of(&row.right);

    let mut el = div()
        .id(("diff-row", ix))
        .h(px(18.0))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .font_family("Consolas")
        .text_xs();
    if let Some(bg) = bg {
        el = el.bg(bg);
    }
    el = el
        .child(line_no(&row.left, false))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_color(left_color)
                .child(SharedString::from(left_text)),
        )
        .child(line_no(&row.right, true))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_color(right_color)
                .child(SharedString::from(right_text)),
        );
    el
}
