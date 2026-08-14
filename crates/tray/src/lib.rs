//! System tray integration (placeholder).
//!
//! Full tray support (show/hide main window, quick actions, exit) is
//! implemented in a later step; this crate currently provides the
//! initialization hook and the tray state type so the rest of the
//! workspace can depend on it.

use gpui::App;

/// Initialize the system tray.
pub fn init(_cx: &mut App) {}

/// Show the tray icon (no-op until fully implemented).
pub fn show_tray(_cx: &mut App) {}

/// Hide the tray icon (no-op until fully implemented).
pub fn hide_tray(_cx: &mut App) {}
