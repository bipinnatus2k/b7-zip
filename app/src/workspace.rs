//! Main workspace view: toolbar + archive explorer + status bar.

use bit7z_rs::{ArchiveEngine, Bit7zEngine};
use gpui::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled,
    Window, div, px,
};
use session::{ArchiveSession, SessionStore};
use std::path::PathBuf;
use std::sync::Arc;
use ui_kit::{Button, ButtonStyle, StatusBar, Toolbar};

/// The main application window view.
pub struct Workspace {
    engine: Arc<dyn ArchiveEngine>,
    sessions: SessionStore,
    explorer: Option<Entity<bit7z_explorer::ArchiveExplorer>>,
    current_archive: Option<PathBuf>,
    status: SharedString,
}

impl Workspace {
    /// Create the workspace with a freshly loaded engine.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let engine: Arc<dyn ArchiveEngine> = match Bit7zEngine::new(find_dll().as_deref()) {
            Ok(engine) => Arc::new(engine),
            Err(error) => {
                eprintln!("failed to load 7-Zip engine: {error}");
                Arc::new(NoopEngine)
            }
        };
        let _ = cx;
        Self {
            engine,
            sessions: SessionStore::new(),
            explorer: None,
            current_archive: None,
            status: "Ready".into(),
        }
    }

    /// Open an archive and show it in the explorer.
    pub fn open_archive(&mut self, path: &PathBuf, cx: &mut Context<Self>) {
        let id = session::next_archive_id();
        match ArchiveSession::open(id, self.engine.clone(), path, None) {
            Ok(session) => {
                let handle = match self.sessions.insert(session) {
                    Ok(handle) => handle,
                    Err(error) => {
                        self.status = format!("error: {error}").into();
                        cx.notify();
                        return;
                    }
                };
                let explorer = bit7z_explorer::ArchiveExplorer::new(handle.clone(), cx);
                self.explorer = Some(explorer);
                self.current_archive = Some(path.clone());
                self.status = format!("opened {}", path.display()).into();
            }
            Err(error) => {
                self.status = format!("failed to open {}: {error}", path.display()).into();
            }
        }
        cx.notify();
    }

    /// Navigate up in the explorer (if any).
    pub fn navigate_up(&mut self, cx: &mut Context<Self>) {
        if let Some(explorer) = &self.explorer {
            explorer.update(cx, |explorer, cx| {
                explorer.navigate_up();
                cx.notify();
            });
        }
    }

    /// Test the currently open archive.
    pub fn test_archive(&self) {
        if let Some(path) = &self.current_archive {
            eprintln!("testing {}", path.display());
        }
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> Toolbar {
        let mut toolbar = Toolbar::new()
            .item(
                Button::new("open", "Open...")
                    .on_click(|cx| {
                        eprintln!("open clicked");
                        let _ = cx;
                    }),
            )
            .item(Button::new("up", "Up").on_click({
                let weak = cx.weak_entity();
                move |cx| {
                    if let Some(workspace) = weak.upgrade() {
                        workspace.update(cx, |workspace, cx| workspace.navigate_up(cx));
                    }
                }
            }))
            .item(
                Button::new("test", "Test")
                    .style(ButtonStyle::Default)
                    .on_click(|_cx| {}),
            );
        if self.explorer.is_some() {
            toolbar = toolbar
                .item(Button::new("extract", "Extract...").on_click(|_cx| {}))
                .item(Button::new("add", "Add...").on_click(|_cx| {}));
        }
        toolbar
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let toolbar = self.toolbar(cx);
        let content = if let Some(explorer) = &self.explorer {
            div().flex_grow_1().child(explorer.clone())
        } else {
            div()
                .flex_grow_1()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_color(ui_kit::tokens::TEXT_SECONDARY)
                        .text_size(px(16.0))
                        .child("Open an archive to start (pass a path on the command line)"),
                )
        };

        div()
            .flex_col()
            .size_full()
            .bg(ui_kit::tokens::BACKGROUND)
            .child(toolbar)
            .child(content)
            .child(StatusBar::new().left(self.status.clone()))
    }
}

/// Locate the 7-Zip DLL: next to the executable, then VCPKG_ROOT.
fn find_dll() -> Option<String> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    for name in ["7z.dll", "7zip.dll"] {
        let candidate = exe_dir.join(name);
        if candidate.exists() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    if let Ok(vcpkg) = std::env::var("VCPKG_ROOT") {
        for name in ["7zip.dll", "7z.dll"] {
            let candidate = PathBuf::from(&vcpkg)
                .join("installed/x64-windows/bin")
                .join(name);
            if candidate.exists() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// A no-op engine used when the 7-Zip DLL is unavailable.
struct NoopEngine;

impl ArchiveEngine for NoopEngine {
    fn list(&self, _path: &std::path::Path, _password: Option<&password::Password>) -> Result<Vec<bit7z_rs::ArchiveEntry>, bit7z_rs::ArchiveError> {
        Err(bit7z_rs::ArchiveError::Engine("7-Zip engine unavailable".into()))
    }
    fn extract(&self, _path: &std::path::Path, _indices: &[u32], _dest: &std::path::Path, _password: Option<&password::Password>, _options: &bit7z_rs::ExtractOptions) -> Result<(), bit7z_rs::ArchiveError> {
        Err(bit7z_rs::ArchiveError::Engine("7-Zip engine unavailable".into()))
    }
    fn extract_to_buffer(&self, _path: &std::path::Path, _index: u32, _password: Option<&password::Password>) -> Result<Vec<u8>, bit7z_rs::ArchiveError> {
        Err(bit7z_rs::ArchiveError::Engine("7-Zip engine unavailable".into()))
    }
    fn test(&self, _path: &std::path::Path, _password: Option<&password::Password>) -> Result<bit7z_rs::TestResult, bit7z_rs::ArchiveError> {
        Err(bit7z_rs::ArchiveError::Engine("7-Zip engine unavailable".into()))
    }
    fn compress(&self, _inputs: &[PathBuf], _target: &std::path::Path, _options: &bit7z_rs::CompressOptions) -> Result<(), bit7z_rs::ArchiveError> {
        Err(bit7z_rs::ArchiveError::Engine("7-Zip engine unavailable".into()))
    }
    fn update(&self, _path: &std::path::Path, _ops: &[bit7z_rs::EngineOp], _password: Option<&password::Password>) -> Result<(), bit7z_rs::ArchiveError> {
        Err(bit7z_rs::ArchiveError::Engine("7-Zip engine unavailable".into()))
    }
    fn is_encrypted(&self, _path: &std::path::Path) -> Result<bool, bit7z_rs::ArchiveError> {
        Ok(false)
    }
    fn is_header_encrypted(&self, _path: &std::path::Path) -> Result<bool, bit7z_rs::ArchiveError> {
        Ok(false)
    }
}
