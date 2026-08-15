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
use bit7z_rs::{ArchiveEngine, ArchiveError, Bit7zEngine};
use clap::Parser;
use gpui::{AppContext, Bounds, SharedString, TitlebarOptions, WindowBounds, WindowOptions, px, size};
use gpui_platform;
use guise::theme::Theme;
use password::Password;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

/// Load the 7-Zip engine, falling back to a no-op engine on failure.
fn load_engine() -> Arc<dyn ArchiveEngine> {
    match Bit7zEngine::new(bit7z_rs::locate_dll().as_deref()) {
        Ok(engine) => Arc::new(engine),
        Err(error) => {
            eprintln!("failed to load 7-Zip engine: {error}");
            Arc::new(NoopEngine)
        }
    }
}

fn main() {
    let args = Args::parse();
    let app = build_application().with_assets(Assets);
    let engine = load_engine();

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
            move |_window, cx| {
                cx.new(move |cx| {
                    let mut workspace = workspace::Workspace::new(engine, cx);
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

/// A no-op engine used when the 7-Zip DLL is unavailable.
struct NoopEngine;

impl ArchiveEngine for NoopEngine {
    fn list(&self, _path: &Path, _password: Option<&Password>) -> Result<Vec<bit7z_rs::ArchiveEntry>, ArchiveError> {
        Err(ArchiveError::Engine("7-Zip engine unavailable".into()))
    }
    fn extract(&self, _path: &Path, _indices: &[u32], _dest: &Path, _password: Option<&Password>, _options: &bit7z_rs::ExtractOptions) -> Result<(), ArchiveError> {
        Err(ArchiveError::Engine("7-Zip engine unavailable".into()))
    }
    fn extract_to_buffer(&self, _path: &Path, _index: u32, _password: Option<&Password>) -> Result<Vec<u8>, ArchiveError> {
        Err(ArchiveError::Engine("7-Zip engine unavailable".into()))
    }
    fn test(&self, _path: &Path, _password: Option<&Password>) -> Result<bit7z_rs::TestResult, ArchiveError> {
        Err(ArchiveError::Engine("7-Zip engine unavailable".into()))
    }
    fn compress(&self, _inputs: &[PathBuf], _target: &Path, _options: &bit7z_rs::CompressOptions) -> Result<(), ArchiveError> {
        Err(ArchiveError::Engine("7-Zip engine unavailable".into()))
    }
    fn update(&self, _path: &Path, _ops: &[bit7z_rs::EngineOp], _password: Option<&Password>) -> Result<(), ArchiveError> {
        Err(ArchiveError::Engine("7-Zip engine unavailable".into()))
    }
    fn is_encrypted(&self, _path: &Path) -> Result<bool, ArchiveError> {
        Ok(false)
    }
    fn is_header_encrypted(&self, _path: &Path) -> Result<bool, ArchiveError> {
        Ok(false)
    }
}
