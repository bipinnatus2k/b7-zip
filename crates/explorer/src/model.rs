//! Pure archive-explorer model: row snapshots, path/navigation helpers,
//! index expansion, formatting and sorting. No GUI dependencies.

use session::ArchiveSession;
use std::cmp::Ordering;
use std::sync::{Arc, Mutex};
use vfs::attr;
use vfs::{NodeId, Tree, VfsNode};

/// One row of the file table: a snapshot of a directory entry.
#[derive(Debug, Clone)]
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

/// Logical pixel widths of the table's fixed columns. The single source for
/// both the view (header + rows) and the responsive visibility budget.
/// The name column is flexible; these are the rest.
pub const NAME_MIN_WIDTH: f32 = 140.0;
pub const SIZE_WIDTH: f32 = 76.0;
pub const PACKED_WIDTH: f32 = 76.0;
pub const MODIFIED_WIDTH: f32 = 118.0;
pub const ATTRIBUTES_WIDTH: f32 = 70.0;
pub const CRC_WIDTH: f32 = 74.0;
pub const METHOD_WIDTH: f32 = 60.0;

/// Approximate horizontal padding a cell adds around its content (px_2 both
/// sides), counted once per column when budgeting the table.
const CELL_PADDING: f32 = 16.0;

/// Width at which the table shows every column in full. Inside a narrower
/// container the content keeps this minimum and the table scrolls
/// horizontally — columns are never compressed or hidden to fit.
pub const TOTAL_TABLE_WIDTH: f32 = NAME_MIN_WIDTH
    + SIZE_WIDTH
    + PACKED_WIDTH
    + MODIFIED_WIDTH
    + ATTRIBUTES_WIDTH
    + CRC_WIDTH
    + METHOD_WIDTH
    + CELL_PADDING * 7.0;

/// Floor for every user-resizable column, so a drag cannot swallow content.
pub const COLUMN_MIN_WIDTH: f32 = 40.0;

/// The optional (non-Name) table columns: shown in the order they appear in
/// the column list, resizable and hideable individually. The Name column is
/// not part of this — it is always first and flexes to the remaining width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnKind {
    Size,
    Packed,
    Modified,
    Attributes,
    Crc,
    Method,
}

pub const ALL_COLUMN_KINDS: [ColumnKind; 6] = [
    ColumnKind::Size,
    ColumnKind::Packed,
    ColumnKind::Modified,
    ColumnKind::Attributes,
    ColumnKind::Crc,
    ColumnKind::Method,
];

impl ColumnKind {
    /// Header caption.
    pub fn title(self) -> &'static str {
        match self {
            ColumnKind::Size => "Size",
            ColumnKind::Packed => "Packed",
            ColumnKind::Modified => "Modified",
            ColumnKind::Attributes => "Attributes",
            ColumnKind::Crc => "CRC",
            ColumnKind::Method => "Method",
        }
    }

    /// Width a fresh column starts at.
    pub fn default_width(self) -> f32 {
        match self {
            ColumnKind::Size => SIZE_WIDTH,
            ColumnKind::Packed => PACKED_WIDTH,
            ColumnKind::Modified => MODIFIED_WIDTH,
            ColumnKind::Attributes => ATTRIBUTES_WIDTH,
            ColumnKind::Crc => CRC_WIDTH,
            ColumnKind::Method => METHOD_WIDTH,
        }
    }
}

/// One visible column with its user-chosen width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Column {
    pub kind: ColumnKind,
    pub width: f32,
}

/// The initial column set: every column visible at its default width.
pub fn default_columns() -> Vec<Column> {
    ALL_COLUMN_KINDS
        .iter()
        .copied()
        .map(|kind| Column {
            kind,
            width: kind.default_width(),
        })
        .collect()
}

/// The content's minimum width with these columns: the name column's width
/// (its dragged fixed width once the user has resized it, else its flex
/// floor), each visible column's width, and one padding allowance per column
/// (including Name) — the dynamic form of [`TOTAL_TABLE_WIDTH`].
pub fn total_table_width(columns: &[Column], name_width: Option<f32>) -> f32 {
    name_width.unwrap_or(NAME_MIN_WIDTH)
        + columns.iter().map(|column| column.width).sum::<f32>()
        + CELL_PADDING * (columns.len() as f32 + 1.0)
}

/// Wraps a column-key comparator with the file-manager invariants: sorted
/// rows keep directories ahead of files regardless of key or direction, and
/// equal keys fall back to the natural name order. Every sortable column the
/// table registers goes through this.
pub fn dirs_first(
    key: impl Fn(&EntryRow, &EntryRow) -> std::cmp::Ordering,
) -> impl Fn(&EntryRow, &EntryRow) -> std::cmp::Ordering {
    move |a, b| {
        b.is_directory
            .cmp(&a.is_directory)
            .then_with(|| key(a, b))
            .then_with(|| natural_cmp(&a.name, &b.name))
    }
}

/// Rebuild the rows of `current_path` from a session's working overlay.
pub fn rows_for_path(
    session: &Arc<Mutex<ArchiveSession>>,
    current_path: &str,
) -> Vec<EntryRow> {
    let session = session.lock().expect("session lock poisoned");
    let tree = session.overlay().working();
    let mut rows: Vec<EntryRow> = Vec::new();
    if let Some(dir_id) = tree.resolve_path(current_path) {
        if let Some(children) = tree.children(dir_id) {
            for &id in children {
                let Some(node) = tree.node(id) else { continue };
                rows.push(row_of(tree, current_path, node, id));
            }
        }
    }
    // Directories first, then files, each sorted naturally.
    rows.sort_by(|a, b| {
        b.is_directory
            .cmp(&a.is_directory)
            .then_with(|| natural_cmp(&a.name, &b.name))
    });
    rows
}

/// Build a row snapshot for one VFS node.
pub fn row_of(tree: &Tree, current_path: &str, node: &VfsNode, id: NodeId) -> EntryRow {
    let path = tree.path_of(id).unwrap_or_else(|| {
        if current_path.is_empty() {
            node.name.clone()
        } else {
            format!("{}/{}", current_path, node.name)
        }
    });
    let modified = node
        .attr(attr::MODIFIED)
        .and_then(|v| v.as_datetime())
        .map(|dt| {
            format!(
                "{}-{:02}-{:02} {:02}:{:02}",
                dt.year(),
                dt.month(),
                dt.day(),
                dt.hour(),
                dt.minute()
            )
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

/// Collect the archive indices of `id` and everything below it.
pub fn collect_indices(tree: &Tree, id: NodeId, out: &mut Vec<u32>) {
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

/// Whether a file name looks like a supported archive.
pub fn is_archive_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "7z", "zip", "tar", "gz", "tgz", "bz2", "tbz2", "xz", "txz", "wim", "rar", "cab", "iso",
        "lzma", "zst",
    ]
    .iter()
    .any(|ext| lower.ends_with(&format!(".{ext}")))
}

/// Windows-style attribute letters, like 7zFM's attribute column.
pub fn attr_string(row: &EntryRow) -> String {
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

/// Format a CRC row value as `XXXXXXXX`.
pub fn crc_string(row: &EntryRow) -> String {
    row.crc
        .map(|crc| format!("{crc:08X}"))
        .unwrap_or_default()
}

/// Case-insensitive natural compare: digit runs compare numerically.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
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

/// Format a byte count in a human-readable form. Single home in the `ui`
/// design system; re-exported so the file table and properties dialog use
/// one implementation.
pub use ui::util::format_size;

#[cfg(test)]
mod tests {
    use super::*;
    use vfs::next_node_id;

    fn row(name: &str, size: u64, is_directory: bool) -> EntryRow {
        EntryRow {
            id: next_node_id(),
            index: None,
            name: name.into(),
            path: name.into(),
            is_directory,
            is_encrypted: false,
            is_symlink: false,
            size,
            packed: 0,
            modified: "2026-01-01 00:00".into(),
            attributes: 0,
            crc: None,
            method: String::new(),
        }
    }

    #[test]
    fn dynamic_total_matches_the_all_columns_constant() {
        assert_eq!(
            total_table_width(&default_columns(), None),
            TOTAL_TABLE_WIDTH
        );
        // A dragged Name width replaces its flex floor one-for-one.
        let dragged = total_table_width(&default_columns(), Some(NAME_MIN_WIDTH + 60.0));
        assert_eq!(dragged, TOTAL_TABLE_WIDTH + 60.0);
    }

    #[test]
    fn hiding_a_column_shrinks_the_total() {
        let mut columns = default_columns();
        columns.retain(|column| column.kind != ColumnKind::Method);
        let expected = TOTAL_TABLE_WIDTH - (METHOD_WIDTH + CELL_PADDING);
        assert!((total_table_width(&columns, None) - expected).abs() < f32::EPSILON);
    }

    #[test]
    fn natural_cmp_orders_digits_numerically() {
        assert_eq!(natural_cmp("a2", "a10"), Ordering::Less);
        assert_eq!(natural_cmp("a10", "a2"), Ordering::Greater);
        assert_eq!(natural_cmp("A2", "a2"), Ordering::Equal);
        assert_eq!(natural_cmp("a02", "a2"), Ordering::Greater);
        assert_eq!(natural_cmp("", "a"), Ordering::Less);
    }

    #[test]
    fn total_table_width_covers_every_column() {
        let columns = NAME_MIN_WIDTH
            + SIZE_WIDTH
            + PACKED_WIDTH
            + MODIFIED_WIDTH
            + ATTRIBUTES_WIDTH
            + CRC_WIDTH
            + METHOD_WIDTH;
        assert_eq!(TOTAL_TABLE_WIDTH, columns + CELL_PADDING * 7.0);
        assert!(TOTAL_TABLE_WIDTH > columns, "the budget includes cell padding");
    }

    #[test]
    fn attr_string_letters() {
        let mut r = row("x", 0, false);
        assert_eq!(attr_string(&r), ".....");
        r.attributes = 0x01; // read-only
        assert_eq!(attr_string(&r), "R....");
        r.attributes = 0x20; // archive
        assert_eq!(attr_string(&r), "....A");
        r.is_directory = true;
        assert_eq!(attr_string(&r), "...DA");
    }

    #[test]
    fn dirs_first_keeps_directories_ahead_of_files() {
        // Size ascending: the directory leads regardless of its key value,
        // then files follow in ascending size.
        let by_size = dirs_first(|a: &EntryRow, b: &EntryRow| a.size.cmp(&b.size));
        let mut rows = vec![
            row("b.txt", 2, false),
            row("z/", 0, true),
            row("a.txt", 10, false),
        ];
        rows.sort_by(|a, b| by_size(a, b));
        assert_eq!(rows[0].name, "z/");
        assert_eq!(rows[1].name, "b.txt");
        assert_eq!(rows[2].name, "a.txt");
    }
}
