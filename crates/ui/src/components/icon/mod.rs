pub mod icon_data;

use gpui::{div, App, Hsla, IntoElement, Pixels, RenderOnce, Window, prelude::*, FontWeight, TextOverflow, FontFeatures};
use crate::components::icon::icon_data::IconData;

#[derive(IntoElement)]
pub struct Icon {
    icon: IconData,
    size: Option<Pixels>,
    color: Option<Hsla>,
    weight: FontWeight,
}

impl Icon {
    pub fn new(data: IconData) -> Self {
        Self {
            icon: data,
            size: None,
            color: None,
            weight: FontWeight::default()
        }
    }

    pub fn size(mut self, size: impl Into<Pixels>) -> Self {
        self.size = Some(size.into());
        self
    }

    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }

    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

}

impl RenderOnce for Icon {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let ch = char::from_u32(self.icon.code_point).unwrap_or('□'.into());

        div()
            .font_family(self.icon.font_family)
            .flex()
            .items_center()
            .justify_center()
            .font_weight(self.weight)
            .when_some(self.size,|el, size| {
                el
                    // .size(size)
                    .text_size(size)
            })
            .when_some(self.color,|el, color| {
                el.text_color(color)
            })
            // .text_overflow(TextOverflow::TruncateStart("".into()))
            .child(ch.to_string())
    }
}