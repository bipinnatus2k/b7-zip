use std::collections::HashSet;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, hsla, px, uniform_list, App, AppContext, Context, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Task, UniformListScrollHandle, Window,
};
use gpui_kit::base::dock::PanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable};

use crate::archive_workspace::{FilesTree, TreeRow};
use crate::globals;
use crate::panels::panel_title;

/// Poll cadence for mirroring the active archive's tree snapshot.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(400);

/// The dock "Files" tab: the active archive's folder tree plus its comment.
/// A pure view — it mirrors the active workspace's snapshot caches (the same
/// ones the workspace refreshes on its own poll ticks) and never touches the
/// session mutex.
pub(crate) struct FilesPanel {
    focus_handle: FocusHandle,
    /// Directories the user collapsed, by archive path.
    collapsed: HashSet<String>,
    scroll: UniformListScrollHandle,
    tree: Option<FilesTree>,
    _poll: Option<Task<()>>,
}

impl FilesPanel {
    pub(crate) fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            let mut this = Self {
                focus_handle: cx.focus_handle(),
                collapsed: HashSet::new(),
                scroll: UniformListScrollHandle::new(),
                tree: None,
                _poll: None,
            };
            this.start_poll(cx);
            this
        })
    }

    fn start_poll(&mut self, cx: &mut Context<Self>) {
        self._poll = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(POLL_INTERVAL).await;
            if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                break;
            }
        }));
    }

    /// Copies the active archive's tree snapshot in, notifying only on an
    /// actual change (the timer outruns the data by design).
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let next = globals::active_archive(cx)
            .and_then(|weak| weak.upgrade())
            .and_then(|workspace| workspace.read(cx).tree_snapshot(cx));
        if next != self.tree {
            self.tree = next;
            cx.notify();
        }
    }
}

impl BasePanel for FilesPanel {
    fn panel_name(&self) -> &'static str {
        "FilesPanel"
    }
}

impl Panel for FilesPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        panel_title(IconName::FolderOpen, SharedString::from("Files"))
    }
}

impl EventEmitter<PanelEvent> for FilesPanel {}

impl Focusable for FilesPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for FilesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(tree) = self.tree.clone() else {
            return div()
                .id("files-panel")
                .size_full()
                .p_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Open an archive to browse its folders"),
                );
        };

        let theme = cx.theme();
        let (muted, secondary, primary) = (theme.muted_foreground, theme.secondary, theme.primary);
        let border = hsla(theme.border.h, theme.border.s, theme.border.l, 0.35);
        let selected_bg = hsla(primary.h, primary.s, primary.l, 0.18);
        let current_path = tree.current_path;
        let title = tree.title;

        // Collapse filtering is a pure projection of the snapshot: a row
        // shows only when none of its ancestor directories is collapsed.
        let visible: Vec<TreeRow> = tree
            .rows
            .iter()
            .filter(|row| {
                let mut ancestor = row.path.as_str();
                loop {
                    match ancestor.rfind('/') {
                        // "" (the root) is the last ancestor; never collapsed.
                        Some(pos) => ancestor = &ancestor[..pos],
                        None => return true,
                    }
                    if self.collapsed.contains(ancestor) {
                        return false;
                    }
                }
            })
            .cloned()
            .collect();
        let row_count = visible.len();
        let collapsed = self.collapsed.clone();
        let scroll = self.scroll.clone();
        let weak = cx.weak_entity();

        let mut panel = div()
            .id("files-panel")
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                uniform_list("files-tree", row_count, move |range, _window, _cx| {
                    range
                        .filter_map(|ix| visible.get(ix).map(|row| (ix, row.clone())))
                        .map(|(ix, row)| {
                            let is_current = row.path == current_path;
                            let indent = px(2.0 + row.depth as f32 * 14.0);
                            let name = if row.path.is_empty() {
                                title.clone()
                            } else {
                                row.name.clone()
                            };
                            let twist = if row.has_subdirs {
                                let is_collapsed = collapsed.contains(&row.path);
                                div()
                                    .id(("tree-twist", ix))
                                    .w(px(16.0))
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .flex_shrink_0()
                                    .cursor_pointer()
                                    .child(if is_collapsed {
                                        Icon::new(IconName::ChevronRight)
                                            .xsmall()
                                            .text_color(muted)
                                    } else {
                                        Icon::new(IconName::ChevronDown)
                                            .xsmall()
                                            .text_color(muted)
                                    })
                                    .on_click({
                                        let weak = weak.clone();
                                        let path = row.path.clone();
                                        move |_, _, cx| {
                                            cx.stop_propagation();
                                            let _ = weak.update(cx, |this, cx| {
                                                if !this.collapsed.remove(&path) {
                                                    this.collapsed.insert(path.clone());
                                                }
                                                cx.notify();
                                            });
                                        }
                                    })
                            } else {
                                div()
                                    .id(("tree-space", ix))
                                    .w(px(16.0))
                                    .h_full()
                                    .flex_shrink_0()
                            };
                            div()
                                .id(("tree-row", ix))
                                .h(px(22.0))
                                .w_full()
                                .flex()
                                .flex_row()
                                .items_center()
                                .pl(indent)
                                .pr_1()
                                .cursor_pointer()
                                .when(is_current, |el| el.bg(selected_bg))
                                .when(!is_current, |el| el.hover(|el| el.bg(secondary)))
                                .child(twist)
                                .child(Icon::new(IconName::Folder).xsmall().text_color(muted))
                                .child(
                                    div()
                                        .ml_1()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .truncate()
                                        .text_xs()
                                        .child(name),
                                )
                                .on_click({
                                    let path = row.path.clone();
                                    move |_, _, cx| {
                                        let Some(active) =
                                            globals::active_archive(cx).and_then(|w| w.upgrade())
                                        else {
                                            return;
                                        };
                                        active.update(cx, |ws, cx| ws.navigate_tree(&path, cx));
                                    }
                                })
                                .into_any_element()
                        })
                        .collect()
                })
                .track_scroll(&scroll)
                .flex_1()
                .min_h_0(),
            );
        if let Some(comment) = &tree.comment {
            panel = panel.child(
                div()
                    .id("archive-comment")
                    .flex_shrink_0()
                    .max_h(px(180.0))
                    .overflow_y_scroll()
                    .border_t_1()
                    .border_color(border)
                    .p_2()
                    .child(div().text_xs().text_color(muted).pb_1().child("Comment"))
                    .child(div().text_xs().child(comment.clone())),
            );
        }
        panel
    }
}

/// One panel implementation serving two tabs: the persisted `panel_name`
/// picks the content when the layout is restored.
pub(crate) struct SidebarPanel {
    focus_handle: FocusHandle,
    name: &'static str,
    title: &'static str,
    icon: IconName,
    items: Vec<&'static str>,
}

impl SidebarPanel {
    pub(crate) fn outline(cx: &mut App) -> Entity<Self> {
        Self::build(
            "OutlinePanel",
            "Outline",
            IconName::BookOpen,
            vec![
                "struct Workspace",
                "impl Workspace",
                "fn new",
                "fn render",
                "impl Panel",
            ],
            cx,
        )
    }

    pub(crate) fn output(cx: &mut App) -> Entity<Self> {
        Self::build(
            "OutputPanel",
            "Output",
            IconName::SquareTerminal,
            Vec::new(),
            cx,
        )
    }

    fn build(
        name: &'static str,
        title: &'static str,
        icon: IconName,
        items: Vec<&'static str>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| Self {
            focus_handle: cx.focus_handle(),
            name,
            title,
            icon,
            items,
        })
    }
}

impl BasePanel for SidebarPanel {
    fn panel_name(&self) -> &'static str {
        self.name
    }
}

impl Panel for SidebarPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        panel_title(self.icon.clone(), SharedString::from(self.title))
    }
}

impl EventEmitter<PanelEvent> for SidebarPanel {}

impl Focusable for SidebarPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SidebarPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(SharedString::from(self.name))
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_0p5()
            .p_2()
            .children(self.items.iter().map(|item| {
                div()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .text_sm()
                    .hover(|this| this.bg(cx.theme().accent))
                    .child(*item)
            }))
    }
}
