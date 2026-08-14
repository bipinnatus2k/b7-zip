//! Status bar and breadcrumb components.

use gpui::{App, Div, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div, px, rems};

/// A status bar: a horizontal strip of status items.
#[derive(gpui::IntoElement)]
pub struct StatusBar {
    left: Vec<SharedString>,
    right: Vec<SharedString>,
}

impl StatusBar {
    pub fn new() -> Self {
        Self {
            left: Vec::new(),
            right: Vec::new(),
        }
    }

    pub fn left(mut self, text: impl Into<SharedString>) -> Self {
        self.left.push(text.into());
        self
    }

    pub fn right(mut self, text: impl Into<SharedString>) -> Self {
        self.right.push(text.into());
        self
    }
}

impl RenderOnce for StatusBar {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let left_items: Vec<Div> = self
            .left
            .iter()
            .map(|t| div().child(t.clone()))
            .collect();
        let right_items: Vec<Div> = self
            .right
            .iter()
            .map(|t| div().child(t.clone()))
            .collect();
        div()
            .flex_row()
            .items_center()
            .justify_between()
            .px(px(crate::tokens::SPACE_3))
            .h(px(24.0))
            .bg(crate::tokens::SURFACE)
            .border_t_1()
            .border_color(crate::tokens::BORDER)
            .text_color(crate::tokens::TEXT_SECONDARY)
            .text_size(rems(crate::tokens::FONT_SIZE_SM))
            .child(div().flex_row().gap(px(crate::tokens::SPACE_3)).children(left_items))
            .child(div().flex_row().gap(px(crate::tokens::SPACE_3)).children(right_items))
    }
}

/// A breadcrumb: a row of path segments separated by separators.
#[derive(gpui::IntoElement)]
pub struct Breadcrumb {
    segments: Vec<SharedString>,
}

impl Breadcrumb {
    pub fn new() -> Self {
        Self {
            segments: Vec::new(),
        }
    }

    pub fn push(mut self, segment: impl Into<SharedString>) -> Self {
        self.segments.push(segment.into());
        self
    }
}

impl RenderOnce for Breadcrumb {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut children: Vec<Div> = Vec::new();
        let last = self.segments.len().saturating_sub(1);
        for (index, segment) in self.segments.iter().enumerate() {
            if index > 0 {
                children.push(
                    div()
                        .text_color(crate::tokens::TEXT_DISABLED)
                        .child("/"),
                );
            }
            children.push(
                div()
                    .text_color(if index == last {
                        crate::tokens::TEXT_PRIMARY
                    } else {
                        crate::tokens::TEXT_SECONDARY
                    })
                    .child(segment.clone()),
            );
        }
        div()
            .flex_row()
            .items_center()
            .gap(px(crate::tokens::SPACE_1))
            .text_size(rems(crate::tokens::FONT_SIZE_SM))
            .children(children)
    }
}
