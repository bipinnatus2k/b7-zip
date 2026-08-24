//! Session storage: keeps open archive sessions alive for the manager GUI.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::session::ArchiveSession;

static NEXT_ARCHIVE_ID: AtomicU64 = AtomicU64::new(1);

/// Allocate the next session id.
pub fn next_archive_id() -> u64 {
    NEXT_ARCHIVE_ID.fetch_add(1, Ordering::Relaxed)
}

/// Stable identifier for an archive editing session.
pub type SessionId = u64;

/// In-memory session store (thread-safe).
#[derive(Default, Clone)]
pub struct SessionStore {
    sessions: Arc<Mutex<HashMap<SessionId, Arc<Mutex<ArchiveSession>>>>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a session; fails if the id is already taken.
    pub fn insert(
        &self,
        session: ArchiveSession,
    ) -> Result<Arc<Mutex<ArchiveSession>>, crate::SessionError> {
        let id = session.id();
        let arc = Arc::new(Mutex::new(session));
        let mut map = self.sessions.lock().expect("session store lock poisoned");
        if map.contains_key(&id) {
            return Err(crate::SessionError::SessionExists(id));
        }
        map.insert(id, arc.clone());
        Ok(arc)
    }

    /// Get a session by id.
    pub fn get(&self, id: SessionId) -> Option<Arc<Mutex<ArchiveSession>>> {
        self.sessions
            .lock()
            .expect("session store lock poisoned")
            .get(&id)
            .cloned()
    }

    /// Remove a session and return it.
    pub fn remove(&self, id: SessionId) -> Option<Arc<Mutex<ArchiveSession>>> {
        self.sessions
            .lock()
            .expect("session store lock poisoned")
            .remove(&id)
    }

    /// Whether the store contains the session.
    pub fn contains(&self, id: SessionId) -> bool {
        self.sessions
            .lock()
            .expect("session store lock poisoned")
            .contains_key(&id)
    }

    /// All session ids.
    pub fn all(&self) -> Vec<SessionId> {
        self.sessions
            .lock()
            .expect("session store lock poisoned")
            .keys()
            .copied()
            .collect()
    }

    /// Remove all sessions (used on shutdown).
    pub fn clear(&self) {
        self.sessions
            .lock()
            .expect("session store lock poisoned")
            .clear();
    }
}

/// Helper for tests and tooling: build a stable id from a path.
#[allow(dead_code)]
pub fn session_id_for(path: &PathBuf) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}
