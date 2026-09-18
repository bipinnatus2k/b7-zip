//! Archive editing sessions and session storage.

mod session;
mod store;

pub use session::{ArchiveSession, ChangeEntry, SessionError};
pub use store::{SessionId, SessionStore, next_archive_id};
