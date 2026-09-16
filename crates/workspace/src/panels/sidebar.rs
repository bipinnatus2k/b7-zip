use gpui::{div, App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, Render, Window, Styled, ParentElement, AnyElement, Entity, AppContext, InteractiveElement, SharedString, StatefulInteractiveElement};
use gpui_kit::base::dock::PanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable};
use crate::panels::panel_title;


/// One panel implementation serving two tabs: the persisted `panel_name`
/// picks the content when the layout is restored.
pub(crate) struct SidebarPanel {
    focus_handle: FocusHandle,
    name: &'static str,
    title: &'static str,
    icon: IconName,
    items: Vec<&'static str>,
}

impl SidebarPanel {
    pub(crate) fn files(cx: &mut App) -> Entity<Self> {
        Self::build(
            "FilesPanel",
            "Files",
            IconName::FolderOpen,
            vec![
                "main.rs",
                "panels.rs",
                "workspace.rs",
                "theme.rs",
                "layout.rs",
            ],
            cx,
        )
    }

    pub(crate) fn outline(cx: &mut App) -> Entity<Self> {
        Self::build(
            "OutlinePanel",
            "Outline",
            IconName::BookOpen,
            vec![
                "struct Workspace",
                "impl Workspace",
                "fn new",
                "fn render",
                "impl Panel",
            ],
            cx,
        )
    }

    fn build(
        name: &'static str,
        title: &'static str,
        icon: IconName,
        items: Vec<&'static str>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| Self {
            focus_handle: cx.focus_handle(),
            name,
            title,
            icon,
            items,
        })
    }
}

impl BasePanel for SidebarPanel {
    fn panel_name(&self) -> &'static str {
        self.name
    }
}

impl Panel for SidebarPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        panel_title(self.icon.clone(), self.title)
    }
}

impl EventEmitter<PanelEvent> for SidebarPanel {}

impl Focusable for SidebarPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SidebarPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(SharedString::from(self.name))
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_0p5()
            .p_2()
            .children(self.items.iter().map(|item| {
                div()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .text_sm()
                    .hover(|this| this.bg(cx.theme().accent))
                    .child(*item)
            }))
    }
}