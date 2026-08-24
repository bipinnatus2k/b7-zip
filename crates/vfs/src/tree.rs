use super::{VfsError, VfsNode, VfsNodeId, next_vfs_id};
use std::collections::{HashMap, HashSet};
use std::fmt;

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

    pub fn node_mut(&mut self, id: VfsNodeId) -> Option<&mut VfsNode> {
        self.nodes.get_mut(&id)
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
        let mut seen = HashSet::new();
        loop {
            if !seen.insert(current) {
                return None; // cycle or repeated parent chain
            }
            let node = self.nodes.get(&current)?;
            if let Some(parent) = node.parent {
                parts.push(node.name.clone());
                current = parent;
            } else {
                if current != self.root {
                    return None;
                }
                break;
            }
        }
        parts.reverse();
        Some(parts.join("/"))
    }

    pub fn insert_node(&mut self, node: VfsNode) -> Result<(), VfsError> {
        self.insert_node_inner(node, false)
    }

    /// Insert a node while allowing a sibling with the same name.
    ///
    /// Archive listings may legitimately contain duplicate paths. Path
    /// lookup remains ambiguous (the first matching child wins), but the
    /// entry is no longer silently lost.
    pub fn insert_node_allow_duplicate(&mut self, node: VfsNode) -> Result<(), VfsError> {
        self.insert_node_inner(node, true)
    }

    fn insert_node_inner(&mut self, node: VfsNode, allow_duplicate: bool) -> Result<(), VfsError> {
        let id = node.id;
        let parent = node.parent;
        let name = node.name.clone();
        if self.nodes.contains_key(&id) {
            return Err(VfsError::AlreadyExists(name));
        }
        if let Some(pid) = parent {
            if !self.nodes.contains_key(&pid) {
                return Err(VfsError::NodeNotFound(pid));
            }
            if !allow_duplicate {
                let siblings = self.children.entry(Some(pid)).or_default();
                if siblings
                    .iter()
                    .any(|&sid| self.nodes.get(&sid).map(|n| n.name.as_str()) == Some(&name))
                {
                    return Err(VfsError::AlreadyExists(name));
                }
            }
        } else if id != self.root {
            return Err(VfsError::Internal(
                "only the tree root may have no parent".into(),
            ));
        }
        self.nodes.insert(id, node);
        self.children.entry(parent).or_default().push(id);
        Ok(())
    }

    pub fn rename_node(&mut self, node_id: VfsNodeId, new_name: &str) -> Result<String, VfsError> {
        self.reparent_node(node_id, None, new_name)
    }

    /// Rename `node_id` and, when `new_parent` is supplied, move it under a
    /// different parent.
    pub fn reparent_node(
        &mut self,
        node_id: VfsNodeId,
        new_parent: Option<VfsNodeId>,
        new_name: &str,
    ) -> Result<String, VfsError> {
        let (old_name, old_parent) = {
            let node = self
                .nodes
                .get(&node_id)
                .ok_or(VfsError::NodeNotFound(node_id))?;
            (node.name.clone(), node.parent)
        };
        let new_parent = new_parent.or(old_parent);
        if let Some(pid) = new_parent {
            if !self.nodes.contains_key(&pid) {
                return Err(VfsError::NodeNotFound(pid));
            }
            if pid != node_id
                && let Some(siblings) = self.children.get(&Some(pid))
                && siblings.iter().any(|&sid| {
                    sid != node_id
                        && self.nodes.get(&sid).map(|n| n.name.as_str()) == Some(new_name)
                })
            {
                return Err(VfsError::AlreadyExists(new_name.to_string()));
            }
        } else if node_id != self.root {
            return Err(VfsError::Internal(
                "only the tree root may have no parent".into(),
            ));
        }

        if old_parent != new_parent {
            if let Some(pid) = old_parent
                && let Some(siblings) = self.children.get_mut(&Some(pid))
            {
                siblings.retain(|&sid| sid != node_id);
            }
            self.children.entry(new_parent).or_default().push(node_id);
        }
        let node = self.nodes.get_mut(&node_id).expect("node checked above");
        node.parent = new_parent;
        node.name = new_name.to_string();
        Ok(old_name)
    }

    pub fn remove_node(&mut self, node_id: VfsNodeId) -> Result<VfsNode, VfsError> {
        let node = self
            .nodes
            .get(&node_id)
            .cloned()
            .ok_or(VfsError::NodeNotFound(node_id))?;
        self.remove_subtree(node_id);
        Ok(node)
    }

    /// Remove `node_id` and every descendant, including their `children`
    /// bookkeeping, so no orphan nodes or stale child lists remain.
    fn remove_subtree(&mut self, node_id: VfsNodeId) {
        let mut stack = vec![node_id];
        while let Some(id) = stack.pop() {
            let Some(node) = self.nodes.remove(&id) else {
                continue;
            };
            if let Some(parent) = node.parent
                && let Some(siblings) = self.children.get_mut(&Some(parent))
            {
                siblings.retain(|&sid| sid != id);
            }
            if let Some(kids) = self.children.remove(&Some(id)) {
                stack.extend(kids);
            }
        }
    }

    /// Return all node paths sorted.
    pub fn all_paths(&self) -> Vec<String> {
        let mut paths: Vec<String> = self
            .nodes
            .keys()
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

fn fmt_attr_value(f: &mut fmt::Formatter<'_>, v: &crate::AttrValue) -> fmt::Result {
    match v {
        crate::AttrValue::String(s) => write!(f, "{s:?}"),
        crate::AttrValue::UInt(n) => write!(f, "{n}"),
        crate::AttrValue::Int(n) => write!(f, "{n}"),
        crate::AttrValue::Bool(b) => write!(f, "{b}"),
        crate::AttrValue::DateTime(dt) => write!(f, "{dt}"),
        crate::AttrValue::Bytes(b) => write!(f, "<{} bytes>", b.len()),
    }
}

impl fmt::Display for Tree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn write_node(
            tree: &Tree,
            id: VfsNodeId,
            prefix: &str,
            is_last: bool,
            f: &mut fmt::Formatter<'_>,
        ) -> fmt::Result {
            let node = match tree.node(id) {
                Some(n) => n,
                None => return Ok(()),
            };

            let connector = if is_last { "└── " } else { "├── " };
            let kind = if node.is_directory { "/" } else { "" };

            writeln!(f, "{prefix}{connector}{}{} [id={},dir={}]", node.name, kind, id.as_u64(),node.is_directory)?;

            let attrs = &node.attrs;
            let attr_count = attrs.len();
            if attr_count > 0 {
                let child_prefix = format!("{prefix}{}", if is_last { "    " } else { "│   " });
                let mut sorted_attrs: Vec<_> = attrs.iter().collect();
                sorted_attrs.sort_by_key(|(k, _)| k.as_str());
                for (i, (k, v)) in sorted_attrs.iter().enumerate() {
                    let attr_connector = if i + 1 == attr_count { "└── " } else { "├── " };
                    write!(f, "{child_prefix}{attr_connector}{k}=")?;
                    fmt_attr_value(f, v)?;
                    writeln!(f)?;
                }
            }

            let children = tree.children(id).unwrap_or(&[]);
            let child_count = children.len();
            for (i, &child_id) in children.iter().enumerate() {
                let new_prefix = format!("{prefix}{}", if is_last { "    " } else { "│   " });
                write_node(tree, child_id, &new_prefix, i + 1 == child_count, f)?;
            }
            Ok(())
        }

        writeln!(f, "/ [id={}]", self.root().as_u64())?;
        if let Some(root_node) = self.node(self.root()) {
            let attrs = &root_node.attrs;
            let attr_count = attrs.len();
            if attr_count > 0 {
                let mut sorted_attrs: Vec<_> = attrs.iter().collect();
                sorted_attrs.sort_by_key(|(k, _)| k.as_str());
                for (i, (k, v)) in sorted_attrs.iter().enumerate() {
                    let attr_connector = if i + 1 == attr_count { "└── " } else { "├── " };
                    write!(f, "{attr_connector}{k}=")?;
                    fmt_attr_value(f, v)?;
                    writeln!(f)?;
                }
            }
        }
        let children = self.children(self.root()).unwrap_or(&[]);
        let child_count = children.len();
        for (i, &child_id) in children.iter().enumerate() {
            write_node(self, child_id, "", i + 1 == child_count, f)?;
        }
        Ok(())
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
        VfsNode::new(next_vfs_id(), parent, name, is_dir)
    }

    #[test]
    fn test_tree_insert_and_find() {
        let root_id = next_vfs_id();
        let mut tree = Tree::new(root_id);
        let root_node = VfsNode::new(root_id, None, "", true);
        tree.insert_node(root_node).unwrap();

        let child = make_node("file.txt", Some(root_id), false);
        let child_id = child.id;
        tree.insert_node(child).unwrap();
        println!("{}", tree);
        assert_eq!(tree.resolve_path("file.txt"), Some(child_id));
    }

    #[test]
    fn test_tree_resolve_nested_path() {
        let root_id = next_vfs_id();
        let mut tree = Tree::new(root_id);
        tree.insert_node(VfsNode::new(root_id, None, "", true))
            .unwrap();

        let dir = make_node("dir", Some(root_id), true);
        let dir_id = dir.id;
        tree.insert_node(dir).unwrap();

        let file = make_node("inner.txt", Some(dir_id), false);
        let file_id = file.id;
        tree.insert_node(file).unwrap();
        println!("{}", tree);
        assert_eq!(tree.resolve_path("dir/inner.txt"), Some(file_id));
        assert_eq!(tree.resolve_path("dir"), Some(dir_id));
    }

    #[test]
    fn test_tree_rename() {
        let root_id = next_vfs_id();
        let mut tree = Tree::new(root_id);
        tree.insert_node(VfsNode::new(root_id, None, "", true))
            .unwrap();

        let file = make_node("old.txt", Some(root_id), false);
        let file_id = file.id;
        tree.insert_node(file).unwrap();
        println!("{}", tree);
        tree.rename_node(file_id, "new.txt").unwrap();
        println!("{}", tree);
        assert_eq!(tree.node(file_id).unwrap().name, "new.txt");
        assert!(tree.resolve_path("old.txt").is_none());
        assert_eq!(tree.resolve_path("new.txt"), Some(file_id));
    }

    #[test]
    fn test_tree_remove_node() {
        let root_id = next_vfs_id();
        let mut tree = Tree::new(root_id);
        tree.insert_node(VfsNode::new(root_id, None, "", true))
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

#[cfg(test)]
mod integrity_tests {
    use super::*;

    #[test]
    fn remove_node_drops_the_whole_subtree() {
        let root_id = next_vfs_id();
        let mut tree = Tree::new(root_id);
        tree.insert_node(VfsNode::new(root_id, None, "", true))
            .unwrap();

        let dir = next_vfs_id();
        tree.insert_node(VfsNode::new(dir, Some(root_id), "dir", true))
            .unwrap();
        let inner = next_vfs_id();
        tree.insert_node(VfsNode::new(inner, Some(dir), "inner", true))
            .unwrap();
        let leaf = next_vfs_id();
        tree.insert_node(VfsNode::new(leaf, Some(inner), "leaf.txt", false))
            .unwrap();

        tree.remove_node(dir).unwrap();
        assert_eq!(tree.len(), 1);
        assert!(tree.node(inner).is_none());
        assert!(tree.node(leaf).is_none());
        assert_eq!(tree.all_paths(), vec![""]);
    }

    #[test]
    fn insert_rejects_missing_parent() {
        let root_id = next_vfs_id();
        let mut tree = Tree::new(root_id);
        tree.insert_node(VfsNode::new(root_id, None, "", true))
            .unwrap();

        let missing = next_vfs_id();
        let orphan = VfsNode::new(next_vfs_id(), Some(missing), "orphan", false);
        assert!(matches!(
            tree.insert_node(orphan),
            Err(VfsError::NodeNotFound(_))
        ));
    }
}
