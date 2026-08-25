use bit7z_rs::{ArchiveEngine};
use explorer_view::ArchiveExplorer;
use gpui::{div, px, AppContext, Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled, Window};
use guise::theme::Size;
use guise::Text;
use session::ArchiveSession;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct WorkspaceId(i64);

impl WorkspaceId {
    pub fn from_i64(value: i64) -> Self {
        Self(value)
    }
}

impl From<WorkspaceId> for i64 {
    fn from(val: WorkspaceId) -> Self {
        val.0
    }
}

enum WorkspaceState {
    /// No archive loaded — the landing view of a fresh tab.
    Welcome,
    /// An opened archive with its browsable file table.
    Open {
        path: PathBuf,
        explorer: Entity<ArchiveExplorer>,
    },
    /// The archive could not be opened; the tab stays so the user sees why.
    Failed { message: SharedString },
}

/// One tab in the [`crate::multi_workspace::MultiWorkspace`] window.
pub struct Workspace {
    workspace_id: Option<WorkspaceId>,
    title: SharedString,
    state: WorkspaceState,
}

impl Workspace {
    /// A tab with no content yet: shows the welcome page.
    pub fn welcome(workspace_id: Option<WorkspaceId>) -> Self {
        Self {
            workspace_id,
            title: "Welcome".into(),
            state: WorkspaceState::Welcome,
        }
    }

    /// Opens `archive_path` through `engine` and shows it in an
    /// [`ArchiveExplorer`] table. On failure the tab switches to an error
    /// page instead of disappearing.
    pub fn open_archive(
        workspace_id: Option<WorkspaceId>,
        archive_path: &Path,
        engine: &Arc<dyn ArchiveEngine>,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = display_title(archive_path);
        match ArchiveSession::open(session::next_archive_id(), engine.clone(), archive_path, None) {
            Ok(archive_session) => {
                let explorer = ArchiveExplorer::new(Arc::new(Mutex::new(archive_session)), cx);
                Self {
                    workspace_id,
                    title: title.into(),
                    state: WorkspaceState::Open {
                        path: archive_path.to_path_buf(),
                        explorer,
                    },
                }
            }
            Err(error) => Self {
                workspace_id,
                title: title.into(),
                state: WorkspaceState::Failed {
                    message: SharedString::from(format!(
                        "Failed to open {}: {error}",
                        archive_path.display()
                    )),
                },
            },
        }
    }

    /// A tab explaining why an archive could not even be attempted (e.g.
    /// the engine DLL failed to load).
    pub fn failed(workspace_id: Option<WorkspaceId>, archive_path: &Path, reason: SharedString) -> Self {
        Self {
            workspace_id,
            title: display_title(archive_path).into(),
            state: WorkspaceState::Failed {
                message: SharedString::from(format!(
                    "Failed to open {}: {reason}",
                    archive_path.display()
                )),
            },
        }
    }

    pub fn workspace_id(&self) -> Option<WorkspaceId> {
        self.workspace_id
    }

    /// Tab-strip label.
    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn is_welcome(&self) -> bool {
        matches!(self.state, WorkspaceState::Welcome)
    }
}

fn display_title(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(match &self.state {
                WorkspaceState::Welcome => welcome_page(),
                WorkspaceState::Open { path, explorer } => div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex_none()
                            .w_full()
                            .px(px(12.))
                            .py(px(6.))
                            .child(Text::new(path.to_string_lossy()).size(Size::Sm).dimmed()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_hidden()
                            .child(explorer.clone()),
                    ),
                WorkspaceState::Failed { message } => div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .px(px(32.))
                    .child(Text::new(message.clone())),
            })
    }
}

fn welcome_page() -> gpui::Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .child(Text::new("Bit7zFM").size(Size::Xl).bold())
        .child(Text::new("A 7-Zip / WinRAR class archive manager").dimmed())
        .child(
            Text::new(
                "Open an archive from the command line, or press + for a new tab.",
            )
            .size(Size::Sm)
            .dimmed(),
        )
}
