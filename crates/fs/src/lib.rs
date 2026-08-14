//! Filesystem VFS: a [`vfs::Tree`] backed by a real directory on disk.
//!
//! The [`FsTree`] keeps the tree in sync with the physical filesystem: it
//! can be built by scanning a directory, and it can be updated incrementally
//! from [`fs_watcher::FsEvent`]s. Nodes carry the conventional attributes
//! (`size`, `modified`, `fs_path`, `source`) so the generic overlay/diff
//! machinery can merge them with trees from other sources (e.g. archives).

use fs_watcher::{FsEvent, FsEventKind};
use jiff::civil::DateTime;
use std::path::{Path, PathBuf};
use vfs::{AttrValue, Tree, VfsNode, attr, next_node_id};

/// A tree mirroring a directory on disk.
#[derive(Debug, Clone)]
pub struct FsTree {
    tree: Tree,
    root: PathBuf,
}

impl FsTree {
    /// Scan `root` recursively and build the tree.
    pub fn scan(root: &Path) -> std::io::Result<Self> {
        let root = root.to_path_buf();
        let mut tree = Tree::new(next_node_id());
        tree.insert_node(VfsNode::new(tree.root(), None, "", true))
            .expect("fresh tree accepts root");
        scan_dir(&root, &root, tree.root(), &mut tree)?;
        Ok(Self { tree, root })
    }

    /// The watched/scanned root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The underlying tree.
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    /// Apply a watcher event, keeping the tree in sync.
    pub fn apply_event(&mut self, event: &FsEvent) -> Result<Vec<FsChange>, FsError> {
        // Ignore events for the root itself.
        if event.path == self.root {
            return Ok(Vec::new());
        }
        let relative = event
            .path
            .strip_prefix(&self.root)
            .map_err(|_| FsError::OutsideRoot(event.path.clone()))?;
        let rel_str = to_forward_slashes(relative);

        match &event.kind {
            FsEventKind::Create => {
                let metadata = std::fs::metadata(&event.path)?;
                let parent_rel = parent_of(&rel_str);
                let parent_id = self
                    .tree
                    .resolve_path(&parent_rel)
                    .ok_or_else(|| FsError::ParentMissing(parent_rel.clone()))?;
                let node_id = next_node_id();
                let name = rel_str.rsplit('/').next().unwrap_or(&rel_str).to_string();
                let mut node = VfsNode::new(node_id, Some(parent_id), name, metadata.is_dir());
                fill_fs_attrs(&mut node, &event.path, &metadata);
                if self.tree.insert_node(node).is_ok() {
                    return Ok(vec![FsChange::Added(node_id)]);
                }
                Ok(Vec::new())
            }
            FsEventKind::Modify => {
                // Directory modify events (content changes) carry no useful
                // size information; only refresh file nodes.
                if event.path.is_dir() {
                    return Ok(Vec::new());
                }
                let id = self
                    .tree
                    .resolve_path(&rel_str)
                    .ok_or_else(|| FsError::PathNotFound(rel_str.clone()))?;
                let metadata = std::fs::metadata(&event.path)?;
                if let Some(node) = self.tree.node_mut(id) {
                    fill_fs_attrs(node, &event.path, &metadata);
                }
                Ok(vec![FsChange::Modified(id)])
            }
            FsEventKind::Remove => {
                let id = self
                    .tree
                    .resolve_path(&rel_str)
                    .ok_or_else(|| FsError::PathNotFound(rel_str.clone()))?;
                self.tree.remove_node(id)?;
                Ok(vec![FsChange::Removed(id)])
            }
            FsEventKind::Rename { from } => {
                let from_rel = from
                    .strip_prefix(&self.root)
                    .map_err(|_| FsError::OutsideRoot(from.clone()))?;
                let from_str = to_forward_slashes(from_rel);
                let id = self
                    .tree
                    .resolve_path(&from_str)
                    .ok_or_else(|| FsError::PathNotFound(from_str.clone()))?;
                let name = rel_str.rsplit('/').next().unwrap_or(&rel_str).to_string();
                self.tree.rename_node(id, &name)?;
                Ok(vec![FsChange::Renamed(id)])
            }
        }
    }
}

/// What changed in the tree after applying an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsChange {
    Added(vfs::NodeId),
    Modified(vfs::NodeId),
    Removed(vfs::NodeId),
    Renamed(vfs::NodeId),
}

/// Errors produced by [`FsTree`] operations.
#[derive(Debug, thiserror::Error)]
pub enum FsError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("path outside root: {0:?}")]
    OutsideRoot(PathBuf),
    #[error("path not found in tree: {0}")]
    PathNotFound(String),
    #[error("parent directory missing in tree: {0}")]
    ParentMissing(String),
    #[error("tree error: {0}")]
    Tree(#[from] vfs::VfsError),
}

fn scan_dir(root: &Path, dir: &Path, dir_id: vfs::NodeId, tree: &mut Tree) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = entry.metadata()?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let node_id = next_node_id();
        let mut node = VfsNode::new(node_id, Some(dir_id), name.clone(), metadata.is_dir());
        fill_fs_attrs(&mut node, &path, &metadata);
        let is_dir = metadata.is_dir();
        if tree.insert_node(node).is_ok() && is_dir {
            scan_dir(root, &path, node_id, tree)?;
        }
    }
    Ok(())
}

fn fill_fs_attrs(node: &mut VfsNode, path: &Path, metadata: &std::fs::Metadata) {
    node.set_attr(attr::SOURCE, AttrValue::String("fs".into()));
    node.set_attr(attr::FS_PATH, AttrValue::String(path.to_string_lossy().into_owned()));
    if metadata.is_file() {
        node.set_attr(attr::SIZE, AttrValue::UInt(metadata.len()));
    }
    if let Ok(modified) = metadata.modified()
        && let Ok(system_time) = modified.duration_since(std::time::UNIX_EPOCH)
    {
        let secs = system_time.as_secs() as i64;
        if let Ok(ts) = jiff::Timestamp::from_second(secs)
            && let Ok(zoned) = ts.in_tz("UTC")
        {
            node.set_attr(attr::MODIFIED, AttrValue::DateTime(zoned.datetime()));
        }
    }
}

fn to_forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn parent_of(path: &str) -> String {
    match path.rfind('/') {
        Some(idx) => path[..idx].to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs_watcher::{FsEvent, FsEventKind};

    fn scan_temp() -> (tempfile::TempDir, FsTree) {
        let dir = tempfile::tempdir().unwrap();
        let tree = FsTree::scan(dir.path()).unwrap();
        (dir, tree)
    }

    fn event(kind: FsEventKind, path: PathBuf) -> FsEvent {
        FsEvent { kind, path }
    }

    #[test]
    fn scan_builds_tree() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("a.txt"), b"hello").unwrap();
        std::fs::write(dir.path().join("sub/b.txt"), b"world").unwrap();
        let tree = FsTree::scan(dir.path()).unwrap();
        let a = tree.tree().resolve_path("a.txt").unwrap();
        assert_eq!(tree.tree().node(a).unwrap().attr(attr::SIZE).and_then(|v| v.as_u64()), Some(5));
        assert_eq!(tree.tree().node(a).unwrap().attr(attr::SOURCE).and_then(|v| v.as_str()), Some("fs"));
        assert!(tree.tree().resolve_path("sub/b.txt").is_some());
        let sub = tree.tree().resolve_path("sub").unwrap();
        assert!(tree.tree().node(sub).unwrap().is_directory);
    }

    #[test]
    fn create_event_adds_node() {
        let (dir, mut tree) = scan_temp();
        let file = dir.path().join("new.txt");
        std::fs::write(&file, b"x").unwrap();
        let changes = tree.apply_event(&event(FsEventKind::Create, file.clone())).unwrap();
        assert_eq!(changes.len(), 1);
        assert!(matches!(changes[0], FsChange::Added(_)));
        let id = tree.tree().resolve_path("new.txt").unwrap();
        assert_eq!(tree.tree().node(id).unwrap().attr(attr::FS_PATH).and_then(|v| v.as_str()), Some(file.to_str().unwrap()));
    }

    #[test]
    fn modify_event_updates_size() {
        let (dir, mut tree) = scan_temp();
        let file = dir.path().join("m.txt");
        std::fs::write(&file, b"12345").unwrap();
        tree.apply_event(&event(FsEventKind::Create, file.clone())).unwrap();
        std::fs::write(&file, b"1234567890").unwrap();
        let changes = tree.apply_event(&event(FsEventKind::Modify, file.clone())).unwrap();
        assert!(matches!(changes[0], FsChange::Modified(_)));
        let id = tree.tree().resolve_path("m.txt").unwrap();
        assert_eq!(tree.tree().node(id).unwrap().attr(attr::SIZE).and_then(|v| v.as_u64()), Some(10));
    }

    #[test]
    fn remove_event_deletes_node() {
        let (dir, mut tree) = scan_temp();
        let file = dir.path().join("r.txt");
        std::fs::write(&file, b"x").unwrap();
        tree.apply_event(&event(FsEventKind::Create, file.clone())).unwrap();
        std::fs::remove_file(&file).unwrap();
        let changes = tree.apply_event(&event(FsEventKind::Remove, file)).unwrap();
        assert!(matches!(changes[0], FsChange::Removed(_)));
        assert!(tree.tree().resolve_path("r.txt").is_none());
    }

    #[test]
    fn rename_event_renames_node() {
        let (dir, mut tree) = scan_temp();
        let from = dir.path().join("old.txt");
        let to = dir.path().join("new.txt");
        std::fs::write(&from, b"x").unwrap();
        tree.apply_event(&event(FsEventKind::Create, from.clone())).unwrap();
        let changes = tree
            .apply_event(&event(FsEventKind::Rename { from: from.clone() }, to.clone()))
            .unwrap();
        assert!(matches!(changes[0], FsChange::Renamed(_)));
        assert!(tree.tree().resolve_path("new.txt").is_some());
        assert!(tree.tree().resolve_path("old.txt").is_none());
    }

    #[test]
    fn directory_modify_is_ignored() {
        let (dir, mut tree) = scan_temp();
        std::fs::create_dir_all(dir.path().join("d")).unwrap();
        let changes = tree.apply_event(&event(FsEventKind::Modify, dir.path().join("d"))).unwrap();
        assert!(changes.is_empty());
    }
}
