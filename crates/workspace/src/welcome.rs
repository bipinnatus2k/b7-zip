//! Welcome tab content: the empty-state landing view of a fresh workspace.

use gpui::{div, App, IntoElement, ParentElement, RenderOnce, Styled, Window};
use guise::{Button};
use ui::components::EmptyState;
use ui::IconName;

use crate::multi_workspace::OpenArchive;

/// Landing view shown by workspace tabs that have no archive open.
///
/// Thin app-specific wrapper over the shared [`EmptyState`] widget: fills
/// the available area and centers the empty state inside it.
#[derive(IntoElement)]
pub struct Welcome;

impl RenderOnce for Welcome {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                EmptyState::new("Bit7zFM")
                    .icon(IconName::FolderArchive)
                    .description("Open an archive to browse and manage its contents.")
                    .action(
                        Button::new("welcome-open-archive", "Open archive…").on_click(
                            |_, window, cx| {
                                window.dispatch_action(Box::new(OpenArchive), cx);
                            },
                        ),
                    ),
            )
    }
}
