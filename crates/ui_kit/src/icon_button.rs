//! Icon button component: a square button showing a glyph.

use crate::tokens::{
    ICON_BUTTON_SIZE, RADIUS_SM, SURFACE, SURFACE_ACTIVE, SURFACE_HOVER, TEXT_SECONDARY,
};
use gpui::{
    App, Context, Div, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    SharedString, Stateful, StatefulInteractiveElement, Styled, Window, div, px,
};
use std::sync::Arc;

/// A square icon button (the icon is a glyph string, e.g. an emoji or
/// symbol; rich vector icons are provided by the host app via child
/// elements in a future extension).
#[derive(gpui::IntoElement)]
pub struct IconButton {
    id: ElementId,
    glyph: SharedString,
    enabled: bool,
    on_click: Option<Arc<dyn Fn(&mut App) + Send + Sync + 'static>>,
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>, glyph: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            glyph: glyph.into(),
            enabled: true,
            on_click: None,
        }
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&mut App) + Send + Sync + 'static) -> Self {
        self.on_click = Some(Arc::new(handler));
        self
    }

    fn build(&self) -> Stateful<Div> {
        let mut element = div()
            .id(self.id.clone())
            .size(px(ICON_BUTTON_SIZE))
            .rounded(px(RADIUS_SM))
            .bg(SURFACE)
            .items_center()
            .justify_center()
            .text_color(TEXT_SECONDARY)
            .cursor_pointer()
            .hover(|s| s.bg(SURFACE_HOVER))
            .active(|s| s.bg(SURFACE_ACTIVE))
            .child(self.glyph.clone());

        if self.enabled {
            if let Some(handler) = &self.on_click {
                let handler = handler.clone();
                element = element.on_click(move |_event: &gpui::ClickEvent, _window: &mut Window, cx: &mut App| {
                    handler(cx)
                });
            }
        }
        element
    }
}

impl RenderOnce for IconButton {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        self.build()
    }
}
