use std::collections::HashSet;

use gpui::{
    div, hsla, px, App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement,
    Styled, Task, Window,
};
use gpui_kit::base::dock::PanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::tree::{tree as tree_view, TreeEntry, TreeEvent, TreeItem, TreeState};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable};

use crate::archive_workspace::{DirChildren, FilesTree, TreeRow};
use crate::globals;
use crate::panels::panel_title;

/// Poll cadence for mirroring the active archive's tree snapshot.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(400);

/// Marks the placeholder child that keeps the disclosure toggle visible on a
/// directory whose children are not fetched yet. The NUL byte cannot occur
/// in an archive path, so the id never collides with a real directory.
const LAZY_MARK: char = '\u{0}';

fn is_lazy(item: &TreeItem) -> bool {
    item.id.contains(LAZY_MARK)
}

fn lazy_placeholder(parent_path: &str) -> TreeItem {
    TreeItem::new(format!("{parent_path}/{LAZY_MARK}"), "…")
}

fn kid_item(row: &TreeRow) -> TreeItem {
    let mut item = TreeItem::new(row.path.clone(), row.name.clone());
    if row.has_subdirs {
        item = item.child(lazy_placeholder(&row.path));
    }
    item
}

/// The dock "Files" tab: the active archive's folder tree plus its comment.
/// A pure view built on the gpui-kit tree. Directory levels are fetched
/// lazily from the active workspace as the user expands nodes — every node
/// starts collapsed, so opening an archive only ever reads the root level.
/// The panel mirrors the workspace's snapshot header on its own poll ticks
/// (the same ones the workspace refreshes) and never touches the session
/// mutex on the render path.
pub(crate) struct FilesPanel {
    focus_handle: FocusHandle,
    /// Entity id of the archive workspace the current tree belongs to.
    active_id: Option<u64>,
    /// gpui-kit tree behavior state (visible entries, selection, scroll).
    tree: Entity<TreeState>,
    /// Panel-owned materialized model, pushed into `tree` via `set_items`.
    /// Expanded flags live on shared `Rc`s, so re-pushing keeps them.
    roots: Vec<TreeItem>,
    /// Directories whose children have been fetched (root is always there).
    materialized: HashSet<String>,
    /// Directory awaiting expansion while the session mutex was busy, plus
    /// whether the expansion should be re-applied once fetched.
    pending_expand: Option<(String, bool)>,
    /// The overlay moved under a materialized tree; resync on the next tick.
    stale: bool,
    /// Header data mirrored from the active workspace.
    snapshot: Option<FilesTree>,
    _poll: Option<Task<()>>,
    _subscription: gpui::Subscription,
}

impl FilesPanel {
    pub(crate) fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            let tree = cx.new(|cx| TreeState::new(cx));
            let subscription =
                cx.subscribe(&tree, |this: &mut Self, _, event: &TreeEvent, cx| {
                    this.on_tree_event(event, cx);
                });
            let mut this = Self {
                focus_handle: cx.focus_handle(),
                active_id: None,
                tree,
                roots: Vec::new(),
                materialized: HashSet::new(),
                pending_expand: None,
                stale: false,
                snapshot: None,
                _poll: None,
                _subscription: subscription,
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

    /// Copies the active archive's tree header in (the timer outruns the
    /// data by design), then retries deferred work: a resync after an
    /// overlay change, or an expansion that hit a busy session mutex.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let workspace = globals::active_archive(cx).and_then(|weak| weak.upgrade());
        let next = workspace
            .as_ref()
            .and_then(|workspace| workspace.read(cx).tree_snapshot(cx));
        // Tree state is bound to one archive; a switch (or none) rebuilds it
        // collapsed — expanded paths of different archives would collide.
        let workspace_id = workspace.as_ref().map(|w| w.entity_id().as_u64());
        if workspace_id != self.active_id {
            self.active_id = workspace_id;
            self.snapshot = next;
            self.rebuild(false, cx);
            cx.notify();
            return;
        }
        let Some(next) = next else {
            if self.snapshot.take().is_some() {
                self.rebuild(false, cx);
                cx.notify();
            }
            return;
        };
        let Some(prev) = self.snapshot.replace(next.clone()) else {
            // The session just bound for this archive: build the tree.
            self.rebuild(false, cx);
            cx.notify();
            return;
        };
        if prev.fingerprint != next.fingerprint {
            self.stale = true;
        }
        if prev.current_path != next.current_path {
            self.sync_selection(cx);
        }
        if prev.title != next.title || prev.comment != next.comment {
            cx.notify();
        }
        if self.stale {
            self.rebuild(true, cx);
            cx.notify();
            return;
        }
        if let Some((path, expand)) = self.pending_expand.take() {
            self.materialize(&path, cx);
            if expand && self.materialized.contains(&path) {
                self.set_node_expanded(&path, true, cx);
            }
        }
    }

    /// Fetches one directory level from the active workspace.
    fn fetch_kids(&self, path: &str, cx: &App) -> DirChildren {
        let Some(workspace) = globals::active_archive(cx).and_then(|weak| weak.upgrade()) else {
            return DirChildren::Missing;
        };
        workspace.read(cx).dir_children(path)
    }

    /// Rebuilds the model from scratch: only the root level is fetched, and
    /// every node below starts collapsed. With `keep_expansion`, the
    /// directories materialized before an overlay change are re-fetched and
    /// re-expanded (those that still exist); never-expanded rows keep their
    /// placeholder and stay lazy.
    fn rebuild(&mut self, keep_expansion: bool, cx: &mut Context<Self>) {
        self.stale = false;
        self.pending_expand = None;
        let previous = if keep_expansion {
            collect_nodes(&self.roots)
                .into_iter()
                .filter(|(path, _)| self.materialized.contains(path))
                .collect()
        } else {
            Vec::new()
        };
        self.materialized.clear();
        let title = self
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.title.clone())
            .unwrap_or_default();
        let mut root = TreeItem::new("", title);
        match self.fetch_kids("", cx) {
            DirChildren::Found(rows) => {
                self.materialized.insert(String::new());
                if !rows.is_empty() {
                    root = root.children(rows.iter().map(kid_item)).expanded(true);
                }
            }
            DirChildren::Missing => {
                self.materialized.insert(String::new());
            }
            DirChildren::Busy => {
                root = root.child(lazy_placeholder("")).expanded(true);
                self.pending_expand = Some((String::new(), false));
            }
        }
        self.roots = vec![root];
        for (path, was_expanded) in previous {
            if path.is_empty() || self.materialized.contains(&path) {
                continue;
            }
            match self.fetch_kids(&path, cx) {
                DirChildren::Found(rows) => {
                    self.materialized.insert(path.clone());
                    self.attach(&path, rows, was_expanded.then_some(true));
                }
                // Deleted in the overlay (or the lock got busy again): the
                // subtree is simply dropped; `stale` retries a busy resync.
                DirChildren::Missing => {}
                DirChildren::Busy => {
                    if was_expanded {
                        self.stale = true;
                    }
                }
            }
        }
        self.push_items(cx);
    }

    /// Fetches the children of `path` if not materialized yet and attaches
    /// them to the model. Called when the tree reports an expansion.
    fn materialize(&mut self, path: &str, cx: &mut Context<Self>) {
        if self.materialized.contains(path) {
            return;
        }
        match self.fetch_kids(path, cx) {
            DirChildren::Found(rows) => {
                self.materialized.insert(path.to_string());
                self.attach(path, rows, None);
                self.push_items(cx);
            }
            DirChildren::Missing => {
                // The directory vanished (e.g. removed by a commit): drop
                // the disclosure marker and resync against the overlay.
                self.materialized.insert(path.to_string());
                self.attach(path, Vec::new(), None);
                self.push_items(cx);
                self.stale = true;
            }
            DirChildren::Busy => {
                // A commit holds the session mutex: bounce the expansion
                // back so no placeholder row is shown, and retry on the
                // next poll tick when the lock should be free again.
                self.set_node_expanded(path, false, cx);
                self.pending_expand = Some((path.to_string(), true));
            }
        }
    }

    /// Replaces the children of `path` in the model. `expand` re-applies an
    /// expansion flag (used by the overlay resync); `None` keeps the flag
    /// the tree already toggled.
    fn attach(&mut self, path: &str, rows: Vec<TreeRow>, expand: Option<bool>) {
        attach_children(&mut self.roots, path, rows, expand);
    }

    fn set_node_expanded(&mut self, path: &str, value: bool, cx: &mut Context<Self>) {
        set_expanded_in(&mut self.roots, path, value);
        self.push_items(cx);
    }

    /// Pushes the model into the tree state (re-flattening only the
    /// expanded branches) and re-aligns the selection with the snapshot.
    fn push_items(&mut self, cx: &mut Context<Self>) {
        let roots = self.roots.clone();
        self.tree.update(cx, |state, cx| state.set_items(roots, cx));
        self.sync_selection(cx);
    }

    fn sync_selection(&mut self, cx: &mut Context<Self>) {
        let Some(current) = self
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.current_path.clone())
        else {
            return;
        };
        self.tree.update(cx, |state, cx| {
            let id = SharedString::from(current);
            state.set_selected_index(state.index_of(&id), cx);
        });
    }

    fn on_tree_event(&mut self, event: &TreeEvent, cx: &mut Context<Self>) {
        match event {
            TreeEvent::Expanded(id) => {
                self.materialize(id, cx);
                // `set_items` resets the tree's own selection; keep the row
                // the user just clicked highlighted until the next poll
                // tick re-aligns it with the explorer's current path.
                self.select(id, cx);
            }
            TreeEvent::Collapsed(id) => {
                if self
                    .pending_expand
                    .as_ref()
                    .is_some_and(|(path, _)| path == id.as_ref())
                {
                    self.pending_expand = None;
                }
            }
        }
    }

    fn select(&mut self, path: &str, cx: &mut Context<Self>) {
        self.tree.update(cx, |state, cx| {
            let id = SharedString::from(path);
            state.set_selected_index(state.index_of(&id), cx);
        });
    }
}

/// Collects every real (non-placeholder) node with its expansion flag,
/// parents before children.
fn collect_nodes(nodes: &[TreeItem]) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    fn walk(nodes: &[TreeItem], out: &mut Vec<(String, bool)>) {
        for node in nodes {
            if is_lazy(node) {
                continue;
            }
            out.push((node.id.to_string(), node.is_expanded()));
            walk(&node.children, out);
        }
    }
    walk(nodes, &mut out);
    out
}

fn attach_children(
    nodes: &mut [TreeItem],
    path: &str,
    rows: Vec<TreeRow>,
    expand: Option<bool>,
) -> bool {
    for node in nodes.iter_mut() {
        if node.id.as_ref() == path {
            std::mem::take(&mut node.children);
            node.children = rows.iter().map(kid_item).collect();
            if let Some(value) = expand {
                *node = node.clone().expanded(value);
            }
            return true;
        }
        if attach_children(&mut node.children, path, rows.clone(), expand) {
            return true;
        }
    }
    false
}

fn set_expanded_in(nodes: &mut [TreeItem], path: &str, value: bool) -> bool {
    for node in nodes.iter_mut() {
        if node.id.as_ref() == path {
            // The expansion flag lives on an Rc shared with the tree state,
            // so this flip is visible to the already-pushed entries too.
            *node = node.clone().expanded(value);
            return true;
        }
        if set_expanded_in(&mut node.children, path, value) {
            return true;
        }
    }
    false
}

/// Renders one visible entry. The gpui-kit tree owns selection, expansion,
/// and keyboard handling; this only supplies the row content (disclosure
/// marker, folder icon, label) and the navigation click. The current-path
/// highlight is the tree's own selection, kept in sync with the snapshot.
fn tree_item(ix: usize, entry: &TreeEntry, cx: &App) -> ListItem {
    let item = entry.item();
    if is_lazy(item) {
        return ListItem::new(("files-tree-lazy", ix))
            .disabled(true)
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("…"),
            );
    }
    let muted = cx.theme().muted_foreground;
    let path = item.id.clone();
    let twist = if entry.is_folder() {
        let icon = if entry.is_expanded() {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        };
        div()
            .w(px(16.0))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .child(Icon::new(icon).xsmall().text_color(muted))
    } else {
        div().w(px(16.0)).h_full().flex_shrink_0()
    };
    ListItem::new(("files-tree-item", ix))
        .pl(px(2.0 + entry.depth() as f32 * 14.0))
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            let Some(active) = globals::active_archive(cx).and_then(|weak| weak.upgrade()) else {
                return;
            };
            active.update(cx, |workspace, cx| workspace.navigate_tree(&path, cx));
        })
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .min_w(px(0.0))
                .flex_1()
                .child(twist)
                .child(Icon::new(IconName::Folder).xsmall().text_color(muted))
                .child(
                    div()
                        .ml_1()
                        .min_w(px(0.0))
                        .flex_1()
                        .truncate()
                        .text_xs()
                        .child(item.label.clone()),
                ),
        )
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
        let Some(snapshot) = self.snapshot.clone() else {
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
        let muted = theme.muted_foreground;
        let border = hsla(theme.border.h, theme.border.s, theme.border.l, 0.35);

        let mut panel = div()
            .id("files-panel")
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                tree_view(&self.tree, move |ix, entry, _selected, _window, cx| {
                    tree_item(ix, entry, cx)
                })
                .flex_1()
                .min_h_0(),
            );
        if let Some(comment) = &snapshot.comment {
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
