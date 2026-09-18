//! Path-level comparison of two [`Tree`]s.

use vfs::attr;
use vfs::{Tree, VfsNodeId};
use std::collections::HashMap;

/// How an entry differs between the two trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    /// Present only in the second (working) tree.
    Added,
    /// Present only in the first (base) tree.
    Removed,
    /// Present in both but with differing comparable attributes.
    Modified,
}

/// One differing file entry of a [`DiffReport`].
#[derive(Debug, Clone)]
pub struct DiffEntry {
    /// Path inside the tree (forward slashes, no leading slash).
    pub path: String,
    pub kind: DiffKind,
    pub base_size: Option<u64>,
    pub working_size: Option<u64>,
}

impl DiffEntry {
    /// One-letter status for the changes list (`A` / `R` / `M`).
    pub fn letter(&self) -> &'static str {
        match self.kind {
            DiffKind::Added => "A",
            DiffKind::Removed => "R",
            DiffKind::Modified => "M",
        }
    }
}

/// The full path-level comparison of two trees.
#[derive(Debug, Clone, Default)]
pub struct DiffReport {
    pub entries: Vec<DiffEntry>,
}

impl DiffReport {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Aligns `base` and `working` by relative path and reports differing files.
///
/// Directories are skipped: a changed directory shows up through its
/// differing children. Two files count as [`DiffKind::Modified`] when their
/// sizes or CRCs differ; files whose size and CRC both match (or are both
/// absent on both sides) count as equal — the content layer refines a
/// suspected modification later if the caller asks for it.
pub fn tree_vs_tree(base: &Tree, working: &Tree) -> DiffReport {
    let base_files = collect_files(base);
    let working_files = collect_files(working);

    let mut entries = Vec::new();
    for (path, (base_size, base_crc)) in &base_files {
        match working_files.get(path) {
            None => entries.push(DiffEntry {
                path: path.clone(),
                kind: DiffKind::Removed,
                base_size: Some(*base_size),
                working_size: None,
            }),
            Some((working_size, working_crc)) => {
                if sizes_differ(*base_size, *working_size) || crcs_differ(*base_crc, *working_crc)
                {
                    entries.push(DiffEntry {
                        path: path.clone(),
                        kind: DiffKind::Modified,
                        base_size: Some(*base_size),
                        working_size: Some(*working_size),
                    });
                }
            }
        }
    }
    for (path, (working_size, _)) in &working_files {
        if !base_files.contains_key(path) {
            entries.push(DiffEntry {
                path: path.clone(),
                kind: DiffKind::Added,
                base_size: None,
                working_size: Some(*working_size),
            });
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    DiffReport { entries }
}

fn sizes_differ(a: u64, b: u64) -> bool {
    a != b
}

fn crcs_differ(a: Option<u64>, b: Option<u64>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a != b,
        // A CRC known on only one side is not a difference signal by itself:
        // extracted filesystem trees do not carry CRCs.
        _ => false,
    }
}

/// `(size, crc)` of every file node, keyed by tree path.
fn collect_files(tree: &Tree) -> HashMap<String, (u64, Option<u64>)> {
    let mut out = HashMap::new();
    collect_into(tree, tree.root(), &mut out);
    out
}

fn collect_into(
    tree: &Tree,
    id: VfsNodeId,
    out: &mut HashMap<String, (u64, Option<u64>)>,
) {
    for child in tree.children(id).map(|children| children.to_vec()).unwrap_or_default() {
        let Some(node) = tree.node(child) else {
            continue;
        };
        if node.is_directory {
            collect_into(tree, child, out);
            continue;
        }
        let path = tree.path_of(child).unwrap_or_else(|| node.name.clone());
        let size = node.attr(attr::SIZE).and_then(|v| v.as_u64()).unwrap_or(0);
        let crc = node.attr(attr::CRC).and_then(|v| v.as_u64());
        out.insert(path, (size, crc));
    }
}
