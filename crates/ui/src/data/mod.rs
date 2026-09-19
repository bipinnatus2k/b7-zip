pub mod dataview;
pub mod table_view;

use gpui::{AnyElement, App, Window};

pub type Content = Box<dyn Fn(&mut Window, &mut App) -> AnyElement + 'static>;