//! Toast notification and toolbar components.

use crate::tokens::{
    ACCENT, BORDER, DANGER, RADIUS_MD, SPACE_2, SPACE_3, SUCCESS, SURFACE, TEXT_PRIMARY,
    TEXT_SECONDARY, WARNING, FONT_SIZE_SM,
};
use gpui::{App, Div, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div, px, rems};

/// Toast severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastSeverity {
    Info,
    Success,
    Warning,
    Error,
}

/// A transient notification toast.
#[derive(gpui::IntoElement)]
pub struct Toast {
    message: SharedString,
    severity: ToastSeverity,
}

impl Toast {
    pub fn new(message: impl Into<SharedString>) -> Self {
        Self {
            message: message.into(),
            severity: ToastSeverity::Info,
        }
    }

    pub fn severity(mut self, severity: ToastSeverity) -> Self {
        self.severity = severity;
        self
    }
}

impl RenderOnce for Toast {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let accent = match self.severity {
            ToastSeverity::Info => ACCENT,
            ToastSeverity::Success => SUCCESS,
            ToastSeverity::Warning => WARNING,
            ToastSeverity::Error => DANGER,
        };
        div()
            .flex_row()
            .items_center()
            .gap(px(SPACE_2))
            .px(px(SPACE_3))
            .py(px(SPACE_2))
            .rounded(px(RADIUS_MD))
            .bg(SURFACE)
            .border_1()
            .border_color(BORDER)
            .child(div().size(px(8.0)).rounded(px(4.0)).bg(accent))
            .child(div().text_color(TEXT_PRIMARY).text_size(rems(FONT_SIZE_SM)).child(self.message))
    }
}

/// A horizontal toolbar: a row of buttons/items.
#[derive(gpui::IntoElement)]
pub struct Toolbar {
    items: Vec<gpui::AnyElement>,
}

impl Toolbar {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn item(mut self, item: impl IntoElement) -> Self {
        self.items.push(item.into_element().into_any_element());
        self
    }
}

impl RenderOnce for Toolbar {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut bar = div()
            .flex_row()
            .items_center()
            .gap(px(SPACE_2))
            .px(px(SPACE_3))
            .h(px(36.0))
            .bg(SURFACE)
            .border_b_1()
            .border_color(BORDER);
        for item in self.items {
            bar = bar.child(item);
        }
        bar
    }
}

/// A two-pane split layout with a fixed ratio.
#[derive(gpui::IntoElement)]
pub struct SplitPane {
    left: Option<gpui::AnyElement>,
    right: Option<gpui::AnyElement>,
    /// Fraction of width given to the left pane (0.0..1.0).
    ratio: f32,
    /// Whether the split is horizontal (left/right) or vertical (top/bottom).
    vertical: bool,
}

impl SplitPane {
    pub fn new() -> Self {
        Self {
            left: None,
            right: None,
            ratio: 0.3,
            vertical: false,
        }
    }

    pub fn left(mut self, child: impl IntoElement) -> Self {
        self.left = Some(child.into_element().into_any_element());
        self
    }

    pub fn right(mut self, child: impl IntoElement) -> Self {
        self.right = Some(child.into_element().into_any_element());
        self
    }

    pub fn ratio(mut self, ratio: f32) -> Self {
        self.ratio = ratio.clamp(0.05, 0.95);
        self
    }
}

impl RenderOnce for SplitPane {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let container = if self.vertical {
            div().flex_col()
        } else {
            div().flex_row()
        };
        let has_left = self.left.is_some();
        let has_right = self.right.is_some();
        let mut root = container.w_full().h_full();
        if let Some(left) = self.left {
            root = root.child(div().flex_grow_1().child(left));
        }
        if has_left && has_right {
            root = root.child(div().border_l_1().border_color(BORDER));
        }
        if let Some(right) = self.right {
            root = root.child(div().flex_grow_1().child(right));
        }
        let _ = self.ratio;
        root
    }
}
