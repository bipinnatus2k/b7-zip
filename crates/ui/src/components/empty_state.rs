use gpui::{AnyElement, App, FontWeight, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window};
use crate::components::stack::v_flex;
use crate::prelude::*;

/// A centered placeholder shown when a list/table/collection has no content.
#[derive(IntoElement)]
pub struct EmptyState {
    icon: IconName,
    heading: SharedString,
    description: Option<SharedString>,
    action: Option<AnyElement>,
}

impl EmptyState {
    pub fn new(heading: impl Into<SharedString>) -> Self {
        Self {
            icon: IconName::User,
            heading: heading.into(),
            description: None,
            action: None,
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = icon;
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
        self
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        v_flex()
            .w_full()
            .items_center()
            .justify_center()
            .gap_2()
            .py_12()
            .child(
                Icon::new(self.icon)
                    .size(Size::Xl)
                    // .color(Color::Custom(semantic::text_muted(cx))),
            )
            .child(Text::new(self.heading).weight(FontWeight::MEDIUM))
            .children(
                self.description
                    .map(|d| Text::new(d).size(Size::Sm).dimmed()),
            )
            .children(self.action)
    }
}