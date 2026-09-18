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
        ArchiveEntryBuilder::new(index, name, path).dir().build()
    }
}

/// Builder for [`ArchiveEntry`], providing ergonomic construction of the
/// many optional fields.
pub struct ArchiveEntryBuilder {
    index: u32,
    name: String,
    path: String,
    size: u64,
    packed_size: u64,
    is_directory: bool,
    is_encrypted: bool,
    is_symlink: bool,
    crc: Option<u32>,
    modified: Option<jiff::civil::DateTime>,
    created: Option<jiff::civil::DateTime>,
    accessed: Option<jiff::civil::DateTime>,
    attributes: Option<u32>,
    posix_attrib: Option<u32>,
    host_os: Option<u8>,
    compression_method: Option<String>,
    comment: Option<String>,
    user: Option<String>,
    group: Option<String>,
    extension: Option<String>,
    hardlink: Option<String>,
}

impl ArchiveEntryBuilder {
    /// Begin building an entry at `path`. `name` defaults to the last path
    /// component and `size`/`packed_size` default to zero.
    pub fn new(index: u32, name: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            index,
            name: name.into(),
            path: path.into(),
            size: 0,
            packed_size: 0,
            is_directory: false,
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

    /// Mark the entry as a directory.
    pub fn dir(mut self) -> Self {
        self.is_directory = true;
        self
    }

    pub fn size(mut self, size: u64) -> Self {
        self.size = size;
        self
    }

    pub fn packed_size(mut self, packed_size: u64) -> Self {
        self.packed_size = packed_size;
        self
    }

    pub fn encrypted(mut self) -> Self {
        self.is_encrypted = true;
        self
    }

    pub fn symlink(mut self) -> Self {
        self.is_symlink = true;
        self
    }

    pub fn crc(mut self, crc: u32) -> Self {
        self.crc = Some(crc);
        self
    }

    pub fn modified(mut self, value: jiff::civil::DateTime) -> Self {
        self.modified = Some(value);
        self
    }

    pub fn created(mut self, value: jiff::civil::DateTime) -> Self {
        self.created = Some(value);
        self
    }

    pub fn accessed(mut self, value: jiff::civil::DateTime) -> Self {
        self.accessed = Some(value);
        self
    }

    pub fn attributes(mut self, value: u32) -> Self {
        self.attributes = Some(value);
        self
    }

    pub fn posix_attrib(mut self, value: u32) -> Self {
        self.posix_attrib = Some(value);
        self
    }

    pub fn host_os(mut self, value: u8) -> Self {
        self.host_os = Some(value);
        self
    }

    pub fn compression_method(mut self, value: impl Into<String>) -> Self {
        self.compression_method = Some(value.into());
        self
    }

    pub fn comment(mut self, value: impl Into<String>) -> Self {
        self.comment = Some(value.into());
        self
    }

    pub fn user(mut self, value: impl Into<String>) -> Self {
        self.user = Some(value.into());
        self
    }

    pub fn group(mut self, value: impl Into<String>) -> Self {
        self.group = Some(value.into());
        self
    }

    pub fn extension(mut self, value: impl Into<String>) -> Self {
        self.extension = Some(value.into());
        self
    }

    pub fn hardlink(mut self, value: impl Into<String>) -> Self {
        self.hardlink = Some(value.into());
        self
    }

    /// Consume the builder and produce the entry.
    pub fn build(self) -> ArchiveEntry {
        ArchiveEntry {
            index: self.index,
            name: self.name,
            path: self.path,
            size: self.size,
            packed_size: self.packed_size,
            is_directory: self.is_directory,
            is_encrypted: self.is_encrypted,
            is_symlink: self.is_symlink,
            crc: self.crc,
            modified: self.modified,
            created: self.created,
            accessed: self.accessed,
            attributes: self.attributes,
            posix_attrib: self.posix_attrib,
            host_os: self.host_os,
            compression_method: self.compression_method,
            comment: self.comment,
            user: self.user,
            group: self.group,
            extension: self.extension,
            hardlink: self.hardlink,
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
    Add {
        fs_path: PathBuf,
        archive_path: String,
    },
    /// Replace the content of the entry `archive_index` (previously at
    /// `archive_path`) with `fs_path`.
    Modify {
        archive_index: u32,
        archive_path: String,
        fs_path: PathBuf,
    },
    /// Remove the entry `archive_index`.
    Delete { archive_index: u32 },
    /// Rename the entry `archive_index` to `new_path`.
    Rename {
        archive_index: u32,
        new_path: String,
    },
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

/// A progress callback: `(bytes_processed, bytes_total)`.
/// Return value is not used; cancellation goes through the `cancel` flag.
pub type ProgressFn = dyn Fn(u64, u64) + Send + Sync;

/// A per-file callback: the archive path of the file being processed.
pub type FileFn = dyn Fn(&str) + Send + Sync;

/// An overwrite conflict callback used by [`OverwriteMode::Ask`].
///
/// Receives the destination path of an existing file and returns `true` to
/// overwrite it or `false` to skip it.
pub type ConflictFn = dyn Fn(&str) -> bool + Send + Sync;

/// Options for an extraction operation.
#[derive(Clone, Default)]
pub struct ExtractOptions {
    pub overwrite: OverwriteMode,
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// While set, the operation blocks inside its progress callback. The
    /// pause must be released for cancellation to take effect.
    pub pause: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Optional byte progress callback (invoked from the worker thread).
    pub progress: Option<std::sync::Arc<ProgressFn>>,
    /// Optional per-file callback (invoked from the worker thread).
    pub file: Option<std::sync::Arc<FileFn>>,
    /// Conflict resolver for [`OverwriteMode::Ask`]. `true` overwrites,
    /// `false` skips. Required when `overwrite` is `Ask`.
    pub on_conflict: Option<std::sync::Arc<ConflictFn>>,
}

/// Options for a compression operation.
#[derive(Clone)]
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
    /// While set, the operation blocks inside its progress callback. The
    /// pause must be released for cancellation to take effect.
    pub pause: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Optional byte progress callback (invoked from the worker thread).
    pub progress: Option<std::sync::Arc<ProgressFn>>,
    /// Optional per-file callback (invoked from the worker thread).
    pub file: Option<std::sync::Arc<FileFn>>,
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
            pause: None,
            progress: None,
            file: None,
        }
    }
}

/// Options for a test operation.
#[derive(Clone, Default)]
pub struct TestOptions {
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// While set, the operation blocks inside its progress callback.
    pub pause: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Optional byte progress callback (invoked from the worker thread).
    pub progress: Option<std::sync::Arc<ProgressFn>>,
    /// Optional per-file callback (invoked from the worker thread).
    pub file: Option<std::sync::Arc<FileFn>>,
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
    fn list(
        &self,
        path: &std::path::Path,
        password: Option<&password::Password>,
    ) -> Result<Vec<ArchiveEntry>, ArchiveError>;

    /// Extract the given entries (by index) to `dest`.
    fn extract(
        &self,
        path: &std::path::Path,
        indices: &[u32],
        dest: &std::path::Path,
        password: Option<&password::Password>,
        options: &ExtractOptions,
    ) -> Result<(), ArchiveError>;

    /// Extract a single entry to an in-memory buffer.
    fn extract_to_buffer(
        &self,
        path: &std::path::Path,
        index: u32,
        password: Option<&password::Password>,
    ) -> Result<Vec<u8>, ArchiveError>;

    /// Test the integrity of the archive.
    fn test(
        &self,
        path: &std::path::Path,
        password: Option<&password::Password>,
    ) -> Result<TestResult, ArchiveError> {
        self.test_with_options(path, password, &TestOptions::default())
    }

    /// Like [`test`](Self::test) but streaming progress and honoring
    /// cancel/pause. The default implementation ignores the options.
    fn test_with_options(
        &self,
        path: &std::path::Path,
        password: Option<&password::Password>,
        options: &TestOptions,
    ) -> Result<TestResult, ArchiveError> {
        let _ = options;
        self.test(path, password)
    }

    /// Create a new archive at `target` from `inputs` (fs paths).
    fn compress(
        &self,
        inputs: &[PathBuf],
        target: &std::path::Path,
        options: &CompressOptions,
    ) -> Result<(), ArchiveError>;

    /// Apply edit operations to an existing archive at `path`.
    fn update(
        &self,
        path: &std::path::Path,
        ops: &[EngineOp],
        password: Option<&password::Password>,
    ) -> Result<(), ArchiveError>;

    /// Whether the archive at `path` has any encrypted content (static check).
    fn is_encrypted(&self, path: &std::path::Path) -> Result<bool, ArchiveError>;

    /// Whether the archive at `path` has an encrypted header (static check).
    fn is_header_encrypted(&self, path: &std::path::Path) -> Result<bool, ArchiveError>;
}
