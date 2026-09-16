use gpui::{div, Context, IntoElement, Render, Window, Entity};

pub struct ArchiveExplorer {
    
}

impl ArchiveExplorer {
    
    pub fn new() -> Self {
        
        Self {}
    }
}

impl Render for ArchiveExplorer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}