//! Dialog component: a modal overlay with a centered panel.

use crate::tokens::{
    BACKGROUND, BORDER, RADIUS_LG, SPACE_4, SPACE_5, SURFACE, TEXT_PRIMARY, FONT_SIZE_LG,
};
use gpui::{
    App, Div, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div, px, rems,
};

/// A modal dialog with a dimmed backdrop and a centered panel.
#[derive(gpui::IntoElement)]
pub struct Dialog {
    title: SharedString,
    width: f32,
    content: Vec<gpui::AnyElement>,
    footer: Vec<gpui::AnyElement>,
}

impl Dialog {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            width: 420.0,
            content: Vec::new(),
            footer: Vec::new(),
        }
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.content.push(child.into_element().into_any_element());
        self
    }

    pub fn footer_child(mut self, child: impl IntoElement) -> Self {
        self.footer.push(child.into_element().into_any_element());
        self
    }
}

impl RenderOnce for Dialog {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut panel = div()
            .flex_col()
            .w(px(self.width))
            .rounded(px(RADIUS_LG))
            .bg(SURFACE)
            .border_1()
            .border_color(BORDER)
            .p(px(SPACE_5));

        panel = panel.child(
            div()
                .text_color(TEXT_PRIMARY)
                .text_size(rems(FONT_SIZE_LG))
                .mb(px(SPACE_4))
                .child(self.title),
        );

        let mut body = div().flex_col().gap(px(SPACE_4));
        for item in self.content {
            body = body.child(item);
        }
        panel = panel.child(body);

        if !self.footer.is_empty() {
            let mut footer = div()
                .flex_row()
                .justify_end()
                .gap(px(SPACE_4))
                .mt(px(SPACE_5));
            for item in self.footer {
                footer = footer.child(item);
            }
            panel = panel.child(footer);
        }

        div()
            .w_full()
            .h_full()
            .items_center()
            .justify_center()
            .bg(BACKGROUND)
            .child(panel)
    }
}
