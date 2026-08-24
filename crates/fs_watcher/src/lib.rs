//! File system watcher component.
//!
//! A standalone wrapper around `notify` that produces a stream of
//! normalized [`FsEvent`]s relative to a watched root directory. It has no
//! dependencies on VFS, archives, or the GUI: any component that needs to
//! observe a directory (e.g. the session's extracted-file working directory)
//! can consume it.

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as NotifyWatcher};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

/// What happened to a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsEventKind {
    Create,
    Modify,
    Remove,
    /// A rename from `from` to the event's `path`.
    Rename {
        from: PathBuf,
    },
}

/// A normalized file system event. `path` is absolute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsEvent {
    pub kind: FsEventKind,
    pub path: PathBuf,
}

/// Watcher configuration.
#[derive(Debug, Clone)]
pub struct WatchConfig {
    /// Merge repeated `Modify` events for the same path within this window.
    pub debounce: Duration,
    /// File names ending with any of these suffixes are ignored
    /// (e.g. editor temp files).
    pub ignore_suffixes: Vec<String>,
    /// Ignore dot-files (names starting with `.`).
    pub ignore_hidden: bool,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(200),
            ignore_suffixes: vec![".tmp".into(), ".swp".into(), ".lock".into(), "~".into()],
            ignore_hidden: true,
        }
    }
}

/// A live directory watcher.
pub struct FsWatcher {
    _watcher: RecommendedWatcher,
    rx: Receiver<FsEvent>,
    root: PathBuf,
}

impl FsWatcher {
    /// Start watching `root` recursively.
    pub fn watch(root: impl Into<PathBuf>, config: WatchConfig) -> notify::Result<Self> {
        let root = root.into();
        let (tx, rx) = channel::<FsEvent>();
        let (notify_tx, notify_rx) = channel::<notify::Result<Event>>();
        let mut watcher = notify::recommended_watcher(notify_tx)?;
        watcher.watch(&root, RecursiveMode::Recursive)?;

        // Pump notify events on a worker thread, normalize, debounce, and
        // forward to the public channel.
        let pump_root = root.clone();
        let pump_config = config.clone();
        let pump_handle = std::thread::spawn(move || {
            pump_events(notify_rx, tx, &pump_root, &pump_config);
        });
        let _ = pump_handle; // detached: runs until the process ends

        Ok(Self {
            _watcher: watcher,
            rx,
            root,
        })
    }

    /// The watched root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Receive the next event (blocks).
    pub fn recv(&self) -> std::result::Result<FsEvent, RecvError> {
        self.rx.recv().map_err(|_| RecvError)
    }

    /// Receive the next event without blocking.
    pub fn try_recv(&self) -> std::result::Result<FsEvent, TryRecvError> {
        self.rx.try_recv().map_err(|e| match e {
            std::sync::mpsc::TryRecvError::Empty => TryRecvError::Empty,
            std::sync::mpsc::TryRecvError::Disconnected => TryRecvError::Disconnected,
        })
    }

    /// Receive the next event, waiting at most `timeout`.
    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> std::result::Result<FsEvent, RecvTimeoutError> {
        self.rx.recv_timeout(timeout).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => RecvTimeoutError::Timeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => RecvTimeoutError::Disconnected,
        })
    }
}

/// Error returned when receiving from a disconnected watcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecvError;

/// Errors returned by [`FsWatcher::try_recv`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TryRecvError {
    Empty,
    Disconnected,
}

/// Errors returned by [`FsWatcher::recv_timeout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecvTimeoutError {
    Timeout,
    Disconnected,
}

fn pump_events(
    notify_rx: Receiver<notify::Result<Event>>,
    tx: Sender<FsEvent>,
    _root: &Path,
    config: &WatchConfig,
) {
    let mut pending_rename_from: VecDeque<FsEvent> = VecDeque::new();
    let mut last_modify: HashMap<PathBuf, Instant> = HashMap::new();

    while let Ok(result) = notify_rx.recv() {
        let Ok(event) = result else {
            continue;
        };
        if matches!(
            &event.kind,
            EventKind::Modify(notify::event::ModifyKind::Name(
                notify::event::RenameMode::Both
            ))
        ) {
            let paths: Vec<&std::path::Path> = event
                .paths
                .iter()
                .filter(|p| !ignored(p, config))
                .map(|p| p.as_path())
                .collect();
            if paths.len() >= 2 {
                let _ = tx.send(FsEvent {
                    kind: FsEventKind::Rename {
                        from: paths[0].to_path_buf(),
                    },
                    path: paths[1].to_path_buf(),
                });
            } else if let Some(path) = paths.first() {
                let _ = tx.send(FsEvent {
                    kind: FsEventKind::Create,
                    path: (*path).to_path_buf(),
                });
            }
            continue;
        }
        for path in &event.paths {
            if ignored(path, config) {
                continue;
            }
            match &event.kind {
                EventKind::Create(_) => {
                    let _ = tx.send(FsEvent {
                        kind: FsEventKind::Create,
                        path: path.clone(),
                    });
                }
                EventKind::Remove(_) => {
                    let _ = tx.send(FsEvent {
                        kind: FsEventKind::Remove,
                        path: path.clone(),
                    });
                }
                EventKind::Modify(notify::event::ModifyKind::Name(
                    notify::event::RenameMode::From,
                )) => {
                    // Remember the source; the matching To event follows.
                    pending_rename_from.push_back(FsEvent {
                        kind: FsEventKind::Rename { from: path.clone() },
                        path: path.clone(),
                    });
                }
                EventKind::Modify(notify::event::ModifyKind::Name(
                    notify::event::RenameMode::To,
                )) => {
                    // Pair with the oldest pending From event. The To path is
                    // necessarily different from the From path, which is why
                    // this used to fail to match a keyed map lookup.
                    if let Some(FsEvent {
                        kind: FsEventKind::Rename { from },
                        ..
                    }) = pending_rename_from.pop_front()
                    {
                        let _ = tx.send(FsEvent {
                            kind: FsEventKind::Rename { from },
                            path: path.clone(),
                        });
                    } else {
                        let _ = tx.send(FsEvent {
                            kind: FsEventKind::Create,
                            path: path.clone(),
                        });
                    }
                }
                EventKind::Modify(_) => {
                    // Debounce: drop repeated Modify events within the window.
                    let now = Instant::now();
                    if last_modify
                        .get(path)
                        .is_some_and(|t| now.duration_since(*t) < config.debounce)
                    {
                        continue;
                    }
                    last_modify.insert(path.clone(), now);
                    let _ = tx.send(FsEvent {
                        kind: FsEventKind::Modify,
                        path: path.clone(),
                    });
                }
                _ => {}
            }
        }
    }
}

fn ignored(path: &Path, config: &WatchConfig) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return true;
    };
    if config.ignore_hidden && name.starts_with('.') {
        return true;
    }
    config
        .ignore_suffixes
        .iter()
        .any(|suffix| name.ends_with(suffix))
}

use notify::Event;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn watch_dir(dir: &Path) -> FsWatcher {
        FsWatcher::watch(dir, WatchConfig::default()).unwrap()
    }

    #[test]
    fn detects_create() {
        let dir = tempfile::tempdir().unwrap();
        let watcher = watch_dir(dir.path());
        fs::write(dir.path().join("new.txt"), b"hi").unwrap();
        let event = watcher
            .recv_timeout(Duration::from_secs(5))
            .expect("create event");
        assert_eq!(event.kind, FsEventKind::Create);
        assert!(event.path.ends_with("new.txt"));
    }

    #[test]
    fn detects_remove() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("gone.txt");
        fs::write(&file, b"x").unwrap();
        let watcher = watch_dir(dir.path());
        // Consume the create event first.
        let _ = watcher.recv_timeout(Duration::from_secs(5));
        fs::remove_file(&file).unwrap();
        let event = watcher
            .recv_timeout(Duration::from_secs(5))
            .expect("remove event");
        assert_eq!(event.kind, FsEventKind::Remove);
        assert!(event.path.ends_with("gone.txt"));
    }

    #[test]
    fn detects_rename() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a.txt");
        let to = dir.path().join("b.txt");
        fs::write(&from, b"x").unwrap();
        let watcher = watch_dir(dir.path());
        let _ = watcher.recv_timeout(Duration::from_secs(5)); // create
        fs::rename(&from, &to).unwrap();
        // On some platforms a rename arrives as Remove+Create; accept either.
        let event = watcher
            .recv_timeout(Duration::from_secs(5))
            .expect("rename event");
        match event.kind {
            FsEventKind::Rename { from: f } => {
                assert!(f.ends_with("a.txt"));
                assert!(event.path.ends_with("b.txt"));
            }
            FsEventKind::Create | FsEventKind::Remove => {}
            FsEventKind::Modify => panic!("unexpected modify"),
        }
    }

    #[test]
    fn ignores_temp_suffixes() {
        let dir = tempfile::tempdir().unwrap();
        let watcher = watch_dir(dir.path());
        fs::write(dir.path().join("editor.tmp"), b"x").unwrap();
        fs::write(dir.path().join("normal.txt"), b"x").unwrap();
        let event = watcher
            .recv_timeout(Duration::from_secs(5))
            .expect("normal create");
        assert!(event.path.ends_with("normal.txt"), "got {:?}", event);
    }
}
