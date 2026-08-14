//! UI kit: a small, self-contained GPUI component library.
//!
//! Components are plain structs with a builder API; they have no business
//! logic and no dependencies beyond `gpui`, so they can be reused by any
//! GPUI application.

pub mod button;
pub mod checkbox;
pub mod icon_button;
pub mod text_field;
pub mod tokens;

pub use button::{Button, ButtonStyle};
pub use checkbox::Checkbox;
pub use icon_button::IconButton;
pub use text_field::TextField;
