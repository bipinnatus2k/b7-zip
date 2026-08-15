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

mod workspace;

use assets::Assets;
use clap::Parser;
use gpui::{AppContext, Bounds, SharedString, TitlebarOptions, WindowBounds, WindowOptions, px, size};
use gpui_platform;
use guise::theme::Theme;
use std::path::PathBuf;

/// Command-line arguments.
#[derive(Parser, Debug)]
#[command(name = "bit7zfm", max_term_width = 100)]
struct Args {
    /// Archive paths to open.
    paths: Vec<PathBuf>,
}

fn build_application() -> gpui::Application {
    let platform = gpui_platform::current_platform(false);
    gpui::Application::with_platform(platform)
}

fn main() {
    let args = Args::parse();
    let app = build_application().with_assets(Assets);

    app.run(move |cx| {
        // The Mantine-style theme driving every guise component.
        Theme::dark().init(cx);

        let paths = args.paths.clone();
        let bounds = Bounds::centered(None, size(px(1100.0), px(720.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::new_static("Bit7zFM")),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_window, cx| {
                cx.new(|cx| {
                    let mut workspace = workspace::Workspace::new(cx);
                    if let Some(path) = paths.first() {
                        workspace.open_archive(path, cx);
                    }
                    workspace
                })
            },
        )
        .expect("failed to open window");
    });
}
