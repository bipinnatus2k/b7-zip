//! Data display components: progress bar, label, badge.

use crate::tokens::{
    ACCENT, ACCENT_TEXT, BORDER, FONT_SIZE_MD, FONT_SIZE_XS, RADIUS_SM, SPACE_2, SURFACE,
    SURFACE_ACTIVE, TEXT_PRIMARY, TEXT_SECONDARY,
};
use gpui::{App, Div, IntoElement, ParentElement, RenderOnce, Rgba, SharedString, Styled, Window, div, px, rems};

/// A progress bar (determinate or indeterminate).
#[derive(gpui::IntoElement)]
pub struct ProgressBar {
    progress: Option<f32>,
    height: f32,
}

impl ProgressBar {
    pub fn new() -> Self {
        Self {
            progress: Some(0.0),
            height: 6.0,
        }
    }

    pub fn progress(mut self, progress: f32) -> Self {
        self.progress = Some(progress.clamp(0.0, 1.0));
        self
    }

    pub fn indeterminate(mut self) -> Self {
        self.progress = None;
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    fn build(self) -> Div {
        let track = div()
            .w_full()
            .h(px(self.height))
            .rounded(px(RADIUS_SM))
            .bg(SURFACE)
            .border_1()
            .border_color(BORDER);
        match self.progress {
            Some(fraction) => track.child(
                div()
                    .h_full()
                    .rounded(px(RADIUS_SM))
                    .bg(ACCENT)
                    .w(px(fraction * 200.0)),
            ),
            None => track.child(
                div()
                    .h_full()
                    .w(px(30.0))
                    .rounded(px(RADIUS_SM))
                    .bg(ACCENT),
            ),
        }
    }
}

impl RenderOnce for ProgressBar {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        self.build()
    }
}

/// A simple text label.
#[derive(gpui::IntoElement)]
pub struct Label {
    text: SharedString,
    secondary: bool,
}

impl Label {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            secondary: false,
        }
    }

    pub fn secondary(mut self) -> Self {
        self.secondary = true;
        self
    }
}

impl RenderOnce for Label {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .text_color(if self.secondary { TEXT_SECONDARY } else { TEXT_PRIMARY })
            .text_size(rems(FONT_SIZE_MD))
            .child(self.text)
    }
}

/// A small status badge.
#[derive(gpui::IntoElement)]
pub struct Badge {
    text: SharedString,
    color: Rgba,
}

impl Badge {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            color: SURFACE_ACTIVE,
        }
    }

    pub fn accent(mut self) -> Self {
        self.color = ACCENT;
        self
    }

    pub fn success(mut self) -> Self {
        self.color = crate::tokens::SUCCESS;
        self
    }

    pub fn danger(mut self) -> Self {
        self.color = crate::tokens::DANGER;
        self
    }
}

impl RenderOnce for Badge {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .px(px(SPACE_2))
            .py(px(2.0))
            .rounded(px(4.0))
            .bg(self.color)
            .text_color(ACCENT_TEXT)
            .text_size(rems(FONT_SIZE_XS))
            .child(self.text)
    }
}
