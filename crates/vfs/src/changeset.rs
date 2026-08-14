//! Changesets: the outcome of diffing an overlay, ready to be consumed by a
//! task runner to commit edits back to an archive.

use std::path::PathBuf;

/// A file from the working tree to add to the archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddOp {
    /// Absolute path of the file on disk.
    pub fs_path: PathBuf,
    /// Path inside the archive (forward slashes).
    pub archive_path: String,
}

/// An existing archive entry whose content was replaced by a working file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModifyOp {
    /// Index of the original archive entry.
    pub archive_index: u32,
    /// Absolute path of the replacement file on disk.
    pub fs_path: PathBuf,
}

/// An archive entry to remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeleteOp {
    pub archive_index: u32,
}

/// An archive entry to rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameOp {
    pub archive_index: u32,
    /// New path inside the archive.
    pub new_path: String,
}

/// The complete set of edits needed to commit an overlay back to an archive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changeset {
    pub additions: Vec<AddOp>,
    pub modifications: Vec<ModifyOp>,
    pub deletions: Vec<DeleteOp>,
    pub renames: Vec<RenameOp>,
}

impl Changeset {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.additions.is_empty()
            && self.modifications.is_empty()
            && self.deletions.is_empty()
            && self.renames.is_empty()
    }

    pub fn len(&self) -> usize {
        self.additions.len() + self.modifications.len() + self.deletions.len() + self.renames.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_len() {
        let cs = Changeset::new();
        assert!(cs.is_empty());
        assert_eq!(cs.len(), 0);

        let mut cs = Changeset::new();
        cs.additions.push(AddOp { fs_path: PathBuf::from("a"), archive_path: "a".into() });
        cs.deletions.push(DeleteOp { archive_index: 1 });
        assert!(!cs.is_empty());
        assert_eq!(cs.len(), 2);
    }
}
