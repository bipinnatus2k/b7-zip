//! Archive explorer: a GPUI view that browses an open archive session.
//!
//! The view renders the session's working tree as a breadcrumb + table
//! (name / size / packed / modified), supports selection, and reports the
//! selection back to the host. It has no business logic of its own: it
//! reads the [`session::ArchiveSession`] overlay and forwards interactions.

use gpui::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled,
    Window, div, px,
};
use session::ArchiveSession;
use std::sync::{Arc, Mutex};
use ui_kit::{Breadcrumb, StatusBar, Table};
use vfs::{NodeId, VfsNode};

/// A browsable archive explorer view.
pub struct ArchiveExplorer {
    session: Arc<Mutex<ArchiveSession>>,
    /// Current directory path inside the archive ("" is the root).
    current_path: String,
    /// Selected node in the current directory.
    selected: Option<NodeId>,
}

impl ArchiveExplorer {
    /// Create the explorer for a session.
    pub fn new(session: Arc<Mutex<ArchiveSession>>, cx: &mut App) -> Entity<Self> {
        cx.new(|_cx| Self {
            session,
            current_path: String::new(),
            selected: None,
        })
    }

    /// Replace the session (e.g. when the user opens a different archive).
    pub fn set_session(&mut self, session: Arc<Mutex<ArchiveSession>>) {
        self.session = session;
        self.current_path = String::new();
        self.selected = None;
    }

    /// Navigate to a directory inside the archive.
    pub fn navigate(&mut self, path: &str) {
        let session = self.session.lock().expect("session lock poisoned");
        if session.overlay().working().resolve_path(path).is_some() {
            self.current_path = path.to_string();
            self.selected = None;
        }
    }

    /// Navigate to the parent directory.
    pub fn navigate_up(&mut self) {
        if self.current_path.is_empty() {
            return;
        }
        let trimmed = self.current_path.trim_end_matches('/');
        self.current_path = match trimmed.rfind('/') {
            Some(idx) => trimmed[..idx].to_string(),
            None => String::new(),
        };
        self.selected = None;
    }

    /// The current directory path.
    pub fn current_path(&self) -> &str {
        &self.current_path
    }

    /// The selected node id, if any.
    pub fn selection(&self) -> Option<NodeId> {
        self.selected
    }

    /// The selected node's path inside the archive, if any.
    pub fn selected_path(&self) -> Option<String> {
        let session = self.session.lock().expect("session lock poisoned");
        self.selected.and_then(|id| session.overlay().working().path_of(id))
    }

    /// The selected node itself (for extraction etc.).
    pub fn selected_node(&self) -> Option<VfsNode> {
        let session = self.session.lock().expect("session lock poisoned");
        self.selected.and_then(|id| session.overlay().working().node(id).cloned())
    }

    fn current_entries(&self) -> Vec<VfsNode> {
        let session = self.session.lock().expect("session lock poisoned");
        let tree = session.overlay().working();
        let Some(dir_id) = tree.resolve_path(&self.current_path) else {
            return Vec::new();
        };
        let mut entries: Vec<VfsNode> = tree
            .children(dir_id)
            .unwrap_or(&[])
            .iter()
            .filter_map(|&id| tree.node(id).cloned())
            .collect();
        // Directories first, then files, each alphabetically.
        entries.sort_by(|a, b| {
            b.is_directory
                .cmp(&a.is_directory)
                .then_with(|| a.name.cmp(&b.name))
        });
        entries
    }
}

impl Render for ArchiveExplorer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entries = self.current_entries();
        let weak = cx.weak_entity();

        // Breadcrumb segments from the current path.
        let mut breadcrumb = Breadcrumb::new().push("/");
        for segment in self.current_path.split('/').filter(|s| !s.is_empty()) {
            breadcrumb = breadcrumb.push(segment);
        }

        // Table rows for the current directory.
        let mut table = Table::new()
            .column("name", "Name", 260.0)
            .column("size", "Size", 100.0)
            .column("packed", "Packed", 100.0)
            .column("modified", "Modified", 160.0);
        let mut selected_size: u64 = 0;
        for entry in &entries {
            let id = entry.id;
            let name = if entry.is_directory {
                format!("{}/", entry.name)
            } else {
                entry.name.clone()
            };
            let size = entry
                .attr(vfs::attr::SIZE)
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let packed = entry
                .attr(vfs::attr::PACKED_SIZE)
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let modified = entry
                .attr(vfs::attr::MODIFIED)
                .and_then(|v| v.as_datetime())
                .map(|dt| dt.to_string())
                .unwrap_or_else(|| "-".into());
            if self.selected == Some(id) {
                selected_size += size;
            }
            let row_id = id.as_u64().to_string();
            let row_weak = weak.clone();
            table = table
                .row(
                    row_id.clone(),
                    vec![name.into(), format_size(size).into(), format_size(packed).into(), modified.into()],
                )
                .on_row_click(row_id.clone(), move |cx| {
                    if let Some(explorer) = row_weak.upgrade() {
                        explorer.update(cx, |explorer, cx| {
                            explorer.selected = Some(id);
                            cx.notify();
                        });
                    }
                });
        }
        if let Some(selected_id) = self.selected {
            table = table.select(selected_id.as_u64().to_string());
        }

        let status = StatusBar::new()
            .left(format!("{} entries", entries.len()))
            .right(format!("selected: {}", format_size(selected_size)));

        div()
            .flex_col()
            .size_full()
            .child(div().p(px(8.0)).child(breadcrumb))
            .child(div().flex_grow_1().overflow_y_hidden().child(table))
            .child(status)
    }
}

/// Format a byte count in a human-readable form.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".into();
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} B", bytes)
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}
