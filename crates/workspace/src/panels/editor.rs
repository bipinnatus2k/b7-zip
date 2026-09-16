use std::path::PathBuf;
use gpui::{div, App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, Render, Window, Styled, ParentElement, AnyElement, Entity, AppContext, SharedString};
use gpui_kit::base::dock::PanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel};
use gpui_kit::component::{Icon, IconName, Sizable};
use explorer::explorer::ArchiveExplorer;
use crate::panels::panel_title;

pub(crate) const PANEL_EXPLORER : &str = "explorer";

pub(crate) struct ExplorerPanel {
    pub(crate) focus_handle: FocusHandle,
    title: SharedString,
    content: Entity<ArchiveExplorer>
}

impl ExplorerPanel {

    fn new(cx: &mut Context<Self>) -> Self {

        let explorer = cx.new(|_cx| {
            ArchiveExplorer::new()
        });

        Self {
            focus_handle: cx.focus_handle(),
            title: "Welcome".into(),
            content: explorer
        }
    }
    
    pub(crate) fn new_tab(cx: &mut App) -> Entity<ExplorerPanel> {
        cx.new(
            |cx| {
                Self::new(cx)
            }
        )
    }
    
    pub(crate) fn open(path: &PathBuf, cx: &mut App) -> Entity<ExplorerPanel> {
        let title = path.file_name().unwrap().to_str().unwrap().to_string();

        let explorer = cx.new(|_cx| {
            ArchiveExplorer::new()
        });
        
        let instance = Self {
            focus_handle: cx.focus_handle(),
            title: title.into(),
            content: explorer,
        };
        
        cx.new(move |x| {
            instance
        })
    }

}

impl EventEmitter<PanelEvent> for ExplorerPanel {}

impl Focusable for ExplorerPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for ExplorerPanel {
    fn panel_name(&self) -> &'static str {
        PANEL_EXPLORER
    }
}

impl Panel for ExplorerPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        panel_title(IconName::Info, self.title.clone())
    }
}

impl Render for ExplorerPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
            .child(self.content.clone())
    }
}