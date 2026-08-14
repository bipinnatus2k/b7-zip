//! UI kit: a small, self-contained GPUI component library.
//!
//! Components are plain structs with a builder API; they have no business
//! logic and no dependencies beyond `gpui`, so they can be reused by any
//! GPUI application.

pub mod button;
pub mod checkbox;
pub mod data;
pub mod data_extra;
pub mod icon_button;
pub mod list;
pub mod tabs;
pub mod text_field;
pub mod tokens;

pub use button::{Button, ButtonStyle};
pub use checkbox::Checkbox;
pub use data::{Badge, Label, ProgressBar};
pub use data_extra::{Breadcrumb, StatusBar};
pub use icon_button::IconButton;
pub use list::List;
pub use tabs::Tabs;
pub use text_field::TextField;
