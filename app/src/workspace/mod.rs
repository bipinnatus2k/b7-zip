//! Main workspace view: a 7zFM-style archive file manager built on guise.
//!
//! Toolbar + breadcrumb address bar + file table + status bar, with modal
//! dialogs for add/extract/test/delete/rename/password/info and a progress
//! dialog driven by real engine callbacks.

mod render;
mod task;

use self::task::{PendingOp, RunningTask, TaskKind, task_meta};
use bit7z_explorer::{ArchiveExplorer, EntryRow, ExplorerCommand, ExplorerEvent};
use bit7z_rs::{ArchiveEngine, ArchiveError};
use gpui::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled,
    WeakEntity, Window, div, px,
};
use guise::prelude::*;
use password::Password;
use session::{ArchiveSession, SessionError, SessionStore};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use ::task::{JobSpec, OverwriteSpec};

/// The main application window view.
pub struct Workspace {
    engine: Arc<dyn ArchiveEngine>,
    sessions: SessionStore,
    session_handle: Option<Arc<Mutex<ArchiveSession>>>,
    explorer: Option<Entity<ArchiveExplorer>>,
    current_archive: Option<PathBuf>,
    password: Option<Password>,
    toasts: Entity<ToastStack>,
    task: Option<RunningTask>,
    pending_op: Option<PendingOp>,
    // Selection summary, fed by explorer events.
    selected_count: usize,
    selected_size: u64,
    entry_count: usize,
    status: SharedString,
    // Dialog state
    extract_open: bool,
    extract_target: Entity<TextInput>,
    extract_overwrite: Entity<Select>,
    add_open: bool,
    add_inputs: Vec<PathBuf>,
    add_name: Entity<TextInput>,
    add_format: Entity<Select>,
    add_level: Entity<Select>,
    add_solid: bool,
    add_volume: Entity<NumberInput>,
    add_threads: Entity<NumberInput>,
    add_password: Entity<PasswordInput>,
    add_encrypt_headers: bool,
    password_open: bool,
    password_field: Entity<PasswordInput>,
    rename_open: bool,
    rename_field: Entity<TextInput>,
    confirm_delete: bool,
    info_open: bool,
    /// Weak self-reference for background tasks started from App contexts.
    self_entity: WeakEntity<Self>,
}

impl Workspace {
    /// Create the workspace with the given engine.
    pub fn new(engine: Arc<dyn ArchiveEngine>, cx: &mut Context<Self>) -> Self {
        Self {
            engine,
            sessions: SessionStore::new(),
            session_handle: None,
            explorer: None,
            current_archive: None,
            password: None,
            toasts: cx.new(|_| ToastStack::new()),
            task: None,
            pending_op: None,
            selected_count: 0,
            selected_size: 0,
            entry_count: 0,
            status: "Ready".into(),
            extract_open: false,
            extract_target: cx.new(|cx| TextInput::new(cx).label("Extract to")),
            extract_overwrite: cx.new(|cx| {
                Select::new(cx)
                    .label("If files exist")
                    .data(["Overwrite existing files", "Skip existing files"])
                    .selected(0)
            }),
            add_open: false,
            add_inputs: Vec::new(),
            add_name: cx.new(|cx| TextInput::new(cx).label("Archive")),
            add_format: cx.new(|cx| {
                Select::new(cx)
                    .label("Format")
                    .data(["7z", "zip", "tar", "gzip", "bzip2", "xz", "wim"])
                    .selected(0)
            }),
            add_level: cx.new(|cx| {
                Select::new(cx)
                    .label("Compression level")
                    .data(["Store", "Fastest", "Fast", "Normal", "Maximum", "Ultra"])
                    .selected(3)
            }),
            add_solid: true,
            add_volume: cx.new(|cx| NumberInput::new(cx).label("Volume size (MiB, 0 = single)").value(0.0)),
            add_threads: cx.new(|cx| NumberInput::new(cx).label("Threads (0 = auto)").value(0.0)),
            add_password: cx.new(|cx| PasswordInput::new(cx).label("Password")),
            add_encrypt_headers: false,
            password_open: false,
            password_field: cx.new(|cx| {
                PasswordInput::new(cx)
                    .label("Password")
                    .description("This archive is encrypted.")
            }),
            rename_open: false,
            rename_field: cx.new(|cx| TextInput::new(cx).label("New name")),
            confirm_delete: false,
            info_open: false,
            self_entity: cx.weak_entity(),
        }
    }

    // ====================================================================
    // Opening archives
    // ====================================================================

    /// Open an archive and show it in the explorer.
    pub fn open_archive(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.do_open(path.to_path_buf(), cx);
    }

    fn do_open(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let id = session::next_archive_id();
        match ArchiveSession::open(id, self.engine.clone(), &path, self.password.as_ref()) {
            Ok(session) => {
                let handle = match self.sessions.insert(session) {
                    Ok(handle) => handle,
                    Err(error) => {
                        self.toast_err(format!("error: {error}"), cx);
                        return;
                    }
                };
                if let Some(explorer) = &self.explorer {
                    explorer.update(cx, |explorer, cx| explorer.set_session(handle.clone(), cx));
                } else {
                    let explorer = ArchiveExplorer::new(handle.clone(), cx);
                    cx.subscribe(
                        &explorer,
                        |this, explorer, event: &ExplorerEvent, cx| {
                            this.on_explorer_event(&explorer, event, cx)
                        },
                    )
                    .detach();
                    self.explorer = Some(explorer);
                }
                self.session_handle = Some(handle);
                self.current_archive = Some(path.clone());
                self.status = format!("Opened {}", path.display()).into();
                self.entry_count = self.count_entries();
                cx.refresh_windows();
            }
            Err(SessionError::Archive(ArchiveError::WrongPassword)) => {
                // Ask for the password and retry the same open.
                self.pending_op = Some(PendingOp::Open { path });
                self.open_password_dialog(cx);
            }
            Err(error) => {
                self.toast_err(format!("Failed to open {}: {error}", path.display()), cx);
            }
        }
    }

    /// Pick an archive with the native file dialog and open it.
    fn pick_archive(&mut self, cx: &mut App) {
        let this = self.self_entity.clone();
        cx.spawn(async move |cx| {
            let picked = cx.background_executor().spawn(async move {
                rfd::FileDialog::new()
                    .add_filter("Archives", &["7z", "zip", "tar", "gz", "tgz", "bz2", "xz", "wim", "rar", "cab", "iso", "lzma", "zst"])
                    .add_filter("All files", &["*"])
                    .pick_file()
            }).await;
            if let Some(path) = picked {
                this.update(cx, |ws, cx| ws.do_open(path, cx)).ok();
            }
        })
        .detach();
    }

    // ====================================================================
    // Explorer events
    // ====================================================================

    fn on_explorer_event(
        &mut self,
        explorer: &Entity<ArchiveExplorer>,
        event: &ExplorerEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            ExplorerEvent::Activated(row) => {
                let entry = explorer.read(cx).rows().get(*row).cloned();
                if let Some(entry) = entry {
                    self.open_entry(entry, cx);
                }
            }
            ExplorerEvent::SelectionChanged(rows) => {
                self.selected_count = rows.len();
                let entries = explorer.read(cx).rows();
                self.selected_size = rows
                    .iter()
                    .filter_map(|ix| entries.get(*ix))
                    .map(|entry| entry.size)
                    .sum();
                cx.notify();
            }
            ExplorerEvent::Command(command) => match command {
                ExplorerCommand::NavigateUp => {
                    explorer.update(cx, |explorer, cx| explorer.navigate_up(cx));
                    cx.notify();
                }
                ExplorerCommand::Delete => self.request_delete(cx),
                ExplorerCommand::Rename => self.request_rename(cx),
            },
        }
    }

    /// Open a file entry: extract it to the temp dir and hand it to the OS.
    fn open_entry(&mut self, entry: EntryRow, cx: &mut Context<Self>) {
        let Some(archive) = self.current_archive.clone() else { return };
        let Some(index) = entry.index else { return };
        let engine = self.engine.clone();
        let password = self.password.clone();
        let name = entry.name.clone();
        let archive_inner = archive.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    engine
                        .extract_to_buffer(&archive_inner, index, password.as_ref())
                        .map(|bytes| (name.clone(), bytes))
                })
                .await;
            match result {
                Ok((name, bytes)) => {
                    let out = temp::resolve_in_temp(Path::new(&name));
                    if std::fs::write(&out, bytes).is_ok() {
                        open_with_os(&out);
                    }
                }
                Err(ArchiveError::WrongPassword) => {
                    this.update(cx, |ws, cx| {
                        ws.pending_op = Some(PendingOp::Job(JobSpec::Extract {
                            archive: archive.clone(),
                            items: vec![index],
                            target: temp::temp_root().join("open"),
                            overwrite: OverwriteSpec::Overwrite,
                            password_hint: true,
                        }));
                        ws.open_password_dialog(cx);
                    })
                    .ok();
                }
                Err(error) => {
                    this.update(cx, |ws, cx| ws.toast_err(error.to_string(), cx))
                        .ok();
                }
            }
        })
        .detach();
    }

    // ====================================================================
    // Toolbar actions
    // ====================================================================

    /// Open the extract dialog for the current selection (or whole archive).
    fn open_extract_dialog(&mut self, cx: &mut App) {
        let default = default_extract_dir(self.current_archive.as_deref());
        self.extract_target.update(cx, |input, cx| {
            input.set_text(&default, cx);
        });
        self.extract_open = true;
        cx.refresh_windows();
    }

    fn submit_extract(&mut self, cx: &mut App) {
        let Some(archive) = self.current_archive.clone() else { return };
        let target = PathBuf::from(self.extract_target.read(cx).text());
        let overwrite = match self.extract_overwrite.read(cx).selected_index() {
            Some(1) => OverwriteSpec::Skip,
            _ => OverwriteSpec::Overwrite,
        };
        let items = match &self.explorer {
            Some(explorer) => explorer.read(cx).target_indices(cx),
            None => Vec::new(),
        };
        if items.is_empty() {
            self.toast_err("Nothing to extract".into(), cx);
        } else {
            self.extract_open = false;
            self.start_task(
                TaskKind::Extract,
                format!("Extracting {}", archive.display()),
                JobSpec::Extract {
                    archive,
                    items,
                    target,
                    overwrite,
                    password_hint: false,
                },
                cx,
            );
        }
    }

    /// Pick files, then open the add-to-archive dialog.
    fn pick_add_inputs(&mut self, cx: &mut App) {
        let this = self.self_entity.clone();
        cx.spawn(async move |cx| {
            let picked = cx.background_executor().spawn(async move {
                rfd::FileDialog::new().add_filter("All files", &["*"]).pick_files()
            }).await;
            if let Some(files) = picked.filter(|files| !files.is_empty()) {
                this.update(cx, |ws, cx| ws.open_add_dialog(files, cx)).ok();
            }
        })
        .detach();
    }

    fn open_add_dialog(&mut self, files: Vec<PathBuf>, cx: &mut App) {
        self.add_inputs = files;
        if self.current_archive.is_none() {
            let default = self
                .add_inputs
                .first()
                .map(default_archive_path)
                .unwrap_or_else(|| PathBuf::from("archive.7z"));
            self.add_name.update(cx, |input, cx| {
                input.set_text(&default.to_string_lossy(), cx);
            });
        }
        self.add_open = true;
        cx.refresh_windows();
    }

    fn submit_add(&mut self, cx: &mut App) {
        let inputs = self.add_inputs.clone();
        if inputs.is_empty() {
            self.add_open = false;
            return;
        }
        let password = {
            let text = self.add_password.read(cx).text();
            (!text.is_empty()).then_some(text)
        };
        let job = if let Some(archive) = self.current_archive.clone() {
            // Add into the currently open archive.
            let items = inputs
                .iter()
                .map(|fs_path| ::task::AddItem {
                    fs_path: fs_path.clone(),
                    archive_path: fs_path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| fs_path.to_string_lossy().into_owned()),
                })
                .collect();
            self.add_open = false;
            JobSpec::Add { archive, items, password_hint: password.is_some() }
        } else {
            let target = PathBuf::from(self.add_name.read(cx).text());
            let format = FORMATS[self.add_format.read(cx).selected_index().unwrap_or(0)];
            let level = LEVELS[self.add_level.read(cx).selected_index().unwrap_or(3)];
            let volume_mb = self.add_volume.read(cx).value_f64().unwrap_or(0.0);
            let volume = (volume_mb > 0.0).then_some((volume_mb * 1024.0 * 1024.0) as u64);
            let threads = self.add_threads.read(cx).value_f64().map(|v| v as u32);
            self.add_open = false;
            JobSpec::Compress {
                inputs,
                target,
                format,
                level,
                solid: Some(self.add_solid),
                volume,
                threads,
                encrypt_headers: self.add_encrypt_headers,
                password_hint: password.is_some(),
            }
        };
        self.password = password.map(Password::new);
        let label = match &job {
            JobSpec::Compress { target, .. } => format!("Creating {}", target.display()),
            JobSpec::Add { archive, .. } => format!("Updating {}", archive.display()),
            _ => "Running".to_string(),
        };
        let kind = match &job {
            JobSpec::Compress { .. } => TaskKind::Compress,
            _ => TaskKind::Update,
        };
        self.start_task(kind, label, job, cx);
    }

    fn start_test(&mut self, cx: &mut App) {
        let Some(archive) = self.current_archive.clone() else { return };
        self.start_task(
            TaskKind::Test,
            format!("Testing {}", archive.display()),
            JobSpec::Test { archive, password_hint: false },
            cx,
        );
    }

    fn request_delete(&mut self, cx: &mut App) {
        if self.selected_count == 0 {
            return;
        }
        self.confirm_delete = true;
        cx.refresh_windows();
    }

    fn submit_delete(&mut self, cx: &mut App) {
        self.confirm_delete = false;
        let Some(archive) = self.current_archive.clone() else { return };
        let indices = match &self.explorer {
            Some(explorer) => explorer.read(cx).target_indices(cx),
            None => Vec::new(),
        };
        if indices.is_empty() {
            return;
        }
        self.start_task(
            TaskKind::Update,
            format!("Deleting from {}", archive.display()),
            JobSpec::Delete { archive, indices, password_hint: false },
            cx,
        );
    }

    fn request_rename(&mut self, cx: &mut App) {
        let Some(explorer) = &self.explorer else { return };
        let Some(row) = explorer.read(cx).focused_row(cx) else { return };
        self.rename_field.update(cx, |input, cx| {
            input.set_text(&row.name, cx);
        });
        self.rename_open = true;
        cx.refresh_windows();
    }

    fn submit_rename(&mut self, cx: &mut App) {
        let Some(archive) = self.current_archive.clone() else { return };
        let Some(explorer) = &self.explorer else { return };
        let Some(row) = explorer.read(cx).focused_row(cx) else { return };
        let Some(index) = row.index else {
            self.toast_err("Cannot rename this entry".into(), cx);
            return;
        };
        let new_name = self.rename_field.read(cx).text();
        if new_name.is_empty() || new_name == row.name {
            self.rename_open = false;
            return;
        }
        let parent = row.path.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
        let new_path = if parent.is_empty() {
            new_name
        } else {
            format!("{parent}/{new_name}")
        };
        self.rename_open = false;
        self.start_task(
            TaskKind::Update,
            format!("Renaming in {}", archive.display()),
            JobSpec::Rename { archive, index, new_path, password_hint: false },
            cx,
        );
    }

    // ====================================================================
    // Password dialog
    // ====================================================================

    fn open_password_dialog(&mut self, cx: &mut App) {
        self.password_field.update(cx, |input, cx| input.set_text("", cx));
        self.password_open = true;
        cx.refresh_windows();
    }

    fn submit_password(&mut self, cx: &mut App) {
        let password = self.password_field.read(cx).text();
        if password.is_empty() {
            return;
        }
        let op = self.pending_op.take();
        let this = self.self_entity.clone();
        let password = Password::new(password);
        this.update(cx, |ws, cx| {
            ws.password = Some(password);
            ws.password_open = false;
            match op {
                Some(PendingOp::Open { path }) => ws.do_open(path, cx),
                Some(PendingOp::Job(job)) => {
                    let (kind, label, _) = task_meta(&job);
                    ws.start_task(kind, label, job, cx);
                }
                None => {}
            }
            cx.notify();
        })
        .ok();
    }

    fn cancel_password(&mut self, cx: &mut App) {
        self.password_open = false;
        self.pending_op = None;
        cx.refresh_windows();
    }

    /// Re-list the archive after an in-place update (delete/rename/add).
    fn reload_session(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = &self.session_handle {
            let reloaded = {
                let mut session = handle.lock().expect("session lock poisoned");
                session.reload()
            };
            match reloaded {
                Ok(()) => {
                    if let Some(explorer) = &self.explorer {
                        explorer.update(cx, |explorer, cx| {
                            explorer.refresh(cx);
                        });
                    }
                    self.entry_count = self.count_entries();
                }
                Err(error) => {
                    self.toast_err(format!("Failed to reload archive: {error}"), cx);
                }
            }
        }
        cx.notify();
    }

    // ====================================================================
    // Helpers
    // ====================================================================

    fn count_entries(&self) -> usize {
        self.session_handle
            .as_ref()
            .map(|handle| {
                let session = handle.lock().expect("session lock poisoned");
                session
                    .overlay()
                    .base()
                    .nodes()
                    .values()
                    .filter(|node| node.archive_index().is_some())
                    .count()
            })
            .unwrap_or(0)
    }

    fn archive_stats(&self) -> Option<(usize, u64, u64)> {
        let handle = self.session_handle.as_ref()?;
        let session = handle.lock().expect("session lock poisoned");
        let mut count = 0usize;
        let mut total = 0u64;
        let mut packed = 0u64;
        for node in session.overlay().base().nodes().values() {
            if node.archive_index().is_some() {
                count += 1;
                total += node.attr(vfs::attr::SIZE).and_then(|v| v.as_u64()).unwrap_or(0);
                packed += node
                    .attr(vfs::attr::PACKED_SIZE)
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
            }
        }
        Some((count, total, packed))
    }

    fn toast_err(&mut self, message: String, cx: &mut App) {
        self.toasts.update(cx, |toasts, cx| {
            toasts.push_titled("Error", message, ColorName::Red, cx);
        });
    }

    fn toast_titled(
        &mut self,
        title: impl Into<SharedString>,
        message: impl Into<SharedString>,
        color: ColorName,
        cx: &mut App,
    ) {
        self.toasts.update(cx, |toasts, cx| {
            toasts.push_titled(title, message, color, cx);
        });
    }

    fn close_dialogs(&mut self, cx: &mut App) {
        self.extract_open = false;
        self.add_open = false;
        self.password_open = false;
        self.rename_open = false;
        self.confirm_delete = false;
        self.info_open = false;
        cx.refresh_windows();
    }

    fn pick_extract_folder(&mut self, cx: &mut App) {
        let this = self.self_entity.clone();
        cx.spawn(async move |cx| {
            let picked = cx.background_executor().spawn(async move {
                rfd::FileDialog::new().pick_folder()
            }).await;
            if let Some(path) = picked {
                this.update(cx, |ws, cx| {
                    ws.extract_target.update(cx, |input, cx| {
                        input.set_text(&path.to_string_lossy(), cx);
                    });
                    cx.refresh_windows();
                })
                .ok();
            }
        })
        .detach();
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.global::<Theme>();
        let body = t.body().hsla();
        let text = t.text().hsla();

        let toolbar = self.toolbar(cx);
        let address = self.address_bar(cx);
        let content = if let Some(explorer) = &self.explorer {
            div().flex_1().min_h(px(0.0)).child(explorer.clone()).into_any_element()
        } else {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(12.0))
                .child(Icon::new(IconName::Archive).size(Size::Xl).color(ColorName::Gray))
                .child(Text::new("Open an archive to start").size(Size::Md).dimmed())
                .child(Button::new("empty-open", "Open archive…").on_click(cx.listener(|this, _, _, cx| this.pick_archive(cx))))
                .into_any_element()
        };
        let status = self.status_bar(cx);

        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(body)
            .text_color(text)
            .child(toolbar)
            .child(address)
            .child(content)
            .child(status);

        if self.extract_open {
            root = root.child(self.extract_modal(cx));
        }
        if self.add_open {
            root = root.child(self.add_modal(cx));
        }
        if self.password_open {
            root = root.child(self.password_modal(cx));
        }
        if self.rename_open {
            root = root.child(self.rename_modal(cx));
        }
        if self.confirm_delete {
            root = root.child(self.delete_confirm(cx));
        }
        if self.info_open {
            root = root.child(self.info_modal(cx));
        }
        if self.task.is_some() {
            root = root.child(self.task_modal(cx));
        }
        root.child(self.toasts.clone())
    }
}

/// `FORMATS[i]` mirrors the format selector in the add dialog.
const FORMATS: [::task::FormatSpec; 7] = [
    ::task::FormatSpec::SevenZip,
    ::task::FormatSpec::Zip,
    ::task::FormatSpec::Tar,
    ::task::FormatSpec::GZip,
    ::task::FormatSpec::BZip2,
    ::task::FormatSpec::Xz,
    ::task::FormatSpec::Wim,
];

/// `LEVELS[i]` mirrors the level selector in the add dialog.
const LEVELS: [::task::LevelSpec; 6] = [
    ::task::LevelSpec::None,
    ::task::LevelSpec::Fastest,
    ::task::LevelSpec::Fast,
    ::task::LevelSpec::Normal,
    ::task::LevelSpec::Max,
    ::task::LevelSpec::Ultra,
];

/// The default extraction directory: the archive's folder plus its stem.
fn default_extract_dir(archive: Option<&Path>) -> String {
    let Some(archive) = archive else { return ".".to_string() };
    let parent = archive.parent().unwrap_or(Path::new("."));
    let stem = archive
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "extracted".to_string());
    parent.join(&stem).to_string_lossy().into_owned()
}

/// The default new-archive path: the first input's folder + stem + `.7z`.
fn default_archive_path(input: &PathBuf) -> PathBuf {
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "archive".to_string());
    let parent = input.parent().unwrap_or(Path::new("."));
    parent.join(format!("{stem}.7z"))
}

/// Open a path with the OS default association.
fn open_with_os(path: &Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", &path.to_string_lossy()])
            .spawn();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}
