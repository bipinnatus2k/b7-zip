//! Task specifications: the serializable contract between the shell plugin,
//! the CLI, the manager, and the executor.
//!
//! **Passwords are never part of a job file**: specs carry only a
//! `password_hint` flag; the actual secret is supplied out-of-band (dialog,
//! environment, command line) via [`crate::run`].

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Current job file schema version.
pub const JOB_FILE_VERSION: u32 = 1;

/// A job file: schema version + id + the operation to perform.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobFile {
    pub version: u32,
    pub id: String,
    pub spec: JobSpec,
}

impl JobFile {
    pub fn new(id: impl Into<String>, spec: JobSpec) -> Self {
        Self {
            version: JOB_FILE_VERSION,
            id: id.into(),
            spec,
        }
    }

    /// Serialize to JSON for a job file on disk.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Parse a job file from JSON, checking the schema version.
    pub fn from_json(json: &str) -> Result<Self, JobFileError> {
        let file: JobFile = serde_json::from_str(json)?;
        if file.version != JOB_FILE_VERSION {
            return Err(JobFileError::UnsupportedVersion(file.version));
        }
        Ok(file)
    }
}

/// Errors produced when reading job files.
#[derive(Debug, thiserror::Error)]
pub enum JobFileError {
    #[error("invalid job file: {0}")]
    Invalid(String),
    #[error("unsupported job file version: {0}")]
    UnsupportedVersion(u32),
}

impl From<serde_json::Error> for JobFileError {
    fn from(error: serde_json::Error) -> Self {
        JobFileError::Invalid(error.to_string())
    }
}

/// Overwrite behavior for extraction.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum OverwriteSpec {
    #[default]
    Ask,
    Overwrite,
    Skip,
    AutoRename,
}

/// Archive format for compression jobs.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum FormatSpec {
    #[default]
    SevenZip,
    Zip,
    Tar,
    GZip,
    BZip2,
    Xz,
    Wim,
}

impl From<FormatSpec> for bit7z_rs::WriterFormat {
    fn from(value: FormatSpec) -> Self {
        match value {
            FormatSpec::SevenZip => bit7z_rs::WriterFormat::SevenZip,
            FormatSpec::Zip => bit7z_rs::WriterFormat::Zip,
            FormatSpec::Tar => bit7z_rs::WriterFormat::Tar,
            FormatSpec::GZip => bit7z_rs::WriterFormat::GZip,
            FormatSpec::BZip2 => bit7z_rs::WriterFormat::BZip2,
            FormatSpec::Xz => bit7z_rs::WriterFormat::Xz,
            FormatSpec::Wim => bit7z_rs::WriterFormat::Wim,
        }
    }
}

/// Compression level for compression jobs.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum LevelSpec {
    None,
    Fastest,
    Fast,
    #[default]
    Normal,
    Max,
    Ultra,
}

impl From<LevelSpec> for bit7z_rs::WriterCompressionLevel {
    fn from(value: LevelSpec) -> Self {
        match value {
            LevelSpec::None => bit7z_rs::WriterCompressionLevel::None,
            LevelSpec::Fastest => bit7z_rs::WriterCompressionLevel::Fastest,
            LevelSpec::Fast => bit7z_rs::WriterCompressionLevel::Fast,
            LevelSpec::Normal => bit7z_rs::WriterCompressionLevel::Normal,
            LevelSpec::Max => bit7z_rs::WriterCompressionLevel::Max,
            LevelSpec::Ultra => bit7z_rs::WriterCompressionLevel::Ultra,
        }
    }
}

/// A file to add to an archive (with an optional archive-side path).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AddItem {
    pub fs_path: PathBuf,
    pub archive_path: String,
}

/// The operation a job performs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum JobSpec {
    Extract {
        archive: PathBuf,
        /// Entry indices; `None`/empty means all entries.
        items: Vec<u32>,
        target: PathBuf,
        overwrite: OverwriteSpec,
        password_hint: bool,
    },
    Compress {
        inputs: Vec<PathBuf>,
        target: PathBuf,
        format: FormatSpec,
        level: LevelSpec,
        solid: Option<bool>,
        volume: Option<u64>,
        threads: Option<u32>,
        encrypt_headers: bool,
        password_hint: bool,
    },
    Test {
        archive: PathBuf,
        password_hint: bool,
    },
    Add {
        archive: PathBuf,
        items: Vec<AddItem>,
        password_hint: bool,
    },
    Delete {
        archive: PathBuf,
        indices: Vec<u32>,
        password_hint: bool,
    },
    Rename {
        archive: PathBuf,
        index: u32,
        new_path: String,
        password_hint: bool,
    },
    NewFolder {
        archive: PathBuf,
        folder_path: String,
        password_hint: bool,
    },
    Checksum {
        path: PathBuf,
        algorithm: checksum::ChecksumAlgorithm,
    },
}
