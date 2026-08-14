
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use jiff::__jcore::civil::DateTime;

static NEXT_VFS_ID: AtomicU64 = AtomicU64::new(1);

pub fn next_vfs_id() -> VfsNodeId {
    VfsNodeId(NEXT_VFS_ID.fetch_add(1, Ordering::Relaxed))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VfsNodeId(u64);

impl VfsNodeId {
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct VfsNode {
    pub id: VfsNodeId,
    pub parent: Option<VfsNodeId>,
    pub name: String,
    pub is_directory: bool,
    pub original_index: Option<u32>,
    pub fs_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VfsMetadata {
    pub size: u64,
    pub compressed_size: u64,
    pub modified: Option<DateTime>,
    pub created: Option<DateTime>,
    pub accessed: Option<DateTime>,
    pub crc: Option<u32>,
    pub is_encrypted: bool,
    pub is_symlink: bool,
    pub attributes: Option<u32>,
    pub posix_attrib: Option<u32>,
    pub host_os: Option<u8>,
    pub compression_method: Option<String>,
    pub comment: Option<String>,
    pub user: Option<String>,
    pub group: Option<String>,
    pub extension: Option<String>,
    pub hardlink: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    #[error("Node not found: {0:?}")]
    NodeNotFound(VfsNodeId),
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

pub mod overlay;
pub mod queue;
pub mod tree;

pub use overlay::OverlayVfs;
pub use queue::{EditOperation, EditQueue, EditTransaction};
pub use tree::{DirtyEntry, DirtyTree, DirtyType, Tree};
