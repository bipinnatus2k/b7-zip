//! The application-owned tab bar: every dock tab gets a close button and a
//! right-click menu (Close / Close Others / Close All). The renderer wraps
//! the built-in `DockSkin` — dock chrome, splits, tiles, and drop indicators
//! stay on the stock implementation; only `render_tab_bar` is owned here.

use gpui_kit::base::ResizeHandleContext;
use gpui_kit::component::{
    ActiveTheme as _, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    dock::{
        BasePanelView, DockAreaRenderer, DockContext, DockSkin, DragPanel, DropIndicator, NodeId,
        PanelHandle, PanelId, PanelState, TabGroupContext, TabGroupRenderer, TilesRenderer,
    },
    h_flex,
    menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem},
    tab::Tab,
};
use gpui_kit::gpui::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::{rc::Rc, sync::Arc};

/// The size the drag preview reports, so drop placeholders know where to fly
/// in from. Matches the built-in tab bar.
const DRAG_PREVIEW_SIZE: Size<Pixels> = size(px(96.), px(30.));

/// The area-level renderer: DockSkin for everything except the tab bar.
pub(crate) struct TabBarSkin {
    default: Rc<DockSkin>,
}

impl TabBarSkin {
    pub(crate) fn new(default: Rc<DockSkin>) -> Rc<Self> {
        Rc::new(Self { default })
    }
}

impl DockAreaRenderer for TabBarSkin {
    fn frame(&self, window: &mut Window, cx: &mut App) -> Stateful<Div> {
        self.default.frame(window, cx)
    }

    fn split_frame(
        &self,
        node: NodeId,
        axis: Axis,
        window: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div> {
        self.default.split_frame(node, axis, window, cx)
    }

    fn center_frame(&self, window: &mut Window, cx: &mut App) -> Stateful<Div> {
        self.default.center_frame(window, cx)
    }

    fn render_split_handle(
        &self,
        handle: &ResizeHandleContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        self.default.render_split_handle(handle, window, cx)
    }

    fn render_dock(
        &self,
        dock: &DockContext,
        content: AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        self.default.render_dock(dock, content, window, cx)
    }

    fn build_placeholder(
        &self,
        state: &PanelState,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Arc<dyn BasePanelView>> {
        self.default.build_placeholder(state, window, cx)
    }

    fn tab_group_renderer(&self) -> Rc<dyn TabGroupRenderer> {
        Rc::new(TabBarGroupRenderer {
            default: self.default.tab_group_renderer(),
        })
    }

    fn tiles_renderer(&self) -> Rc<dyn TilesRenderer> {
        self.default.tiles_renderer()
    }
}

/// The group-level renderer: the built-in one for everything except the tab
/// bar, which this example draws itself.
struct TabBarGroupRenderer {
    default: Rc<dyn TabGroupRenderer>,
}

impl TabGroupRenderer for TabBarGroupRenderer {
    fn frame(&self, group: &TabGroupContext, window: &mut Window, cx: &mut App) -> Stateful<Div> {
        self.default.frame(group, window, cx)
    }

    fn content_frame(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div> {
        self.default.content_frame(group, window, cx)
    }

    fn render_active_panel(
        &self,
        panel: AnyView,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        self.default.render_active_panel(panel, group, window, cx)
    }

    fn render_drop_indicator(
        &self,
        indicator: DropIndicator,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        self.default.render_drop_indicator(indicator, window, cx)
    }

    fn render_empty(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        self.default.render_empty(group, window, cx)
    }

    fn render_tab_bar(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        // A collapsed strip (a dock folded down to its tabs) keeps the
        // built-in rendering: it is a way back into the dock, not a working
        // tab bar.
        if group.is_collapsed() {
            return self.default.render_tab_bar(group, window, cx);
        }

        let closable = group.is_closable();
        let droppable = group.is_droppable();
        let draggable = group.is_draggable();
        let node = group.node();
        let tabs_count = group.panels().len();

        // Collect visible panels first: the filter borrows `cx`, and the
        // tab builder below needs `&mut App` for the panels' own titles.
        let visible: Vec<(usize, Arc<dyn BasePanelView>)> = group
            .panels()
            .iter()
            .enumerate()
            .filter(|(_, panel)| panel.visible(cx))
            .map(|(ix, panel)| (ix, panel.clone()))
            .collect();

        let tabs: Vec<AnyElement> = visible
            .into_iter()
            .map(|(ix, panel)| -> AnyElement {
                let panel_id = panel.panel_id(cx);
                let panel_name = panel.panel_name(cx);
                let handle = PanelHandle::of(&panel);
                let drag = if draggable {
                    group.drag_panel(ix, cx)
                } else {
                    None
                };
                let tab_closable = closable && panel.closable(cx);
                let group = group.clone();

                let tab = Tab::new()
                    .when_some(
                        handle.and_then(|handle| handle.title_style(cx)),
                        |tab, style| tab.bg(style.background).text_color(style.foreground),
                    )
                    .child(tab_title(&panel, window, cx))
                    .selected(ix == group.active_ix())
                    .on_click({
                        let group = group.clone();
                        move |_, window, cx| group.select_tab(ix, window, cx)
                    })
                    .when_some(drag, |tab, drag| {
                        tab.on_drag(drag, move |drag, offset, _, cx| {
                            cx.stop_propagation();
                            drag.set_drag_offset(offset);
                            drag.set_preview_size(DRAG_PREVIEW_SIZE);
                            let preview = TabDragPreview::new(&panel, cx);
                            cx.new(|_| preview)
                        })
                    })
                    .when(droppable, |tab| {
                        tab.drag_over::<DragPanel>(|tab, _, _, cx| {
                            tab.border_color(cx.theme().drag_border)
                        })
                        .on_drop({
                            let group = group.clone();
                            move |drag: &DragPanel, window, cx| {
                                group.drop_panel(drag.clone(), Some(ix), true, window, cx);
                            }
                        })
                    })
                    .when(tab_closable, |tab| {
                        tab.suffix(
                            // Occlude, so a click on the button does not also
                            // select the tab under it.
                            h_flex().occlude().child(
                                Button::new(("close-tab", ix as u64))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Close)
                                    .on_click({
                                        let group = group.clone();
                                        move |_, window, cx| {
                                            if needs_close_confirm(panel_name) {
                                                confirm_close(&group, vec![panel_id], window, cx);
                                            } else {
                                                group.close(panel_id, window, cx);
                                            }
                                        }
                                    }),
                            ),
                        )
                    });

                // The right-click menu rides on a wrapper: `ContextMenu`
                // needs an element that is stateful, a parent, and styled,
                // and `Stateful<Tab>` is not one.
                div()
                    .id(("dock-tab", ix as u64))
                    .context_menu({
                        let group = group.clone();
                        move |menu, _, cx| {
                            tab_menu(menu, &group, ix, panel_id, panel_name, tab_closable, cx)
                        }
                    })
                    .child(tab)
                    .into_any_element()
            })
            .collect();

        let active = group.active_panel();
        let active_panel = active
            .as_ref()
            .and_then(|panel| PanelHandle::of(panel).cloned());
        let zoomable = active.as_ref().is_some_and(|panel| panel.zoomable(cx));
        let zoomed = group.is_zoomed();

        // The bar's own buttons: the active panel's `toolbar_buttons` first,
        // then a zoom toggle for the group. The stock ellipsis menu is gone —
        // with the bar owned by the app, what sits here is entirely an app
        // decision.
        let controls = h_flex()
            .flex_shrink_0()
            .gap_1()
            .pr_1()
            .when_some(
                active_panel
                    .as_ref()
                    .and_then(|handle| handle.toolbar_buttons(window, cx)),
                |this, buttons| {
                    this.children(
                        buttons
                            .into_iter()
                            .map(|button| button.xsmall().ghost().tab_stop(false)),
                    )
                },
            )
            .when(zoomable, |this| {
                this.child(
                    Button::new("zoom-group")
                        .ghost()
                        .xsmall()
                        .icon(if zoomed {
                            IconName::Minimize
                        } else {
                            IconName::Maximize
                        })
                        .tooltip("Zoom")
                        .selected(zoomed)
                        .on_click({
                            let group = group.clone();
                            move |_, window, cx| group.toggle_zoom(window, cx)
                        }),
                )
            });

        // The bar is an application-drawn row — not the built-in `TabBar`,
        // whose children must stay plain `Tab`s — so each tab is free to be
        // a right-click `ContextMenu` wrapper.
        let bar = h_flex()
            .id("app-tab-bar")
            .w_full()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().tokens.tab_bar)
            .children(tabs)
            .when(droppable, |bar| {
                bar.child(
                    div()
                        .id("tab-bar-empty-space")
                        .h_full()
                        .flex_grow_1()
                        .min_w_16()
                        .drag_over::<DragPanel>(|this, _, _, cx| {
                            this.bg(cx.theme().tokens.drop_target)
                        })
                        .on_drop({
                            let group = group.clone();
                            move |drag: &DragPanel, window, cx| {
                                // Dropped past the last tab: from this group
                                // it lands in the final slot, from elsewhere
                                // it is appended.
                                let ix = (drag.source() == node).then(|| tabs_count - 1);
                                group.drop_panel(drag.clone(), ix, false, window, cx);
                            }
                        }),
                )
            })
            .child(controls);
        bar.into_any_element()
    }
}

/// The element a tab draws: the panel's own `title`, or its name when the
/// panel reached the dock without the presentation handle.
fn tab_title(panel: &Arc<dyn BasePanelView>, window: &mut Window, cx: &mut App) -> AnyElement {
    match PanelHandle::of(panel) {
        Some(handle) => handle.title(window, cx),
        None => SharedString::from(panel.panel_name(cx)).into_any_element(),
    }
}

/// Panels whose tab asks before it closes. The demo keys the policy off the
/// panel name — the editor may hold unsaved text. A real app would move the
/// decision onto the panel itself (e.g. a dirty flag).
fn needs_close_confirm(panel_name: &str) -> bool {
    panel_name == "EditorPanel"
}

/// Ask before closing, then close on confirm. The prompt answers
/// asynchronously, so the actual close runs from the window handle once the
/// user confirms; a closed window just drops the close.
fn confirm_close(
    group: &TabGroupContext,
    panel_ids: Vec<PanelId>,
    window: &mut Window,
    cx: &mut App,
) {
    let group = group.clone();
    let handle = window.window_handle();
    let message = if panel_ids.len() > 1 {
        format!(
            "Close {} tabs? Unsaved changes will be lost.",
            panel_ids.len()
        )
    } else {
        "Close this tab? Unsaved changes will be lost.".to_string()
    };
    let answer = window.prompt(PromptLevel::Info, &message, None, &["Close", "Cancel"], cx);
    println!("confirm_close: prompt shown, waiting for answer");
    cx.spawn(async move |cx| {
        if answer.await == Ok(0) {
            println!("confirm_close: confirmed, closing");
            let _ = handle.update(cx, |_, window, cx| {
                for panel_id in panel_ids {
                    group.close(panel_id, window, cx);
                }
            });
        } else {
            println!("confirm_close: cancelled or dismissed");
        }
    })
    .detach();
}

/// The right-click menu of one tab: close it, its neighbours, or the group.
/// Any batch that touches a confirm-on-close panel asks once for the batch.
fn tab_menu(
    mut menu: PopupMenu,
    group: &TabGroupContext,
    ix: usize,
    panel_id: PanelId,
    panel_name: &'static str,
    closable: bool,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let other_ids: Vec<PanelId> = group
        .panels()
        .iter()
        .enumerate()
        .filter(|(other_ix, panel)| *other_ix != ix && panel.closable(cx))
        .map(|(_, panel)| panel.panel_id(cx))
        .collect();
    let all_ids: Vec<PanelId> = group
        .panels()
        .iter()
        .filter(|panel| panel.closable(cx))
        .map(|panel| panel.panel_id(cx))
        .collect();

    menu = menu.item(PopupMenuItem::new("Close").disabled(!closable).on_click({
        let group = group.clone();
        move |_, window, cx| {
            if needs_close_confirm(panel_name) {
                confirm_close(&group, vec![panel_id], window, cx);
            } else {
                group.close(panel_id, window, cx);
            }
        }
    }));
    if !other_ids.is_empty() {
        let asks = group.panels().iter().any(|panel| {
            other_ids.contains(&panel.panel_id(cx)) && needs_close_confirm(panel.panel_name(cx))
        });
        menu = menu.item(PopupMenuItem::new("Close Other Tabs").on_click({
            let group = group.clone();
            move |_, window, cx| {
                if asks {
                    confirm_close(&group, other_ids.clone(), window, cx);
                } else {
                    for panel_id in &other_ids {
                        group.close(*panel_id, window, cx);
                    }
                }
            }
        }));
    }
    if !all_ids.is_empty() {
        let asks = group.panels().iter().any(|panel| {
            all_ids.contains(&panel.panel_id(cx)) && needs_close_confirm(panel.panel_name(cx))
        });
        menu = menu.item(PopupMenuItem::new("Close All Tabs").on_click({
            let group = group.clone();
            move |_, window, cx| {
                if asks {
                    confirm_close(&group, all_ids.clone(), window, cx);
                } else {
                    for panel_id in &all_ids {
                        group.close(*panel_id, window, cx);
                    }
                }
            }
        }));
    }
    menu
}

/// The ghost that follows the cursor while a tab is dragged.
struct TabDragPreview {
    name: SharedString,
}

impl TabDragPreview {
    fn new(panel: &Arc<dyn BasePanelView>, cx: &App) -> Self {
        Self {
            name: SharedString::from(panel.panel_name(cx)),
        }
    }
}

impl Render for TabDragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_sm()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .text_sm()
            .opacity(0.85)
            .child(self.name.clone())
    }
}
