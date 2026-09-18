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

/// Which column the table is sorted by, and in which direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    Size,
    Packed,
    Modified,
    Crc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// A sort spec: column plus direction. Directories always sort first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub column: SortColumn,
    pub direction: SortDirection,
}

impl Sort {
    pub fn toggled(self, column: SortColumn) -> Self {
        if self.column == column {
            let direction = match self.direction {
                SortDirection::Ascending => SortDirection::Descending,
                SortDirection::Descending => SortDirection::Ascending,
            };
            Self { column, direction }
        } else {
            Self {
                column,
                direction: SortDirection::Ascending,
            }
        }
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

/// Re-sorts `rows` in place by `sort`. Directories always stay ahead of
/// files; only the key comparison follows the requested direction, with the
/// natural name order as the tiebreak.
pub fn apply_sort(rows: &mut [EntryRow], sort: Sort) {
    let reverse = sort.direction == SortDirection::Descending;
    rows.sort_by(|a, b| {
        b.is_directory
            .cmp(&a.is_directory)
            .then_with(|| {
                let ord = match sort.column {
                    SortColumn::Name => natural_cmp(&a.name, &b.name),
                    SortColumn::Size => a.size.cmp(&b.size),
                    SortColumn::Packed => a.packed.cmp(&b.packed),
                    SortColumn::Modified => a.modified.cmp(&b.modified),
                    SortColumn::Crc => a.crc.cmp(&b.crc),
                };
                if reverse { ord.reverse() } else { ord }
                    .then_with(|| natural_cmp(&a.name, &b.name))
            })
    });
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
    fn natural_cmp_orders_digits_numerically() {
        assert_eq!(natural_cmp("a2", "a10"), Ordering::Less);
        assert_eq!(natural_cmp("a10", "a2"), Ordering::Greater);
        assert_eq!(natural_cmp("A2", "a2"), Ordering::Equal);
        assert_eq!(natural_cmp("a02", "a2"), Ordering::Greater);
        assert_eq!(natural_cmp("", "a"), Ordering::Less);
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
    fn apply_sort_keeps_directories_first() {
        let mut rows = vec![
            row("b.txt", 2, false),
            row("z/", 0, true),
            row("a.txt", 10, false),
        ];
        apply_sort(
            &mut rows,
            Sort {
                column: SortColumn::Size,
                direction: SortDirection::Descending,
            },
        );
        assert_eq!(rows[0].name, "z/");
        assert_eq!(rows[1].name, "a.txt");
        assert_eq!(rows[2].name, "b.txt");
    }

    #[test]
    fn sort_toggle_flips_direction() {
        let s = Sort {
            column: SortColumn::Name,
            direction: SortDirection::Ascending,
        };
        assert_eq!(
            s.toggled(SortColumn::Name).direction,
            SortDirection::Descending
        );
        assert_eq!(s.toggled(SortColumn::Size).column, SortColumn::Size);
        assert_eq!(
            s.toggled(SortColumn::Size).direction,
            SortDirection::Ascending
        );
    }
}
