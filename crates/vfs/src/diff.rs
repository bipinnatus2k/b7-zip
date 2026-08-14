//! Diffing: turn an overlay's dirty state into a [`Changeset`].
//!
//! The overlay records *which* nodes changed and how; this module re-walks
//! those nodes against the untouched base tree (the original archive entry
//! tree) to reconstruct the exact file structure that must be committed.

use crate::changeset::{AddOp, Changeset, DeleteOp, ModifyOp, RenameOp};
use crate::node::NodeId;
use crate::overlay::DirtyState;
use crate::tree::Tree;
use std::collections::HashMap;

/// Build a [`Changeset`] from the overlay's dirty map.
///
/// * `base` — the untouched tree (e.g. the original archive entries).
/// * `working` — the current overlay view.
/// * `dirty` — per-node dirty states recorded by the overlay.
pub fn build_changeset(
    base: &Tree,
    working: &Tree,
    dirty: &HashMap<NodeId, DirtyState>,
) -> Changeset {
    let mut changeset = Changeset::new();

    for (node_id, state) in dirty {
        match state {
            DirtyState::Added => {
                if let Some(node) = working.node(*node_id) {
                    let archive_path = working
                        .path_of(*node_id)
                        .unwrap_or_else(|| node.name.clone());
                    let fs_path = node
                        .fs_path()
                        .map(ToOwned::to_owned)
                        .unwrap_or_else(|| std::path::PathBuf::from(&archive_path));
                    changeset.additions.push(AddOp { fs_path, archive_path });
                }
            }
            DirtyState::Modified => {
                if let Some(node) = working.node(*node_id)
                    && let Some(archive_index) = base.node(*node_id).and_then(|n| n.archive_index())
                {
                    let fs_path = node
                        .fs_path()
                        .map(ToOwned::to_owned)
                        .unwrap_or_default();
                    changeset.modifications.push(ModifyOp { archive_index, fs_path });
                }
            }
            DirtyState::Renamed => {
                if let Some(archive_index) = base.node(*node_id).and_then(|n| n.archive_index())
                    && let Some(node) = working.node(*node_id)
                {
                    let new_path = working
                        .path_of(*node_id)
                        .unwrap_or_else(|| node.name.clone());
                    changeset.renames.push(RenameOp { archive_index, new_path });
                }
            }
            DirtyState::Deleted => {
                if let Some(archive_index) = base.node(*node_id).and_then(|n| n.archive_index()) {
                    changeset.deletions.push(DeleteOp { archive_index });
                }
            }
        }
    }

    changeset
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attr;
    use crate::attr::AttrValue;
    use crate::next_node_id;

    fn tree_with(root_name: &str) -> Tree {
        let mut tree = Tree::new(next_node_id());
        tree.insert_node(crate::node::VfsNode::new(tree.root(), None, "", true)).unwrap();
        tree
    }

    #[test]
    fn added_file_becomes_add_op() {
        let mut base = tree_with("root");
        let root = base.root();
        let mut working = base.clone();

        let id = next_node_id();
        let mut node = crate::node::VfsNode::new(id, Some(root), "new.txt", false);
        node.set_attr(attr::FS_PATH, AttrValue::String("C:/tmp/new.txt".into()));
        working.insert_node(node).unwrap();

        let mut dirty = HashMap::new();
        dirty.insert(id, DirtyState::Added);
        let cs = build_changeset(&base, &working, &dirty);
        assert_eq!(cs.additions.len(), 1);
        assert_eq!(cs.additions[0].archive_path, "new.txt");
        assert_eq!(cs.additions[0].fs_path.to_string_lossy(), "C:/tmp/new.txt");
    }

    #[test]
    fn modified_and_deleted_and_renamed() {
        let mut base = tree_with("root");
        let root = base.root();
        let mut working = base.clone();

        // base entries
        let mod_id = next_node_id();
        let mut mod_node = crate::node::VfsNode::new(mod_id, Some(root), "mod.txt", false);
        mod_node.set_attr(attr::ARCHIVE_INDEX, AttrValue::UInt(5));
        base.insert_node(mod_node.clone()).unwrap();
        working.insert_node(mod_node).unwrap();

        let del_id = next_node_id();
        let mut del_node = crate::node::VfsNode::new(del_id, Some(root), "del.txt", false);
        del_node.set_attr(attr::ARCHIVE_INDEX, AttrValue::UInt(6));
        base.insert_node(del_node).unwrap();

        let ren_id = next_node_id();
        let mut ren_node = crate::node::VfsNode::new(ren_id, Some(root), "old.txt", false);
        ren_node.set_attr(attr::ARCHIVE_INDEX, AttrValue::UInt(7));
        base.insert_node(ren_node.clone()).unwrap();
        ren_node.name = "new.txt".into();
        working.insert_node(ren_node).unwrap();

        let mut dirty = HashMap::new();
        dirty.insert(mod_id, DirtyState::Modified);
        dirty.insert(del_id, DirtyState::Deleted);
        dirty.insert(ren_id, DirtyState::Renamed);

        let cs = build_changeset(&base, &working, &dirty);
        assert_eq!(cs.modifications.len(), 1);
        assert_eq!(cs.modifications[0].archive_index, 5);
        assert_eq!(cs.deletions.len(), 1);
        assert_eq!(cs.deletions[0].archive_index, 6);
        assert_eq!(cs.renames.len(), 1);
        assert_eq!(cs.renames[0].archive_index, 7);
        assert_eq!(cs.renames[0].new_path, "new.txt");
    }
}
