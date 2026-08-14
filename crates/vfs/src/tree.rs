use super::{VfsError, VfsNode, VfsNodeId, next_vfs_id};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Tree {
    nodes: HashMap<VfsNodeId, VfsNode>,
    children: HashMap<Option<VfsNodeId>, Vec<VfsNodeId>>,
    root: VfsNodeId,
}

impl Tree {
    pub fn new(root_id: VfsNodeId) -> Self {
        Self {
            nodes: HashMap::new(),
            children: HashMap::new(),
            root: root_id,
        }
    }

    pub fn root(&self) -> VfsNodeId {
        self.root
    }

    pub fn node(&self, id: VfsNodeId) -> Option<&VfsNode> {
        self.nodes.get(&id)
    }

    pub fn nodes(&self) -> &HashMap<VfsNodeId, VfsNode> {
        &self.nodes
    }

    pub fn children(&self, parent: VfsNodeId) -> Option<&[VfsNodeId]> {
        self.children.get(&Some(parent)).map(|v| v.as_slice())
    }

    pub fn root_children(&self) -> &[VfsNodeId] {
        self.children
            .get(&Some(self.root))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn resolve_path(&self, path: &str) -> Option<VfsNodeId> {
        if path.is_empty() || path == "/" {
            return Some(self.root);
        }
        let clean = path.trim_end_matches('/');
        let parts: Vec<&str> = clean.split('/').filter(|p| !p.is_empty()).collect();
        let mut current = self.root;
        for part in &parts {
            let kids = self.children.get(&Some(current))?;
            let found = kids
                .iter()
                .find(|&&id| self.nodes.get(&id).map(|n| n.name.as_str()) == Some(part))?;
            current = *found;
        }
        Some(current)
    }

    pub fn path_of(&self, node_id: VfsNodeId) -> Option<String> {
        let mut parts = Vec::new();
        let mut current = node_id;
        loop {
            let node = self.nodes.get(&current)?;
            if let Some(parent) = node.parent {
                parts.push(node.name.clone());
                current = parent;
            } else {
                break;
            }
        }
        parts.reverse();
        Some(parts.join("/"))
    }

    pub fn insert_node(&mut self, node: VfsNode) -> Result<(), VfsError> {
        let id = node.id;
        let parent = node.parent;
        let name = node.name.clone();
        if self.nodes.contains_key(&id) {
            return Err(VfsError::AlreadyExists(name));
        }
        if let Some(pid) = parent {
            let siblings = self.children.entry(Some(pid)).or_default();
            if siblings
                .iter()
                .any(|&sid| self.nodes.get(&sid).map(|n| n.name.as_str()) == Some(&name))
            {
                return Err(VfsError::AlreadyExists(name));
            }
        }
        self.nodes.insert(id, node);
        self.children.entry(parent).or_default().push(id);
        Ok(())
    }

    pub fn rename_node(&mut self, node_id: VfsNodeId, new_name: &str) -> Result<String, VfsError> {
        let node = self
            .nodes
            .get_mut(&node_id)
            .ok_or(VfsError::NodeNotFound(node_id))?;
        let old_name = std::mem::replace(&mut node.name, new_name.to_string());
        Ok(old_name)
    }

    pub fn remove_node(&mut self, node_id: VfsNodeId) -> Result<VfsNode, VfsError> {
        let node = self
            .nodes
            .remove(&node_id)
            .ok_or(VfsError::NodeNotFound(node_id))?;
        if let Some(parent) = node.parent
            && let Some(siblings) = self.children.get_mut(&Some(parent))
        {
            siblings.retain(|&id| id != node_id);
        }
        if let Some(kids) = self.children.remove(&Some(node_id)) {
            for kid in &kids {
                self.nodes.remove(kid);
            }
        }
        Ok(node)
    }

    /// Return all node paths sorted.
    pub fn all_paths(&self) -> Vec<String> {
        let mut paths: Vec<String> = self.nodes.keys()
            .filter_map(|&id| self.path_of(id))
            .collect();
        paths.sort();
        paths
    }

    pub fn all_ids(&self) -> Vec<VfsNodeId> {
        self.nodes.keys().copied().collect()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

impl Default for Tree {
    fn default() -> Self {
        Self::new(next_vfs_id())
    }
}

impl From<Tree> for Vec<VfsNode> {
    fn from(tree: Tree) -> Self {
        tree.nodes.into_values().collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirtyType {
    Added,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone)]
pub struct DirtyEntry {
    pub node_id: VfsNodeId,
    pub change_type: DirtyType,
}

#[derive(Debug, Clone)]
pub struct DirtyTree {
    entries: HashMap<VfsNodeId, DirtyType>,
}

impl DirtyTree {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn mark(&mut self, node_id: VfsNodeId, change_type: DirtyType) {
        self.entries.insert(node_id, change_type);
    }

    pub fn clear(&mut self, node_id: VfsNodeId) {
        self.entries.remove(&node_id);
    }

    pub fn clear_all(&mut self) {
        self.entries.clear();
    }

    pub fn is_dirty(&self, node_id: VfsNodeId) -> bool {
        self.entries.contains_key(&node_id)
    }

    pub fn has_changes(&self) -> bool {
        !self.entries.is_empty()
    }

    pub fn entries(&self) -> &HashMap<VfsNodeId, DirtyType> {
        &self.entries
    }

    pub fn by_type(&self, change_type: DirtyType) -> Vec<VfsNodeId> {
        self.entries
            .iter()
            .filter(|(_, t)| **t == change_type)
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&VfsNodeId, &DirtyType)> {
        self.entries.iter()
    }
}

impl Default for DirtyTree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(name: &str, parent: Option<VfsNodeId>, is_dir: bool) -> VfsNode {
        VfsNode {
            id: next_vfs_id(),
            parent,
            name: name.to_string(),
            is_directory: is_dir,
            original_index: None,
            fs_path: None,
        }
    }

    #[test]
    fn test_tree_insert_and_find() {
        let root_id = next_vfs_id();
        let mut tree = Tree::new(root_id);
        let root_node = VfsNode {
            id: root_id,
            parent: None,
            name: String::new(),
            is_directory: true,
            original_index: None,
            fs_path: None,
        };
        tree.insert_node(root_node).unwrap();

        let child = make_node("file.txt", Some(root_id), false);
        let child_id = child.id;
        tree.insert_node(child).unwrap();

        assert_eq!(tree.resolve_path("file.txt"), Some(child_id));
    }

    #[test]
    fn test_tree_resolve_nested_path() {
        let root_id = next_vfs_id();
        let mut tree = Tree::new(root_id);
        tree.insert_node(VfsNode {
            id: root_id,
            parent: None,
            name: String::new(),
            is_directory: true,
            original_index: None,
            fs_path: None,
        })
        .unwrap();

        let dir = make_node("dir", Some(root_id), true);
        let dir_id = dir.id;
        tree.insert_node(dir).unwrap();

        let file = make_node("inner.txt", Some(dir_id), false);
        let file_id = file.id;
        tree.insert_node(file).unwrap();

        assert_eq!(tree.resolve_path("dir/inner.txt"), Some(file_id));
        assert_eq!(tree.resolve_path("dir"), Some(dir_id));
    }

    #[test]
    fn test_tree_rename() {
        let root_id = next_vfs_id();
        let mut tree = Tree::new(root_id);
        tree.insert_node(VfsNode {
            id: root_id,
            parent: None,
            name: String::new(),
            is_directory: true,
            original_index: None,
            fs_path: None,
        })
        .unwrap();

        let file = make_node("old.txt", Some(root_id), false);
        let file_id = file.id;
        tree.insert_node(file).unwrap();
        tree.rename_node(file_id, "new.txt").unwrap();

        assert_eq!(tree.node(file_id).unwrap().name, "new.txt");
        assert!(tree.resolve_path("old.txt").is_none());
        assert_eq!(tree.resolve_path("new.txt"), Some(file_id));
    }

    #[test]
    fn test_tree_remove_node() {
        let root_id = next_vfs_id();
        let mut tree = Tree::new(root_id);
        tree.insert_node(VfsNode {
            id: root_id,
            parent: None,
            name: String::new(),
            is_directory: true,
            original_index: None,
            fs_path: None,
        })
        .unwrap();

        let file = make_node("delete_me.txt", Some(root_id), false);
        let file_id = file.id;
        tree.insert_node(file).unwrap();
        tree.remove_node(file_id).unwrap();

        assert!(tree.node(file_id).is_none());
        assert!(tree.resolve_path("delete_me.txt").is_none());
    }

    #[test]
    fn test_dirty_tree_mark_and_check() {
        let id1 = next_vfs_id();
        let id2 = next_vfs_id();
        let mut dt = DirtyTree::new();

        assert!(!dt.has_changes());
        dt.mark(id1, DirtyType::Added);
        assert!(dt.has_changes());
        assert!(dt.is_dirty(id1));
        assert!(!dt.is_dirty(id2));

        dt.clear(id1);
        assert!(!dt.is_dirty(id1));
    }

    #[test]
    fn test_dirty_tree_by_type() {
        let id1 = next_vfs_id();
        let id2 = next_vfs_id();
        let mut dt = DirtyTree::new();

        dt.mark(id1, DirtyType::Added);
        dt.mark(id2, DirtyType::Deleted);

        let added = dt.by_type(DirtyType::Added);
        assert_eq!(added, vec![id1]);

        let deleted = dt.by_type(DirtyType::Deleted);
        assert_eq!(deleted, vec![id2]);
    }

    #[test]
    fn test_dirty_tree_clear_all() {
        let id1 = next_vfs_id();
        let id2 = next_vfs_id();
        let mut dt = DirtyTree::new();

        dt.mark(id1, DirtyType::Added);
        dt.mark(id2, DirtyType::Renamed);
        assert!(dt.has_changes());

        dt.clear_all();
        assert!(!dt.has_changes());
    }
}
