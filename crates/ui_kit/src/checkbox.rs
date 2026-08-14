//! Checkbox component.

use crate::tokens::{
    ACCENT, BORDER, CONTROL_HEIGHT, RADIUS_SM, SPACE_2, SURFACE, SURFACE_HOVER, TEXT_DISABLED,
    TEXT_PRIMARY, FONT_SIZE_MD,
};
use gpui::{
    App, Context, Div, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    SharedString, Stateful, StatefulInteractiveElement, Styled, Window, div, px, rems,
};
use std::sync::Arc;

/// A checkbox with an optional label.
#[derive(gpui::IntoElement)]
pub struct Checkbox {
    id: ElementId,
    checked: bool,
    enabled: bool,
    label: Option<SharedString>,
    on_toggle: Option<Arc<dyn Fn(bool, &mut App) + Send + Sync + 'static>>,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            checked: false,
            enabled: true,
            label: None,
            on_toggle: None,
        }
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn on_toggle(mut self, handler: impl Fn(bool, &mut App) + Send + Sync + 'static) -> Self {
        self.on_toggle = Some(Arc::new(handler));
        self
    }

    fn build(&self) -> Stateful<Div> {
        let box_bg = if self.checked { ACCENT } else { SURFACE };
        let mut box_el = div()
            .size(px(18.0))
            .rounded(px(RADIUS_SM))
            .border_1()
            .border_color(BORDER)
            .bg(box_bg)
            .items_center()
            .justify_center()
            .text_color(crate::tokens::ACCENT_TEXT)
            .text_size(rems(0.75));
        if self.checked {
            box_el = box_el.child("\u{2713}");
        }

        let mut element = div()
            .id(self.id.clone())
            .flex_row()
            .items_center()
            .gap(px(SPACE_2))
            .h(px(CONTROL_HEIGHT))
            .cursor_pointer()
            .child(box_el);

        if let Some(label) = &self.label {
            element = element.child(
                div()
                    .text_color(if self.enabled { TEXT_PRIMARY } else { TEXT_DISABLED })
                    .text_size(rems(FONT_SIZE_MD))
                    .child(label.clone()),
            );
        }

        if self.enabled {
            if let Some(handler) = &self.on_toggle {
                let handler = handler.clone();
                let checked = self.checked;
                element = element.on_click(move |_event: &gpui::ClickEvent, _window: &mut Window, cx: &mut App| {
                    handler(!checked, cx)
                });
            }
        } else {
            element = element.cursor_default().hover(|s| s.bg(SURFACE_HOVER));
        }
        element
    }
}

impl RenderOnce for Checkbox {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        self.build()
    }
}
