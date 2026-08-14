//! List component: a scrollable list of rows with selection support.

use crate::tokens::{
    ACCENT, BORDER, RADIUS_SM, SELECTED, SPACE_2, SURFACE, TEXT_PRIMARY, TEXT_SECONDARY,
};
use gpui::{
    App, Div, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    Styled, Stateful, StatefulInteractiveElement, Window, div, px,
};
use std::sync::Arc;

/// A single list row.
pub struct ListRow {
    pub id: SharedString,
    pub label: SharedString,
    pub selected: bool,
    pub on_click: Option<Arc<dyn Fn(&mut App) + Send + Sync + 'static>>,
}

/// A scrollable list.
#[derive(gpui::IntoElement)]
pub struct List {
    rows: Vec<ListRow>,
    max_height: Option<f32>,
}

impl List {
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            max_height: None,
        }
    }

    pub fn row(
        mut self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
    ) -> Self {
        self.rows.push(ListRow {
            id: id.into(),
            label: label.into(),
            selected: false,
            on_click: None,
        });
        self
    }

    pub fn select(mut self, id: impl Into<SharedString>) -> Self {
        let id = id.into();
        for row in &mut self.rows {
            row.selected = row.id == id;
        }
        self
    }

    pub fn on_row_click(
        mut self,
        id: impl Into<SharedString>,
        handler: impl Fn(&mut App) + Send + Sync + 'static,
    ) -> Self {
        let id = id.into();
        let handler = Arc::new(handler);
        for row in &mut self.rows {
            if row.id == id {
                row.on_click = Some(handler.clone());
            }
        }
        self
    }

    pub fn max_height(mut self, height: f32) -> Self {
        self.max_height = Some(height);
        self
    }

    fn build(self) -> Div {
        let mut container = div()
            .flex_col()
            .overflow_y_hidden()
            .bg(SURFACE)
            .border_1()
            .border_color(BORDER)
            .rounded(px(RADIUS_SM));
        if let Some(height) = self.max_height {
            container = container.max_h(px(height));
        }
        for row in self.rows {
            let label = row.label;
            let selected = row.selected;
            let mut row_el: Stateful<Div> = div()
                .id(ElementId::Name(row.id.into()))
                .flex_row()
                .items_center()
                .gap(px(SPACE_2))
                .px(px(SPACE_2))
                .py(px(6.0))
                .cursor_pointer()
                .text_color(if selected { TEXT_PRIMARY } else { TEXT_SECONDARY })
                .bg(if selected { SELECTED } else { crate::tokens::SURFACE })
                .hover(|s| s.bg(if selected { SELECTED } else { crate::tokens::SURFACE_HOVER }));
            if let Some(handler) = &row.on_click {
                let handler = handler.clone();
                row_el = row_el.on_click(move |_event: &gpui::ClickEvent, _window: &mut Window, cx: &mut App| {
                    handler(cx)
                });
            }
            container = container.child(row_el.child(label));
        }
        container
    }
}

impl RenderOnce for List {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        self.build()
    }
}


