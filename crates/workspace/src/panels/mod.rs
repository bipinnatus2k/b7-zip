use gpui::{div, AnyElement, Styled, ParentElement, IntoElement, App, SharedString};
use gpui_kit::component::dock::{panel_handle, register_panel};
use gpui_kit::component::{Icon, IconName, Sizable};
use crate::panels::editor::{ExplorerPanel, PANEL_EXPLORER};
use crate::panels::sidebar::SidebarPanel;
pub(crate) use crate::panels::welcome::WelcomePanel;

pub mod editor;
pub mod welcome;
pub mod sidebar;

/// Register every panel of this example, so `DockArea::load` can rebuild a
/// saved layout by looking `panel_name` up in this registry.
pub(crate) fn register_panels(cx: &mut App) {
    register_panel(cx, PANEL_EXPLORER, |_, window, cx| {
        panel_handle(ExplorerPanel::new_tab(cx))
    });
    register_panel(cx, "FilesPanel", |_, _, cx| {
        panel_handle(SidebarPanel::files(cx))
    });
    register_panel(cx, "OutlinePanel", |_, _, cx| {
        panel_handle(ListPanel::outline(cx))
    });
    register_panel(cx, "OutputPanel", |_, _, cx| {
        panel_handle(OutputPanel::new(cx))
    });
}

/// The element drawn inside a tab: an icon plus a label.
fn panel_title(icon: IconName, label: SharedString) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .child(Icon::new(icon).xsmall())
        .child(label)
        .into_any_element()
}