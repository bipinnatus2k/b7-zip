//! Archive editing sessions: the full "open -> extract to temp -> edit ->
//! save back" workflow wiring.
//!
//! A session owns:
//!
//! * the opened archive's entry tree ([`Overlay`] over an archive VFS tree),
//! * a working directory under the temp root where entries are lazily
//!   extracted for editing,
//! * a [`fs::FsTree`] mirroring that working directory, kept in sync by a
//!   [`fs_watcher::FsWatcher`],
//! * the dirty tracking that turns user edits into a commit-ready
//!   [`vfs::Changeset`].

use archive_vfs as archive_vfs_crate;
use bit7z_rs::{ArchiveEngine, ArchiveError};
use fs::FsTree;
use fs_watcher::{FsEvent, FsEventKind, FsWatcher, WatchConfig};
use password::Password;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use vfs::{Changeset, Overlay};

/// A live archive editing session.
pub struct ArchiveSession {
    id: u64,
    archive_path: PathBuf,
    engine: Arc<dyn ArchiveEngine>,
    overlay: Overlay,
    fs_tree: FsTree,
    watcher: Option<FsWatcher>,
    work_dir: PathBuf,
    password: Option<Password>,
}

impl ArchiveSession {
    /// Open an archive and build the base overlay from its entries.
    pub fn open(
        id: u64,
        engine: Arc<dyn ArchiveEngine>,
        archive_path: &Path,
        password: Option<&Password>,
    ) -> Result<Self, SessionError> {
        let entries = engine.list(archive_path, password)?;
        let tree = archive_vfs_crate::build_tree(&entries);
        let overlay = Overlay::new(tree);

        let work_dir = temp::session_dir(id);
        // Session directories are derived from a predictable id. Start from
        // an empty directory so stale files from a previous run/instance can
        // never leak into the new overlay.
        let _ = std::fs::remove_dir_all(&work_dir);
        std::fs::create_dir_all(&work_dir)?;

        let fs_tree = FsTree::scan(&work_dir)?;
        let watcher = FsWatcher::watch(work_dir.clone(), WatchConfig::default()).ok();

        Ok(Self {
            id,
            archive_path: archive_path.to_path_buf(),
            engine,
            overlay,
            fs_tree,
            watcher,
            work_dir,
            password: password.cloned(),
        })
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn archive_path(&self) -> &Path {
        &self.archive_path
    }

    pub fn overlay(&self) -> &Overlay {
        &self.overlay
    }

    pub fn overlay_mut(&mut self) -> &mut Overlay {
        &mut self.overlay
    }

    /// The temp working directory for this session.
    pub fn work_dir(&self) -> &Path {
        &self.work_dir
    }

    /// Lazily extract the entry at `archive_index` into the working
    /// directory (mirroring its archive-relative path). Returns the
    /// extracted file path.
    pub fn extract_to_workdir(&mut self, archive_index: u32) -> Result<PathBuf, SessionError> {
        let entry = self
            .overlay
            .base()
            .nodes()
            .values()
            .find(|n| n.archive_index() == Some(archive_index))
            .ok_or_else(|| SessionError::EntryNotFound(archive_index))?;
        let path = self
            .overlay
            .base()
            .path_of(entry.id)
            .unwrap_or_else(|| entry.name.clone());
        // Mirror the archive-relative path inside the working directory,
        // sanitized so it cannot escape it.
        let target = self
            .work_dir
            .join(temp::sanitize_relative(Path::new(&path)));

        // Extract straight to disk instead of buffering the whole entry in
        // memory. bit7z's SafeOutPathBuilder also guards against path
        // traversal while writing under `work_dir`.
        self.engine.extract(
            &self.archive_path,
            &[archive_index],
            &self.work_dir,
            self.password.as_ref(),
            &bit7z_rs::ExtractOptions {
                overwrite: bit7z_rs::OverwriteMode::Overwrite,
                ..Default::default()
            },
        )?;
        // Register the new file in the fs tree so later Modify events can
        // find it (the watcher only sees changes *after* this point).
        let event = FsEvent {
            kind: fs_watcher::FsEventKind::Create,
            path: target.clone(),
        };
        let _ = self.fs_tree.apply_event(&event);
        self.overlay.sync_from(self.fs_tree.tree());
        Ok(target)
    }

    /// Drain pending watcher events into the fs tree, then sync the overlay.
    pub fn poll_events(&mut self) {
        let mut events = Vec::new();
        if let Some(watcher) = &self.watcher {
            while let Ok(event) = watcher.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            self.apply_event(&event);
        }
    }

    /// Apply a single event (used by tests and headless drivers).
    pub fn apply_event(&mut self, event: &FsEvent) {
        let new_rel = event
            .path
            .strip_prefix(&self.work_dir)
            .ok()
            .map(|p| p.to_string_lossy().replace('\\', "/"));
        let from_rel = match &event.kind {
            FsEventKind::Rename { from } => from
                .strip_prefix(&self.work_dir)
                .ok()
                .map(|p| p.to_string_lossy().replace('\\', "/")),
            _ => None,
        };

        if self.fs_tree.apply_event(event).is_ok() {
            // Explicit filesystem removals must be reflected as overlay
            // deletions; `sync_from` alone only adds/modifies.
            if matches!(event.kind, FsEventKind::Remove)
                && let Some(path) = new_rel.as_deref()
            {
                self.overlay.remove_path(path);
            }
            if matches!(event.kind, FsEventKind::Rename { .. })
                && let (Some(from), Some(to)) = (from_rel.as_deref(), new_rel.as_deref())
            {
                let _ = self.overlay.rename_path(from, to);
            }
            self.overlay.sync_from(self.fs_tree.tree());
        }
    }

    /// The current commit-ready changeset (diff of dirty nodes vs base).
    pub fn changeset(&self) -> Changeset {
        vfs::diff::build_changeset(
            self.overlay.base(),
            self.overlay.working(),
            self.overlay.dirty(),
        )
    }

    /// Whether the session has unsaved changes.
    pub fn has_changes(&self) -> bool {
        self.overlay.has_changes()
    }

    /// Commit the current changeset back to the archive.
    pub fn commit(&mut self) -> Result<(), SessionError> {
        let changeset = self.changeset();
        if changeset.is_empty() {
            // Dirty entries that cannot be mapped to archive operations
            // (e.g. a modified synthetic directory) must not leave the
            // session permanently dirty.
            self.overlay.on_commit_success();
            return Ok(());
        }
        let ops = task::changeset_to_ops(&changeset);
        self.engine
            .update(&self.archive_path, &ops, self.password.as_ref())?;
        self.overlay.on_commit_success();
        Ok(())
    }

    /// Discard pending edits (revert overlay to base).
    pub fn discard(&mut self) {
        self.overlay.discard_pending();
    }

    /// Re-list the archive and rebuild the overlay from scratch. Used by the
    /// manager after operations that modify the archive in place (delete,
    /// rename, ...) so the view reflects the new contents.
    pub fn reload(&mut self) -> Result<(), SessionError> {
        let entries = self
            .engine
            .list(&self.archive_path, self.password.as_ref())?;
        let tree = archive_vfs_crate::build_tree(&entries);
        self.overlay = Overlay::new(tree);
        self.fs_tree = FsTree::scan(&self.work_dir)?;
        // Re-apply local files from the working directory onto the fresh
        // archive tree so unsaved edits survive a reload as dirty nodes.
        self.overlay.sync_from(self.fs_tree.tree());
        Ok(())
    }
}

impl Drop for ArchiveSession {
    fn drop(&mut self) {
        // Stop watching before deleting the watched directory.
        self.watcher.take();
        let _ = std::fs::remove_dir_all(&self.work_dir);
    }
}

/// Errors produced by sessions.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error(transparent)]
    Archive(#[from] ArchiveError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("entry not found: {0}")]
    EntryNotFound(u32),
    #[error("session already exists: {0}")]
    SessionExists(u64),
}
