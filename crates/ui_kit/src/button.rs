//! Button component.

use crate::tokens::{
    CONTROL_HEIGHT, FONT_SIZE_MD, RADIUS_MD, SPACE_2, SPACE_3,
};
use crate::tokens::{
    ACCENT, ACCENT_HOVER, ACCENT_TEXT, DANGER, SURFACE, SURFACE_HOVER, TEXT_DISABLED, TEXT_PRIMARY,
};
use gpui::{
    App, ClickEvent, Context, Div, ElementId, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div, px, rems,
};
use std::sync::Arc;

/// Button style variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonStyle {
    /// Default surface button.
    #[default]
    Default,
    /// Accent-colored primary action.
    Primary,
    /// Danger-colored destructive action.
    Danger,
}

/// A clickable button with a label.
#[derive(gpui::IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    style: ButtonStyle,
    enabled: bool,
    full_width: bool,
    on_click: Option<Arc<dyn Fn(&mut App) + Send + Sync + 'static>>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            style: ButtonStyle::Default,
            enabled: true,
            full_width: false,
            on_click: None,
        }
    }

    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&mut App) + Send + Sync + 'static) -> Self {
        self.on_click = Some(Arc::new(handler));
        self
    }

    fn build(&self) -> gpui::Stateful<Div> {
        let (bg, bg_hover, text) = match self.style {
            ButtonStyle::Default => (SURFACE, SURFACE_HOVER, TEXT_PRIMARY),
            ButtonStyle::Primary => (ACCENT, ACCENT_HOVER, ACCENT_TEXT),
            ButtonStyle::Danger => (DANGER, DANGER, ACCENT_TEXT),
        };

        let mut element = div()
            .flex_row()
            .items_center()
            .justify_center()
            .gap(px(SPACE_2))
            .px(px(SPACE_3))
            .h(px(CONTROL_HEIGHT))
            .rounded(px(RADIUS_MD))
            .bg(bg)
            .text_color(text)
            .text_size(rems(FONT_SIZE_MD))
            .cursor_pointer()
            .hover(|s| s.bg(bg_hover))
            .id(self.id.clone());

        if self.full_width {
            element = element.w_full();
        }
        if self.enabled {
            if let Some(handler) = &self.on_click {
                let handler = handler.clone();
                element = element.on_click(move |_event: &ClickEvent, _window: &mut Window, cx: &mut App| {
                    handler(cx)
                });
            }
        } else {
            element = element.text_color(TEXT_DISABLED).cursor_default();
        }
        element.child(self.label.clone())
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        self.build()
    }
}
