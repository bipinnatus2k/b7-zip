//! Framework-agnostic comparison of VFS trees and file contents.
//!
//! [`tree_vs_tree`] aligns two trees by path and reports added / removed /
//! modified files. [`content`] turns two byte buffers into either a side-by-
//! side line diff (text) or a digest comparison card (binary). Neither layer
//! knows about archives: callers supply content through callbacks, so the
//! same code paths serve base-vs-working diffs inside one session and
//! pairwise workspace comparisons.

pub mod content;
pub mod report;

pub use content::{compare_bytes, is_probably_text, BlobInfo, ContentDiff, DiffLine, LineKind};
pub use report::{tree_vs_tree, DiffEntry, DiffKind, DiffReport};

#[cfg(test)]
mod tests;
