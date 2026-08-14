//! Tab bar component.

use crate::tokens::{
    ACCENT, BORDER, RADIUS_MD, SURFACE, SURFACE_HOVER, TEXT_PRIMARY, TEXT_SECONDARY, SPACE_3,
    FONT_SIZE_MD,
};
use gpui::{
    App, Div, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px, rems,
};
use std::sync::Arc;

/// A single tab.
pub struct TabSpec {
    pub id: SharedString,
    pub label: SharedString,
    pub selected: bool,
    pub on_click: Option<Arc<dyn Fn(&mut App) + Send + Sync + 'static>>,
}

/// A horizontal tab bar.
#[derive(gpui::IntoElement)]
pub struct Tabs {
    tabs: Vec<TabSpec>,
}

impl Tabs {
    pub fn new() -> Self {
        Self { tabs: Vec::new() }
    }

    pub fn tab(
        mut self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
    ) -> Self {
        self.tabs.push(TabSpec {
            id: id.into(),
            label: label.into(),
            selected: false,
            on_click: None,
        });
        self
    }

    pub fn select(mut self, id: impl Into<SharedString>) -> Self {
        let id = id.into();
        for tab in &mut self.tabs {
            tab.selected = tab.id == id;
        }
        self
    }

    pub fn on_tab_click(
        mut self,
        id: impl Into<SharedString>,
        handler: impl Fn(&mut App) + Send + Sync + 'static,
    ) -> Self {
        let id = id.into();
        let handler = Arc::new(handler);
        for tab in &mut self.tabs {
            if tab.id == id {
                tab.on_click = Some(handler.clone());
            }
        }
        self
    }

    fn build(self) -> Div {
        let mut bar = div()
            .flex_row()
            .items_center()
            .gap(px(4.0))
            .px(px(SPACE_3))
            .border_b_1()
            .border_color(BORDER);
        for tab in self.tabs {
            let selected = tab.selected;
            let label = tab.label;
            let mut el = div()
                .id(gpui::ElementId::Name(tab.id.clone()))
                .px(px(SPACE_3))
                .py(px(6.0))
                .rounded(px(RADIUS_MD))
                .text_size(rems(FONT_SIZE_MD))
                .text_color(if selected { TEXT_PRIMARY } else { TEXT_SECONDARY })
                .hover(|s| s.bg(SURFACE_HOVER));
            if selected {
                el = el.bg(SURFACE).border_b_2().border_color(ACCENT);
            }
            if let Some(handler) = &tab.on_click {
                let handler = handler.clone();
                el = el.on_click(move |_event: &gpui::ClickEvent, _window: &mut Window, cx: &mut App| {
                    handler(cx)
                });
            }
            bar = bar.child(el.child(label));
        }
        let _ = &mut bar;
        bar
    }
}

impl RenderOnce for Tabs {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        self.build()
    }
}
