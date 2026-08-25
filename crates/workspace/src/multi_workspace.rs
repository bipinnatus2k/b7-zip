use bit7z_rs::{ArchiveEngine, Bit7zEngine};
use gpui::{div, px, AppContext, Context, Entity, EntityId, EventEmitter, IntoElement, ParentElement, Render, SharedString, Styled, WeakEntity, Window};
use guise::{AppShell, TabBar, TabBarEvent, Text};
use platform_title_bar::{DEFAULT_TITLE_BAR_HEIGHT, PlatformTitleBar};
use std::path::PathBuf;
use std::sync::Arc;

use crate::workspace::{Workspace, WorkspaceId};

const ENGINE_UNAVAILABLE: &str = "the 7-Zip engine could not be loaded";

pub enum MultiWorkspaceEvent {
    ActiveWorkspaceChanged {
        source_workspace: Option<WeakEntity<Workspace>>,
    },
    WorkspaceAdded(Entity<Workspace>),
    WorkspaceRemoved(EntityId),
}

/// One OS window holding any number of [`Workspace`]s as document-style tabs.
///
/// Owns the workspace list (source of truth for content) and a guise
/// [`TabBar`] (source of truth for the strip UI). Tab events are mirrored
/// back into the workspace list so both stay in sync.
pub struct MultiWorkspace {
    workspaces: Vec<Entity<Workspace>>,
    tab_bar: Entity<TabBar>,
    title_bar: Entity<PlatformTitleBar>,
    /// Shared archive engine; `None` when the 7-Zip DLL failed to load.
    engine: Option<Arc<dyn ArchiveEngine>>,
    active: usize,
    next_id: i64,
}

impl EventEmitter<MultiWorkspaceEvent> for MultiWorkspace {}

impl MultiWorkspace {
    /// Creates the window content. One tab per path in `paths`; a single
    /// welcome tab when no paths were passed.
    pub fn new(paths: Vec<PathBuf>, cx: &mut Context<Self>) -> Self {
        let title_bar = cx.new(|_| PlatformTitleBar::new("app-title-bar"));

        let tab_bar = cx.new(|cx| TabBar::new(cx));
        cx.subscribe(&tab_bar, |this, _bar, event: &TabBarEvent, cx| match event {
            TabBarEvent::Select(index) => this.activate(*index, cx),
            TabBarEvent::Add => this.add_workspace(cx),
            TabBarEvent::Close(index) => this.close_workspace(*index, cx),
        })
        .detach();

        let engine = Bit7zEngine::new(None)
            .ok()
            .map(|engine| Arc::new(engine) as Arc<dyn ArchiveEngine>);

        let mut this = Self {
            workspaces: Vec::new(),
            tab_bar,
            title_bar,
            engine,
            active: 0,
            next_id: 0,
        };
        if paths.is_empty() {
            this.add_workspace(cx);
        } else {
            this.open_paths(&paths, cx);
        }
        this
    }

    /// Opens one tab per path. Paths that cannot be opened still get a tab
    /// so the failure is visible instead of silently dropped.
    pub fn open_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        for path in paths {
            self.open_path(path, cx);
        }
    }

    /// Opens `path` in a new tab and activates it.
    pub fn open_path(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        self.next_id += 1;
        let id = WorkspaceId::from_i64(self.next_id);
        let workspace = match self.engine.clone() {
            Some(engine) => cx.new(|cx| Workspace::open_archive(Some(id), path, &engine, cx)),
            None => cx.new(|_| Workspace::failed(Some(id), path, ENGINE_UNAVAILABLE.into())),
        };
        self.attach_tab(workspace, cx);
    }

    /// The trailing `+` button / fresh-tab entry point: a welcome page tab.
    pub fn add_workspace(&mut self, cx: &mut Context<Self>) {
        self.next_id += 1;
        let id = WorkspaceId::from_i64(self.next_id);
        let workspace = cx.new(|_| Workspace::welcome(Some(id)));
        self.attach_tab(workspace, cx);
    }

    fn attach_tab(&mut self, workspace: Entity<Workspace>, cx: &mut Context<Self>) {
        let title: SharedString = workspace.read(cx).title().to_owned().into();
        self.workspaces.push(workspace.clone());
        self.active = self.workspaces.len() - 1;
        self.tab_bar.update(cx, |bar, cx| bar.add_tab(title, cx));

        cx.emit(MultiWorkspaceEvent::WorkspaceAdded(workspace));
        cx.notify();
    }

    /// Removes the workspace at `index` (no-op when out of range). The
    /// active tab follows the same shift rules the [`TabBar`] applies to
    /// itself. Closing the last tab leaves an empty shell rather than
    /// closing the window.
    pub fn close_workspace(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.workspaces.len() {
            return;
        }
        let workspace = self.workspaces.remove(index);
        let workspace_id = workspace.entity_id();

        self.tab_bar.update(cx, |bar, cx| bar.remove_tab(index, cx));
        self.active = self
            .tab_bar
            .read(cx)
            .active_index()
            .min(self.workspaces.len().saturating_sub(1));

        cx.emit(MultiWorkspaceEvent::WorkspaceRemoved(workspace_id));
        cx.notify();
    }

    /// Makes the workspace at `index` the displayed one.
    pub fn activate(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.workspaces.len() {
            return;
        }
        self.active = index;
        self.tab_bar.update(cx, |bar, cx| bar.set_active(index, cx));
        cx.emit(MultiWorkspaceEvent::ActiveWorkspaceChanged {
            source_workspace: None,
        });
        cx.notify();
    }

    pub fn active_workspace(&self) -> Option<&Entity<Workspace>> {
        self.workspaces.get(self.active)
    }

    pub fn workspaces(&self) -> &[Entity<Workspace>] {
        &self.workspaces
    }

    pub fn active_index(&self) -> usize {
        self.active
    }
}

impl Render for MultiWorkspace {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let title_bar = self.title_bar.clone();

        let content = match self.workspaces.get(self.active) {
            Some(workspace) => div()
                .flex_1()
                .min_h(px(0.))
                .overflow_hidden()
                .child(workspace.clone()),
            None => div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .items_center()
                .justify_center()
                .child(Text::new("No open workspaces").dimmed()),
        };

        div().size_full().child(
            AppShell::new()
                .header(DEFAULT_TITLE_BAR_HEIGHT, move |_, _| title_bar.clone())
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h(px(0.))
                        .child(self.tab_bar.clone())
                        .child(content),
                ),
        )
    }
}
