use std::collections::HashMap;
use std::path::PathBuf;
use crate::{
    DirtyTree, DirtyType, EditOperation, EditQueue, EditTransaction, Tree, VfsMetadata, VfsNode,
    VfsNodeId, next_vfs_id,
};

/// Overlay virtual file system for a single archive session.
///
/// Maintains the three-tree model (Base / Working / Dirty) plus edit queue
/// and metadata cache. Provides conversion between VFS nodes and `ArchiveEntry`.
#[derive(Debug, Clone)]
pub struct OverlayVfs {
    base_tree: Tree,
    working_tree: Tree,
    dirty_tree: DirtyTree,
    edit_queue: EditQueue,
    metadata_cache: HashMap<VfsNodeId, VfsMetadata>,
    archive_path: PathBuf,
}

impl OverlayVfs {
    /// Build overlay VFS state from the initial list of archive entries.
    pub fn build(
        archive_path: PathBuf,
        archive_format: Option<ArchiveFormat>,
        entries: &[ArchiveEntry],
    ) -> Self {
        let root_id = next_vfs_id();
        let mut base_tree = Tree::new(root_id);
        let mut metadata_cache = HashMap::new();
        let mut id_map: HashMap<u32, VfsNodeId> = HashMap::new();
        let mut path_map: HashMap<String, VfsNodeId> = HashMap::new();

        // Insert root
        base_tree
            .insert_node(VfsNode {
                id: root_id,
                parent: None,
                name: String::new(),
                is_directory: true,
                original_index: None,
                fs_path: None,
            })
            .ok();

        path_map.insert(String::new(), root_id);

        // Helper: ensure a directory path exists in the tree, creating all missing
        // intermediate directories recursively.
        fn ensure_dir(
            tree: &mut Tree,
            path_map: &mut HashMap<String, VfsNodeId>,
            path: &str,
            root_id: VfsNodeId,
        ) -> VfsNodeId {
            if path.is_empty() {
                return root_id;
            }
            if let Some(&id) = path_map.get(path) {
                return id;
            }
            let parent_path = parent_dir(path);
            let parent_id = ensure_dir(tree, path_map, &parent_path, root_id);
            let name = path
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or(path)
                .to_string();
            let node_id = next_vfs_id();
            let node = VfsNode {
                id: node_id,
                parent: Some(parent_id),
                name,
                is_directory: true,
                original_index: None,
                fs_path: None,
            };
            let _ = tree.insert_node(node);
            path_map.insert(path.to_string(), node_id);
            node_id
        }

        // Build parent→children tree from entry paths.
        // Order does not matter because ensure_dir creates all missing ancestors.
        for entry in entries {
            let normalized_path = entry.path.trim_end_matches('/').replace('\\', "/");
            let parent_path = parent_dir(&normalized_path);
            let parent_id = ensure_dir(&mut base_tree, &mut path_map, &parent_path, root_id);

            let node_id = next_vfs_id();
            let node = VfsNode {
                id: node_id,
                parent: Some(parent_id),
                name: entry.name.clone(),
                is_directory: entry.is_directory,
                original_index: Some(entry.original_index),
                fs_path: None,
            };
            if base_tree.insert_node(node).is_ok() {
                id_map.insert(entry.original_index, node_id);
                path_map.insert(normalized_path, node_id);
            }

            metadata_cache.insert(
                node_id,
                VfsMetadata {
                    size: entry.size,
                    compressed_size: entry.compressed_size,
                    modified: entry.modified,
                    created: entry.created,
                    accessed: entry.accessed,
                    crc: entry.crc,
                    is_encrypted: entry.is_encrypted,
                    is_symlink: entry.is_symlink,
                    attributes: entry.attributes,
                    posix_attrib: entry.posix_attrib,
                    host_os: entry.host_os,
                    compression_method: entry.compression_method.clone(),
                    comment: entry.comment.clone(),
                    user: entry.user.clone(),
                    group: entry.group.clone(),
                    extension: entry.extension.clone(),
                    hardlink: entry.hardlink.clone(),
                },
            );
        }

        let working_tree = base_tree.clone();
        let edit_queue = EditQueue::new();
        let dirty_tree = DirtyTree::new();

        Self {
            base_tree,
            working_tree,
            dirty_tree,
            edit_queue,
            metadata_cache,
            archive_path,
        }
    }

    pub fn build_for_test(
        base_tree: Tree,
        metadata_cache: HashMap<VfsNodeId, VfsMetadata>,
        archive_format: Option<ArchiveFormat>,
    ) -> Self {
        Self {
            base_tree: base_tree.clone(),
            working_tree: base_tree,
            dirty_tree: DirtyTree::new(),
            edit_queue: EditQueue::new(),
            metadata_cache,
            archive_path: PathBuf::new(),
            archive_format,
        }
    }

    pub fn archive_path(&self) -> &PathBuf {
        &self.archive_path
    }

    pub fn working_tree(&self) -> &Tree {
        &self.working_tree
    }

    pub fn dirty_tree(&self) -> &DirtyTree {
        &self.dirty_tree
    }

    pub fn base_tree(&self) -> &Tree {
        &self.base_tree
    }

    pub fn edit_queue(&self) -> &EditQueue {
        &self.edit_queue
    }

    pub fn edit_queue_mut(&mut self) -> &mut EditQueue {
        &mut self.edit_queue
    }

    /// Apply an edit transaction: update working tree and dirty tree.
    pub fn apply_edit(&mut self, tx: EditTransaction) -> Result<(), ArchiveError> {
        for op in &tx.operations {
            self.apply_operation(op)?;
        }
        self.edit_queue.push(tx);
        Ok(())
    }

    fn apply_operation(&mut self, op: &EditOperation) -> Result<(), ArchiveError> {
        match op {
            EditOperation::Add {
                node_id,
                parent_id,
                name,
                fs_path,
                is_directory,
            } => {
                let node = VfsNode {
                    id: *node_id,
                    parent: Some(*parent_id),
                    name: name.clone(),
                    is_directory: *is_directory,
                    original_index: None,
                    fs_path: fs_path.clone(),
                };
                self.working_tree
                    .insert_node(node)
                    .map_err(|e| ArchiveError::Internal(e.to_string()))?;
                self.dirty_tree.mark(*node_id, DirtyType::Added);
            }
            EditOperation::Delete { node_id, .. } => {
                let _ = self.working_tree.remove_node(*node_id);
                self.dirty_tree.mark(*node_id, DirtyType::Deleted);
            }
            EditOperation::Rename {
                node_id, new_name, ..
            } => {
                self.working_tree
                    .rename_node(*node_id, new_name)
                    .map_err(|e| ArchiveError::Internal(e.to_string()))?;
                self.dirty_tree.mark(*node_id, DirtyType::Renamed);
            }
        }
        Ok(())
    }

    /// Undo: pop from edit queue, reverse apply.
    pub fn undo(&mut self) -> Result<bool, ArchiveError> {
        if !self.edit_queue.can_undo() {
            return Ok(false);
        }
        let tx = self
            .edit_queue
            .undo()
            .ok_or_else(|| ArchiveError::Internal("undo failed: unexpected empty stack".into()))?;
        let reversed = tx.reverse().clone();
        for op in reversed {
            self.apply_operation(&op)?;
        }
        Ok(true)
    }

    /// Redo: pop from redo queue, re-apply.
    pub fn redo(&mut self) -> Result<bool, ArchiveError> {
        if !self.edit_queue.can_redo() {
            return Ok(false);
        }
        let tx = self
            .edit_queue
            .redo()
            .ok_or_else(|| ArchiveError::Internal("redo failed: unexpected empty stack".into()))?;
        let ops = tx.operations.clone();
        for op in &ops {
            self.apply_operation(op)?;
        }
        Ok(true)
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.dirty_tree.has_changes() || !self.edit_queue.undo_stack().is_empty()
    }

    /// Discard all pending edits: reset working tree to base.
    pub fn discard_pending(&mut self) {
        self.working_tree = self.base_tree.clone();
        self.dirty_tree.clear_all();
        self.edit_queue.clear();
    }

    /// On successful commit: base_tree = working_tree, clear dirty.
    pub fn on_commit_success(&mut self) {
        self.base_tree = self.working_tree.clone();
        self.dirty_tree.clear_all();
        self.edit_queue.clear();
    }

    /// Generate a ChangeSet from the dirty tree for commit.
    pub fn generate_changeset(&self) -> ChangeSet {
        let mut cs = ChangeSet::new();

        for (node_id, dirty_type) in self.dirty_tree.iter() {
            match dirty_type {
                DirtyType::Added => {
                    if let Some(node) = self.working_tree.node(*node_id)
                        && let Some(ref fs_path) = node.fs_path
                    {
                        let archive_path = self
                            .working_tree
                            .path_of(*node_id)
                            .unwrap_or_else(|| node.name.clone());
                        cs.add(fs_path.clone(), archive_path);
                    }
                }
                DirtyType::Deleted => {
                    if let Some(node) = self.base_tree.node(*node_id)
                        && let Some(idx) = node.original_index
                    {
                        cs.delete(idx);
                    }
                }
                DirtyType::Renamed => {
                    if let Some(node) = self.working_tree.node(*node_id)
                        && let Some(base_node) = self.base_tree.node(*node_id)
                        && let Some(idx) = base_node.original_index
                    {
                        let new_path = self
                            .working_tree
                            .path_of(*node_id)
                            .unwrap_or_else(|| node.name.clone());
                        cs.rename(idx, new_path);
                    }
                }
            }
        }

        cs
    }

    pub fn set_metadata(&mut self, node_id: VfsNodeId, meta: VfsMetadata) {
        self.metadata_cache.insert(node_id, meta);
    }

    pub fn get_metadata(&self, node_id: VfsNodeId) -> Option<&VfsMetadata> {
        self.metadata_cache.get(&node_id)
    }

    /// Convert a working tree node to ArchiveEntry (with cached metadata).
    pub fn node_to_entry(&self, node_id: VfsNodeId) -> Option<ArchiveEntry> {
        let node = self.working_tree.node(node_id)?;
        let meta = self.metadata_cache.get(&node_id);
        let path = self.working_tree.path_of(node_id).unwrap_or_default();

        Some(ArchiveEntry {
            name: node.name.clone(),
            path,
            size: meta.map(|m| m.size).unwrap_or(0),
            compressed_size: meta.map(|m| m.compressed_size).unwrap_or(0),
            is_directory: node.is_directory,
            is_encrypted: meta.map(|m| m.is_encrypted).unwrap_or(false),
            is_symlink: meta.map(|m| m.is_symlink).unwrap_or(false),
            modified: meta.and_then(|m| m.modified),
            created: meta.and_then(|m| m.created),
            accessed: meta.and_then(|m| m.accessed),
            crc: meta.and_then(|m| m.crc),
            attributes: meta.and_then(|m| m.attributes),
            posix_attrib: meta.and_then(|m| m.posix_attrib),
            host_os: meta.and_then(|m| m.host_os),
            compression_method: meta.and_then(|m| m.compression_method.clone()),
            comment: meta.and_then(|m| m.comment.clone()),
            user: meta.and_then(|m| m.user.clone()),
            group: meta.and_then(|m| m.group.clone()),
            extension: meta.and_then(|m| m.extension.clone()),
            hardlink: meta.and_then(|m| m.hardlink.clone()),
            original_index: node.original_index.unwrap_or(u32::MAX),
        })
    }

    pub fn list_page(&self, offset: usize, limit: usize) -> Page<ArchiveEntry> {
        let all_ids = self.working_tree.all_ids();
        let total = all_ids.len();
        let items: Vec<ArchiveEntry> = all_ids
            .into_iter()
            .skip(offset)
            .take(limit)
            .filter_map(|id| self.node_to_entry(id))
            .collect();
        Page::new(items, offset, Some(total))
    }

    pub fn list_directory(&self, path: &str) -> Vec<ArchiveEntry> {
        let dir_id = match self.working_tree.resolve_path(path) {
            Some(id) => id,
            None => return Vec::new(),
        };
        let child_ids = self.working_tree.children(dir_id).unwrap_or(&[]).to_vec();
        child_ids
            .iter()
            .filter_map(|&id| self.node_to_entry(id))
            .collect()
    }

    pub fn get_properties(&self) -> ArchiveProperties {
        let mut folders = 0u32;
        let mut files = 0u32;
        let mut total_size = 0u64;
        let mut packed_size = 0u64;

        for id in self.working_tree.all_ids() {
            if let Some(node) = self.working_tree.node(id) {
                if id == self.working_tree.root() {
                    continue;
                }
                if node.is_directory {
                    folders += 1;
                } else {
                    files += 1;
                }
                if let Some(meta) = self.metadata_cache.get(&id) {
                    total_size += meta.size;
                    packed_size += meta.compressed_size;
                }
            }
        }

        ArchiveProperties {
            items_count: files + folders,
            folders_count: folders,
            files_count: files,
            total_size,
            packed_size,
            ..Default::default()
        }
    }

    pub fn node_id_by_original_index(&self, index: u32) -> Option<VfsNodeId> {
        for id in self.base_tree.all_ids() {
            if let Some(node) = self.base_tree.node(id)
                && node.original_index == Some(index)
            {
                return Some(id);
            }
        }
        None
    }

    pub fn original_index_of(&self, node_id: VfsNodeId) -> Option<u32> {
        self.base_tree.node(node_id).and_then(|n| n.original_index)
    }

    /// Panics if `other` has a different tree structure or essential metadata.
    /// Used by the cross-harness test framework.
    ///
    /// Only compares structural fields (size, crc, is_encrypted, is_symlink).
    /// Skips implementation-specific fields like compressed_size, timestamps,
    /// attributes, and compression method that vary between harnesses.
    pub fn assert_structural_eq(&self, other: &OverlayVfs) {
        let base = self.base_tree();
        let other_base = other.base_tree();

        let paths = base.all_paths();
        let other_paths = other_base.all_paths();
        assert_eq!(paths, other_paths, "tree structure mismatch");

        let crc_supported = !self
            .archive_format
            .or(other.archive_format)
            .is_some_and(|f| matches!(f, ArchiveFormat::Tar | ArchiveFormat::TarGz | ArchiveFormat::TarBz2 | ArchiveFormat::TarXz));

        for path in &paths {
            let id = base.resolve_path(path).unwrap();
            let other_id = other_base.resolve_path(path).unwrap();
            let meta = self.metadata_cache.get(&id);
            let other_meta = other.metadata_cache.get(&other_id);

            match (meta, other_meta) {
                (None, None) => {}
                (None, Some(_)) => panic!("metadata missing for '{path}' in left"),
                (Some(_), None) => panic!("metadata missing for '{path}' in right"),
                (Some(m), Some(o)) => {
                    assert_eq!(m.size, o.size, "size mismatch for '{path}'");
                    if crc_supported {
                        assert_eq!(m.crc, o.crc, "crc mismatch for '{path}'");
                    }
                    assert_eq!(
                        m.is_encrypted, o.is_encrypted,
                        "is_encrypted mismatch for '{path}'"
                    );
                    assert_eq!(
                        m.is_symlink, o.is_symlink,
                        "is_symlink mismatch for '{path}'"
                    );
                    if let (Some(a), Some(b)) = (m.attributes, o.attributes) {
                        assert_eq!(a, b, "attributes mismatch for '{path}'");
                    }
                    if let (Some(a), Some(b)) = (m.posix_attrib, o.posix_attrib) {
                        assert_eq!(a, b, "posix_attrib mismatch for '{path}'");
                    }
                }
            }
        }
    }
}

fn parent_dir(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if let Some(pos) = trimmed.rfind('/') {
        trimmed[..pos].to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use crate::archive::archive_entry::ArchiveEntry;
    use crate::vfs::{EditOperation, EditTransaction, OverlayVfs};

    fn sample_entries() -> Vec<ArchiveEntry> {
        vec![
            ArchiveEntry {
                name: "file1.txt".into(),
                path: "file1.txt".into(),
                size: 100,
                compressed_size: 50,
                original_index: 0,
                ..Default::default()
            },
            ArchiveEntry {
                name: "dir".into(),
                path: "dir".into(),
                is_directory: true,
                original_index: 1,
                ..Default::default()
            },
            ArchiveEntry {
                name: "inner.txt".into(),
                path: "dir/inner.txt".into(),
                size: 200,
                compressed_size: 100,
                original_index: 2,
                ..Default::default()
            },
        ]
    }

    #[test]
    fn test_build_and_list_page() {
        let vfs = OverlayVfs::build(PathBuf::from("test.7z"), None, &sample_entries());
        let page = vfs.list_page(0, 10);
        assert_eq!(page.items.len(), 4);
    }

    #[test]
    fn test_list_directory_root() {
        let vfs = OverlayVfs::build(PathBuf::from("test.7z"), None, &sample_entries());
        let root_entries = vfs.list_directory("");
        assert_eq!(root_entries.len(), 2);
        assert!(root_entries.iter().any(|e| e.name == "file1.txt"));
        assert!(root_entries.iter().any(|e| e.name == "dir"));
    }

    #[test]
    fn test_trailing_slash_directory_paths_are_normalized() {
        let entries = vec![
            ArchiveEntry {
                name: "a".into(),
                path: "a/".into(),
                is_directory: true,
                original_index: 0,
                ..Default::default()
            },
            ArchiveEntry {
                name: "b.txt".into(),
                path: "a/b.txt".into(),
                size: 10,
                compressed_size: 5,
                original_index: 1,
                ..Default::default()
            },
            ArchiveEntry {
                name: "c.txt".into(),
                path: "c.txt".into(),
                size: 20,
                compressed_size: 10,
                original_index: 2,
                ..Default::default()
            },
        ];
        let vfs = OverlayVfs::build(PathBuf::from("test.7z"), None, &entries);

        let root_entries = vfs.list_directory("");
        assert_eq!(root_entries.len(), 2);
        assert!(root_entries.iter().any(|e| e.name == "a" && e.is_directory));
        assert!(root_entries.iter().any(|e| e.name == "c.txt"));

        let a_entries = vfs.list_directory("a");
        assert_eq!(a_entries.len(), 1);
        assert_eq!(a_entries[0].name, "b.txt");
    }

    #[test]
    fn test_build_hierarchy_when_children_come_before_parent() {
        let entries = vec![
            ArchiveEntry {
                name: "deep.txt".into(),
                path: "a/b/deep.txt".into(),
                size: 10,
                compressed_size: 5,
                original_index: 0,
                ..Default::default()
            },
            ArchiveEntry {
                name: "b".into(),
                path: "a/b".into(),
                is_directory: true,
                original_index: 1,
                ..Default::default()
            },
            ArchiveEntry {
                name: "a".into(),
                path: "a".into(),
                is_directory: true,
                original_index: 2,
                ..Default::default()
            },
            ArchiveEntry {
                name: "root.txt".into(),
                path: "root.txt".into(),
                size: 20,
                compressed_size: 10,
                original_index: 3,
                ..Default::default()
            },
        ];
        let vfs = OverlayVfs::build(PathBuf::from("test.7z"), None, &entries);

        let root_entries = vfs.list_directory("");
        assert_eq!(root_entries.len(), 2);
        assert!(root_entries.iter().any(|e| e.name == "a" && e.is_directory));
        assert!(root_entries.iter().any(|e| e.name == "root.txt"));

        let a_entries = vfs.list_directory("a");
        assert_eq!(a_entries.len(), 1);
        assert_eq!(a_entries[0].name, "b");

        let b_entries = vfs.list_directory("a/b");
        assert_eq!(b_entries.len(), 1);
        assert_eq!(b_entries[0].name, "deep.txt");
    }

    #[test]
    fn test_list_directory_subdir() {
        let vfs = OverlayVfs::build(PathBuf::from("test.7z"), None, &sample_entries());
        let entries = vfs.list_directory("dir");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "inner.txt");
    }

    #[test]
    fn test_get_properties() {
        let vfs = OverlayVfs::build(PathBuf::from("test.7z"), None, &sample_entries());
        let props = vfs.get_properties();
        assert_eq!(props.files_count, 2);
        assert_eq!(props.folders_count, 1);
        assert_eq!(props.total_size, 300);
        assert_eq!(props.packed_size, 150);
    }

    #[test]
    fn test_apply_edit_rename() {
        let mut vfs = OverlayVfs::build(PathBuf::from("test.7z"), None, &sample_entries());
        let file1_id = vfs.working_tree.resolve_path("file1.txt").unwrap();

        let tx = EditTransaction::with_ops(
            "rename file1",
            vec![EditOperation::Rename {
                node_id: file1_id,
                old_name: "file1.txt".into(),
                new_name: "renamed.txt".into(),
            }],
        );
        vfs.apply_edit(tx).unwrap();

        assert!(vfs.has_unsaved_changes());
        assert!(vfs.working_tree.resolve_path("renamed.txt").is_some());
        assert!(vfs.working_tree.resolve_path("file1.txt").is_none());
    }

    #[test]
    fn test_undo_redo() {
        let mut vfs = OverlayVfs::build(PathBuf::from("test.7z"), None, &sample_entries());
        let file1_id = vfs.working_tree.resolve_path("file1.txt").unwrap();

        vfs.apply_edit(EditTransaction::with_ops(
            "rename file1",
            vec![EditOperation::Rename {
                node_id: file1_id,
                old_name: "file1.txt".into(),
                new_name: "renamed.txt".into(),
            }],
        ))
        .unwrap();

        assert!(vfs.undo().unwrap());
        assert!(vfs.working_tree.resolve_path("file1.txt").is_some());
        assert!(vfs.working_tree.resolve_path("renamed.txt").is_none());

        assert!(vfs.redo().unwrap());
        assert!(vfs.working_tree.resolve_path("renamed.txt").is_some());
        assert!(vfs.working_tree.resolve_path("file1.txt").is_none());
    }

    #[test]
    fn test_discard_pending() {
        let mut vfs = OverlayVfs::build(PathBuf::from("test.7z"), None, &sample_entries());
        let file1_id = vfs.working_tree.resolve_path("file1.txt").unwrap();

        vfs.apply_edit(EditTransaction::with_ops(
            "rename",
            vec![EditOperation::Rename {
                node_id: file1_id,
                old_name: "file1.txt".into(),
                new_name: "gone.txt".into(),
            }],
        ))
        .unwrap();

        vfs.discard_pending();
        assert!(!vfs.has_unsaved_changes());
        assert!(vfs.working_tree.resolve_path("file1.txt").is_some());
    }

    #[test]
    fn test_generate_changeset() {
        let mut vfs = OverlayVfs::build(PathBuf::from("test.7z"), None, &sample_entries());
        let file1_id = vfs.working_tree.resolve_path("file1.txt").unwrap();

        vfs.apply_edit(EditTransaction::with_ops(
            "delete file1",
            vec![EditOperation::Delete {
                node_id: file1_id,
                saved_name: "file1.txt".into(),
                saved_parent: None,
                saved_is_directory: false,
                saved_original_index: Some(0),
                saved_fs_path: None,
                saved_child_ids: Vec::new(),
            }],
        ))
        .unwrap();

        let cs = vfs.generate_changeset();
        assert_eq!(cs.len(), 1);
    }

    #[test]
    fn test_on_commit_success_clears_state() {
        let mut vfs = OverlayVfs::build(PathBuf::from("test.7z"), None, &sample_entries());
        let file1_id = vfs.working_tree.resolve_path("file1.txt").unwrap();

        vfs.apply_edit(EditTransaction::with_ops(
            "rename",
            vec![EditOperation::Rename {
                node_id: file1_id,
                old_name: "file1.txt".into(),
                new_name: "done.txt".into(),
            }],
        ))
        .unwrap();

        vfs.on_commit_success();
        assert!(!vfs.has_unsaved_changes());
        assert!(vfs.base_tree.resolve_path("done.txt").is_some());
    }
}
