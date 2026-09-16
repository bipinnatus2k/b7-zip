use gpui::{div, AnyElement, App, Element, FontWeight, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window};
use gpui::prelude::FluentBuilder;
use crate::components::stack::v_flex;

/// A centered placeholder shown when a list/table/collection has no content.
#[derive(IntoElement)]
pub struct EmptyState {
    icon: Option<AnyElement>,
    heading: SharedString,
    description: Option<SharedString>,
    action: Option<AnyElement>,
}

impl EmptyState {
    pub fn new(heading: impl Into<SharedString>) -> Self {
        Self {
            icon: None,
            heading: heading.into(),
            description: None,
            action: None,
        }
    }

    pub fn icon(mut self, icon: AnyElement) -> Self {
        self.icon = Some(icon);
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
            .when_some(self.icon, |x, t| {
                x.child(t)
            })
            .child(div().child(self.heading).font_weight(FontWeight::MEDIUM))
            .children(
                self.description
                    .map(|d| div().text_sm().child(d)),
            )
            .children(self.action)
    }
}