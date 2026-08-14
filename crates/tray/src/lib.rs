use gpui::{App, Global, Image, SharedString};
use gpui_tray::{TrayHandle, TrayState};

struct AppTrayState {
    icon: Image,
    tray_visible: bool,
    tray_title: SharedString,
    tray_tooltip: SharedString,
    tray_handle: Option<TrayHandle>,
}

impl AppTrayState {
    fn new() -> Self {
        Self {
            icon: Image::from_bytes(),
            tray_visible: true,
            tray_title: "Tray App".into(),
            tray_tooltip: "This is a tray icon".into(),
            tray_handle: None,
        }
    }
}

impl Global for AppTrayState {}


fn init(cx: &mut App) {
    let async_app = cx.to_async();
    let state = build_tray_state(cx.global::<AppTrayState>());
    match gpui_tray::tray::set_up_tray(cx, async_app, state, on_tray_event) {
        Ok(handle) => {
            cx.global_mut::<AppTrayState>().tray_handle = Some(handle);
        }
        Err(error) => {
            eprintln!("failed to set up tray: {error:#}");
        }
    }
}

fn build_tray_state(p0: &AppTrayState) -> _ {
    todo!()
}

fn refresh_tray(cx: &mut App) {
    let app_state = cx.global::<AppTrayState>();
    let Some(handle) = app_state.tray_handle.clone() else {
        return;
    };
    let state = build_tray_state(app_state);
    if let Err(error) = handle.set_state(state) {
        eprintln!("failed to sync tray: {error:#}");
        return;
    }
    if let Err(error) = handle.flush_now(cx) {
        eprintln!("failed to flush tray: {error:#}");
    }
}