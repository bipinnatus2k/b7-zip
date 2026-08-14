// Disable command line from opening on release mode.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Ensure the binary name stays in sync with APP_NAME so that the paths used
// at runtime (data dir, config dir, etc.) match what the binary is called.
const _: () = assert!(
    app_constants::APP_NAME_LOWERCASE
        .as_bytes()
        .eq_ignore_ascii_case(env!("CARGO_BIN_NAME").as_bytes()),
    "app_constants::APP_NAME_LOWERCASE must match the binary name.",
);

mod ui;

use assets::Assets;
use gpui::{AppContext, Application, WindowOptions};
use gpui_platform;

fn build_application() -> Application {
    let platform = gpui_platform::current_platform(false);
    Application::with_platform(platform)
}

fn main() {
    let app = build_application().with_assets(Assets);

    app.run(move |cx| {
        cx.open_window(WindowOptions::default(), |_window, cx| {
            cx.new(|cx| ui::Gallery::new(cx))
        })
        .expect("failed to open window");
    });
}
