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
            Ordering::Equal,
        );
        assert_eq!(natural_cmp("A.txt", "b.txt"), Ordering::Less);
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
