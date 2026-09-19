//! System tray integration: a persistent tray icon with Show / Quit actions.
//!
//! Left click (and double click) activates the main window; the context menu
//! carries the same actions. `set_tooltip` lets long-running jobs surface
//! their progress while the window is minimized to the taskbar.

use gpui::{App, Global, Image, ImageFormat, MouseButton};
use gpui_tray::tray::set_up_tray;
use gpui_tray::{
    TrayClickAction, TrayClickPolicy, TrayEvent, TrayMenuItem, TrayState,
};
use std::sync::Arc;

const ICON_BYTES: &[u8] = include_bytes!("../../resources/bit7z.ico");

/// The idle tray tooltip; progress pages restore it when their job ends.
pub const BASE_TOOLTIP: &str = "Bit7zFM";

/// The live tray handle, kept so the tooltip can be updated later.
#[derive(Default)]
pub struct TrayGlobal(Option<Arc<gpui_tray::TrayHandle>>);

impl Global for TrayGlobal {}

/// What to do when the user asks for the main window (tray click / menu).
pub type ShowWindow = Arc<dyn Fn(&mut App) + Send + Sync + 'static>;

/// Creates the tray icon and menu. Call once at startup.
pub fn init(cx: &mut App, show_window: ShowWindow) {
    let icon = Image::from_bytes(ImageFormat::Ico, ICON_BYTES.to_vec());
    let state = TrayState::new()
        .icon(icon)
        .title("Bit7zFM")
        .tooltip(BASE_TOOLTIP)
        .click_policy(
            TrayClickPolicy::platform_default()
                .left(TrayClickAction::EmitEvent)
                .double_click(TrayClickAction::EmitEvent)
                .right(TrayClickAction::OpenMenu),
        )
        .submenu(TrayMenuItem::menu("show", "Open Bit7zFM", vec![]))
        .submenu(TrayMenuItem::separator())
        .submenu(TrayMenuItem::menu("quit", "Quit", vec![]));

    let show_for_events = show_window.clone();
    let result = set_up_tray(
        cx,
        cx.to_async(),
        state,
        move |event: TrayEvent, cx: &mut App| match event {
            TrayEvent::MenuClick { id, .. } => match id.as_str() {
                "show" => show_for_events(cx),
                "quit" => cx.quit(),
                _ => {}
            },
            TrayEvent::TrayClick {
                button: MouseButton::Left,
                ..
            } => show_for_events(cx),
            _ => {}
        },
    );
    match result {
        Ok(handle) => {
            cx.set_global(TrayGlobal(Some(Arc::new(handle))));
        }
        Err(err) => {
            eprintln!("tray initialization failed: {err}");
        }
    }
}

/// Updates the tray tooltip (e.g. job progress while the window is hidden).
pub fn set_tooltip(cx: &mut App, text: impl Into<String>) {
    let Some(handle) = cx.try_global::<TrayGlobal>().and_then(|g| g.0.clone()) else {
        return;
    };
    let state = TrayState::new().tooltip(text.into());
    let _ = handle.set_state(state);
}
