use gpui::{div, AnyView, AnyWeakView, Context, Div, Entity, EventEmitter, IntoElement, Render, Subscription, WeakEntity, Window, Styled, ParentElement};
use gpui::prelude::FluentBuilder;
use guise::panegroup::Pane;

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct WorkspaceId(i64);

impl WorkspaceId {
    pub fn from_i64(value: i64) -> Self {
        Self(value)
    }
}

impl From<WorkspaceId> for i64 {
    fn from(val: WorkspaceId) -> Self {
        val.0
    }
}

#[derive(PartialEq, Eq, Debug)]
pub enum CloseIntent {
    /// Quit the program entirely.
    Quit,
    /// Close a window.
    CloseWindow,
    /// Replace the workspace in an existing window.
    ReplaceWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpenMode {
    /// Open the workspace in a new window.
    NewWindow,
    /// Add to the window's multi workspace without activating it (used during deserialization).
    Add,
    /// Add to the window's multi workspace and activate it.
    #[default]
    Activate,
}

pub struct Workspace {
    workspace_actions: Vec<Box<dyn Fn(Div, &Workspace, &mut Window, &mut Context<Self>) -> Div>>,
    zoomed: Option<AnyWeakView>,
    _subscriptions: Vec<Subscription>,
    workspace_id: Option<WorkspaceId>
}

impl Workspace {

    pub fn new(
        workspace_id: Option<WorkspaceId>,
        // window: &mut Window,
        cx: &mut Context<Self>
    ) -> Self {
        let weak_handle = cx.entity().downgrade();

        cx.emit(Event::WorkspaceCreated(weak_handle.clone()));

        // let multi_workspace = window
        //     .root::<MultiWorkspace>()
        //     .flatten()
        //     .map(|mw| mw.downgrade());

        let subscriptions = vec![
            // cx.observe_window_activation(window, Self::on_window_activation_changed),
            // cx.observe_window_bounds(window, move |this, window, cx| {
            //     if !window.is_window_active() {
            //         return;
            //     }
            //     if this.bounds_save_task_queued.is_some() {
            //         return;
            //     }
            //     this.bounds_save_task_queued = Some(cx.spawn_in(window, async move |this, cx| {
            //         cx.background_executor()
            //             .timer(Duration::from_millis(100))
            //             .await;
            //         this.update_in(cx, |this, window, cx| {
            //             this.save_window_bounds(window, cx).detach();
            //             this.bounds_save_task_queued.take();
            //         })
            //             .ok();
            //     }));
            //     cx.notify();
            // }),
            // cx.observe_window_appearance(window, |_, window, cx| {
            //     let window_appearance = window.appearance();
            //
            //     *SystemAppearance::global_mut(cx) = SystemAppearance(window_appearance.into());
            //
            //     theme_settings::reload_theme(cx);
            //     theme_settings::reload_icon_theme(cx);
            // }),
            // cx.on_release({
            //     let weak_handle = weak_handle.clone();
            //     move |this, cx| {
            //         this.app_state.workspace_store.update(cx, move |store, _| {
            //             store.workspaces.retain(|(_, weak)| weak != &weak_handle);
            //         })
            //     }
            // }),
        ];


        Self {
            // weak_self: weak_handle.clone(),
            workspace_id,
            zoomed: None,
            // zoomed_position: None,
            // maximized_pane: None,
            // previous_dock_drag_coordinates: None,
            // center,
            // panes: vec![center_pane.clone()],
            // panes_by_item: Default::default(),
            // active_pane: center_pane.clone(),
            // last_active_center_pane: Some(center_pane.downgrade()),
            // last_active_view_id: None,
            // status_bar,
            // modal_layer,
            // toast_layer,
            // titlebar_item: None,
            // titlebar_focus_handle: cx.focus_handle(),
            // region_focus_handles: RegionFocusHandles::new(cx),
            // notifications: Notifications::default(),
            // suppressed_notifications: HashSet::default(),
            // left_dock,
            // bottom_dock,
            // right_dock,
            // _panels_task: None,
            // project: project.clone(),
            // follower_states: Default::default(),
            // last_leaders_by_pane: Default::default(),
            // auto_watch: AutoWatch::Off,
            // dispatching_keystrokes: Default::default(),
            // window_edited: false,
            // last_window_title: None,
            // dirty_items: Default::default(),
            // active_call,
            // database_id: workspace_id,
            // app_state,
            // _observe_current_user,
            // _apply_leader_updates,
            // _schedule_serialize_workspace: None,
            // _serialize_workspace_task: None,
            // _schedule_serialize_ssh_paths: None,
            // leader_updates_tx,
            _subscriptions: subscriptions,
            // pane_history_timestamp,
            workspace_actions: Default::default(),
            // This data will be incorrect, but it will be overwritten by the time it needs to be used.
            // bounds: Default::default(),
            // centered_layout: false,
            // bounds_save_task_queued: None,
            // on_prompt_for_new_path: None,
            // on_prompt_for_open_path: None,
            // terminal_provider: None,
            // debugger_provider: None,
            // serializable_items_tx,
            // _items_serializer,
            // session_id: Some(session_id),
            //
            // scheduled_tasks: Vec::new(),
            // last_open_dock_positions: Vec::new(),
            // removing: false,
            // sidebar_focus_handle: None,
            // multi_workspace,
            // active_workspace_id: None,
            // active_worktree_creation: ActiveWorktreeCreation::default(),
            // open_in_dev_container: false,
            // _dev_container_task: None,
            // deferred_save_items: Vec::new(),
        }
    }
    
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .when_some(self.workspace_id, |x, t| {
                x.child(format!("Workspace {}", t.0))
            })
    }
}


impl EventEmitter<Event> for Workspace {}

pub enum Event {
    PaneAdded(Entity<Pane>),
    PaneRemoved,
    WorkspaceCreated(WeakEntity<Workspace>),
    ZoomChanged,
    Activate,
    PanelAdded(AnyView),
}

