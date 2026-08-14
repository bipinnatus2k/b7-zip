//! A minimal, source-agnostic virtual file system core.
//!
//! This crate defines only the building blocks:
//!
//! * [`VfsNode`]s — identity, parent, name, `is_directory`, plus a free-form
//!   attribute map ([`attr`]) with *conventional* names shared across VFS
//!   implementations.
//! * [`Tree`] — the tree structure (insert/remove/rename, path resolution).
//! * [`Overlay`] — layering a working tree over a base tree, tracking
//!   dirty nodes; [`diff::build_changeset`] turns that into a [`Changeset`]
//!   ready for a task runner to commit.
//! * [`queue`] — an undo/redo transaction queue for in-memory edits.
//!
//! There is no notion of "layers" or "kinds" here: any component (filesystem
//! driver, archive driver, network driver, ...) builds [`Tree`]s of
//! [`VfsNode`]s and overlays them. The crate has no dependencies on engines,
//! archives, or the GUI.

pub mod attr;
pub mod changeset;
pub mod diff;
pub mod node;
pub mod overlay;
pub mod queue;
pub mod tree;

pub use attr::{AttrMap, AttrName, AttrValue};
pub use changeset::Changeset;
pub use node::{NodeId, VfsNode, next_node_id};
pub use overlay::{DirtyState, Overlay, OverlayError};
pub use queue::{EditOperation, EditQueue, EditTransaction};
pub use tree::Tree;

/// Backwards-compatible alias used by the edit queue and existing code.
pub type VfsNodeId = NodeId;

/// Backwards-compatible alias for [`next_node_id`].
pub fn next_vfs_id() -> NodeId {
    next_node_id()
}

/// Errors produced by tree operations.
#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    #[error("Node not found: {0:?}")]
    NodeNotFound(NodeId),
    #[error("Path not found: {0}")]
    PathNotFound(String),
    #[error("Not a directory: {0}")]
    NotADirectory(String),
    #[error("Entry already exists: {0}")]
    AlreadyExists(String),
    #[error("Operation not supported")]
    UnsupportedOperation,
    #[error("Internal error: {0}")]
    Internal(String),
}
