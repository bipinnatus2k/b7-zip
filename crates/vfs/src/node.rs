//! Nodes of the virtual file system.

use crate::attr::{AttrMap, AttrName, AttrValue};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_NODE_ID: AtomicU64 = AtomicU64::new(1);

/// Monotonic identifier for a VFS node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(u64);

impl NodeId {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// Allocate a fresh node id.
pub fn next_node_id() -> NodeId {
    NodeId(NEXT_NODE_ID.fetch_add(1, Ordering::Relaxed))
}

/// A node in the virtual file system.
///
/// The node itself only knows its identity, parent, name, and whether it is
/// a directory. Everything else lives in [`attrs`](VfsNode::attrs) using the
/// conventional names from [`crate::attr`], so any data source can attach
/// whatever properties it needs without changing the core types.
#[derive(Debug, Clone)]
pub struct VfsNode {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub name: String,
    pub is_directory: bool,
    pub attrs: AttrMap,
}

impl VfsNode {
    pub fn new(id: NodeId, parent: Option<NodeId>, name: impl Into<String>, is_directory: bool) -> Self {
        Self {
            id,
            parent,
            name: name.into(),
            is_directory,
            attrs: AttrMap::new(),
        }
    }

    pub fn attr(&self, name: AttrName) -> Option<&AttrValue> {
        self.attrs.get(&name)
    }

    pub fn set_attr(&mut self, name: AttrName, value: AttrValue) {
        self.attrs.insert(name, value);
    }

    pub fn remove_attr(&mut self, name: AttrName) -> Option<AttrValue> {
        self.attrs.remove(&name)
    }

    /// Merge `other`'s attributes into this node, overwriting on conflicts.
    pub fn merge_attrs(&mut self, other: &Self) {
        for (name, value) in &other.attrs {
            self.attrs.insert(*name, value.clone());
        }
    }

    /// The `archive_index` attribute, if present.
    pub fn archive_index(&self) -> Option<u32> {
        self.attr(crate::attr::ARCHIVE_INDEX).and_then(|v| v.as_u64()).map(|v| v as u32)
    }

    /// The `fs_path` attribute, if present.
    pub fn fs_path(&self) -> Option<&std::path::Path> {
        self.attr(crate::attr::FS_PATH).and_then(|v| v.as_str()).map(std::path::Path::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attr;

    #[test]
    fn node_attrs_roundtrip() {
        let mut node = VfsNode::new(next_node_id(), None, "a.txt", false);
        node.set_attr(attr::SIZE, AttrValue::UInt(42));
        node.set_attr(attr::ENCRYPTED, AttrValue::Bool(true));
        assert_eq!(node.attr(attr::SIZE).and_then(|v| v.as_u64()), Some(42));
        assert_eq!(node.attr(attr::ENCRYPTED).and_then(|v| v.as_bool()), Some(true));
        node.remove_attr(attr::SIZE);
        assert!(node.attr(attr::SIZE).is_none());
    }

    #[test]
    fn merge_attrs_overwrites() {
        let mut a = VfsNode::new(next_node_id(), None, "x", false);
        a.set_attr(attr::SIZE, AttrValue::UInt(1));
        let mut b = VfsNode::new(next_node_id(), None, "x", false);
        b.set_attr(attr::SIZE, AttrValue::UInt(2));
        b.set_attr(attr::CRC, AttrValue::UInt(3));
        a.merge_attrs(&b);
        assert_eq!(a.attr(attr::SIZE).and_then(|v| v.as_u64()), Some(2));
        assert_eq!(a.attr(attr::CRC).and_then(|v| v.as_u64()), Some(3));
    }
}
