//! Temp directory management.
//!
//! A tiny, dependency-free component that resolves the application's temp
//! root. The root defaults to the system temp directory
//! ([`std::env::temp_dir`]) and can be replaced at runtime via
//! [`register_temp_root`] — for example when the user changes the temp
//! location in settings. Newly created sessions/jobs use the current root;
//! existing ones are not migrated.
//!
//! This crate has no knowledge of archives, VFS, or the GUI, so it can be
//! reused by any component (session, executor, shell, app).

use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// Errors produced by temp-root operations.
#[derive(Debug, thiserror::Error)]
pub enum TempError {
    #[error("failed to create temp root {path:?}: {source}")]
    CreateRoot { path: PathBuf, source: std::io::Error },
}

/// Global temp-root state. `None` means "use the system temp directory".
static ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// The current temp root.
///
/// Returns the registered root if one was set, otherwise the system temp
/// directory ([`std::env::temp_dir`]).
pub fn temp_root() -> PathBuf {
    ROOT.read()
        .ok()
        .and_then(|guard| guard.clone())
        .unwrap_or_else(std::env::temp_dir)
}

/// Whether a custom temp root has been registered.
pub fn is_registered() -> bool {
    ROOT.read().map(|guard| guard.is_some()).unwrap_or(false)
}

/// Register a custom temp root, replacing the previous one (if any).
///
/// The directory is created if it does not exist. New sessions/jobs resolve
/// against the new root; already-created directories keep their old location.
pub fn register_temp_root(path: impl Into<PathBuf>) -> Result<PathBuf, TempError> {
    let path = path.into();
    std::fs::create_dir_all(&path).map_err(|source| TempError::CreateRoot {
        path: path.clone(),
        source,
    })?;
    let canonical = strip_verbatim_prefix(path.canonicalize().unwrap_or(path));
    *ROOT.write().expect("temp root lock poisoned") = Some(canonical.clone());
    Ok(canonical)
}

/// Windows `canonicalize` returns extended-length paths prefixed with
/// `\\\\?\`; strip the prefix so downstream consumers (FFI, display,
/// path comparison) see the same form as ordinary user paths.
fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix("\\\\?\\") {
            if !rest.starts_with("UNC") {
                return PathBuf::from(rest);
            }
        }
    }
    path
}

/// Reset to the system temp directory (drop the custom root).
pub fn unregister_temp_root() {
    *ROOT.write().expect("temp root lock poisoned") = None;
}

/// Directory used for a session's extracted/working files.
pub fn session_dir(session_id: u64) -> PathBuf {
    temp_root().join("sessions").join(session_id.to_string())
}

/// Directory used for transient job files.
pub fn jobs_dir() -> PathBuf {
    temp_root().join("jobs")
}

/// Path for a job file with the given id (extension included).
pub fn job_file_path(id: &str) -> PathBuf {
    jobs_dir().join(format!("{id}.job.json"))
}

/// Ensure the jobs directory exists, creating it if necessary.
pub fn ensure_jobs_dir() -> std::io::Result<PathBuf> {
    let dir = jobs_dir();
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A relative path (e.g. an archive entry path) resolved against the temp
/// root, sanitized so it cannot escape the root.
pub fn resolve_in_temp(relative: &Path) -> PathBuf {
    temp_root().join(sanitize_relative(relative))
}

/// Strip components that would escape the root (`..`), and normalize
/// separators. Returns the sanitized relative path.
pub fn sanitize_relative(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => out.push(part),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                // A parent component cancels the previous normal component;
                // if there is none (would escape the root), it is ignored.
                out.pop();
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                // Absolute paths are treated as relative: drop the root.
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    static ROOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn defaults_to_system_temp() {
        let _root = ROOT_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        unregister_temp_root();
        assert!(!is_registered());
        assert_eq!(temp_root(), std::env::temp_dir());
    }

    #[test]
    fn register_and_reset() {
        let _root = ROOT_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let dir_canon = strip_verbatim_prefix(dir.path().canonicalize().unwrap());
        let root = register_temp_root(dir.path()).unwrap();
        assert!(is_registered());
        assert_eq!(temp_root(), root);
        assert!(temp_root().starts_with(&dir_canon));

        unregister_temp_root();
        assert!(!is_registered());
        assert_eq!(temp_root(), std::env::temp_dir());
    }

    #[test]
    fn register_replaces_previous() {
        let _root = ROOT_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        register_temp_root(a.path()).unwrap();
        register_temp_root(b.path()).unwrap();
        assert_eq!(temp_root(), strip_verbatim_prefix(b.path().canonicalize().unwrap()));
    }

    #[test]
    fn derived_paths_use_current_root() {
        let _root = ROOT_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        unregister_temp_root();
        let dir = tempfile::tempdir().unwrap();
        let dir_canon = strip_verbatim_prefix(dir.path().canonicalize().unwrap());
        register_temp_root(dir.path()).unwrap();

        let session = session_dir(42);
        assert!(session.starts_with(&dir_canon));
        assert_eq!(session.file_name().unwrap().to_str().unwrap(), "42");

        let job = job_file_path("abc");
        assert!(job.starts_with(&dir_canon));
        assert_eq!(job.file_name().unwrap().to_str().unwrap(), "abc.job.json");
    }

    #[test]
    fn sanitize_blocks_parent_escape() {
        assert_eq!(
            sanitize_relative(Path::new("../evil.txt")),
            PathBuf::from("evil.txt")
        );
        assert_eq!(
            sanitize_relative(Path::new("a/../../evil.txt")),
            PathBuf::from("evil.txt")
        );
        assert_eq!(
            sanitize_relative(Path::new("C:/windows/system32")),
            PathBuf::from("windows/system32")
        );
        assert_eq!(
            sanitize_relative(Path::new("a/b/c.txt")),
            PathBuf::from("a/b/c.txt")
        );
    }

    #[test]
    fn resolve_in_temp_stays_inside_root() {
        let _root = ROOT_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let dir_canon = strip_verbatim_prefix(dir.path().canonicalize().unwrap());
        register_temp_root(dir.path()).unwrap();
        let resolved = resolve_in_temp(Path::new("../../escape.txt"));
        assert!(resolved.starts_with(&dir_canon));
        assert!(resolved.ends_with("escape.txt"));
    }
}
