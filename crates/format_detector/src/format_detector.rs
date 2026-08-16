
//! Format detection port.
//!
//! Defines the abstract interface for magic-bytes-based archive format
//! detection. Backends implement this trait to advertise what formats
//! they can identify from file content alone.

use std::path::Path;
use crate::archive_format::ArchiveFormat;

/// Detects archive format from file content (magic bytes / header).
pub trait FormatDetector: Send + Sync {
    /// Detect the archive format by reading the file header.
    fn detect_format(&self, path: &Path) -> Result<ArchiveFormat, DetectError>;
}

/// Errors from format detection.
#[derive(Debug, thiserror::Error)]
pub enum DetectError {
    #[error("unknown or unsupported archive format")]
    UnknownFormat,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}