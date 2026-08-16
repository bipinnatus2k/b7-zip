//! Archive format detection by magic bytes and file extension.
//!
//! Detection is powered by the `file_format` crate; results are reported as
//! the generic [`ArchiveFormat`] identifier that other components (engine,
//! session, task) map to their own format types.

mod archive_format;
mod auto_format;
mod format_detector;
pub mod validator;
pub mod validators;

pub use archive_format::{ALL_FORMATS, ArchiveFormat};
pub use auto_format::{AutoFormat, Detection, DetectionError};
pub use format_detector::{DetectError, FormatDetector};
pub use validator::{FormatValidator, ValidatorRegistry};
