//! Attribute keys and values for VFS nodes.
//!
//! Nodes carry a free-form attribute map: the key is a static string name
//! and the value is one of a small set of cross-platform types. The names
//! listed below are *conventions* — every VFS implementation (filesystem,
//! archive, network, ...) fills them so that overlays can align and merge
//! nodes from different sources. Unknown names are allowed and simply
//! ignored by generic code.

use std::collections::HashMap;

/// A static attribute name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AttrName(&'static str);

impl AttrName {
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    pub fn as_str(self) -> &'static str {
        self.0
    }
}

impl From<&'static str> for AttrName {
    fn from(name: &'static str) -> Self {
        Self(name)
    }
}

impl std::fmt::Display for AttrName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// The value of a node attribute.
#[derive(Debug, Clone, PartialEq)]
pub enum AttrValue {
    String(String),
    UInt(u64),
    Int(i64),
    Bool(bool),
    /// A civil (wall-clock) date-time; timezone-free so it is portable
    /// across platforms.
    DateTime(jiff::civil::DateTime),
    Bytes(Vec<u8>),
}

impl AttrValue {
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            AttrValue::UInt(v) => Some(*v),
            AttrValue::Int(v) if *v >= 0 => Some(*v as u64),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            AttrValue::Int(v) => Some(*v),
            AttrValue::UInt(v) if *v <= i64::MAX as u64 => Some(*v as i64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            AttrValue::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            AttrValue::String(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_datetime(&self) -> Option<&jiff::civil::DateTime> {
        match self {
            AttrValue::DateTime(v) => Some(v),
            _ => None,
        }
    }
}

impl From<String> for AttrValue {
    fn from(v: String) -> Self {
        AttrValue::String(v)
    }
}

impl From<&str> for AttrValue {
    fn from(v: &str) -> Self {
        AttrValue::String(v.to_string())
    }
}

impl From<u64> for AttrValue {
    fn from(v: u64) -> Self {
        AttrValue::UInt(v)
    }
}

impl From<bool> for AttrValue {
    fn from(v: bool) -> Self {
        AttrValue::Bool(v)
    }
}

/// A convenience alias: the attribute map carried by every node.
pub type AttrMap = HashMap<AttrName, AttrValue>;

// ============================================================================
// Conventional attribute names
// ============================================================================
// These names are *conventions*, not a closed schema: any implementation may
// add its own names. Generic code (overlay merging, diffing) only
// understands the ones listed here.

/// Size in bytes (files).
pub const SIZE: AttrName = AttrName::new("size");
/// Packed/compressed size in bytes (archive entries).
pub const PACKED_SIZE: AttrName = AttrName::new("packed_size");
/// Last modified time.
pub const MODIFIED: AttrName = AttrName::new("modified");
/// Creation time.
pub const CREATED: AttrName = AttrName::new("created");
/// Last access time.
pub const ACCESSED: AttrName = AttrName::new("accessed");
/// CRC-32 (archive entries).
pub const CRC: AttrName = AttrName::new("crc");
/// Whether the entry is encrypted.
pub const ENCRYPTED: AttrName = AttrName::new("encrypted");
/// Whether the entry is a symbolic link.
pub const SYMLINK: AttrName = AttrName::new("symlink");
/// Platform attributes (e.g. Windows file attributes).
pub const ATTRIBUTES: AttrName = AttrName::new("attributes");
/// POSIX permission bits.
pub const POSIX_MODE: AttrName = AttrName::new("posix_mode");
/// Host OS of the archive entry.
pub const HOST_OS: AttrName = AttrName::new("host_os");
/// Compression method (archive entries).
pub const METHOD: AttrName = AttrName::new("method");
/// Entry comment.
pub const COMMENT: AttrName = AttrName::new("comment");
/// Owning user.
pub const USER: AttrName = AttrName::new("user");
/// Owning group.
pub const GROUP: AttrName = AttrName::new("group");
/// File extension.
pub const EXTENSION: AttrName = AttrName::new("extension");
/// Hard-link target.
pub const HARDLINK: AttrName = AttrName::new("hardlink");
/// The data source of this node (e.g. `"archive"`, `"fs"`).
pub const SOURCE: AttrName = AttrName::new("source");
/// Original entry index inside an archive (used to locate entries when
/// committing changes).
pub const ARCHIVE_INDEX: AttrName = AttrName::new("archive_index");
/// Absolute path on disk (fs-backed nodes).
pub const FS_PATH: AttrName = AttrName::new("fs_path");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attr_name_is_static_and_comparable() {
        assert_eq!(SIZE, AttrName::new("size"));
        assert_ne!(SIZE, CRC);
        assert_eq!(SIZE.as_str(), "size");
    }

    #[test]
    fn attr_value_accessors() {
        assert_eq!(AttrValue::UInt(7).as_u64(), Some(7));
        assert_eq!(AttrValue::String("x".into()).as_str(), Some("x"));
        assert_eq!(AttrValue::Bool(true).as_bool(), Some(true));
        assert_eq!(AttrValue::UInt(7).as_bool(), None);
        assert_eq!(AttrValue::Int(-3).as_u64(), None);
        assert_eq!(AttrValue::Int(9).as_u64(), Some(9));
    }

    #[test]
    fn attr_map_supports_custom_names() {
        let mut map = AttrMap::new();
        map.insert(AttrName::new("my.custom"), AttrValue::UInt(1));
        map.insert(SIZE, AttrValue::UInt(2));
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&AttrName::new("my.custom")).and_then(|v| v.as_u64()), Some(1));
    }
}
