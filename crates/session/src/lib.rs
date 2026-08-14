//! Archive editing sessions and session storage.

mod session;
mod store;

pub use session::{ArchiveSession, SessionError};
pub use store::{SessionId, SessionStore, next_archive_id};
