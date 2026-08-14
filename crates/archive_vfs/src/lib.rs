//! Build [`vfs::Tree`]s from engine-agnostic archive entry lists.
//!
//! This crate is the "archive VFS": it maps [`bit7z_rs::ArchiveEntry`]s into
//! [`vfs::VfsNode`]s, filling the conventional attribute names (size,
//! packed_size, modified, crc, encrypted, archive_index, ...) so that
//! overlays and diffs can work with archive trees exactly like with any
//! other source.

use bit7z_rs::ArchiveEntry;
use std::collections::HashMap;
use vfs::{AttrValue, Tree, VfsNode, next_node_id};

/// Build a tree from archive entries, synthesizing missing directory nodes.
pub fn build_tree(entries: &[ArchiveEntry]) -> Tree {
    let root_id = next_node_id();
    let mut tree = Tree::new(root_id);
    let mut path_map: HashMap<String, vfs::NodeId> = HashMap::new();

    tree.insert_node(VfsNode::new(root_id, None, "", true))
        .expect("fresh tree accepts root");
    path_map.insert(String::new(), root_id);

    for entry in entries {
        let normalized = entry.path.trim_end_matches('/').replace('\\', "/");
        let parent_path = parent_of(&normalized);
        let parent_id = ensure_dir(&mut tree, &mut path_map, &parent_path, root_id);

        let node_id = next_node_id();
        let name = normalized.rsplit('/').next().unwrap_or(&normalized).to_string();
        let mut node = VfsNode::new(node_id, Some(parent_id), name, entry.is_directory);
        fill_attrs(&mut node, entry);
        if tree.insert_node(node).is_ok() {
            path_map.insert(normalized, node_id);
        }
    }
    tree
}

fn fill_attrs(node: &mut VfsNode, entry: &ArchiveEntry) {
    use vfs::attr;
    node.set_attr(attr::SIZE, AttrValue::UInt(entry.size));
    node.set_attr(attr::PACKED_SIZE, AttrValue::UInt(entry.packed_size));
    node.set_attr(attr::ARCHIVE_INDEX, AttrValue::UInt(entry.index as u64));
    node.set_attr(attr::SOURCE, AttrValue::String("archive".into()));
    if let Some(crc) = entry.crc {
        node.set_attr(attr::CRC, AttrValue::UInt(crc as u64));
    }
    if entry.is_encrypted {
        node.set_attr(attr::ENCRYPTED, AttrValue::Bool(true));
    }
    if entry.is_symlink {
        node.set_attr(attr::SYMLINK, AttrValue::Bool(true));
    }
    if let Some(dt) = entry.modified {
        node.set_attr(attr::MODIFIED, AttrValue::DateTime(dt));
    }
    if let Some(dt) = entry.created {
        node.set_attr(attr::CREATED, AttrValue::DateTime(dt));
    }
    if let Some(dt) = entry.accessed {
        node.set_attr(attr::ACCESSED, AttrValue::DateTime(dt));
    }
    if let Some(v) = entry.attributes {
        node.set_attr(attr::ATTRIBUTES, AttrValue::UInt(v as u64));
    }
    if let Some(v) = entry.posix_attrib {
        node.set_attr(attr::POSIX_MODE, AttrValue::UInt(v as u64));
    }
    if let Some(v) = entry.host_os {
        node.set_attr(attr::HOST_OS, AttrValue::UInt(v as u64));
    }
    if let Some(v) = &entry.compression_method {
        node.set_attr(attr::METHOD, AttrValue::String(v.clone()));
    }
    if let Some(v) = &entry.extension {
        node.set_attr(attr::EXTENSION, AttrValue::String(v.clone()));
    }
}

fn parent_of(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(idx) => trimmed[..idx].to_string(),
        None => String::new(),
    }
}

fn ensure_dir(
    tree: &mut Tree,
    path_map: &mut HashMap<String, vfs::NodeId>,
    path: &str,
    root_id: vfs::NodeId,
) -> vfs::NodeId {
    if path.is_empty() {
        return root_id;
    }
    if let Some(&id) = path_map.get(path) {
        return id;
    }
    let parent_path = parent_of(path);
    let parent_id = ensure_dir(tree, path_map, &parent_path, root_id);
    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    let node_id = next_node_id();
    let node = VfsNode::new(node_id, Some(parent_id), name, true);
    if tree.insert_node(node).is_ok() {
        path_map.insert(path.to_string(), node_id);
    }
    node_id
}

#[cfg(test)]
mod tests {
    use super::*;
    use bit7z_rs::ArchiveEntry;
    use vfs::attr;

    fn entry(index: u32, path: &str, size: u64) -> ArchiveEntry {
        ArchiveEntry {
            index,
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            path: path.to_string(),
            size,
            packed_size: size / 2,
            is_directory: false,
            is_encrypted: false,
            is_symlink: false,
            crc: Some(0x1234),
            modified: None,
            created: None,
            accessed: None,
            attributes: None,
            posix_attrib: None,
            host_os: None,
            compression_method: None,
            comment: None,
            user: None,
            group: None,
            extension: None,
            hardlink: None,
        }
    }

    #[test]
    fn builds_hierarchy_with_synthetic_dirs() {
        let entries = vec![
            entry(0, "a/b/c.txt", 10),
            entry(1, "a/b/d.txt", 20),
            entry(2, "root.txt", 30),
        ];
        let tree = build_tree(&entries);
        assert!(tree.resolve_path("a/b/c.txt").is_some());
        assert!(tree.resolve_path("a/b/d.txt").is_some());
        assert!(tree.resolve_path("root.txt").is_some());
        let dir = tree.resolve_path("a/b").unwrap();
        assert!(tree.node(dir).unwrap().is_directory);
        assert_eq!(tree.children(dir).unwrap().len(), 2);
    }

    #[test]
    fn attrs_are_filled() {
        let entries = vec![entry(7, "f.bin", 42)];
        let tree = build_tree(&entries);
        let id = tree.resolve_path("f.bin").unwrap();
        let node = tree.node(id).unwrap();
        assert_eq!(node.attr(attr::SIZE).and_then(|v| v.as_u64()), Some(42));
        assert_eq!(node.attr(attr::ARCHIVE_INDEX).and_then(|v| v.as_u64()), Some(7));
        assert_eq!(node.attr(attr::CRC).and_then(|v| v.as_u64()), Some(0x1234));
        assert_eq!(node.attr(attr::SOURCE).and_then(|v| v.as_str()), Some("archive"));
    }

    #[test]
    fn backslash_paths_are_normalized() {
        let entries = vec![entry(0, "dir\\file.txt", 1)];
        let tree = build_tree(&entries);
        assert!(tree.resolve_path("dir/file.txt").is_some());
    }
}
