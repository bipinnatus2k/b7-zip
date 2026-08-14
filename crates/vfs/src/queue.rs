use super::VfsNodeId;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum EditOperation {
    Add {
        node_id: VfsNodeId,
        parent_id: VfsNodeId,
        name: String,
        fs_path: Option<PathBuf>,
        is_directory: bool,
    },
    Delete {
        node_id: VfsNodeId,
        saved_name: String,
        saved_parent: Option<VfsNodeId>,
        saved_is_directory: bool,
        saved_original_index: Option<u32>,
        saved_fs_path: Option<PathBuf>,
        saved_child_ids: Vec<VfsNodeId>,
    },
    Rename {
        node_id: VfsNodeId,
        old_name: String,
        new_name: String,
    },
}

#[derive(Debug, Clone)]
pub struct EditTransaction {
    pub description: String,
    pub operations: Vec<EditOperation>,
}

impl EditTransaction {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            operations: Vec::new(),
        }
    }

    pub fn with_ops(description: impl Into<String>, operations: Vec<EditOperation>) -> Self {
        Self {
            description: description.into(),
            operations,
        }
    }

    pub fn push(&mut self, op: EditOperation) {
        self.operations.push(op);
    }

    pub fn reverse(&self) -> Vec<EditOperation> {
        self.operations
            .iter()
            .rev()
            .map(|op| match op {
                EditOperation::Add {
                    node_id,
                    parent_id: _,
                    name: _,
                    fs_path: _,
                    is_directory: _,
                } => EditOperation::Delete {
                    node_id: *node_id,
                    saved_name: String::new(),
                    saved_parent: None,
                    saved_is_directory: false,
                    saved_original_index: None,
                    saved_fs_path: None,
                    saved_child_ids: Vec::new(),
                },
                EditOperation::Delete {
                    node_id,
                    saved_name,
                    saved_parent,
                    saved_is_directory,
                    saved_original_index: _,
                    saved_fs_path,
                    saved_child_ids: _,
                } => EditOperation::Add {
                    node_id: *node_id,
                    parent_id: saved_parent.unwrap_or(*node_id),
                    name: saved_name.clone(),
                    fs_path: saved_fs_path.clone(),
                    is_directory: *saved_is_directory,
                },
                EditOperation::Rename {
                    node_id,
                    old_name,
                    new_name,
                } => EditOperation::Rename {
                    node_id: *node_id,
                    old_name: new_name.clone(),
                    new_name: old_name.clone(),
                },
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct EditQueue {
    undo_stack: Vec<EditTransaction>,
    redo_stack: Vec<EditTransaction>,
    max_depth: usize,
}

impl EditQueue {
    pub fn new() -> Self {
        Self::with_max_depth(128)
    }

    pub fn with_max_depth(max_depth: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_depth,
        }
    }

    pub fn push(&mut self, tx: EditTransaction) {
        if self.undo_stack.len() >= self.max_depth {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(tx);
        self.redo_stack.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo(&mut self) -> Option<&EditTransaction> {
        let tx = self.undo_stack.pop()?;
        self.redo_stack.push(tx);
        self.redo_stack.last()
    }

    pub fn redo(&mut self) -> Option<&EditTransaction> {
        let tx = self.redo_stack.pop()?;
        self.undo_stack.push(tx);
        self.undo_stack.last()
    }

    pub fn undo_stack(&self) -> &[EditTransaction] {
        &self.undo_stack
    }

    pub fn redo_stack(&self) -> &[EditTransaction] {
        &self.redo_stack
    }

    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
}

impl Default for EditQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::next_vfs_id;
    use super::*;

    #[test]
    fn test_edit_queue_push_and_undo() {
        let mut queue = EditQueue::new();
        let id = next_vfs_id();

        let tx = EditTransaction::with_ops(
            "rename file",
            vec![EditOperation::Rename {
                node_id: id,
                old_name: "old.txt".into(),
                new_name: "new.txt".into(),
            }],
        );
        queue.push(tx);
        assert!(queue.can_undo());
        assert!(!queue.can_redo());

        let undone = queue.undo().unwrap();
        assert_eq!(undone.operations.len(), 1);

        assert!(queue.can_redo());
        assert!(!queue.can_undo());
    }

    #[test]
    fn test_edit_queue_undo_redo_cycle() {
        let mut queue = EditQueue::new();
        let id = next_vfs_id();

        queue.push(EditTransaction::with_ops(
            "op1",
            vec![EditOperation::Rename {
                node_id: id,
                old_name: "a".into(),
                new_name: "b".into(),
            }],
        ));
        queue.push(EditTransaction::with_ops(
            "op2",
            vec![EditOperation::Rename {
                node_id: id,
                old_name: "b".into(),
                new_name: "c".into(),
            }],
        ));

        queue.undo();
        assert!(queue.can_undo());
        assert!(queue.can_redo());

        queue.redo();
        assert!(!queue.can_redo());
        assert!(queue.can_undo());
    }

    #[test]
    fn test_edit_queue_redo_cleared_on_new_push() {
        let mut queue = EditQueue::new();
        let id = next_vfs_id();

        queue.push(EditTransaction::with_ops(
            "op1",
            vec![EditOperation::Rename {
                node_id: id,
                old_name: "a".into(),
                new_name: "b".into(),
            }],
        ));
        queue.undo();
        assert!(queue.can_redo());

        queue.push(EditTransaction::with_ops(
            "op2",
            vec![EditOperation::Rename {
                node_id: id,
                old_name: "b".into(),
                new_name: "c".into(),
            }],
        ));
        assert!(!queue.can_redo());
    }

    #[test]
    fn test_edit_queue_max_depth() {
        let mut queue = EditQueue::with_max_depth(3);
        for i in 0..5 {
            queue.push(EditTransaction::with_ops(
                format!("op{}", i),
                vec![EditOperation::Add {
                    node_id: next_vfs_id(),
                    parent_id: next_vfs_id(),
                    name: format!("f{}.txt", i),
                    fs_path: None,
                    is_directory: false,
                }],
            ));
        }
        assert_eq!(queue.undo_stack().len(), 3);
    }

    #[test]
    fn test_transaction_reverse_add() {
        let node_id = next_vfs_id();
        let parent_id = next_vfs_id();

        let tx = EditTransaction::with_ops(
            "add file",
            vec![EditOperation::Add {
                node_id,
                parent_id,
                name: "new.txt".into(),
                fs_path: None,
                is_directory: false,
            }],
        );

        let reversed = tx.reverse();
        assert_eq!(reversed.len(), 1);
        match &reversed[0] {
            EditOperation::Delete { node_id: n, .. } => assert_eq!(*n, node_id),
            _ => panic!("expected Delete operation"),
        }
    }

    #[test]
    fn test_transaction_reverse_delete() {
        let node_id = next_vfs_id();

        let tx = EditTransaction::with_ops(
            "delete file",
            vec![EditOperation::Delete {
                node_id,
                saved_name: "file.txt".into(),
                saved_parent: None,
                saved_is_directory: false,
                saved_original_index: Some(5),
                saved_fs_path: None,
                saved_child_ids: Vec::new(),
            }],
        );

        let reversed = tx.reverse();
        assert_eq!(reversed.len(), 1);
        match &reversed[0] {
            EditOperation::Add {
                node_id: n, name, ..
            } => {
                assert_eq!(*n, node_id);
                assert_eq!(name, "file.txt");
            }
            _ => panic!("expected Add operation"),
        }
    }

    #[test]
    fn test_reverse_rename_swaps_names() {
        let node_id = next_vfs_id();

        let tx = EditTransaction::with_ops(
            "rename",
            vec![EditOperation::Rename {
                node_id,
                old_name: "old".into(),
                new_name: "new".into(),
            }],
        );

        let reversed = tx.reverse();
        match &reversed[0] {
            EditOperation::Rename {
                node_id: n,
                old_name,
                new_name,
            } => {
                assert_eq!(*n, node_id);
                assert_eq!(old_name, "new");
                assert_eq!(new_name, "old");
            }
            _ => panic!("expected Rename operation"),
        }
    }
}
