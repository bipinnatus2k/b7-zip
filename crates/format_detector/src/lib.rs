//! Archive format detection by magic bytes and file extension.
//!
//! A small leaf component with no dependencies. Returns a generic
//! [`ArchiveFormat`] identifier that other components (engine, session,
//! task) map to their own format types.

mod format_detector;
pub(crate) mod validator;
mod validators;
mod auto_format;
mod archive_format;

#[cfg(test)]
mod tests {}
