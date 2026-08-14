//! Tree view component: a collapsible hierarchy of nodes.

use crate::tokens::{
    SELECTED, SPACE_1, SPACE_2, SURFACE_HOVER, TEXT_PRIMARY, TEXT_SECONDARY, FONT_SIZE_SM,
};
use gpui::{
    App, Div, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    Stateful, StatefulInteractiveElement, Styled, Window, div, px, rems,
};
use std::sync::Arc;

/// A tree node.
pub struct TreeNode {
    pub id: SharedString,
    pub label: SharedString,
    pub expanded: bool,
    pub selected: bool,
    pub is_directory: bool,
    pub children: Vec<TreeNode>,
    pub on_toggle: Option<Arc<dyn Fn(&mut App) + Send + Sync + 'static>>,
    pub on_click: Option<Arc<dyn Fn(&mut App) + Send + Sync + 'static>>,
}

/// A collapsible tree view.
#[derive(gpui::IntoElement)]
pub struct TreeView {
    roots: Vec<TreeNode>,
}

impl TreeView {
    pub fn new() -> Self {
        Self { roots: Vec::new() }
    }

    pub fn root(mut self, node: TreeNode) -> Self {
        self.roots.push(node);
        self
    }

    pub fn roots(mut self, nodes: Vec<TreeNode>) -> Self {
        self.roots.extend(nodes);
        self
    }

    fn build_node(&self, node: &TreeNode, depth: usize) -> Div {
        let mut row: Stateful<Div> = div()
            .id(ElementId::Name(node.id.clone()))
            .flex_row()
            .items_center()
            .gap(px(SPACE_1))
            .pl(px(SPACE_2 + depth as f32 * 14.0))
            .py(px(3.0))
            .cursor_pointer()
            .bg(if node.selected { SELECTED } else { crate::tokens::SURFACE })
            .hover(|s| s.bg(if node.selected { SELECTED } else { SURFACE_HOVER }));

        let disclosure = if node.is_directory {
            if node.expanded { "\u{25BC}" } else { "\u{25B6}" }
        } else {
            "\u{2022}"
        };
        let _ = disclosure;
        let disclosure_glyph = if node.is_directory {
            if node.expanded { "\u{25BC}" } else { "\u{25B6}" }
        } else {
            ""
        };

        if let Some(handler) = &node.on_toggle {
            let handler = handler.clone();
            let dir = node.is_directory;
            row = row.on_click(move |_event: &gpui::ClickEvent, _window: &mut Window, cx: &mut App| {
                if dir {
                    handler(cx)
                }
            });
        } else if let Some(handler) = &node.on_click {
            let handler = handler.clone();
            row = row.on_click(move |_event: &gpui::ClickEvent, _window: &mut Window, cx: &mut App| {
                handler(cx)
            });
        }

        row = row
            .child(
                div()
                    .w(px(14.0))
                    .text_color(TEXT_SECONDARY)
                    .text_size(rems(0.7))
                    .child(disclosure_glyph),
            )
            .child(
                div()
                    .text_color(TEXT_PRIMARY)
                    .text_size(rems(FONT_SIZE_SM))
                    .child(node.label.clone()),
            );

        let mut container = div().flex_col();
        container = container.child(row);
        if node.is_directory && node.expanded {
            for child in &node.children {
                container = container.child(self.build_node(child, depth + 1));
            }
        }
        container
    }
}

impl RenderOnce for TreeView {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut container = div().flex_col().text_size(rems(FONT_SIZE_SM));
        for root in &self.roots {
            container = container.child(self.build_node(root, 0));
        }
        container
    }
}
