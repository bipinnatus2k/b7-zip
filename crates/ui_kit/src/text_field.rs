//! Text field component: a focusable single-line text input.
//!
//! Unlike the stateless components, a text field holds focus state, so it
//! is an entity: use [`TextField::new`] and configure it via setters.

use crate::tokens::{
    ACCENT, BORDER, CONTROL_HEIGHT, FONT_SIZE_MD, RADIUS_MD, SPACE_2, SURFACE, TEXT_DISABLED,
    TEXT_PRIMARY, TEXT_SECONDARY,
};
use gpui::{
    App, AppContext, Context, Div, ElementId, Entity, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyDownEvent, ParentElement, Render, SharedString, Styled, Window, div, px,
    rems,
};
use std::sync::Arc;

/// A single-line text input.
pub struct TextField {
    id: ElementId,
    value: SharedString,
    placeholder: SharedString,
    enabled: bool,
    focus_handle: FocusHandle,
    on_change: Option<Arc<dyn Fn(&str, &mut App) + Send + Sync + 'static>>,
}

impl TextField {
    /// Create a new text field entity.
    pub fn new(id: impl Into<ElementId>, cx: &mut App) -> Entity<Self> {
        let id = id.into();
        cx.new(|cx| Self {
            id,
            value: SharedString::default(),
            placeholder: SharedString::default(),
            enabled: true,
            focus_handle: cx.focus_handle(),
            on_change: None,
        })
    }

    pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>) {
        self.placeholder = placeholder.into();
    }

    pub fn set_value(&mut self, value: impl Into<SharedString>) {
        self.value = value.into();
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn set_on_change(&mut self, handler: impl Fn(&str, &mut App) + Send + Sync + 'static) {
        self.on_change = Some(Arc::new(handler));
    }

    pub fn get_value(&self) -> &str {
        &self.value
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        use gpui::Modifiers;
        let keystroke = &event.keystroke;
        if keystroke.modifiers == Modifiers::default() {
            match keystroke.key.as_str() {
                "backspace" => {
                    let mut value = self.value.to_string();
                    value.pop();
                    self.value = value.into();
                }
                "enter" | "escape" => {
                    window.focus_next(cx);
                }
                key => {
                    if let Some(ch) = key_char_from_key(key) {
                        let mut value = self.value.to_string();
                        value.push(ch);
                        self.value = value.into();
                    }
                }
            }
        }
        if let Some(handler) = &self.on_change {
            let handler = handler.clone();
            let value = self.value.clone();
            handler(&value, cx);
        }
        cx.notify();
    }
}

fn key_char_from_key(key: &str) -> Option<char> {
    let mut chars = key.chars();
    let first = chars.next()?;
    if chars.next().is_none() && !key.starts_with('[') && !key.starts_with('f') && key.len() == 1 {
        Some(first)
    } else {
        None
    }
}

impl Focusable for TextField {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus_handle.is_focused(window);
        let show_placeholder = self.value.is_empty() && !self.placeholder.is_empty();

        let weak = cx.weak_entity();
        let mut element = div()
            .id(self.id.clone())
            .track_focus(&self.focus_handle)
            .flex_row()
            .items_center()
            .gap(px(SPACE_2))
            .px(px(SPACE_2))
            .h(px(CONTROL_HEIGHT))
            .rounded(px(RADIUS_MD))
            .border_1()
            .border_color(if focused { ACCENT } else { BORDER })
            .bg(SURFACE)
            .on_key_down(move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                if let Some(this) = weak.upgrade() {
                    this.update(cx, |this, cx| this.handle_key(event, window, cx));
                }
            });

        if !self.enabled {
            element = element.text_color(TEXT_DISABLED);
        }

        let text_color = if show_placeholder { TEXT_SECONDARY } else { TEXT_PRIMARY };
        let text = if show_placeholder {
            self.placeholder.clone()
        } else {
            self.value.clone()
        };
        let cursor = if focused { "|" } else { "" };
        element
            .child(
                div()
                    .text_color(text_color)
                    .text_size(rems(FONT_SIZE_MD))
                    .child(text),
            )
            .child(
                div()
                    .text_color(ACCENT)
                    .text_size(rems(FONT_SIZE_MD))
                    .child(cursor),
            )
    }
}
