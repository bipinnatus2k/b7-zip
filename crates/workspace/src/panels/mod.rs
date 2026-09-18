use gpui::{div, AnyElement, Styled, ParentElement, IntoElement, App, AppContext, SharedString};
use gpui_kit::component::dock::{panel_handle, register_panel};
use gpui_kit::component::{Icon, IconName, Sizable};
use std::sync::Arc;
use crate::archive_workspace::ArchiveWorkspace;
use crate::diff_panel::{self, DiffPanel};
use crate::settings_panel::SettingsPanel;
pub(crate) use crate::panels::sidebar::SidebarPanel;

pub mod sidebar;

/// The canonical panel-name sets. `zone_of` in `multi_workspace` reads these
/// so the layout-zone policy and the [`register_panels`] factories cannot
/// drift apart (a name present in registration but absent here would be
/// silently evicted by `enforce_zones`).
///
/// * [`CENTER_PANELS`] open as center workspace tabs.
/// * [`TOOL_PANELS`] live in the side/bottom docks.
pub(crate) const CENTER_PANELS: &[&str] =
    &["explorer", "diff", "progress", "settings", "devtools"];
pub(crate) const TOOL_PANELS: &[&str] = &["FilesPanel", "OutlinePanel", "OutputPanel"];

/// Register every panel of this crate, so `DockArea::load` can rebuild a
/// saved layout by looking `panel_name` up in this registry.
pub(crate) fn register_panels(cx: &mut App) {
    register_panel(cx, "explorer", |_, _, cx| {
        panel_handle(cx.new(|cx| ArchiveWorkspace::home(cx)))
    });
    // A restored diff tab has no snapshot to show; an empty panel reads
    // better than dropping the layout.
    register_panel(cx, "diff", |_, _, cx| {
        panel_handle(cx.new(|cx| {
            DiffPanel::new(
                "Diff",
                compare::DiffReport::default(),
                Arc::new(diff_panel::EmptyProvider),
                cx,
            )
        }))
    });
    // A restored progress page has no live job behind it; an empty finished
    // card reads better than dropping the layout.
    register_panel(cx, "progress", |_, _, cx| {
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = tx.send(task::TaskEvent::Finished {
            success: true,
            message: String::new(),
        });
        panel_handle(cx.new(|cx| {
            crate::progress_panel::ProgressPanel::new(
                "Progress",
                rx,
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
                cx,
            )
        }))
    });
    // Developer tools: debug builds only. Release layouts that carried the
    // panel fall back to the registry miss path instead of showing dev UI.
    #[cfg(debug_assertions)]
    register_panel(cx, "devtools", |_, _, cx| {
        panel_handle(cx.new(|cx| crate::devtools_panel::DevToolsPanel::new(cx)))
    });
    register_panel(cx, "settings", |_, window, cx| {
        panel_handle(cx.new(|cx| crate::settings_panel::SettingsPanel::new(window, cx)))
    });
    register_panel(cx, "FilesPanel", |_, _, cx| {
        panel_handle(SidebarPanel::files(cx))
    });
    register_panel(cx, "OutlinePanel", |_, _, cx| {
        panel_handle(SidebarPanel::outline(cx))
    });
    register_panel(cx, "OutputPanel", |_, _, cx| {
        panel_handle(SidebarPanel::output(cx))
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
