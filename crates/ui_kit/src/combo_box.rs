//! Combo box component (dropdown selector).

use crate::tokens::{
    ACCENT, BORDER, RADIUS_MD, SPACE_2, SURFACE, SURFACE_HOVER, TEXT_PRIMARY, TEXT_SECONDARY,
    CONTROL_HEIGHT, FONT_SIZE_MD,
};
use gpui::{
    App, Div, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    Stateful, StatefulInteractiveElement, Styled, Window, div, px, rems,
};
use std::sync::Arc;

/// A combo box option.
pub struct ComboOption {
    pub id: SharedString,
    pub label: SharedString,
}

/// A dropdown combo box. The expanded popup list is a simple absolute
/// overlay rendered below the trigger (the host positions it by wrapping
/// this component in a positioned container).
#[derive(gpui::IntoElement)]
pub struct ComboBox {
    id: ElementId,
    options: Vec<ComboOption>,
    selected: Option<SharedString>,
    open: bool,
    on_select: Option<Arc<dyn Fn(&str, &mut App) + Send + Sync + 'static>>,
}

impl ComboBox {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            options: Vec::new(),
            selected: None,
            open: false,
            on_select: None,
        }
    }

    pub fn option(mut self, id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        self.options.push(ComboOption {
            id: id.into(),
            label: label.into(),
        });
        self
    }

    pub fn selected(mut self, id: impl Into<SharedString>) -> Self {
        self.selected = Some(id.into());
        self
    }

    pub fn open(mut self) -> Self {
        self.open = true;
        self
    }

    pub fn on_select(mut self, handler: impl Fn(&str, &mut App) + Send + Sync + 'static) -> Self {
        self.on_select = Some(Arc::new(handler));
        self
    }

    fn selected_label(&self) -> SharedString {
        self.options
            .iter()
            .find(|option| Some(&option.id) == self.selected.as_ref())
            .map(|option| option.label.clone())
            .unwrap_or_else(|| "\u{2014}".into())
    }
}

impl RenderOnce for ComboBox {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let label = self.selected_label();
        let mut trigger: Stateful<Div> = div()
            .id(self.id.clone())
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(SPACE_2))
            .px(px(SPACE_2))
            .h(px(CONTROL_HEIGHT))
            .rounded(px(RADIUS_MD))
            .border_1()
            .border_color(BORDER)
            .bg(SURFACE)
            .cursor_pointer()
            .hover(|s| s.bg(SURFACE_HOVER));

        let mut expanded = div().flex_col().mt(px(2.0)).rounded(px(RADIUS_MD)).border_1().border_color(BORDER).bg(SURFACE);
        for option in &self.options {
            let is_selected = Some(&option.id) == self.selected.as_ref();
            let option_id = option.id.clone();
            let option_label = option.label.clone();
            let mut item: Stateful<Div> = div()
                .id(ElementId::Name(option.id.clone()))
                .px(px(SPACE_2))
                .py(px(5.0))
                .cursor_pointer()
                .text_color(if is_selected { TEXT_PRIMARY } else { TEXT_SECONDARY })
                .hover(|s| s.bg(SURFACE_HOVER));
            if let Some(handler) = &self.on_select {
                let handler = handler.clone();
                let selected_id = option_id.clone();
                item = item.on_click(move |_event: &gpui::ClickEvent, _window: &mut Window, cx: &mut App| {
                    handler(&selected_id, cx)
                });
            }
            let _ = is_selected;
            expanded = expanded.child(item.child(option_label));
            let _ = option_id;
        }

        let mut root = div().flex_col().w(px(180.0));
        root = root.child(
            trigger
                .child(div().text_color(TEXT_PRIMARY).text_size(rems(FONT_SIZE_MD)).child(label))
                .child(div().text_color(TEXT_SECONDARY).text_size(rems(0.7)).child("\u{25BC}")),
        );
        if self.open {
            root = root.child(expanded);
        }
        root
    }
}
