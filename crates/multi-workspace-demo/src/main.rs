//! GPUI demo for `multi-workspace-core`.
//!
//! One window hosts several fake "workspaces" (colored panels). A sidebar
//! lists project groups; members are derived from pinned rows exactly like in
//! zed's MultiWorkspace. Demonstrates:
//!
//! - hold -> pin -> activate lifecycle (navigate-away promotes transients)
//! - groups as pure display metadata, rekeyed on rename (not shown here)
//! - the shared `Rc<Cell>` chrome-ownership cell: every panel prints whether
//!   it currently owns the platform window title
//! - three-phase removal with a workspace factory fallback
//!
//! Run: cargo run

use gpui::{
    actions, div, prelude::*, px, rgb, size, App, Bounds, Context, Div, Entity, EntityId,
    KeyBinding, Render, SharedString, Stateful, Window, WindowBounds, WindowOptions,
};
use gpui_platform::application;
use multi_workspace_core::{Adapter, Core, RemovalIntent};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

actions!(
    demo,
    [NextProject, PrevProject, ToggleSidebar, RemoveActive, Quit]
);

const PROJECTS: [&str; 3] = ["alpha", "beta", "gamma"];
const PROJECT_COLORS: [u32; 6] = [0x2563eb, 0x059669, 0xd97706, 0xdb2777, 0x7c3aed, 0xdc2626];

// ---------------------------------------------------------------------------
// Adapter: how the core queries live state from GPUI entities.
//
// The core stores only `EntityId` handles and asks the adapter for group keys,
// mirroring zed reading `project_group_key(cx)` straight off the entity. Here
// a registry stands in for the real Project.
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct DemoAdapter {
    info: Rc<RefCell<HashMap<EntityId, (String, String)>>>,
}

impl DemoAdapter {
    fn register(&self, id: EntityId, key: String, label: String) {
        self.info.borrow_mut().insert(id, (key, label));
    }

    fn label(&self, id: EntityId) -> String {
        self.info
            .borrow()
            .get(&id)
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| "<unknown>".into())
    }
}

impl Adapter for DemoAdapter {
    type Workspace = EntityId;
    type Key = String;

    fn group_key(&self, workspace: &EntityId) -> Self::Key {
        self.info
            .borrow()
            .get(workspace)
            .map(|(key, _)| key.clone())
            .unwrap_or_else(|| format!("orphan-{}", workspace.as_u64()))
    }

    fn is_available(&self, _workspace: &EntityId) -> bool {
        true
    }

    /// "empty" workspaces created by the removal factory are not reopenable
    /// projects.
    fn allows_reopen(&self, key: &String) -> bool {
        key != "empty"
    }
}

// ---------------------------------------------------------------------------
// WorkspaceView: the content of one held workspace.
//
// It learns about chrome ownership through the shared cell instead of reading
// its parent — the same trick zed uses to dodge GPUI's single-writer rule.
// ---------------------------------------------------------------------------

struct WorkspaceView {
    #[allow(dead_code)]
    id: EntityId,
    label: String,
    color: u32,
    active_cell: Rc<Cell<Option<EntityId>>>,
}

impl WorkspaceView {
    fn new(
        label: String,
        color: u32,
        active_cell: Rc<Cell<Option<EntityId>>>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            id: cx.entity_id(),
            label,
            color,
            active_cell,
        }
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let owns_chrome = self.active_cell.get() == Some(self.id);
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .bg(rgb(self.color))
            .child(div().text_xl().child(self.label.clone()))
            .child(
                div()
                    .text_sm()
                    .opacity(0.8)
                    .child(format!("owns window chrome: {owns_chrome}")),
            )
    }
}

// ---------------------------------------------------------------------------
// Host: the window root. Owns the core plus the strong entity references.
// ---------------------------------------------------------------------------

struct Host {
    core: Core<DemoAdapter>,
    adapter: DemoAdapter,
    workspaces: Vec<Entity<WorkspaceView>>,
    sidebar_open: bool,
    counter: usize,
    log: String,
}

impl Host {
    fn new(cx: &mut Context<Self>) -> Self {
        let adapter = DemoAdapter::default();
        let first = cx.new(|cx| {
            WorkspaceView::new(
                "alpha#1".into(),
                PROJECT_COLORS[0],
                Rc::new(Cell::new(None)),
                cx,
            )
        });
        adapter.register(first.entity_id(), "alpha".into(), "alpha#1".into());

        let core = Core::new(first.entity_id(), true);

        Self {
            core,
            adapter,
            workspaces: vec![first],
            sidebar_open: true,
            counter: 1,
            log: "started with alpha#1".into(),
        }
    }

    /// find-or-create by live group key, then activate (zed:
    /// `workspace_for_paths` + `find_or_create_workspace`).
    fn open_project(&mut self, project: &'static str, cx: &mut Context<Self>) {
        if let Some(existing) = self.workspaces.iter().find_map(|w| {
            let id = w.read(cx).id;
            (self.adapter.group_key(&id) == project).then_some(id)
        }) {
            self.core.activate(existing, None, &self.adapter);
            self.log = format!("activated existing {project}");
            return;
        }

        self.counter += 1;
        let label = format!("{project}#{}", self.counter);
        let color = PROJECT_COLORS[self.counter % PROJECT_COLORS.len()];
        let entity =
            cx.new(|cx| WorkspaceView::new(label.clone(), color, Rc::new(Cell::new(None)), cx));
        self.adapter.register(entity.entity_id(), project.into(), label);
        self.workspaces.push(entity);
        self.core
            .activate(self.workspaces.last().unwrap().read(cx).id, None, &self.adapter);
        self.log = format!("created + activated {project}#{}", self.counter);
    }

    fn run_removal(&mut self, targets: Vec<EntityId>, intent: RemovalIntent, cx: &mut Context<Self>) {
        // Phase 1: freeze the neighborhood; nothing is mutated yet. A real
        // app would show save prompts here between begin and commit.
        let Some(session) = self.core.begin_removal(targets.clone(), intent, &self.adapter)
        else {
            return;
        };

        let created: Rc<RefCell<Option<Entity<WorkspaceView>>>> = Default::default();
        let outcome = {
            let created = created.clone();
            let adapter = self.adapter.clone();
            let counter = self.counter;
            let cx = &mut *cx;
            session.commit(&mut self.core, &[true], &self.adapter, move || {
                // Replacement factory: an empty workspace, only invoked when
                // no same-group or neighbor candidate exists.
                let label = format!("empty#{}", counter + 100);
                let entity = cx.new(|cx| {
                    WorkspaceView::new(label.clone(), 0x3f3f46, Rc::new(Cell::new(None)), cx)
                });
                adapter.register(entity.entity_id(), "empty".into(), label);
                *created.borrow_mut() = Some(entity.clone());
                entity.entity_id()
            })
        };

        if let Some(entity) = created.borrow_mut().take() {
            self.workspaces.push(entity);
        }

        if outcome.aborted {
            self.log = "removal aborted by consent".into();
        } else {
            // Drop strong references to removed entities so they are freed.
            self.workspaces
                .retain(|w| self.core.held_index(&w.read(cx).id).is_some());
            self.log = format!(
                "removed {} workspace(s); reopen={:?}",
                targets.len(),
                outcome.reopen_key,
            );
        }
    }

    fn cycle_group(&mut self, forward: bool) {
        let keys: Vec<String> = self.core.groups().iter().map(|g| g.key.clone()).collect();
        if keys.is_empty() {
            return;
        }
        let current = self.adapter.group_key(&self.core.displayed());
        let index = keys.iter().position(|key| *key == current).unwrap_or(0);
        let step = if forward { 1 } else { keys.len() - 1 };
        let next = keys[(index + step) % keys.len()].clone();

        let target = self
            .core
            .last_active_for_group(&next, &self.adapter)
            .or_else(|| {
                self.core
                    .members_of_group(&next, &self.adapter)
                    .first()
                    .copied()
            });
        if let Some(target) = target {
            self.core.activate(target, None, &self.adapter);
            self.log = format!("cycled to group '{next}'");
        }
    }

    /// Drain core events, sync the platform window title to the active
    /// workspace (chrome ownership), and refresh every child's view of the
    /// shared cell.
    fn after_mutation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let events = self.core.take_events();
        if let Some(last) = events.last() {
            match last {
                multi_workspace_core::Event::ActiveWorkspaceChanged { .. } => {
                    self.log.push_str(" [active changed]");
                }
                multi_workspace_core::Event::WorkspaceAdded(_) => {
                    self.log.push_str(" [added]");
                }
                multi_workspace_core::Event::WorkspaceRemoved(_) => {
                    self.log.push_str(" [removed]");
                }
                multi_workspace_core::Event::GroupsChanged => {
                    self.log.push_str(" [groups]");
                }
            }
        }

        let active = self.core.displayed();
        window.set_window_title(&format!("MW Demo — {}", self.adapter.label(active)));

        let cell = self.core.active_cell();
        for workspace in &self.workspaces {
            workspace.update(cx, |view, cx| {
                view.active_cell = cell.clone();
                cx.notify();
            });
        }
        cx.notify();
    }

    fn sidebar_row(&self, id: String, label: String, active: bool) -> Stateful<Div> {
        div()
            .id(SharedString::from(id))
            .flex()
            .items_center()
            .justify_between()
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .text_sm()
            .when(active, |el| el.bg(rgb(0x365a96)))
            .when(!active, |el| {
                el.hover(|style| style.bg(rgb(0x24293a)))
            })
            .child(label)
    }

    fn button(id: &str, label: &str) -> Stateful<Div> {
        div()
            .id(SharedString::from(id.to_string()))
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(0x2a2f3a))
            .hover(|style| style.bg(rgb(0x3a4150)))
            .cursor_pointer()
            .text_sm()
            .child(label.to_string())
    }
}

impl Render for Host {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let displayed = self.core.displayed();

        // Toolbar ----------------------------------------------------------
        let toolbar = div()
            .flex()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(rgb(0x2a2f3a))
            .children(PROJECTS.map(|project| {
                Self::button(project, &format!("+ {project}")).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.open_project(project, cx);
                        this.after_mutation(window, cx);
                    },
                ))
            }))
            .child(
                Self::button("remove", "× remove current").on_click(cx.listener(
                    |this, _, window, cx| {
                        let target = this.core.displayed();
                        this.run_removal(vec![target], RemovalIntent::KeepProject, cx);
                        this.after_mutation(window, cx);
                    },
                )),
            )
            .child(
                Self::button("sidebar", "toggle sidebar").on_click(cx.listener(
                    |this, _, window, cx| {
                        this.sidebar_open = !this.sidebar_open;
                        this.after_mutation(window, cx);
                    },
                )),
            );

        // Sidebar ----------------------------------------------------------
        let mut sidebar_rows = Vec::new();
        for (group_index, group) in self.core.groups().iter().enumerate() {
            let members = self.core.members_of_group(&group.key, &self.adapter);
            let chevron = if group.expanded { "▾" } else { "▸" };

            sidebar_rows.push(
                div()
                    .id(SharedString::from(format!("group-{group_index}")))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .text_sm()
                    .text_color(rgb(0x9ca3af))
                    .hover(|style| style.bg(rgb(0x24293a)))
                    .on_click({
                        let key = group.key.clone();
                        cx.listener(move |this, _, window, cx| {
                            let expanded = this
                                .core
                                .groups()
                                .iter()
                                .find(|g| g.key == key)
                                .is_some_and(|g| g.expanded);
                            this.core.set_expanded(&key, !expanded);
                            this.after_mutation(window, cx);
                        })
                    })
                    .child(format!("{chevron} {}", group.key))
                    .child(format!("{}", members.len())),
            );

            if group.expanded {
                for member in members {
                    let active = member == displayed;
                    let label = self.adapter.label(member);
                    let row_id = format!("ws-{}-{}", group_index, member.as_u64());

                    sidebar_rows.push(
                        self.sidebar_row(row_id.clone(), label.clone(), active)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.core.activate(member, None, &this.adapter);
                                this.log = format!("activated {label}");
                                this.after_mutation(window, cx);
                            }))
                            // .hover(|style| style.bg(rgb(0x24293a)))
                            .child(
                                div()
                                    .id(SharedString::from(format!("{row_id}-close")))
                                    .px_1()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .text_color(rgb(0x9ca3af))
                                    .hover(|style| style.bg(rgb(0x7f1d1d)))
                                    .child("✕")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.run_removal(
                                            vec![member],
                                            RemovalIntent::KeepProject,
                                            cx,
                                        );
                                        this.after_mutation(window, cx);
                                    })),
                            ),
                    );
                }
            }
        }

        let sidebar = div()
            .flex()
            .flex_col()
            .gap_px()
            .w_56()
            .flex_shrink_0()
            .h_full()
            .p_2()
            .border_r_1()
            .border_color(rgb(0x2a2f3a))
            .children(sidebar_rows);

        // Active panel -------------------------------------------------------
        let active_view = self
            .workspaces
            .iter()
            .find(|w| w.read(cx).id == displayed)
            .cloned();

        let status = div()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_1()
            .text_xs()
            .text_color(rgb(0x9ca3af))
            .border_t_1()
            .border_color(rgb(0x2a2f3a))
            .child(format!(
                "displayed={} retained={} groups={}",
                self.adapter.label(displayed),
                self.workspaces.len(),
                self.core.groups().len(),
            ))
            .child(self.log.clone());

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x111318))
            .text_color(rgb(0xe5e7eb))
            .on_action(cx.listener(|this, _: &NextProject, window, cx| {
                this.cycle_group(true);
                this.after_mutation(window, cx);
            }))
            .on_action(cx.listener(|this, _: &PrevProject, window, cx| {
                this.cycle_group(false);
                this.after_mutation(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, window, cx| {
                this.sidebar_open = !this.sidebar_open;
                this.after_mutation(window, cx);
            }))
            .on_action(cx.listener(|this, _: &RemoveActive, window, cx| {
                let target = this.core.displayed();
                this.run_removal(vec![target], RemovalIntent::KeepProject, cx);
                this.after_mutation(window, cx);
            }))
            .child(toolbar)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .when(self.sidebar_open, |el| el.child(sidebar))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .when_some(active_view, |el, view| el.child(view)),
                    ),
            )
            .child(status)
    }
}

fn main() {
    application().run(|cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("ctrl-tab", NextProject, None),
            KeyBinding::new("ctrl-shift-tab", PrevProject, None),
            KeyBinding::new("ctrl-b", ToggleSidebar, None),
            KeyBinding::new("ctrl-r", RemoveActive, None),
            KeyBinding::new("cmd-q", Quit, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());

        let bounds = Bounds::centered(None, size(px(1000.), px(700.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(Host::new),
        )
            .unwrap();

        cx.activate(true);
    });
}
