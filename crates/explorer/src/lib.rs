//! Archive explorer: a 7zFM-style file table over the current directory of
//! an open archive.
//!
//! The view renders the session's overlay as a virtualized, sortable,
//! multi-selectable table (built on the guise `TableView`), handles
//! directory navigation, and forwards interactions to the host through
//! [`ExplorerEvent`]. It keeps no business logic of its own.

use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, SharedString, Styled, Window, div, px,
};
use guise::prelude::*;
use session::ArchiveSession;
use std::sync::{Arc, Mutex};
use vfs::attr;
use vfs::{NodeId, VfsNode};

/// One row of the file table: a snapshot of a directory entry.
#[derive(Clone)]
pub struct EntryRow {
    pub id: NodeId,
    /// Archive entry index (synthetic directory nodes have none).
    pub index: Option<u32>,
    pub name: String,
    /// Full path inside the archive (forward slashes).
    pub path: String,
    pub is_directory: bool,
    pub is_encrypted: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub packed: u64,
    /// `YYYY-MM-DD HH:MM` — sorts chronologically as a string.
    pub modified: String,
    pub attributes: u32,
    pub crc: Option<u32>,
    pub method: String,
}

/// Events forwarded to the host.
#[derive(Debug, Clone)]
pub enum ExplorerEvent {
    /// A row was activated (double-click or Enter). The host opens files and
    /// the explorer navigates into directories itself.
    Activated(usize),
    /// The selected set of source rows changed (ascending).
    SelectionChanged(Vec<usize>),
    /// A keyboard command the table does not handle itself.
    Command(ExplorerCommand),
}

/// Extra keyboard commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerCommand {
    /// Backspace: go to the parent directory.
    NavigateUp,
    /// Delete: remove the selected entries from the archive.
    Delete,
    /// F2: rename the focused entry.
    Rename,
}

/// A browsable archive explorer view.
pub struct ArchiveExplorer {
    session: Arc<Mutex<ArchiveSession>>,
    current_path: String,
    rows: Vec<EntryRow>,
    table: Entity<TableView<EntryRow>>,
}

impl EventEmitter<ExplorerEvent> for ArchiveExplorer {}

impl ArchiveExplorer {
    /// Create the explorer for a session.
    pub fn new(session: Arc<Mutex<ArchiveSession>>, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            let table = cx.new(|cx| {
                TableView::new(cx)
                    .columns(vec![
                        Column::new("Name")
                            .min_width(160.0)
                            .cell(|row: &EntryRow, _window, cx| name_cell(row, cx))
                            .sortable_by(|a: &EntryRow, b: &EntryRow| natural_cmp(&a.name, &b.name)),
                        Column::new("Size")
                            .width(88.0)
                            .align(Align::End)
                            .text(|row: &EntryRow| SharedString::from(format_size(row.size)))
                            .sortable_by(|a: &EntryRow, b: &EntryRow| a.size.cmp(&b.size)),
                        Column::new("Packed")
                            .width(88.0)
                            .align(Align::End)
                            .text(|row: &EntryRow| SharedString::from(format_size(row.packed)))
                            .sortable_by(|a: &EntryRow, b: &EntryRow| a.packed.cmp(&b.packed)),
                        Column::new("Modified")
                            .width(140.0)
                            .text(|row: &EntryRow| SharedString::from(row.modified.clone()))
                            .sortable_by(|a: &EntryRow, b: &EntryRow| a.modified.cmp(&b.modified)),
                        Column::new("Attributes")
                            .width(92.0)
                            .align(Align::Center)
                            .text(|row: &EntryRow| SharedString::from(attr_string(row))),
                        Column::new("CRC")
                            .width(80.0)
                            .align(Align::End)
                            .text(|row: &EntryRow| SharedString::from(crc_string(row)))
                            .sortable_by(|a: &EntryRow, b: &EntryRow| a.crc.cmp(&b.crc)),
                        Column::new("Method")
                            .width(84.0)
                            .text(|row: &EntryRow| SharedString::from(row.method.clone())),
                    ])
                    .selection_mode(SelectionMode::Multi)
                    .striped(true)
                    .highlight_on_hover(true)
                    .height(320.0)
            });
            let mut this = Self {
                session,
                current_path: String::new(),
                rows: Vec::new(),
                table,
            };
            this.refresh(cx);
            cx.subscribe(&this.table, |this, _table, event: &TableViewEvent, cx| match event {
                TableViewEvent::Activated(row) => {
                    let Some(entry) = this.rows.get(*row).cloned() else { return };
                    if entry.is_directory {
                        this.navigate(&entry.path, cx);
                    } else {
                        cx.emit(ExplorerEvent::Activated(*row));
                    }
                }
                TableViewEvent::SelectionChanged(rows) => {
                    cx.emit(ExplorerEvent::SelectionChanged(rows.clone()));
                }
                TableViewEvent::Sorted(_) => {}
            })
            .detach();
            this
        })
    }

    /// Replace the session (e.g. when the user opens a different archive).
    pub fn set_session(&mut self, session: Arc<Mutex<ArchiveSession>>, cx: &mut Context<Self>) {
        self.session = session;
        self.current_path.clear();
        self.refresh(cx);
    }

    /// Navigate to a directory inside the archive.
    pub fn navigate(&mut self, path: &str, cx: &mut Context<Self>) {
        let session = self.session.lock().expect("session lock poisoned");
        if session.overlay().working().resolve_path(path).is_some() {
            self.current_path = path.to_string();
            drop(session);
            self.refresh(cx);
        }
    }

    /// Navigate to the parent directory.
    pub fn navigate_up(&mut self, cx: &mut Context<Self>) {
        if self.current_path.is_empty() {
            return;
        }
        let trimmed = self.current_path.trim_end_matches('/');
        self.current_path = match trimmed.rfind('/') {
            Some(idx) => trimmed[..idx].to_string(),
            None => String::new(),
        };
        self.refresh(cx);
    }

    /// The current directory path inside the archive.
    pub fn current_path(&self) -> &str {
        &self.current_path
    }

    /// The underlying table entity.
    pub fn table(&self) -> &Entity<TableView<EntryRow>> {
        &self.table
    }

    /// The table's focus handle, so the host can focus it after opening.
    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.table.read(cx).focus_handle()
    }

    /// The rows of the current directory (source order).
    pub fn rows(&self) -> &[EntryRow] {
        &self.rows
    }

    /// The currently selected rows (source order preserved by selection).
    pub fn selected_rows(&self, cx: &App) -> Vec<EntryRow> {
        self.table
            .read(cx)
            .selected()
            .into_iter()
            .filter_map(|ix| self.rows.get(ix).cloned())
            .collect()
    }

    /// Entry indices to operate on: the selected entries, expanded through
    /// directories; when nothing is selected, everything below the current
    /// directory (which is the whole archive at the root).
    pub fn target_indices(&self, cx: &App) -> Vec<u32> {
        let selected = self.table.read(cx).selected();
        let rows: Vec<&EntryRow> = if selected.is_empty() {
            self.rows.iter().collect()
        } else {
            selected
                .iter()
                .filter_map(|ix| self.rows.get(*ix))
                .collect()
        };
        let session = self.session.lock().expect("session lock poisoned");
        let tree = session.overlay().working();
        let mut indices = Vec::new();
        for row in rows {
            collect_indices(tree, row.id, &mut indices);
        }
        indices.sort_unstable();
        indices.dedup();
        indices
    }

    /// The selected row, if exactly one row is selected (rename target).
    pub fn focused_row(&self, cx: &App) -> Option<EntryRow> {
        let selected = self.table.read(cx).selected();
        let ix = if selected.len() == 1 {
            selected[0]
        } else {
            return None;
        };
        self.rows.get(ix).cloned()
    }

    /// Rebuild the rows of the current directory from the session overlay.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let mut rows: Vec<EntryRow> = {
            let session = self.session.lock().expect("session lock poisoned");
            let tree = session.overlay().working();
            let mut rows: Vec<EntryRow> = Vec::new();
            if let Some(dir_id) = tree.resolve_path(&self.current_path) {
                if let Some(children) = tree.children(dir_id) {
                    for &id in children {
                        let Some(node) = tree.node(id) else { continue };
                        rows.push(self.row_of(tree, node, id));
                    }
                }
            }
            rows
        };
        // Directories first, then files, each sorted naturally.
        rows.sort_by(|a, b| {
            b.is_directory
                .cmp(&a.is_directory)
                .then_with(|| natural_cmp(&a.name, &b.name))
        });
        self.table.update(cx, |table, cx| table.set_rows(rows.clone(), cx));
        self.rows = rows;
        cx.notify();
    }

    fn row_of(&self, tree: &vfs::Tree, node: &VfsNode, id: NodeId) -> EntryRow {
        let path = tree
            .path_of(id)
            .unwrap_or_else(|| {
                if self.current_path.is_empty() {
                    node.name.clone()
                } else {
                    format!("{}/{}", self.current_path, node.name)
                }
            });
        let modified = node
            .attr(attr::MODIFIED)
            .and_then(|v| v.as_datetime())
            .map(|dt| {
                format!("{}-{:02}-{:02} {:02}:{:02}", dt.year(), dt.month(), dt.day(), dt.hour(), dt.minute())
            })
            .unwrap_or_else(|| "-".into());
        EntryRow {
            id,
            index: node.archive_index(),
            name: node.name.clone(),
            path,
            is_directory: node.is_directory,
            is_encrypted: node
                .attr(attr::ENCRYPTED)
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            is_symlink: node
                .attr(attr::SYMLINK)
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            size: node.attr(attr::SIZE).and_then(|v| v.as_u64()).unwrap_or(0),
            packed: node
                .attr(attr::PACKED_SIZE)
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            modified,
            attributes: node
                .attr(attr::ATTRIBUTES)
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            crc: node.attr(attr::CRC).and_then(|v| v.as_u64()).map(|v| v as u32),
            method: node
                .attr(attr::METHOD)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let command = match key {
            "backspace" => Some(ExplorerCommand::NavigateUp),
            "delete" => Some(ExplorerCommand::Delete),
            "f2" => Some(ExplorerCommand::Rename),
            _ => None,
        };
        if let Some(command) = command {
            cx.emit(ExplorerEvent::Command(command));
            cx.stop_propagation();
            return;
        }
        if event.keystroke.modifiers.platform && key == "a" {
            // self.table.update(cx, |table, cx| table.select_all(cx));
            // cx.stop_propagation();
        }
    }
}

impl Render for ArchiveExplorer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Keep the virtualized body filling the space below the workspace
        // chrome (toolbar + breadcrumb + status bar + paddings).
        // let height = (f32::from(window.viewport_size().height) - 148.0).max(140.0);
        // self.table.update(cx, |table, cx| table.set_height(height, cx));

        div()
            .id("archive-explorer")
            .size_full()
            .flex()
            .flex_col()
            .on_key_down(cx.listener(Self::on_key))
            .child(self.table.clone())
    }
}

/// Collect the archive indices of `id` and everything below it.
fn collect_indices(tree: &vfs::Tree, id: NodeId, out: &mut Vec<u32>) {
    if let Some(node) = tree.node(id) {
        if let Some(index) = node.archive_index() {
            out.push(index);
        }
    }
    if let Some(children) = tree.children(id) {
        for &child in children {
            collect_indices(tree, child, out);
        }
    }
}

/// Render the name cell: icon (folder/file/archive), optional lock, name.
fn name_cell(row: &EntryRow, cx: &App) -> gpui::Div {
    let t = cx.global::<Theme>();
    let icon = if row.is_directory {
        IconName::Folder
    } else if is_archive_name(&row.name) {
        IconName::FileArchive
    } else {
        IconName::File
    };
    let tint = if row.is_directory {
        ColorName::Blue
    } else if is_archive_name(&row.name) {
        ColorName::Grape
    } else {
        ColorName::Gray
    };
    let mut el = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .min_w(px(0.0))
        .child(Icon::new(icon).size(Size::Sm).color(tint));
    el = el.child(
        div()
            .min_w(px(0.0))
            .truncate()
            .text_color(t.text().hsla())
            .child(SharedString::from(row.name.clone())),
    );
    if row.is_encrypted {
        el = el.child(Icon::new(IconName::Lock).size(Size::Xs).color(ColorName::Yellow));
    }
    el
}

fn is_archive_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "7z", "zip", "tar", "gz", "tgz", "bz2", "tbz2", "xz", "txz", "wim", "rar",
        "cab", "iso", "lzma", "zst",
    ]
    .iter()
    .any(|ext| lower.ends_with(&format!(".{ext}")))
}

/// Windows-style attribute letters, like 7zFM's attribute column.
fn attr_string(row: &EntryRow) -> String {
    let a = row.attributes;
    let mut out = String::new();
    out.push(if a & 0x01 != 0 { 'R' } else { '.' }); // read-only
    out.push(if a & 0x02 != 0 { 'H' } else { '.' }); // hidden
    out.push(if a & 0x04 != 0 { 'S' } else { '.' }); // system
    if row.is_directory {
        out.push('D');
    } else {
        out.push('.');
    }
    out.push(if a & 0x20 != 0 { 'A' } else { '.' }); // archive
    if a & 0x0800 != 0 {
        out.push('C'); // compressed
    }
    if row.is_encrypted || a & 0x4000 != 0 {
        out.push('E'); // encrypted
    }
    out
}

fn crc_string(row: &EntryRow) -> String {
    row.crc.map(|crc| format!("{crc:08X}")).unwrap_or_default()
}

/// Case-insensitive natural compare: digit runs compare numerically.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut a_chars = a.chars().peekable();
    let mut b_chars = b.chars().peekable();
    loop {
        let a_digit = a_chars.peek().is_some_and(|c| c.is_ascii_digit());
        let b_digit = b_chars.peek().is_some_and(|c| c.is_ascii_digit());
        match (a_digit, b_digit) {
            (true, true) => {
                let a_num: String = (&mut a_chars).take_while(|c| c.is_ascii_digit()).collect();
                let b_num: String = (&mut b_chars).take_while(|c| c.is_ascii_digit()).collect();
                let a_val = a_num.trim_start_matches('0');
                let b_val = b_num.trim_start_matches('0');
                let ord = a_val
                    .len()
                    .cmp(&b_val.len())
                    .then_with(|| a_val.cmp(b_val))
                    .then_with(|| a_num.len().cmp(&b_num.len()));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (false, false) => {
                let a_next = a_chars.next();
                let b_next = b_chars.next();
                match (a_next, b_next) {
                    (None, None) => return Ordering::Equal,
                    (None, Some(_)) => return Ordering::Less,
                    (Some(_), None) => return Ordering::Greater,
                    (Some(a_c), Some(b_c)) => {
                        let ord = a_c
                            .to_ascii_lowercase()
                            .cmp(&b_c.to_ascii_lowercase());
                        if ord != Ordering::Equal {
                            return ord;
                        }
                    }
                }
            }
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
        }
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
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_cmp_orders_digits_numerically() {
        let mut names = vec!["f10", "f2", "f1", "f20"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, vec!["f1", "f2", "f10", "f20"]);
    }

    #[test]
    fn natural_cmp_is_case_insensitive() {
        assert_eq!(
            natural_cmp("Readme.md", "readme.md"),
            std::cmp::Ordering::Equal,
        );
        assert_eq!(natural_cmp("A.txt", "b.txt"), std::cmp::Ordering::Less);
    }

    #[test]
    fn format_size_scales() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2048), "2.0 KB");
        assert_eq!(format_size(3 * 1024 * 1024), "3.0 MB");
    }

    #[test]
    fn attr_letters_reflect_flags() {
        let row = EntryRow {
            id: vfs::next_node_id(),
            index: None,
            name: "x".into(),
            path: "x".into(),
            is_directory: true,
            is_encrypted: false,
            is_symlink: false,
            size: 0,
            packed: 0,
            modified: String::new(),
            attributes: 0x01 | 0x02,
            crc: Some(0xDEADBEEF),
            method: String::new(),
        };
        assert_eq!(attr_string(&row), "RH.D.");
        assert_eq!(crc_string(&row), "DEADBEEF");
    }
}
