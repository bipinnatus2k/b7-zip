//! App-level globals: the shared archive engine, the session store, and the
//! workspace registry the tab bar consults for dirty flags.

use bit7z_rs::{ArchiveEngine, Bit7zEngine};
use gpui::{App, Global, WeakEntity};
use session::SessionStore;
use std::collections::HashMap;
use std::sync::Arc;

use crate::archive_workspace::ArchiveWorkspace;
use crate::diff_panel::DiffPanel;

/// The process-wide archive engine, `None` when the 7-Zip runtime failed to
/// load. All engine access is serialized behind one Mutex per the bit7z
/// single-thread contract.
pub struct EngineGlobal(Option<Arc<dyn ArchiveEngine>>);

impl Global for EngineGlobal {}

pub struct SessionGlobal(SessionStore);

impl Global for SessionGlobal {}

/// Weak handle onto the window's host view, so panels can open archives
/// through the same path as the actions without going through the action
/// bubble.
#[derive(Default)]
pub struct HostGlobal(Option<WeakEntity<crate::multi_workspace::MultiWorkspace>>);

impl Global for HostGlobal {}

/// Weak handle onto the window's dock area, so panels can add themselves.
#[derive(Default)]
pub struct DockAreaGlobal(Option<gpui::WeakEntity<gpui_kit::component::dock::DockArea>>);

impl Global for DockAreaGlobal {}

/// The archive workspace whose tab is currently displayed. Stamped by
/// `ArchiveWorkspace::render` — a hidden dock tab never renders, so the last
/// visible tab wins with no event plumbing. Single-slot by the same
/// concession as the other host globals (multi-window overwrite is accepted
/// debt); home tabs never stamp, so the tree keeps showing the last archive
/// while the home tab is on screen.
#[derive(Default)]
pub struct ActiveArchiveGlobal(Option<WeakEntity<ArchiveWorkspace>>);

impl Global for ActiveArchiveGlobal {}

/// Maps dock panel ids to their workspace entity so the tab bar can ask a
/// panel whether it is dirty before closing it, without downcasting views.
#[derive(Default)]
pub struct WorkspaceRegistry {
    by_panel: HashMap<u64, WeakEntity<ArchiveWorkspace>>,
}

impl Global for WorkspaceRegistry {}

impl WorkspaceRegistry {
    fn register(&mut self, panel_id: u64, workspace: WeakEntity<ArchiveWorkspace>) {
        self.by_panel.insert(panel_id, workspace);
    }

    fn unregister(&mut self, panel_id: u64) {
        self.by_panel.remove(&panel_id);
    }

    pub fn get(&self, panel_id: u64) -> Option<WeakEntity<ArchiveWorkspace>> {
        self.by_panel.get(&panel_id).cloned()
    }

    pub fn all(&self) -> impl Iterator<Item = WeakEntity<ArchiveWorkspace>> + '_ {
        self.by_panel.values().cloned()
    }
}

/// Creates the engine, session store, and workspace registry. Call once at
/// startup, before any window is opened.
pub fn init(cx: &mut App) {
    let engine: Option<Arc<dyn ArchiveEngine>> =
        Bit7zEngine::new(bit7z_rs::locate_dll().as_deref())
            .ok()
            .map(|engine| Arc::new(engine) as Arc<dyn ArchiveEngine>);
    if engine.is_none() {
        eprintln!("7-Zip engine failed to load; archives cannot be opened");
    }
    cx.set_global(EngineGlobal(engine));
    cx.set_global(SessionGlobal(SessionStore::new()));
    cx.set_global(WorkspaceRegistry::default());
    cx.set_global(DockAreaGlobal::default());
    cx.set_global(HostGlobal::default());
    cx.set_global(HostWindowGlobal::default());
    cx.set_global(ActiveArchiveGlobal::default());
}

pub fn set_host(cx: &mut App, host: WeakEntity<crate::multi_workspace::MultiWorkspace>) {
    cx.global_mut::<HostGlobal>().0 = Some(host);
}

pub fn host(cx: &App) -> Option<WeakEntity<crate::multi_workspace::MultiWorkspace>> {
    cx.global::<HostGlobal>().0.clone()
}

/// The host window handle, so async flows (which carry no window) can re-enter
/// the window context after awaiting a file dialog.
#[derive(Default)]
pub struct HostWindowGlobal(Option<gpui::AnyWindowHandle>);

impl Global for HostWindowGlobal {}

pub fn set_host_window(cx: &mut App, handle: gpui::AnyWindowHandle) {
    cx.global_mut::<HostWindowGlobal>().0 = Some(handle);
}

pub fn host_window(cx: &App) -> Option<gpui::AnyWindowHandle> {
    cx.global::<HostWindowGlobal>().0
}

/// Marks `workspace` as the currently displayed archive tab. Called from its
/// render, which only runs while the tab is visible.
pub fn set_active_archive(cx: &mut App, workspace: WeakEntity<ArchiveWorkspace>) {
    cx.global_mut::<ActiveArchiveGlobal>().0 = Some(workspace);
}

/// The archive tab currently on screen, if any (home tabs don't count).
pub fn active_archive(cx: &App) -> Option<WeakEntity<ArchiveWorkspace>> {
    cx.global::<ActiveArchiveGlobal>().0.clone()
}

/// All other open archive workspaces, as (title, session) pairs. Used by the
/// cross-workspace comparison picker; home tabs are skipped.
pub fn other_archive_sessions(
    cx: &App,
    exclude: gpui::EntityId,
) -> Vec<(String, std::sync::Arc<std::sync::Mutex<session::ArchiveSession>>)> {
    let registry = cx.try_global::<WorkspaceRegistry>();
    let Some(registry) = registry else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for weak in registry.all() {
        let Some(entity) = weak.upgrade() else {
            continue;
        };
        if entity.entity_id() == exclude {
            continue;
        }
        let Some((title, session)) = entity.read(cx).title_and_session() else {
            continue;
        };
        out.push((title, session));
    }
    out
}

pub fn engine(cx: &App) -> Option<Arc<dyn ArchiveEngine>> {
    cx.global::<EngineGlobal>().0.clone()
}

pub fn sessions(cx: &App) -> &SessionStore {
    &cx.global::<SessionGlobal>().0
}

pub fn set_dock_area(cx: &mut App, area: gpui::WeakEntity<gpui_kit::component::dock::DockArea>) {
    cx.global_mut::<DockAreaGlobal>().0 = Some(area);
}

/// Opens `panel` as a new tab in the dock's center region. Best effort: a
/// missing dock (before the window exists) drops the request.
pub fn open_center_panel<P>(cx: &mut App, panel: gpui::Entity<P>, window: &mut gpui::Window)
where
    P: gpui_kit::component::dock::Panel
        + gpui::Focusable
        + gpui::EventEmitter<gpui_kit::base::dock::PanelEvent>
        + gpui::Render
        + 'static,
{
    let Some(area) = cx.global_mut::<DockAreaGlobal>().0.clone() else {
        return;
    };
    let _ = area.update(cx, |area, cx| {
        use gpui_kit::component::dock::{DockPlacement, panel_handle};
        area.add_panel_view(
            panel_handle(panel),
            DockPlacement::Center,
            None,
            window,
            cx,
        );
    });
}

/// Removes a center panel (the panel's own Close button). Do not dispatch a
/// `ClosePanel` action for this: the action bubbles from the focused element
/// and never reaches the dock unless focus happens to be inside it.
pub fn close_center_panel<P>(cx: &mut App, panel: gpui::Entity<P>, window: &mut gpui::Window)
where
    P: gpui_kit::component::dock::Panel
        + gpui::Focusable
        + gpui::EventEmitter<gpui_kit::base::dock::PanelEvent>
        + gpui::Render
        + 'static,
{
    let Some(area) = cx.global_mut::<DockAreaGlobal>().0.clone() else {
        return;
    };
    let _ = area.update(cx, |area, cx| {
        area.remove_panel(panel, window, cx);
    });
}

pub(crate) fn register_workspace(
    cx: &mut App,
    panel_id: u64,
    workspace: WeakEntity<ArchiveWorkspace>,
) {
    cx.global_mut::<WorkspaceRegistry>().register(panel_id, workspace);
}

pub(crate) fn unregister_workspace(cx: &mut App, panel_id: u64) {
    cx.global_mut::<WorkspaceRegistry>().unregister(panel_id);
}
