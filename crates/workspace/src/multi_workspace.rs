use crate::archive_workspace::ArchiveWorkspace;
use crate::globals;
use crate::panels;
use crate::tab_bar;
use crate::take_pending_open;
use anyhow::Context as _;
use gpui::{div, px, App, AppContext, Context, Entity, EntityId, FocusHandle, Focusable, InteractiveElement, IntoElement, KeyBinding, ParentElement, PromptLevel, Render, StatefulInteractiveElement, Styled, Task, WeakEntity, Window};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dock::{ClosePanel, DockArea, DockAreaState, DockEvent, DockLayout, DockPlacement, DockSkin, InsertTarget, PaneRef, PanelStyle, ToggleZoom, panel_handle};
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{IconName, Root, Sizable, TitleBar, WindowExt};
use multi_workspace_core::{Adapter, Core, RemovalIntent};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

/// Bump when the default layout changes; stale saved layouts then offer a
/// reset instead of restoring something outdated.
const DOCK_AREA_VERSION: usize = 1;

const MAIN_DOCK_AREA: DockAreaTab = DockAreaTab {
    id: "main-dock",
    version: DOCK_AREA_VERSION,
};

#[cfg(debug_assertions)]
const STATE_FILE: &str = "target/dock-tabs.json";
#[cfg(not(debug_assertions))]
const STATE_FILE: &str = "dock-tabs.json";

/// Which region of the layout a panel kind belongs to. The rule is keyed by
/// `panel_name`, so it survives save/load round trips.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Zone {
    /// Files and projects — the center workspace.
    Workspace,
    /// Tool panels — the side and bottom docks.
    Tools,
}

fn zone_of(panel_name: &str) -> Option<Zone> {
    match panel_name {
        // All of these open as center tabs (open_center_panel); tools live in
        // the side/bottom docks. A mismatch here makes enforce_zones evict
        // the panel on the next LayoutChanged — it "never shows up".
        "explorer" | "diff" | "progress" | "devtools" | "settings" | "EditorPanel"
        | "WelcomePanel" => Some(Zone::Workspace),
        "FilesPanel" | "OutlinePanel" | "OutputPanel" => Some(Zone::Tools),
        _ => None,
    }
}

fn zone_of_placement(placement: DockPlacement) -> Option<Zone> {
    match placement {
        DockPlacement::Center => Some(Zone::Workspace),
        DockPlacement::Left | DockPlacement::Right | DockPlacement::Bottom => Some(Zone::Tools),
    }
}

/// A drop target inside the panel's home zone: its first tab group. A dock
/// can never be emptied by dragging — the last visible panel of a lone group
/// is not draggable — so the home zone always has a group to receive the
/// panel back.
fn home_target(area: &DockArea, home: Zone) -> Option<InsertTarget> {
    let placements: &[DockPlacement] = match home {
        Zone::Workspace => &[DockPlacement::Center],
        Zone::Tools => &[DockPlacement::Left, DockPlacement::Bottom],
    };
    for placement in placements {
        let Some(tree) = area.layout(*placement) else {
            continue;
        };
        for node_id in tree.node_ids() {
            let Some(node) = tree.find_node(node_id) else {
                continue;
            };
            if matches!(node.kind(), PaneRef::Tabs { .. }) {
                return Some(InsertTarget::Tabs {
                    node: node_id,
                    ix: None,
                    activate: true,
                });
            }
        }
    }
    None
}

/// Maps a workspace entity to its group key: the normalized archive path, or
/// the empty string for the home tab. Shared with the [`Core`] through
/// [`WorkspaceAdapter`], mirroring zed's "query the key off the entity" rule.
#[derive(Default)]
struct WorkspaceAdapter {
    keys: Rc<RefCell<HashMap<EntityId, String>>>,
}

impl Adapter for WorkspaceAdapter {
    type Workspace = EntityId;
    type Key = String;

    fn group_key(&self, workspace: &EntityId) -> String {
        self.keys
            .borrow()
            .get(workspace)
            .cloned()
            .unwrap_or_default()
    }

    fn is_available(&self, _workspace: &EntityId) -> bool {
        true
    }
}

/// One OS window holding any number of [`ArchiveWorkspace`] tabs. The
/// container state (which tabs exist, which is displayed, removal
/// bookkeeping) lives in a framework-agnostic [`Core`]; this struct owns the
/// entities, the dock, and the mapping between them.
pub struct MultiWorkspace {
    focus_handle: FocusHandle,
    dock_area: Entity<DockArea>,
    dock_skin: Rc<DockSkin>,
    core: Core<WorkspaceAdapter>,
    adapter: WorkspaceAdapter,
    workspaces: HashMap<EntityId, Entity<ArchiveWorkspace>>,
    last_layout_state: Option<DockAreaState>,
    _save_layout_task: Option<Task<()>>,
}

struct DockAreaTab {
    id: &'static str,
    version: usize,
}

impl MultiWorkspace {
    /// Registers panels, keybindings, and the archive globals. Call once at
    /// startup, before any window is opened.
    pub fn init(cx: &mut App) {
        globals::init(cx);
        panels::register_panels(cx);

        cx.bind_keys(vec![
            KeyBinding::new("shift-escape", ToggleZoom, None),
            KeyBinding::new("ctrl-w", ClosePanel, None),
            KeyBinding::new("ctrl-o", app_action::OpenArchiveDialog, None),
            KeyBinding::new("ctrl-alt-i", app_action::ToggleDevTools, None),
            KeyBinding::new("ctrl-,", app_action::ToggleSettings, None),
        ]);
    }

    fn on_open_archive_dialog(
        &mut self,
        _: &app_action::OpenArchiveDialog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_archive_dialog(window, cx);
    }

    /// Opens the developer tools panel. Debug builds only; the on_action
    /// handler receives the live window, so no stored handles are involved.
    #[cfg(debug_assertions)]
    fn on_toggle_dev_tools(
        &mut self,
        _: &app_action::ToggleDevTools,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = cx.new(|cx| crate::devtools_panel::DevToolsPanel::new(cx));
        globals::open_center_panel(cx, panel, window);
    }

    /// Opens the settings panel.
    fn on_toggle_settings(
        &mut self,
        _: &app_action::ToggleSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = cx.new(|cx| crate::settings_panel::SettingsPanel::new(window, cx));
        globals::open_center_panel(cx, panel, window);
    }

    /// Shows the native open dialog and opens the chosen archive.
    ///
    /// The dialog runs through rfd's *async* API on a helper thread: the
    /// synchronous variant re-enters the Win32 message loop while we hold
    /// the `App` borrow, and any timer task ticking inside that nested loop
    /// (e.g. a workspace's watcher poll) then hits "RefCell already
    /// borrowed" and crashes.
    pub fn open_archive_dialog(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn(async move |_, cx| {
            let picked = rfd::AsyncFileDialog::new()
                .set_title("Open archive")
                .add_filter(
                    "Archives",
                    &[
                        "7z", "zip", "rar", "tar", "gz", "tgz", "bz2", "xz", "zst", "wim", "cab",
                        "iso",
                    ],
                )
                .pick_file()
                .await;
            let Some(handle) = picked else {
                return;
            };
            let path = handle.path().to_path_buf();
            let Some((host, window)) = cx.update(|cx| match (globals::host(cx), globals::host_window(cx)) {
                (Some(host), Some(window)) => Some((host, window)),
                _ => None,
            }) else {
                return;
            };
            let _ = window.update(cx, |_, window, cx| {
                let _ = host.update(cx, |host, cx| host.open_archive(path, window, cx, true));
            });
        })
        .detach();
    }

    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Build the area with the application-owned tab bar: the custom
        // renderer wraps the built-in DockSkin and only takes over the tab
        // bar, adding a close button and a right-click menu to every tab.
        let mut skin = None;
        let dock_area = cx.new(|cx| {
            let built = DockSkin::new(cx);
            let renderer = tab_bar::TabBarSkin::new(built.clone());
            skin = Some(built);
            DockArea::new(MAIN_DOCK_AREA.id, Some(MAIN_DOCK_AREA.version), window, cx).with_renderer(renderer)
        });
        let skin = skin.expect("DockSkin::new ran inside the constructor");
        // The custom tab bar always draws real tabs; keep the stock style in
        // sync for the one case still delegated to it: the collapsed strip.
        skin.set_panel_style(PanelStyle::TabBar, cx);
        let weak_dock_area = dock_area.downgrade();
        globals::set_dock_area(cx, weak_dock_area.clone());

        match Self::load_layout(dock_area.clone(), window, cx) {
            Ok(()) => {}
            Err(err) => {
                eprintln!("load layout error: {err:?}");
                Self::reset_default_layout(weak_dock_area.clone(), window, cx);
            }
        }

        // The center holds live workspaces, which are session state — never
        // restored from disk. Whatever the saved layout put there is replaced
        // by a fresh home tab; tool docks stay as restored.
        let keys = Rc::new(RefCell::new(HashMap::new()));
        let home = cx.new(|cx| ArchiveWorkspace::home(cx));
        let home_id = home.entity_id();
        keys.borrow_mut().insert(home_id, String::new());
        globals::register_workspace(cx, home_id.as_u64(), home.downgrade());
        let window_handle = window.window_handle();
        home.update(cx, |home, _| home.set_window(window_handle));
        dock_area.update(cx, |area, cx| {
            let center = DockLayout::tabs()
                .panel_view(panel_handle(home.clone()), cx)
                .active_index(0);
            area.set_center(center, window, cx);
        });

        cx.subscribe_in(
            &dock_area,
            window,
            |this, dock_area, ev: &DockEvent, window, cx| match ev {
                DockEvent::LayoutChanged => {
                    // Drop bookkeeping for panels the dock already removed.
                    this.prune(window, cx);
                    // Refuse cross-zone moves before persisting the layout.
                    this.enforce_zones(window, cx);
                    this.save_layout(dock_area, window, cx);
                }
                DockEvent::DragDrop { .. } => {}
            },
        )
            .detach();

        cx.on_app_quit({
            let dock_area = dock_area.clone();
            move |_, cx| {
                let state = dock_area.read(cx).dump(cx);
                cx.background_executor().spawn(async move {
                    let _ = Self::save_state(&state);
                })
            }
        })
            .detach();

        let adapter = WorkspaceAdapter { keys: keys.clone() };
        let workspaces = HashMap::from([(home_id, home)]);
        let core = Core::new(home_id, true);
        let focus_handle = cx.focus_handle();
        let mut this = Self {
            focus_handle: focus_handle.clone(),
            dock_area,
            dock_skin: skin,
            core,
            adapter,
            workspaces,
            last_layout_state: None,
            _save_layout_task: None,
        };

        // Actions (Ctrl+O, tab close, …) bubble from the focused element.
        // Seeding the window's focus with the root handle keeps the action
        // handlers on the root div reachable before any panel takes focus.
        window.focus(&focus_handle, cx);
        let self_entity = cx.entity();
        globals::set_host(cx, self_entity.downgrade());
        globals::set_host_window(cx, window.window_handle());

        // Archives requested on the command line open into tabs now.
        for path in take_pending_open(cx) {
            this.open_archive(path, window, cx, false);
        }
        this
    }

    /// Opens an archive into a new tab, or activates the tab that already
    /// holds it. Duplicate detection keys on the lowercased canonical path.
    /// `interactive` reports whether the user just asked for this (dialogs
    /// allowed) or it came from the startup arguments.
    pub fn open_archive(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
        interactive: bool,
    ) {
        let canonical = dunce::canonicalize(&path).unwrap_or_else(|_| path.clone());
        let key = canonical.to_string_lossy().to_lowercase();

        let existing = self
            .adapter
            .keys
            .borrow()
            .iter()
            .find_map(|(id, existing_key)| (existing_key == &key).then_some(*id));
        if let Some(existing) = existing {
            self.core.activate(existing, None, &self.adapter);
            window.push_notification(
                format!("Already open: {}", canonical.display()),
                cx,
            );
            cx.notify();
            return;
        }

        let entity = ArchiveWorkspace::open_archive(canonical, window, cx, interactive);
        let id = entity.entity_id();
        self.adapter.keys.borrow_mut().insert(id, key);
        globals::register_workspace(cx, id.as_u64(), entity.downgrade());
        self.workspaces.insert(id, entity.clone());
        self.dock_area.update(cx, |area, cx| {
            // Hand the dock the presentation handle, so the tab draws the
            // panel's `title` (archive name + dirty dot) instead of the
            // bare `panel_name`.
            area.add_panel_view(
                panel_handle(entity),
                DockPlacement::Center,
                None,
                window,
                cx,
            );
        });
        self.core.activate(id, None, &self.adapter);
        cx.notify();
    }

    /// Drains container events; nothing downstream consumes them yet, but the
    /// drain is what marks state as changed for the render cache.
    fn after_mutation(&mut self, cx: &mut Context<Self>) {
        let _events = self.core.take_events();
        cx.notify();
    }

    /// Forgets workspaces whose panel the dock removed (tab close). Their
    /// sessions are dropped — deleting the temp work dir — and the container
    /// is repaired with the same three-phase removal the core prescribes.
    fn prune(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let live: std::collections::HashSet<u64> = self
            .dock_area
            .read(cx)
            .layout(DockPlacement::Center)
            .map(|tree| {
                tree.panels()
                    .map(|panel| panel.as_u64())
                    .collect()
            })
            .unwrap_or_default();

        let dead: Vec<EntityId> = self
            .workspaces
            .keys()
            .filter(|id| !live.contains(&(*id).as_u64()))
            .copied()
            .collect();
        if dead.is_empty() {
            return;
        }

        for target in dead {
            if let Some(entity) = self.workspaces.get(&target) {
                entity.update(cx, |workspace, cx| workspace.cleanup(cx));
            }
            self.workspaces.remove(&target);
            self.adapter.keys.borrow_mut().remove(&target);
            globals::unregister_workspace(cx, target.as_u64());

            // Any surviving unclosable home tab serves as the replacement;
            // it always exists because the home tab can never be closed.
            let spare = self
                .workspaces
                .iter()
                .find(|(id, entity)| {
                    entity.read(cx).session().is_none() && **id != target
                })
                .map(|(id, _)| *id);
            let mut core = std::mem::replace(
                &mut self.core,
                Core::new(spare.unwrap_or(target), true),
            );
            let Some(session) =
                core.begin_removal(vec![target], RemovalIntent::CloseProject, &self.adapter)
            else {
                self.core = core;
                continue;
            };
            let outcome =
                session.commit(&mut core, &[true], &self.adapter, || {
                    spare.expect("a home workspace exists as removal fallback")
                });
            self.core = core;
            let _ = outcome;
        }
        self.after_mutation(cx);
    }

    fn save_layout(
        &mut self,
        dock_area: &Entity<DockArea>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dock_area = dock_area.clone();
        self._save_layout_task = Some(cx.spawn_in(window, async move |workspace, window| {
            window
                .background_executor()
                .timer(Duration::from_secs(10))
                .await;

            _ = workspace.update_in(window, move |this, _, cx| {
                let dock_area = dock_area.read(cx);
                let state = dock_area.dump(cx);

                let last_layout_state = this.last_layout_state.clone();
                if Some(&state) == last_layout_state.as_ref() {
                    return;
                }

                if let Err(err) = Self::save_state(&state) {
                    eprintln!("save layout error: {err}");
                }
                this.last_layout_state = Some(state);
            });
        }));
    }

    /// Roll back any panel sitting in a zone its kind does not belong to.
    /// Runs on every `LayoutChanged`; a legal layout makes it a no-op, so the
    /// revert's own `LayoutChanged` event does not loop.
    fn enforce_zones(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let misplaced = self.dock_area.update(cx, |area, cx| {
            let mut misplaced = Vec::new();
            for placement in [
                DockPlacement::Center,
                DockPlacement::Left,
                DockPlacement::Bottom,
            ] {
                let Some(zone) = zone_of_placement(placement) else {
                    continue;
                };
                let Some(tree) = area.layout(placement) else {
                    continue;
                };
                for panel_id in tree.panels() {
                    let home = area
                        .panel(panel_id)
                        .map(|panel| panel.panel_name(cx))
                        .and_then(zone_of);
                    if let Some(home) = home.filter(|home| *home != zone) {
                        misplaced.push((panel_id, home));
                    }
                }
            }
            misplaced
        });

        if misplaced.is_empty() {
            return;
        }

        let mut reverted: Vec<&'static str> = Vec::new();
        self.dock_area.update(cx, |area, cx| {
            for (panel_id, home) in misplaced {
                let name = area
                    .panel(panel_id)
                    .map(|panel| panel.panel_name(cx))
                    .unwrap_or("");
                let Some(target) = home_target(area, home) else {
                    continue;
                };
                area.move_panel(panel_id, target, window, cx);
                println!("zone violation: {name} sent back to its home zone");
                reverted.push(name);
            }
        });

        if !reverted.is_empty() {
            window.push_notification(
                format!("{} cannot live in this zone", reverted.join(", ")),
                cx,
            );
        }
    }

    fn save_state(state: &DockAreaState) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(state)?;
        std::fs::write(STATE_FILE, json)?;
        Ok(())
    }

    fn load_layout(
        dock_area: Entity<DockArea>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let json = std::fs::read_to_string(STATE_FILE)?;
        let state = serde_json::from_str::<DockAreaState>(&json)?;

        // Bump DOCK_AREA_VERSION when the default layout changes; offer the
        // user a reset instead of silently restoring an outdated layout.
        if state.version != Some(DOCK_AREA_VERSION) {
            let answer = window.prompt(
                PromptLevel::Info,
                "The default layout has been updated.\n\
                Do you want to reset the layout to default?",
                None,
                &["Yes", "No"],
                cx,
            );

            let weak_dock_area = dock_area.downgrade();
            cx.spawn_in(window, async move |this, window| {
                if answer.await == Ok(0) {
                    _ = this.update_in(window, |_, window, cx| {
                        Self::reset_default_layout(weak_dock_area, window, cx);
                    });
                }
            })
                .detach();
        }

        dock_area.update(cx, |dock_area, cx| {
            dock_area.load(state, window, cx).context("load layout")?;
            for placement in [DockPlacement::Left, DockPlacement::Bottom] {
                dock_area.set_dock_collapsible(placement, true, window, cx);
            }

            Ok::<(), anyhow::Error>(())
        })
    }

    fn reset_default_layout(dock_area: WeakEntity<DockArea>, window: &mut Window, cx: &mut App) {
        // Panels go in through `panel_handle` — that carries the presentation
        // handle, so tabs draw the panel's `title` and not its `panel_name`.
        let center = DockLayout::tabs()
            .panel_view(panel_handle(cx.new(|cx| ArchiveWorkspace::home(cx))), cx)
            .active_index(0);

        // The left dock holds its own tab group.
        let left = DockLayout::tabs()
            .panel_view(panel_handle(panels::SidebarPanel::files(cx)), cx)
            .panel_view(panel_handle(panels::SidebarPanel::outline(cx)), cx);

        let bottom = DockLayout::tabs().panel_view(panel_handle(panels::SidebarPanel::output(cx)), cx);

        _ = dock_area.update(cx, |view, cx| {
            view.set_center(center, window, cx);
            for (placement, layout, size) in [
                (DockPlacement::Left, left, px(240.)),
                (DockPlacement::Bottom, bottom, px(180.)),
            ] {
                view.set_dock(placement, layout, window, cx);
                view.set_dock_size(placement, size, window, cx);
            }

            let _ = Self::save_state(&view.dump(cx));
        });
    }
}

impl Focusable for MultiWorkspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for MultiWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sheet_layer = Root::render_sheet_layer(window, cx);
        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);

        div()
            .id("story-workspace")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_open_archive_dialog))
            .on_action(cx.listener(Self::on_toggle_dev_tools))
            .on_action(cx.listener(Self::on_toggle_settings))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .child(TitleBar::new())
            .child(div().flex_1().min_h_0().child(self.dock_area.clone()))
            .child(
                StatusBar::new()
                    .left(
                        Button::new("toggle-left-dock")
                            .ghost()
                            .xsmall()
                            .icon(IconName::PanelLeft)
                            .tooltip("Toggle Left Dock")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dock_area.update(cx, |area, cx| {
                                    area.toggle_dock(DockPlacement::Left, window, cx);
                                });
                            })),
                    )
                    .left(
                        Button::new("toggle-bottom-dock")
                            .ghost()
                            .xsmall()
                            .icon(IconName::PanelBottom)
                            .tooltip("Toggle Bottom Dock")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dock_area.update(cx, |area, cx| {
                                    area.toggle_dock(DockPlacement::Bottom, window, cx);
                                });
                            })),
                    )
                    .child(
                        Button::new("toggle-right-dock")
                            .ghost()
                            .xsmall()
                            .icon(IconName::PanelRight)
                            .tooltip("Toggle Right Dock")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dock_area.update(cx, |area, cx| {
                                    area.toggle_dock(DockPlacement::Right, window, cx);
                                });
                            })),
                    ),
            )
            .children(sheet_layer)
            .children(dialog_layer)
            .children(notification_layer)
    }
}

// The chrome-arbitration cell is part of the core's contract; kept within
// reach for the panels that will ask whether they own the window chrome.
#[allow(dead_code)]
fn _chrome_contract(core: &Core<WorkspaceAdapter>) -> Rc<Cell<Option<EntityId>>> {
    core.active_cell()
}
