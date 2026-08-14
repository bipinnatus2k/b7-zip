//! Engine-facing data model and error types.
//!
//! These types are the boundary between the archive engine (`bit7z`) and
//! the rest of the application: VFS implementations build trees from
//! [`ArchiveEntry`]s, task runners translate their changesets into
//! [`EngineOp`]s, and the UI never touches raw FFI handles.

use std::path::PathBuf;

/// A single entry inside an archive.
#[derive(Debug, Clone, PartialEq)]
pub struct ArchiveEntry {
    /// Index of the entry inside the archive (stable while the archive is
    /// open; used to reference the entry for extraction/editing).
    pub index: u32,
    /// Entry name (last path component).
    pub name: String,
    /// Full path inside the archive (forward slashes).
    pub path: String,
    pub size: u64,
    pub packed_size: u64,
    pub is_directory: bool,
    pub is_encrypted: bool,
    pub is_symlink: bool,
    pub crc: Option<u32>,
    pub modified: Option<jiff::civil::DateTime>,
    pub created: Option<jiff::civil::DateTime>,
    pub accessed: Option<jiff::civil::DateTime>,
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

impl ArchiveEntry {
    /// Build a directory placeholder entry (used by listers that synthesize
    /// directory nodes from file paths).
    pub fn directory(index: u32, name: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            index,
            name: name.into(),
            path: path.into(),
            size: 0,
            packed_size: 0,
            is_directory: true,
            is_encrypted: false,
            is_symlink: false,
            crc: None,
            modified: None,
            created: None,
            accessed: None,
            attributes: None,
            posix_attrib: None,
            host_os: None,
            compression_method: None,
            comment: None,
            user: None,
            group: None,
            extension: None,
            hardlink: None,
        }
    }
}

/// Errors produced by the archive engine.
#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("failed to open archive: {0}")]
    OpenFailed(String),
    #[error("wrong password")]
    WrongPassword,
    #[error("archive is corrupted: {0}")]
    Corrupted(String),
    #[error("format not supported: {0}")]
    FormatNotSupported(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("operation cancelled")]
    Cancelled,
    #[error("unsupported operation: {0}")]
    UnsupportedOperation(String),
    #[error("engine error: {0}")]
    Engine(String),
}

impl From<String> for ArchiveError {
    fn from(message: String) -> Self {
        ArchiveError::Engine(message)
    }
}

/// A single edit operation to apply to an open archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineOp {
    /// Add a file from disk at `fs_path` into the archive at `archive_path`.
    Add { fs_path: PathBuf, archive_path: String },
    /// Replace the content of the entry `archive_index` (previously at
    /// `archive_path`) with `fs_path`.
    Modify { archive_index: u32, archive_path: String, fs_path: PathBuf },
    /// Remove the entry `archive_index`.
    Delete { archive_index: u32 },
    /// Rename the entry `archive_index` to `new_path`.
    Rename { archive_index: u32, new_path: String },
}

/// How to behave when the target already exists during extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverwriteMode {
    #[default]
    Ask,
    Overwrite,
    Skip,
    AutoRename,
}

/// Options for an extraction operation.
#[derive(Debug, Clone, Default)]
pub struct ExtractOptions {
    pub overwrite: OverwriteMode,
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

/// Options for a compression operation.
#[derive(Debug, Clone)]
pub struct CompressOptions {
    pub format: crate::WriterFormat,
    pub level: crate::WriterCompressionLevel,
    pub method: Option<crate::WriterCompressionMethod>,
    pub dictionary_size: Option<u32>,
    pub word_size: Option<u32>,
    pub solid: Option<bool>,
    pub volume_size: Option<u64>,
    pub threads: u32,
    pub password: Option<String>,
    pub encrypt_headers: bool,
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl Default for CompressOptions {
    fn default() -> Self {
        Self {
            format: crate::WriterFormat::SevenZip,
            level: crate::WriterCompressionLevel::Normal,
            method: None,
            dictionary_size: None,
            word_size: None,
            solid: None,
            volume_size: None,
            threads: 0,
            password: None,
            encrypt_headers: false,
            cancel: None,
        }
    }
}

/// Result of a test operation.
#[derive(Debug, Clone)]
pub struct TestResult {
    pub all_ok: bool,
    pub total: u32,
    pub failed_count: u32,
    pub errors: Vec<String>,
}

/// Progress information shared with callbacks.
#[derive(Debug, Clone, Copy)]
pub struct ProgressInfo {
    pub processed: u64,
    pub total: u64,
}

/// The archive engine abstraction.
///
/// Implementations must be cheap to clone or share (the engine itself is
/// thread-safe; individual handles are serialized internally).
pub trait ArchiveEngine: Send + Sync {
    /// List all entries of the archive at `path`.
    fn list(&self, path: &std::path::Path, password: Option<&password::Password>) -> Result<Vec<ArchiveEntry>, ArchiveError>;

    /// Extract the given entries (by index) to `dest`.
    fn extract(&self, path: &std::path::Path, indices: &[u32], dest: &std::path::Path, password: Option<&password::Password>, options: &ExtractOptions) -> Result<(), ArchiveError>;

    /// Extract a single entry to an in-memory buffer.
    fn extract_to_buffer(&self, path: &std::path::Path, index: u32, password: Option<&password::Password>) -> Result<Vec<u8>, ArchiveError>;

    /// Test the integrity of the archive.
    fn test(&self, path: &std::path::Path, password: Option<&password::Password>) -> Result<TestResult, ArchiveError>;

    /// Create a new archive at `target` from `inputs` (fs paths).
    fn compress(&self, inputs: &[PathBuf], target: &std::path::Path, options: &CompressOptions) -> Result<(), ArchiveError>;

    /// Apply edit operations to an existing archive at `path`.
    fn update(&self, path: &std::path::Path, ops: &[EngineOp], password: Option<&password::Password>) -> Result<(), ArchiveError>;

    /// Whether the archive at `path` has any encrypted content (static check).
    fn is_encrypted(&self, path: &std::path::Path) -> Result<bool, ArchiveError>;

    /// Whether the archive at `path` has an encrypted header (static check).
    fn is_header_encrypted(&self, path: &std::path::Path) -> Result<bool, ArchiveError>;
}
