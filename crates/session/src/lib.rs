use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ARCHIVE_ID: AtomicU64 = AtomicU64::new(1);

pub fn next_archive_id() -> u64 {
    NEXT_ARCHIVE_ID.fetch_add(1, Ordering::Relaxed)
}

/// Stable identifier for an archive editing session.
pub type SessionId = u64;

/// Lightweight handle to an opened archive session.
///
/// The actual state (VFS, dirty tree, edit queue) is stored separately by the
/// runtime session manager. `ArchiveSession` is cheap to clone and pass around.
#[derive(Debug, Clone)]
pub struct ArchiveSession {
    pub id: SessionId,
    pub path: PathBuf,
}

impl ArchiveSession {
    pub fn new(path: PathBuf) -> Self {
        Self {
            id: next_archive_id(),
            path,
        }
    }

}

/// Complete in-memory state for an archive editing session.
#[derive(Debug, Clone)]
pub struct SessionState {
    pub session: ArchiveSession,
}

/// Opaque reference to a session state.
pub type SessionRef = Arc<SessionState>;

/// Storage backend for runtime session state.
pub trait SessionStore: Send + Sync {
    /// Insert or replace a session state.
    fn insert(&self, state: SessionState);

    /// Get a reference to the session state, if it exists.
    fn get(&self, id: SessionId) -> Option<SessionRef>;

    /// Remove a session state and return it.
    fn remove(&self, id: SessionId) -> Option<SessionState>;

    /// Returns true if the store contains the session.
    fn contains(&self, id: SessionId) -> bool;

    /// Return the ids of all stored sessions.
    fn all(&self) -> Vec<SessionId>;
}


