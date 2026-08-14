//! Table component: column headers plus rows of cells.

use crate::tokens::{
    BORDER, RADIUS_SM, SELECTED, SPACE_2, SPACE_3, SURFACE, SURFACE_HOVER, TEXT_PRIMARY,
    TEXT_SECONDARY, FONT_SIZE_SM,
};
use gpui::{
    App, Div, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    Stateful, StatefulInteractiveElement, Styled, Window, div, px, rems,
};
use std::sync::Arc;

/// Sort direction for a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// A table column definition.
pub struct TableColumn {
    pub id: SharedString,
    pub title: SharedString,
    pub width: f32,
    pub sortable: bool,
    pub sort: Option<SortDirection>,
    pub on_sort: Option<Arc<dyn Fn(&mut App) + Send + Sync + 'static>>,
}

/// A table row.
pub struct TableRow {
    pub id: SharedString,
    pub cells: Vec<SharedString>,
    pub selected: bool,
    pub on_click: Option<Arc<dyn Fn(&mut App) + Send + Sync + 'static>>,
}

/// A data table with a header row.
#[derive(gpui::IntoElement)]
pub struct Table {
    columns: Vec<TableColumn>,
    rows: Vec<TableRow>,
}

impl Table {
    pub fn new() -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
        }
    }

    pub fn column(
        mut self,
        id: impl Into<SharedString>,
        title: impl Into<SharedString>,
        width: f32,
    ) -> Self {
        self.columns.push(TableColumn {
            id: id.into(),
            title: title.into(),
            width,
            sortable: false,
            sort: None,
            on_sort: None,
        });
        self
    }

    pub fn sortable(mut self, id: impl Into<SharedString>) -> Self {
        let id = id.into();
        for column in &mut self.columns {
            if column.id == id {
                column.sortable = true;
            }
        }
        self
    }

    pub fn sorted(mut self, id: impl Into<SharedString>, direction: SortDirection) -> Self {
        let id = id.into();
        for column in &mut self.columns {
            column.sort = if column.id == id { Some(direction) } else { None };
        }
        self
    }

    pub fn on_sort(
        mut self,
        id: impl Into<SharedString>,
        handler: impl Fn(&mut App) + Send + Sync + 'static,
    ) -> Self {
        let id = id.into();
        let handler = Arc::new(handler);
        for column in &mut self.columns {
            if column.id == id {
                column.on_sort = Some(handler.clone());
            }
        }
        self
    }

    pub fn row(
        mut self,
        id: impl Into<SharedString>,
        cells: Vec<SharedString>,
    ) -> Self {
        self.rows.push(TableRow {
            id: id.into(),
            cells,
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

    fn build(self) -> Div {
        // Header row.
        let mut header = div().flex_row().bg(SURFACE).border_b_1().border_color(BORDER);
        for column in &self.columns {
            let title = column.title.clone();
            let mut cell: Stateful<Div> = div()
                .id(ElementId::Name(column.id.clone()))
                .w(px(column.width))
                .px(px(SPACE_2))
                .py(px(6.0))
                .text_color(TEXT_SECONDARY)
                .text_size(rems(FONT_SIZE_SM));
            if column.sortable {
                let sort_marker = match column.sort {
                    Some(SortDirection::Ascending) => " \u{25B2}",
                    Some(SortDirection::Descending) => " \u{25BC}",
                    None => "",
                };
                let full = format!("{title}{sort_marker}");
                if let Some(handler) = &column.on_sort {
                    let handler = handler.clone();
                    cell = cell
                        .cursor_pointer()
                        .hover(|s| s.text_color(TEXT_PRIMARY))
                        .on_click(move |_event: &gpui::ClickEvent, _window: &mut Window, cx: &mut App| {
                            handler(cx)
                        })
                        .child(full);
                } else {
                    cell = cell.child(full);
                }
            } else {
                cell = cell.child(title);
            }
            header = header.child(cell);
        }

        // Data rows.
        let mut body = div().flex_col();
        for row in self.rows {
            let selected = row.selected;
            let mut row_el: Stateful<Div> = div()
                .id(ElementId::Name(row.id.clone()))
                .flex_row()
                .cursor_pointer()
                .bg(if selected { SELECTED } else { SURFACE })
                .hover(|s| s.bg(if selected { SELECTED } else { SURFACE_HOVER }));
            if let Some(handler) = &row.on_click {
                let handler = handler.clone();
                row_el = row_el.on_click(move |_event: &gpui::ClickEvent, _window: &mut Window, cx: &mut App| {
                    handler(cx)
                });
            }
            let mut row_container = row_el;
            for (index, cell_text) in row.cells.iter().enumerate() {
                let width = self.columns.get(index).map(|c| c.width).unwrap_or(120.0);
                row_container = row_container.child(
                    div()
                        .w(px(width))
                        .px(px(SPACE_2))
                        .py(px(4.0))
                        .text_color(TEXT_PRIMARY)
                        .text_size(rems(FONT_SIZE_SM))
                        .child(cell_text.clone()),
                );
            }
            body = body.child(row_container);
        }

        div()
            .flex_col()
            .rounded(px(RADIUS_SM))
            .border_1()
            .border_color(BORDER)
            .bg(SURFACE)
            .overflow_y_hidden()
            .child(header)
            .child(body)
    }
}

impl RenderOnce for Table {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        self.build()
    }
}
