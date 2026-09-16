pub mod dataview;

use gpui::{AnyElement, App, Window};

pub type Content = Box<dyn Fn(&mut Window, &mut App) -> AnyElement + 'static>;