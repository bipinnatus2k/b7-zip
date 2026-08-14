#![forbid(unsafe_code)]

use std::sync::Arc;
use gpui::{div, Context, IntoElement, Render, Window, InteractiveElement, MouseButton, AppContext};

struct ArchiveExplorer {


}


impl Render for ArchiveExplorer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            
    }
}