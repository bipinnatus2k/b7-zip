//! Overlay: merge a working tree (e.g. an extracted-file tree) on top of a
//! base tree (e.g. the original archive entry tree), tracking dirty nodes.
//!
//! The overlay never interprets node attributes itself: it aligns nodes by
//! relative path, merges attributes (working side wins), and records what
//! changed. [`crate::diff::build_changeset`] later turns the dirty state
//! into a commit-ready [`Changeset`].

use crate::node::{NodeId, VfsNode, next_node_id};
use crate::tree::Tree;
use std::collections::HashMap;

/// How a node changed relative to the base tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirtyState {
    Added,
    Modified,
    Deleted,
    Renamed,
}

/// The overlay view over a base tree.
#[derive(Debug, Clone)]
pub struct Overlay {
    base: Tree,
    working: Tree,
    dirty: HashMap<NodeId, DirtyState>,
}

impl Overlay {
    /// Create an overlay whose working view starts as a copy of `base`.
    pub fn new(base: Tree) -> Self {
        let working = base.clone();
        Self {
            base,
            working,
            dirty: HashMap::new(),
        }
    }

    pub fn base(&self) -> &Tree {
        &self.base
    }

    pub fn working(&self) -> &Tree {
        &self.working
    }

    pub fn dirty(&self) -> &HashMap<NodeId, DirtyState> {
        &self.dirty
    }

    pub fn is_dirty(&self, id: NodeId) -> bool {
        self.dirty.contains_key(&id)
    }

    pub fn has_changes(&self) -> bool {
        !self.dirty.is_empty()
    }

    pub fn dirty_state(&self, id: NodeId) -> Option<DirtyState> {
        self.dirty.get(&id).copied()
    }

    /// Mark a node dirty (or replace its state).
    pub fn mark(&mut self, id: NodeId, state: DirtyState) {
        self.dirty.insert(id, state);
    }

    /// Clear a node's dirty state.
    pub fn clear(&mut self, id: NodeId) {
        self.dirty.remove(&id);
    }

    /// Discard all pending changes: working tree reverts to base.
    pub fn discard_pending(&mut self) {
        self.working = self.base.clone();
        self.dirty.clear();
    }

    /// After a successful commit: base becomes working, dirty is cleared.
    pub fn on_commit_success(&mut self) {
        self.base = self.working.clone();
        self.dirty.clear();
    }

    /// Merge the nodes of `source` into the working view.
    ///
    /// Nodes are aligned by relative path. A node present in `source` but
    /// not in the working tree is inserted and marked [`DirtyState::Added`].
    /// A matching node gets `source`'s attributes merged in; if its size or
    /// modification time changed, it is marked [`DirtyState::Modified`].
    /// Nodes in the working tree that `source` does not contain are left
    /// untouched (deletions are explicit via [`Overlay::remove_path`]).
    pub fn sync_from(&mut self, source: &Tree) {
        let mut pending: Vec<VfsNode> = Vec::new();
        collect_all(source, source.root(), &mut pending);
        for node in pending {
            let path = source.path_of(node.id).unwrap_or_default();
            if let Some(existing_id) = self.working.resolve_path(&path) {
                let changed = apply_source_attrs(&mut self.working, existing_id, &node);
                if changed {
                    match self.dirty.get(&existing_id).copied() {
                        Some(DirtyState::Added | DirtyState::Renamed) => {}
                        Some(DirtyState::Deleted) => {
                            let state = if self.base.node(existing_id).is_some() {
                                DirtyState::Modified
                            } else {
                                DirtyState::Added
                            };
                            self.dirty.insert(existing_id, state);
                        }
                        _ => {
                            self.dirty.insert(existing_id, DirtyState::Modified);
                        }
                    }
                }
            } else if let Some(parent_path) = parent_of(&path)
                && let Some(parent_id) = self.working.resolve_path(&parent_path)
                && let Some(new_node) = self.make_working_copy(&node, Some(parent_id))
            {
                if self.working.insert_node(new_node.clone()).is_ok() {
                    self.dirty.insert(new_node.id, DirtyState::Added);
                }
            } else {
                // Path not resolvable (parent missing): insert under the root
                // as a last resort, preserving the relative path via attrs.
                let mut new_node = self
                    .make_working_copy(&node, Some(self.working.root()))
                    .unwrap();
                new_node.name = path;
                if self.working.insert_node(new_node.clone()).is_ok() {
                    self.dirty.insert(new_node.id, DirtyState::Added);
                }
            }
        }
    }

    /// Remove the node at `path` from the working view and mark it deleted.
    /// Returns true if a node was removed.
    pub fn remove_path(&mut self, path: &str) -> bool {
        let Some(id) = self.working.resolve_path(path) else {
            return false;
        };
        let existed_in_base = self.base.node(id).is_some();
        if self.working.remove_node(id).is_ok() {
            if existed_in_base {
                self.dirty.insert(id, DirtyState::Deleted);
            } else {
                // Removing a node that was added in this overlay cancels the
                // addition instead of producing an un-mappable Delete op.
                self.dirty.remove(&id);
            }
            return true;
        }
        false
    }

    /// Rename the node at `from` to `to` (both relative paths).
    pub fn rename_path(&mut self, from: &str, to: &str) -> Result<(), OverlayError> {
        let id = self
            .working
            .resolve_path(from)
            .ok_or(OverlayError::NotFound(from.to_string()))?;
        let new_name = to.rsplit('/').next().unwrap_or(to).to_string();
        let new_parent_id = match parent_of(to) {
            Some(parent) => self
                .working
                .resolve_path(&parent)
                .ok_or_else(|| OverlayError::NotFound(parent))?,
            None => self.working.root(),
        };
        self.working
            .reparent_node(id, Some(new_parent_id), &new_name)
            .map_err(|e| OverlayError::Tree(e.to_string()))?;
        self.dirty.insert(id, DirtyState::Renamed);
        Ok(())
    }

    fn make_working_copy(&self, source: &VfsNode, parent: Option<NodeId>) -> Option<VfsNode> {
        let mut node = source.clone();
        node.id = next_node_id();
        node.parent = parent;
        Some(node)
    }
}

/// Errors produced by overlay operations.
#[derive(Debug, thiserror::Error)]
pub enum OverlayError {
    #[error("path not found: {0}")]
    NotFound(String),
    #[error("tree error: {0}")]
    Tree(String),
}

fn apply_source_attrs(working: &mut Tree, id: NodeId, source: &VfsNode) -> bool {
    let Some(existing) = working.node(id) else {
        return false;
    };
    // Only compare attributes that *both* sides provide: a source without
    // an attribute (e.g. an fs tree that did not populate mtime) is not a
    // change signal.
    let size_changed = match (
        existing.attr(crate::attr::SIZE),
        source.attr(crate::attr::SIZE),
    ) {
        (Some(a), Some(b)) => a != b,
        _ => false,
    };
    let mtime_changed = match (
        existing.attr(crate::attr::MODIFIED),
        source.attr(crate::attr::MODIFIED),
    ) {
        (Some(a), Some(b)) => a != b,
        _ => false,
    };
    let crc_changed = match (
        existing.attr(crate::attr::CRC),
        source.attr(crate::attr::CRC),
    ) {
        (Some(a), Some(b)) => a != b,
        _ => false,
    };
    let changed = size_changed || mtime_changed || crc_changed;
    working
        .node_mut(id)
        .expect("node exists")
        .merge_attrs(source);
    changed
}

fn collect_all(tree: &Tree, id: NodeId, out: &mut Vec<VfsNode>) {
    if let Some(kids) = tree.children(id).map(|v| v.to_vec()) {
        for kid in kids {
            if let Some(node) = tree.node(kid) {
                out.push(node.clone());
                collect_all(tree, kid, out);
            }
        }
    }
}

fn parent_of(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    let idx = trimmed.rfind('/')?;
    Some(trimmed[..idx].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attr;
    use crate::attr::AttrValue;

    fn sample_base() -> Tree {
        let mut tree = Tree::new(next_node_id());
        let root = tree.root();
        tree.insert_node(VfsNode::new(root, None, "", true))
            .unwrap();

        let mut dir = VfsNode::new(next_node_id(), Some(root), "dir", true);
        let dir_id = dir.id;
        tree.insert_node(dir.clone()).unwrap();

        let mut f1 = VfsNode::new(next_node_id(), Some(root), "a.txt", false);
        f1.set_attr(attr::SIZE, AttrValue::UInt(100));
        f1.set_attr(attr::ARCHIVE_INDEX, AttrValue::UInt(0));
        tree.insert_node(f1.clone()).unwrap();

        let mut inner = VfsNode::new(next_node_id(), Some(dir_id), "b.txt", false);
        inner.set_attr(attr::SIZE, AttrValue::UInt(50));
        inner.set_attr(attr::ARCHIVE_INDEX, AttrValue::UInt(1));
        tree.insert_node(inner).unwrap();
        tree
    }

    #[test]
    fn new_overlay_has_no_changes() {
        let overlay = Overlay::new(sample_base());
        assert!(!overlay.has_changes());
        assert_eq!(overlay.working().len(), overlay.base().len());
    }

    #[test]
    fn sync_adds_new_files() {
        let mut overlay = Overlay::new(sample_base());
        let mut fs_tree = Tree::new(next_node_id());
        let root = fs_tree.root();
        fs_tree
            .insert_node(VfsNode::new(root, None, "", true))
            .unwrap();
        let mut extra = VfsNode::new(next_node_id(), Some(root), "extra.txt", false);
        extra.set_attr(attr::SIZE, AttrValue::UInt(10));
        fs_tree.insert_node(extra.clone()).unwrap();

        overlay.sync_from(&fs_tree);
        let id = overlay.working().resolve_path("extra.txt").unwrap();
        assert_eq!(overlay.dirty_state(id), Some(DirtyState::Added));
    }

    #[test]
    fn sync_marks_modified_when_size_changes() {
        let mut overlay = Overlay::new(sample_base());
        let mut fs_tree = Tree::new(next_node_id());
        let root = fs_tree.root();
        fs_tree
            .insert_node(VfsNode::new(root, None, "", true))
            .unwrap();
        let mut changed = VfsNode::new(next_node_id(), Some(root), "a.txt", false);
        changed.set_attr(attr::SIZE, AttrValue::UInt(200));
        fs_tree.insert_node(changed).unwrap();

        overlay.sync_from(&fs_tree);
        let id = overlay.working().resolve_path("a.txt").unwrap();
        assert_eq!(overlay.dirty_state(id), Some(DirtyState::Modified));
        // Working node picked up the new size.
        assert_eq!(
            overlay
                .working()
                .node(id)
                .unwrap()
                .attr(attr::SIZE)
                .and_then(|v| v.as_u64()),
            Some(200)
        );
    }

    #[test]
    fn sync_unchanged_file_stays_clean() {
        let mut overlay = Overlay::new(sample_base());
        let mut fs_tree = Tree::new(next_node_id());
        let root = fs_tree.root();
        fs_tree
            .insert_node(VfsNode::new(root, None, "", true))
            .unwrap();
        let same = VfsNode::new(next_node_id(), Some(root), "a.txt", false);
        fs_tree.insert_node(same).unwrap();
        overlay.sync_from(&fs_tree);
        assert!(!overlay.has_changes());
    }

    #[test]
    fn remove_and_rename() {
        let mut overlay = Overlay::new(sample_base());
        assert!(overlay.remove_path("a.txt"));
        let removed_id = overlay.working().resolve_path("a.txt");
        assert!(removed_id.is_none());
        assert_eq!(overlay.dirty().len(), 1);

        overlay.rename_path("dir", "docs").unwrap();
        assert!(overlay.working().resolve_path("docs").is_some());
        assert!(overlay.working().resolve_path("dir").is_none());
        assert_eq!(overlay.dirty().len(), 2);
    }

    #[test]
    fn discard_and_commit_reset_state() {
        let mut overlay = Overlay::new(sample_base());
        overlay.remove_path("a.txt");
        assert!(overlay.has_changes());
        overlay.discard_pending();
        assert!(!overlay.has_changes());
        assert!(overlay.working().resolve_path("a.txt").is_some());

        overlay.remove_path("a.txt");
        overlay.on_commit_success();
        assert!(!overlay.has_changes());
        assert!(overlay.base().resolve_path("a.txt").is_none());
    }
}
