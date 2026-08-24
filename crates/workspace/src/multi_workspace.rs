
use anyhow::Result;

use gpui::{actions, deferred, px, AnyView, App, AppContext, Context, DragMoveEvent, Entity, EntityId, EventEmitter, FocusHandle, Focusable, ManagedView, MouseButton, Pixels, Render, Subscription, Task, TaskExt, WeakEntity, Window, WindowId, div, IntoElement, Styled, ParentElement};
use std::cell::Cell;
use std::cmp::PartialEq;
use std::future::Future;
use std::path::PathBuf;
use std::rc::Rc;
use util::ResultExt;
use util::path_list::PathList;
use guise::AppShell;
use platform_title_bar::{PlatformTitleBar, DEFAULT_TITLE_BAR_HEIGHT};

const SIDEBAR_RESIZE_HANDLE_SIZE: Pixels = px(6.0);

use crate::key::ProjectGroupKey;
use crate::workspace::{CloseIntent, OpenMode, Workspace, WorkspaceId,Event as WorkspaceEvent};

actions!(
    multi_workspace,
    [
        /// Toggles the workspace switcher sidebar.
        ToggleWorkspaceSidebar,
        /// Closes the workspace sidebar.
        CloseWorkspaceSidebar,
        /// Moves focus to or from the workspace sidebar without closing it.
        FocusWorkspaceSidebar,
        /// Activates the next project in the sidebar.
        NextProject,
        /// Activates the previous project in the sidebar.
        PreviousProject,
        /// Moves the active project up in the sidebar.
        MoveProjectUp,
        /// Moves the active project down in the sidebar.
        MoveProjectDown,
        /// Activates the next thread in sidebar order.
        NextThread,
        /// Activates the previous thread in sidebar order.
        PreviousThread,
        /// Creates a new thread in the current workspace.
        NewThread,
        /// Moves the active project to a new window.
        MoveProjectToNewWindow,
    ]
);

pub enum MultiWorkspaceEvent {
    ActiveWorkspaceChanged {
        source_workspace: Option<WeakEntity<Workspace>>,
    },
    WorkspaceAdded(Entity<Workspace>),
    WorkspaceRemoved(EntityId),
    ProjectGroupsChanged,
}


#[derive(Clone)]
pub struct ProjectGroup {
    pub key: ProjectGroupKey,
    pub workspaces: Vec<Entity<Workspace>>,
    pub expanded: bool,
}

pub struct SerializedProjectGroupState {
    pub key: ProjectGroupKey,
    pub expanded: bool,
}

#[derive(Clone)]
pub struct ProjectGroupState {
    pub key: ProjectGroupKey,
    pub expanded: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RemovalIntent {
    KeepProject,
    CloseProject,
}

/// One row per workspace held by this window. The displayed workspace is
/// always one of these rows. `pinned` records whether the workspace survives
/// being navigated away from; `activated_at` records when it was last
/// displayed.
struct HeldWorkspace {
    workspace: Entity<Workspace>,
    pinned: bool,
    activated_at: Option<u64>,
}

pub struct MultiWorkspace {
    window_id: WindowId,
    held: Vec<HeldWorkspace>,
    project_groups: Vec<ProjectGroupState>,
    /// Source of truth for which workspace is presented in this window, shared
    /// with each member `Workspace` so they can tell whether they own the
    /// platform window's title and edited indicator. This only exists to prevent
    /// Workspaces from having to read their parent MultiWorkspace to check
    /// chrome ownership, as that might cause a double lease. Kept in sync with
    /// `active_workspace`.
    active_workspace_id: Rc<Cell<EntityId>>,
    // sidebar: Option<Box<dyn SidebarHandle>>,
    // sidebar_open: bool,
    // sidebar_overlay: Option<AnyView>,
    pending_removal_tasks: Vec<Task<()>>,
    _serialize_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
    previous_focus_handle: Option<FocusHandle>,
}

impl EventEmitter<MultiWorkspaceEvent> for MultiWorkspace {}

impl MultiWorkspace {

    pub fn new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let release_subscription = cx.on_release(|this: &mut MultiWorkspace, _cx| {
            if let Some(task) = this._serialize_task.take() {
                task.detach();
            }
            for task in std::mem::take(&mut this.pending_removal_tasks) {
                task.detach();
            }
        });
        let quit_subscription = cx.on_app_quit(Self::app_will_quit);

        Self::subscribe_to_workspace(&workspace, window, cx);
        let weak_self = cx.weak_entity();
        let active_workspace_id = Rc::new(Cell::new(workspace.entity_id()));
        workspace.update(cx, |workspace, cx| {
            workspace.set_multi_workspace(weak_self, active_workspace_id.clone(), cx);
        });
        Self {
            window_id: window.window_handle().window_id(),
            held: vec![HeldWorkspace {
                workspace,
                pinned: false,
                activated_at: Some(0),
            }],
            project_groups: Vec::new(),
            active_workspace_id,
            pending_removal_tasks: Vec::new(),
            _serialize_task: None,
            _subscriptions: vec![
                release_subscription,
                quit_subscription,
            ],
            previous_focus_handle: None,
        }
    }

    fn subscribe_to_workspace(
        workspace: &Entity<Workspace>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {

        cx.subscribe_in(workspace, window, |this, workspace, event, window, cx| {
            if let WorkspaceEvent::Activate = event {
                // this.activate(workspace.clone(), None, window, cx);
            }
        })
            .detach();
    }

    pub fn workspace(&self) -> &Entity<Workspace> {
        &self.held[self.displayed_index()].workspace
    }

    pub fn workspaces(&self) -> impl Iterator<Item = &Entity<Workspace>> {
        self.held.iter().map(|held| &held.workspace)
    }

    /// Ensures the workspace is in the multiworkspace and makes it the active one.
    // pub fn activate(
    //     &mut self,
    //     workspace: Entity<Workspace>,
    //     source_workspace: Option<WeakEntity<Workspace>>,
    //     window: &mut Window,
    //     cx: &mut Context<Self>,
    // ) {
    //     if self.workspace() == &workspace {
    //         self.focus_active_workspace(window, cx);
    //         return;
    //     }
    // 
    //     let old_active_workspace = self.workspace().clone();
    //     let old_active_was_retained = self.active_workspace_is_retained();
    //     let should_retain_workspaces = true;
    // 
    //     if should_retain_workspaces && !old_active_was_retained {
    //         let key = old_active_workspace.read(cx).project_group_key(cx);
    //         let index = self.hold(old_active_workspace.clone(), window, cx);
    //         self.pin(index, key, cx);
    //     }
    // 
    //     let displayed = self.hold(workspace.clone(), window, cx);
    //     if should_retain_workspaces {
    //         let key = workspace.read(cx).project_group_key(cx);
    //         self.pin(displayed, key, cx);
    //     }
    // 
    //     // Publish the new active workspace before anyone reads the shared cell
    //     // to decide who owns the window chrome.
    //     self.active_workspace_id.set(workspace.entity_id());
    // 
    //     let stamp = self
    //         .held
    //         .iter()
    //         .filter_map(|held| held.activated_at)
    //         .max()
    //         .map_or(0, |max| max + 1);
    //     self.held[displayed].activated_at = Some(stamp);
    // 
    //     if !should_retain_workspaces && !old_active_was_retained {
    //         self.detach_workspace(&old_active_workspace, cx);
    //     }
    // 
    //     // The platform window is shared across all workspaces in this window.
    //     // The previously-active workspace left the title and edited indicator
    //     // reflecting its own state, so re-apply them from the newly-active
    //     // workspace (which is now the chrome owner per `owns_window_chrome`).
    //     workspace.update(cx, |workspace, cx| {
    //         workspace.refresh_window_state(window, cx);
    //     });
    // 
    //     cx.emit(MultiWorkspaceEvent::ActiveWorkspaceChanged { source_workspace });
    //     self.serialize(cx);
    //     self.focus_active_workspace(window, cx);
    //     cx.notify();
    // }

    /// Detaches a workspace: clears session state, DB binding, cached
    /// group key, and emits `WorkspaceRemoved`. The DB row is preserved
    /// so the workspace still appears in the recent-projects list.
    fn detach_workspace(&mut self, workspace: &Entity<Workspace>, cx: &mut Context<Self>) {
        if let Some(index) = self.held_index(workspace) {
            assert_ne!(
                index,
                self.displayed_index(),
                "the displayed workspace must be re-pointed before it is detached"
            );
            self.held.remove(index);
        }
        cx.emit(MultiWorkspaceEvent::WorkspaceRemoved(workspace.entity_id()));
        // workspace.update(cx, |workspace, _cx| {
        //     workspace.session_id.take();
        //     workspace._schedule_serialize_workspace.take();
        //     workspace._serialize_workspace_task.take();
        // });
        //
        // if let Some(workspace_id) = workspace.read(cx).database_id() {
        //     let db = crate::persistence::WorkspaceDb::global(cx);
        //     self.pending_removal_tasks.retain(|task| !task.is_ready());
        //     self.pending_removal_tasks
        //         .push(cx.background_spawn(async move {
        //             db.set_session_binding(workspace_id, None, None)
        //                 .await
        //                 .log_err();
        //         }));
        // }
    }

    pub fn focus_active_workspace(&self, window: &mut Window, cx: &mut App) {
        // let focus_handle = self.workspace().read(cx).fallback_focus_handle(window, cx);
        // window.focus(&focus_handle, cx);
    }


    /// Promotes the currently active workspace to persistent if it is
    /// transient, so it is retained across workspace switches even when
    /// the sidebar is closed. No-op if the workspace is already persistent.
    // pub fn retain_active_workspace(&mut self, cx: &mut Context<Self>) {
    //     let index = self.displayed_index();
    //     if self.held[index].pinned {
    //         return;
    //     }
    //     let key = self.held[index].workspace.read(cx).project_group_key(cx);
    //     self.pin(index, key, cx);
    //     // self.serialize(cx);
    //     cx.notify();
    // }

    fn held_index(&self, workspace: &Entity<Workspace>) -> Option<usize> {
        self.held
            .iter()
            .position(|held| held.workspace == *workspace)
    }


    pub fn is_workspace_retained(&self, workspace: &Entity<Workspace>) -> bool {
        self.held
            .iter()
            .any(|held| held.pinned && held.workspace == *workspace)
    }

    pub fn active_workspace_is_retained(&self) -> bool {
        self.held[self.displayed_index()].pinned
    }


    /// The displayed workspace is the most recently activated row.
    fn displayed_index(&self) -> usize {
        self.held
            .iter()
            .enumerate()
            .max_by_key(|(_, held)| held.activated_at)
            .expect("a window always holds at least one workspace")
            .0
    }

    /// Ensures `workspace` has a row in `held`, registering it on first
    /// insert, and returns the row's index.
    fn hold(
        &mut self,
        workspace: Entity<Workspace>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> usize {
        if let Some(index) = self.held_index(&workspace) {
            return index;
        }
        // self.register_workspace(&workspace, window, cx);
        self.held.push(HeldWorkspace {
            workspace,
            pinned: false,
            activated_at: None,
        });
        self.held.len() - 1
    }

    /// Pins the row so the workspace survives navigating away, recording
    /// `group` as the project group it was pinned under. No-op if already
    /// pinned.
    fn pin(&mut self, index: usize, group: ProjectGroupKey, cx: &mut Context<Self>) {
        if self.held[index].pinned {
            return;
        }
        self.held[index].pinned = true;
        // self.ensure_project_group_state(group);
        cx.emit(MultiWorkspaceEvent::WorkspaceAdded(
            self.held[index].workspace.clone(),
        ));
    }

    pub(crate) fn activate_provisional_workspace(
        &mut self,
        workspace: Entity<Workspace>,
        provisional_key: ProjectGroupKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = self.hold(workspace.clone(), window, cx);
        self.pin(index, provisional_key, cx);
        // self.activate(workspace, None, window, cx);
    }


    fn app_will_quit(&mut self, _cx: &mut Context<Self>) -> impl Future<Output = ()> + use<> {
        let mut tasks: Vec<Task<()>> = Vec::new();
        if let Some(task) = self._serialize_task.take() {
            tasks.push(task);
        }
        tasks.extend(std::mem::take(&mut self.pending_removal_tasks));

        async move {
            futures::future::join_all(tasks).await;
        }
    }


}

impl Render for MultiWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(
                AppShell::new()
                    .header(DEFAULT_TITLE_BAR_HEIGHT,move |window, cx| {
                        cx.new(|cx| {
                            PlatformTitleBar::new("app-title-bar")
                        })
                    })
                    .child(cx.new(|cx| {
                        Workspace::new(window,cx)
                    }))
            )
    }
}

