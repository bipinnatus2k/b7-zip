//! One workspace tab: an archive (or an empty home tab) bound to a single
//! [`ArchiveSession`]. Edits land in the session's working overlay; only an
//! explicit commit writes them back into the archive.

use crate::diff_panel::{DiffContentProvider, DiffPanel, Side};
use crate::globals;
use compare::{DiffReport, tree_vs_tree};
use explorer::explorer::{ArchiveExplorer, ExplorerCommand, ExplorerEvent};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action as _, AnyWindowHandle, App, AppContext, Context, Div, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement, IntoElement, ParentElement, PromptLevel, Render, SharedString,
    Stateful, StatefulInteractiveElement, Styled, Task, UniformListScrollHandle, WeakEntity,
    Window, div, hsla, px, uniform_list,
};
use gpui_kit::base::dock::PanelEvent;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dock::{BasePanel, Panel};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{
    ActiveTheme, Disableable as _, Icon, IconName, Selectable as _, Sizable, WindowExt,
};
use session::{ArchiveSession, ChangeEntry, next_archive_id};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use task::{JobSpec, OverwriteSpec, TaskRunner};
use vfs::DirtyState;

/// How often the session's watcher queue is drained into the overlay.
const WATCH_POLL_INTERVAL: Duration = Duration::from_millis(700);

/// Height of the staging (changes) strip under the file table.
const CHANGES_STRIP_HEIGHT: f32 = 190.0;

pub struct ArchiveWorkspace {
    focus_handle: FocusHandle,
    explorer: Entity<ArchiveExplorer>,
    session: Option<Arc<Mutex<ArchiveSession>>>,
    #[allow(dead_code)]
    archive_path: Option<PathBuf>,
    title: SharedString,
    dirty: bool,
    status: Option<SharedString>,
    /// A failed open: the tab stays closable and shows a retry button.
    failed: bool,
    /// A content-encrypted archive opened without a password: extraction
    /// needs one before it can proceed.
    needs_password: bool,
    /// Inline password input (rendered under the toolbar while
    /// `needs_password`). Panel-internal UI instead of a dialog: dialogs
    /// brought focus and close handling problems this panel cannot have.
    password_entry: Option<Entity<InputState>>,
    /// What to (re)open when the user supplies a password.
    pending_open: Option<PendingOpen>,
    /// Whether the staging strip is pinned open even when the tree is clean.
    changes_open: bool,
    changes_scroll: UniformListScrollHandle,
    /// Last staging snapshot. The session mutex is held for the whole
    /// duration of a commit (background thread), so the per-frame read path
    /// must not block on it; while the lock is busy the strip renders this
    /// cache and refreshes on the next poll tick after the commit lands.
    changes_cache: Vec<ChangeEntry>,
    /// The hosting window, for prompts and dialogs raised from events that
    /// carry no window. Filled by the host right after creation.
    window: Option<AnyWindowHandle>,
    _subscriptions: Vec<gpui::Subscription>,
    _watch: Option<Task<()>>,
}

/// An archive waiting to be (re)opened once a password is available.
struct PendingOpen {
    path: PathBuf,
    password: Option<password::Password>,
}

impl EventEmitter<PanelEvent> for ArchiveWorkspace {}

impl Focusable for ArchiveWorkspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for ArchiveWorkspace {
    fn panel_name(&self) -> &'static str {
        "explorer"
    }

    /// The home tab is the dock's anchor; archive tabs close, including tabs
    /// whose open failed (so the user can dismiss them).
    fn closable(&self, _: &App) -> bool {
        self.session.is_some() || self.failed
    }

    fn on_removed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.cleanup(cx);
    }
}

impl Panel for ArchiveWorkspace {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut label = if self.dirty {
            format!("{} •", self.title)
        } else {
            self.title.to_string()
        };
        if self.needs_password {
            label.push_str(" 🔒");
        }
        div()
            .flex()
            .items_center()
            .gap_1p5()
            .child(Icon::new(IconName::FolderOpen).xsmall())
            .child(label)
    }
}

impl ArchiveWorkspace {
    /// An empty tab that offers opening an archive.
    pub fn home(cx: &mut Context<Self>) -> Self {
        let (explorer, subscriptions) = Self::build_explorer(cx);
        Self {
            focus_handle: cx.focus_handle(),
            explorer,
            session: None,
            archive_path: None,
            title: "Bit7zFM".into(),
            dirty: false,
            status: None,
            failed: false,
            needs_password: false,
            password_entry: None,
            pending_open: None,
            changes_open: false,
            changes_scroll: UniformListScrollHandle::new(),
            changes_cache: Vec::new(),
            window: None,
            _subscriptions: subscriptions,
            _watch: None,
        }
    }

    /// Fills in the hosting window so prompts and dialogs work from events
    /// that carry no window reference.
    pub fn set_window(&mut self, window: AnyWindowHandle) {
        self.window = Some(window);
    }

    /// Creates the tab for `path` and loads its session in the background.
    /// Header-encrypted archives (encrypted file names) prompt for a password
    /// before opening; wrong passwords offer a retry from the toolbar.
    ///
    /// `interactive` must be false on the startup path: the window is still
    /// being built there and dialogs cannot open yet, so a pending password
    /// is surfaced through the toolbar button instead.
    pub fn open_archive(
        path: PathBuf,
        window: &mut Window,
        cx: &mut App,
        interactive: bool,
    ) -> Entity<Self> {
        let title: SharedString = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string())
            .into();
        let entity = cx.new(|cx| {
            let (explorer, subscriptions) = Self::build_explorer(cx);
            Self {
                focus_handle: cx.focus_handle(),
                explorer,
                session: None,
                archive_path: Some(path.clone()),
                title,
                dirty: false,
                status: Some("Opening…".into()),
                failed: false,
                needs_password: false,
                password_entry: None,
                pending_open: None,
                changes_open: false,
                changes_scroll: UniformListScrollHandle::new(),
                changes_cache: Vec::new(),
                window: Some(window.window_handle()),
                _subscriptions: subscriptions,
                _watch: None,
            }
        });

        let Some(engine) = globals::engine(cx) else {
            window.push_notification("7-Zip engine is not loaded", cx);
            return entity;
        };

        // Header-encrypted archives need the password before even the entry
        // list can be read. The probe only reads the header (fast), so it
        // runs inline in the click context, where dialogs render reliably.
        let header_encrypted = engine.is_header_encrypted(&path).unwrap_or(false);
        if header_encrypted {
            let weak = entity.downgrade();
            let open = PendingOpen {
                path,
                password: None,
            };
            let _ = weak.update(cx, |this, cx| {
                this.pending_open = Some(open);
                this.needs_password = true;
                this.status = Some("Password required — use the Enter password button".into());
                cx.notify();
            });
            return entity;
        }

        Self::spawn_open(entity.downgrade(), engine, path, None, cx);
        entity
    }

    /// Opens (or reopens) the pending archive in the background and binds the
    /// session on success. A wrong password flips the tab into the
    /// password-required state for a retry.
    fn spawn_open(
        weak: WeakEntity<Self>,
        engine: Arc<dyn bit7z_rs::ArchiveEngine>,
        path: PathBuf,
        password: Option<password::Password>,
        cx: &mut App,
    ) {
        cx.spawn(async move |cx| {
            let had_password = password.is_some();
            let opened = cx
                .background_executor()
                .spawn(async move {
                    let session =
                        ArchiveSession::open(next_archive_id(), engine.clone(), &path, password.as_ref());
                    let content_encrypted = engine.is_encrypted(&path).unwrap_or(false);
                    (session, content_encrypted, path)
                })
                .await;
            let (opened, content_encrypted, path) = opened;
            match opened {
                Ok(session) => {
                    let bound = cx.update(|cx| globals::sessions(cx).insert(session).ok());
                    let _ = weak.update(cx, |this, cx| {
                        this.failed = false;
                        this.needs_password = false;
                        this.pending_open = None;
                        if let Some(session) = bound {
                            this.bind_session(session, cx);
                        }
                        // Content-encrypted without a stored password: warn
                        // before the first extraction fails.
                        if content_encrypted && !had_password {
                            this.needs_password = true;
                            this.pending_open = Some(PendingOpen {
                                path,
                                password: None,
                            });
                            this.status = Some(
                                "Content is encrypted — enter a password before extracting"
                                    .into(),
                            );
                        }
                    });
                }
                Err(err) => {
                    let wrong_password = matches!(
                        err,
                        session::SessionError::Archive(bit7z_rs::ArchiveError::WrongPassword)
                    );
                    let _ = weak.update(cx, |this, cx| {
                        // A header-encrypted open keeps its pending state on
                        // failure: a wrong password surfaces as a generic
                        // open error, so offer a retry rather than closing
                        // the door.
                        let awaiting_password =
                            this.pending_open.is_some() && this.session.is_none();
                        if wrong_password || awaiting_password {
                            this.failed = true;
                            this.needs_password = true;
                            this.status = Some(
                                "Wrong password or unreadable archive — use the toolbar button to retry"
                                    .into(),
                            );
                        } else {
                            this.failed = true;
                            this.status = Some(format!("Open failed: {err}").into());
                        }
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    /// Opens the inline password entry under the toolbar. Must be called
    /// from an event handler that carries the live window (button click);
    /// the startup path only flags `needs_password` instead.
    fn prompt_for_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.needs_password = true;
        if self.password_entry.is_none() {
            self.password_entry = Some(cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Password")
                    .masked(true)
            }));
        }
        cx.notify();
    }

    /// Takes the pending open (or password-retry) and re-runs it with the
    /// password the user just typed. Runs in the app context; the retry
    /// itself happens off the UI thread.
    fn apply_supplied_password(&mut self, password: String, cx: &mut Context<Self>) {
        let Some(pending) = &self.pending_open else {
            // Content-encrypted archive already open: just store the password
            // for the next extraction.
            if let Some(session) = &self.session {
                session
                    .lock()
                    .expect("session lock poisoned")
                    .set_password(Some(password::Password::new(password)));
                self.needs_password = false;
                self.status = Some("Password set — retry the operation".into());
                cx.notify();
            }
            return;
        };
        let path = pending.path.clone();
        let password = password::Password::new(password);
        self.pending_open = Some(PendingOpen {
            path: path.clone(),
            password: Some(password.clone()),
        });
        self.status = Some("Opening…".into());
        cx.notify();
        let Some(engine) = globals::engine(cx) else {
            self.status = Some("7-Zip engine not loaded".into());
            cx.notify();
            return;
        };
        let weak = cx.weak_entity();
        Self::spawn_open(weak, engine, path, Some(password), cx);
    }

    /// Toolbar entry point: asks for a password and retries the pending open
    /// (or stores it for an already-open encrypted archive).
    fn enter_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_for_password(window, cx);
    }

    fn build_explorer(
        cx: &mut Context<Self>,
    ) -> (Entity<ArchiveExplorer>, Vec<gpui::Subscription>) {
        let explorer = cx.new(ArchiveExplorer::new);
        let subscription = cx.subscribe(&explorer, |this, _explorer, event: &ExplorerEvent, cx| {
            this.on_explorer_event(event.clone(), cx);
        });
        (explorer, vec![subscription])
    }

    fn bind_session(&mut self, session: Arc<Mutex<ArchiveSession>>, cx: &mut Context<Self>) {
        {
            let archive = session
                .lock()
                .expect("session lock poisoned")
                .archive_path()
                .to_path_buf();
            settings::update(cx, |settings| {
                settings.push_recent_archive(&archive.display().to_string());
            });
        }
        self.title = session
            .lock()
            .expect("session lock poisoned")
            .archive_path()
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "Archive".into())
            .into();
        self.status = None;
        self.session = Some(session.clone());
        self.refresh_dirty(cx);
        self.explorer
            .update(cx, |explorer, cx| explorer.set_session(session, cx));
        self.start_watch_loop(cx);
        cx.notify();
    }

    /// Drains the session's watcher events into the overlay. Returns false
    /// once the workspace is gone, ending the poll loop.
    fn poll_watch(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(session) = &self.session else {
            return false;
        };
        // `try_lock`: a running commit holds this mutex on the background
        // thread for its whole duration. Events pile up in the watcher
        // channel meanwhile and are drained on the next free tick.
        if let Ok(mut session) = session.try_lock() {
            session.poll_events();
        }
        self.refresh_dirty(cx);
        true
    }

    fn start_watch_loop(&mut self, cx: &mut Context<Self>) {
        self._watch = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(WATCH_POLL_INTERVAL).await;
                if this.update(cx, |this, cx| this.poll_watch(cx)).is_err() {
                    break;
                }
            }
        }));
    }

    fn refresh_dirty(&mut self, cx: &mut Context<Self>) {
        let dirty = match self.session.as_ref() {
            None => false,
            Some(session) => {
                // `try_lock` so a long-running commit (which holds this
                // mutex on the background thread) never blocks the UI
                // thread; keep the last known value meanwhile — `after_write`
                // refreshes again once the commit lands.
                match session.try_lock() {
                    Ok(session) => session.has_changes(),
                    Err(_) => return,
                }
            }
        };
        if dirty != self.dirty {
            self.dirty = dirty;
            cx.notify();
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn session(&self) -> Option<&Arc<Mutex<ArchiveSession>>> {
        self.session.as_ref()
    }

    /// (title, session) for cross-workspace pickers; `None` for home tabs.
    pub fn title_and_session(&self) -> Option<(String, Arc<Mutex<ArchiveSession>>)> {
        Some((self.title.to_string(), self.session.clone()?))
    }

    /// Commits the staged subset in the background. When nothing is staged
    /// but the tree is dirty, everything is staged first — the plain "hit
    /// commit" path behaves like a full commit.
    pub fn commit(&mut self, cx: &mut Context<Self>) -> Task<Result<(), String>> {
        let Some(session) = self.session.clone() else {
            return Task::ready(Ok(()));
        };
        {
            let mut session = session.lock().expect("session lock poisoned");
            if session.staged_changeset().is_empty() {
                session.stage_all();
            }
        }
        self.status = Some("Committing…".into());
        cx.notify();
        cx.background_spawn(async move {
            let mut session = session.lock().expect("session lock poisoned");
            session
                .commit_staged()
                .map(|_| ())
                .map_err(|err| err.to_string())
        })
    }

    /// Commits and refreshes afterwards; used by the toolbar action.
    pub fn commit_and_notify(&mut self, cx: &mut Context<Self>) {
        let task = self.commit(cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| this.after_write(result, cx));
        })
        .detach();
    }

    pub fn discard(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &self.session {
            session.lock().expect("session lock poisoned").discard();
        }
        self.after_write(Ok(()), cx);
    }

    // ----------------------------------------------------------------------
    // Staging strip actions.
    // ----------------------------------------------------------------------

    fn snapshot_changes(&mut self) -> Vec<ChangeEntry> {
        match self.session.as_ref().map(|session| session.try_lock()) {
            None => self.changes_cache = Vec::new(),
            Some(Ok(session)) => self.changes_cache = session.changes(),
            // Commit in progress (the background thread holds the mutex for
            // the whole archive write): render the last snapshot. The strip
            // refreshes on the next poll tick and on `after_write`.
            Some(Err(_)) => {}
        }
        self.changes_cache.clone()
    }

    fn toggle_stage(&mut self, id: vfs::NodeId, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            return;
        };
        {
            let mut session = session.lock().expect("session lock poisoned");
            if session.overlay().is_staged(id) {
                session.unstage([id]);
            } else {
                session.stage([id]);
            }
        }
        self.refresh_dirty(cx);
        cx.notify();
    }

    fn stage_all(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &self.session {
            session.lock().expect("session lock poisoned").stage_all();
        }
        self.refresh_dirty(cx);
        cx.notify();
    }

    fn unstage_all(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &self.session {
            session.lock().expect("session lock poisoned").unstage_all();
        }
        cx.notify();
    }

    fn discard_unstaged_changes(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &self.session {
            session
                .lock()
                .expect("session lock poisoned")
                .discard_unstaged();
        }
        self.refresh_dirty(cx);
        self.explorer
            .update(cx, |explorer, cx| explorer.refresh(cx));
        cx.notify();
    }

    fn after_write(&mut self, result: Result<(), String>, cx: &mut Context<Self>) {
        match result {
            Ok(()) => self.status = Some("Archive updated".into()),
            Err(err) => self.status = Some(format!("Write failed: {err}").into()),
        }
        self.refresh_dirty(cx);
        self.explorer
            .update(cx, |explorer, cx| explorer.refresh(cx));
        cx.notify();
    }

    /// Drops the session (deleting its work directory) so a closed tab does
    /// not leak temp state in the session store.
    pub fn cleanup(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = self.session.take() {
            let id = session.lock().expect("session lock poisoned").id();
            globals::sessions(cx).remove(id);
        }
    }

    /// The staging strip under the file table: every dirty entry with its
    /// status letter; clicking a row toggles whether it takes part in the
    /// next commit.
    fn render_changes_strip(
        &mut self,
        changes: &[ChangeEntry],
        staged_count: usize,
        unstaged_count: usize,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = cx.theme();
        let (muted, border, success, warning, danger, primary, secondary, selected_bg) = {
            let t = cx.theme();
            (
                t.muted_foreground,
                t.border,
                t.success,
                t.warning,
                t.danger,
                t.primary,
                t.secondary,
                hsla(t.primary.h, t.primary.s, t.primary.l, 0.18),
            )
        };
        let entity = cx.entity().clone();
        let weak = entity.downgrade();
        let rows = changes.to_vec();
        let row_count = rows.len();

        let summary = if changes.is_empty() {
            "No changes — the working tree matches the archive".to_string()
        } else {
            format!(
                "{staged_count} staged, {unstaged_count} unstaged — click an entry to toggle staging"
            )
        };

        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .h(px(30.0))
            .flex_shrink_0()
            .border_b_1()
            .border_color(border)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .text_xs()
                    .text_color(muted)
                    .child(summary),
            )
            .child(
                Button::new("stage-all")
                    .label("Stage all")
                    .ghost()
                    .xsmall()
                    .disabled(unstaged_count == 0)
                    .on_click(cx.listener(|this, _, _, cx| this.stage_all(cx))),
            )
            .child(
                Button::new("unstage-all")
                    .label("Unstage all")
                    .ghost()
                    .xsmall()
                    .disabled(staged_count == 0)
                    .on_click(cx.listener(|this, _, _, cx| this.unstage_all(cx))),
            )
            .child(
                Button::new("discard-unstaged")
                    .label("Discard unstaged")
                    .ghost()
                    .xsmall()
                    .disabled(unstaged_count == 0)
                    .on_click(cx.listener(|this, _, _, cx| this.discard_unstaged_changes(cx))),
            )
            .child(
                Button::new("commit-staged")
                    .label(format!("Commit staged ({staged_count})"))
                    .primary()
                    .xsmall()
                    .disabled(staged_count == 0)
                    .on_click(cx.listener(|this, _, _, cx| this.commit_and_notify(cx))),
            );

        div()
            .id("changes-strip")
            .h(px(CHANGES_STRIP_HEIGHT))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(border)
            .child(header)
            .child(
                uniform_list("changes-rows", row_count, move |range, _window, cx| {
                    let snapshot: Vec<(usize, ChangeEntry)> = range
                        .clone()
                        .filter_map(|ix| rows.get(ix).map(|entry| (ix, entry.clone())))
                        .collect();
                    snapshot
                        .into_iter()
                        .map(|(ix, entry)| {
                            let weak = weak.clone();
                            let (letter, color) = match entry.state {
                                DirtyState::Added => ("A", success),
                                DirtyState::Modified => ("M", warning),
                                DirtyState::Deleted => ("D", danger),
                                DirtyState::Renamed => ("R", primary),
                            };
                            div()
                                .id(("change-row", ix))
                                .h(px(22.0))
                                .w_full()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .px_2()
                                .when(entry.staged, |el| el.bg(selected_bg))
                                .when(!entry.staged, |el| el.hover(|el| el.bg(secondary)))
                                .on_click(move |_, _, cx| {
                                    weak.update(cx, |this, cx| this.toggle_stage(entry.id, cx))
                                        .ok();
                                })
                                .child(div().w(px(12.0)).text_xs().text_color(color).child(letter))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .truncate()
                                        .text_xs()
                                        .child(SharedString::from(entry.path.clone())),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(if entry.staged { success } else { muted })
                                        .child(if entry.staged { "staged" } else { "unstaged" }),
                                )
                        })
                        .collect()
                })
                .track_scroll(&self.changes_scroll)
                .flex_1()
                .min_h_0()
                .w_full(),
            )
    }

    fn on_explorer_event(&mut self, event: ExplorerEvent, cx: &mut Context<Self>) {
        match event {
            ExplorerEvent::Activated(ix) => self.view_file(ix, cx),
            ExplorerEvent::SelectionChanged(_) => cx.notify(),
            ExplorerEvent::Command(ExplorerCommand::Delete) => self.delete_selected(cx),
            ExplorerEvent::Command(ExplorerCommand::Rename) => self.rename_selected(cx),
        }
    }

    /// Extracts the activated file entry into the session work dir and opens
    /// it with the OS association.
    fn view_file(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            return;
        };
        let row = self.explorer.read(cx).rows(cx).get(ix).cloned();
        let Some(row) = row.filter(|row| !row.is_directory) else {
            return;
        };
        let Some(index) = row.index else {
            return;
        };
        let session = session.clone();
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let opened = cx
                .background_executor()
                .spawn(async move {
                    let mut session = session.lock().expect("session lock poisoned");
                    session.extract_to_workdir(index)
                })
                .await;
            match opened {
                Ok(fs_path) => open_with_association(&fs_path),
                Err(session::SessionError::Archive(bit7z_rs::ArchiveError::WrongPassword)) => {
                    let _ = weak.update(cx, |this, cx| {
                        this.needs_password = true;
                        this.status =
                            Some("Wrong password — use the toolbar button to retry".into());
                        cx.notify();
                    });
                }
                Err(err) => {
                    let _ = weak.update(cx, |this, cx| {
                        this.status = Some(format!("Extract failed: {err}").into());
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            return;
        };
        let Some(window) = self.window else {
            self.status = Some("No window attached".into());
            cx.notify();
            return;
        };
        let indices = self.explorer.read(cx).target_indices(cx);
        if indices.is_empty() {
            return;
        }
        let (archive_path, password) = {
            let session = session.lock().expect("session lock poisoned");
            (
                session.archive_path().to_path_buf(),
                session.password().cloned(),
            )
        };
        let weak = cx.weak_entity();
        let count = indices.len();
        cx.spawn(async move |_, cx| {
            let Ok(answer) = window.update(cx, |_, window, cx| {
                window.prompt(
                    PromptLevel::Info,
                    &format!("Delete {count} entries from the archive?"),
                    None,
                    &["Delete", "Cancel"],
                    cx,
                )
            }) else {
                return;
            };
            if answer.await != Ok(0) {
                return;
            }
            // Run the delete through the task pipeline (progress page, cancel,
            // typed outcome) instead of a direct engine.write on this thread.
            let _ = weak.update(cx, |this, cx| {
                let spec = JobSpec::Delete {
                    archive: archive_path,
                    indices,
                    password_hint: password.is_some(),
                };
                this.run_job(spec, password, cx);
            });
        })
        .detach();
    }

    /// Renames the single selected entry via a text-input dialog; the rename
    /// is applied to the archive on confirm.
    fn rename_selected(&mut self, cx: &mut Context<Self>) {
        let Some(window) = self.window else {
            return;
        };
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let _ = window.update(cx, |_, window, cx| {
                let _ = weak.update(cx, |this, cx| this.rename_selected_in(window, cx));
            });
        })
        .detach();
    }

    fn rename_selected_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.is_none() {
            return;
        }
        let Some(row) = self.explorer.read(cx).focused_row(cx) else {
            self.status = Some("Select exactly one entry to rename".into());
            cx.notify();
            return;
        };
        let Some(archive_index) = row.index else {
            self.status = Some("Synthetic folders cannot be renamed".into());
            cx.notify();
            return;
        };
        let old_name = row.path.rsplit('/').next().unwrap_or(&row.path).to_string();
        let parent = row
            .path
            .rsplit_once('/')
            .map(|(parent, _)| parent.to_string());

        let weak = cx.weak_entity();
        let dialog_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("New name")
                .default_value(old_name.clone())
        });
        window.open_dialog(cx, move |dialog, _, _| {
            let weak = weak.clone();
            let input = dialog_input.clone();
            let parent = parent.clone();
            let old_name = old_name.clone();
            dialog
                .title("Rename")
                .w(px(380.0))
                .child(div().p_3().child(Input::new(&input)))
                .on_ok(move |_, _, cx| {
                    let new_name = input.read(cx).value().trim().to_string();
                    if new_name.is_empty() || new_name.contains('/') {
                        return false;
                    }
                    let new_path = match &parent {
                        Some(parent) => format!("{parent}/{new_name}"),
                        None => new_name,
                    };
                    if new_path == old_name {
                        return true;
                    }
                    // Apply the rename through the task pipeline (progress page,
                    // cancel, typed outcome), then the reload runs on success.
                    let weak = weak.clone();
                    cx.spawn(async move |cx| {
                        let _ = weak.update(cx, |this, cx| {
                            let Some(session) = this.session.clone() else {
                                return;
                            };
                            let (archive, password) = {
                                let session =
                                    session.lock().expect("session lock poisoned");
                                (
                                    session.archive_path().to_path_buf(),
                                    session.password().cloned(),
                                )
                            };
                            let spec = JobSpec::Rename {
                                archive,
                                index: archive_index,
                                new_path,
                                password_hint: password.is_some(),
                            };
                            this.run_job(spec, password, cx);
                        });
                    })
                    .detach();
                    true
                })
        });
    }

    /// Copies picked files into the session work directory under the current
    /// path. They enter the working overlay as unstaged additions, like any
    /// other edit — nothing touches the archive until a commit.
    fn add_files(&mut self, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            return;
        };
        let current_path = self.explorer.read(cx).current_path().to_string();

        // Async dialog: see extract_selected for why the synchronous one
        // would crash inside the held App borrow.
        let session = session.clone();
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let Some(picked) = rfd::AsyncFileDialog::new()
                .set_title("Add files")
                .pick_files()
                .await
            else {
                return;
            };
            if picked.is_empty() {
                return;
            }
            let files: Vec<std::path::PathBuf> = picked
                .into_iter()
                .map(|handle| handle.path().to_path_buf())
                .collect();
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut session = session.lock().expect("session lock poisoned");
                    let mut added = 0usize;
                    for file in files {
                        let Some(name) = file.file_name() else {
                            continue;
                        };
                        let rel = if current_path.is_empty() {
                            name.to_string_lossy().to_string()
                        } else {
                            format!("{current_path}/{}", name.to_string_lossy())
                        };
                        if session.ingest_file(&file, &rel).is_ok() {
                            added += 1;
                        }
                    }
                    added
                })
                .await;
            let _ = weak.update(cx, |this, cx| {
                this.status = Some(format!("{outcome} file(s) added to the staging area").into());
                this.refresh_dirty(cx);
                this.explorer
                    .update(cx, |explorer, cx| explorer.refresh(cx));
                cx.notify();
            });
        })
        .detach();
    }

    fn extract_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            return;
        };
        let (archive_path, password) = {
            let session = session.lock().expect("session lock poisoned");
            (
                session.archive_path().to_path_buf(),
                session.password().cloned(),
            )
        };
        let indices = self.explorer.read(cx).target_indices(cx);
        if indices.is_empty() {
            self.status = Some("Nothing selected to extract".into());
            cx.notify();
            return;
        }
        // An encrypted archive without a stored password cannot even start:
        // ask for the password first, the user retries once it is stored.
        if password.is_none() {
            let engine = globals::engine(cx);
            let encrypted = engine
                .as_ref()
                .is_some_and(|engine| engine.is_encrypted(&archive_path).unwrap_or(false));
            if encrypted {
                self.status = Some("Password required — enter it to extract".into());
                self.prompt_for_password(window, cx);
                return;
            }
        }

        // The folder dialog must run on the async API: the synchronous one
        // re-enters the message loop while we hold the App borrow, and any
        // timer task ticked inside it crashes on the held RefCell.
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let Some(picked) = rfd::AsyncFileDialog::new()
                .set_title("Extract to…")
                .pick_folder()
                .await
            else {
                return; // dialog dismissed
            };
            let target = picked.path().to_path_buf();
            let _ = weak.update(cx, |this, cx| {
                let spec = JobSpec::Extract {
                    archive: archive_path.clone(),
                    items: indices.clone(),
                    target,
                    overwrite: OverwriteSpec::Ask,
                    password_hint: password.is_some(),
                };
                this.run_job(spec, password.clone(), cx);
            });
        })
        .detach();
    }

    fn test_archive(&mut self, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            return;
        };
        let (archive_path, password) = {
            let session = session.lock().expect("session lock poisoned");
            (
                session.archive_path().to_path_buf(),
                session.password().cloned(),
            )
        };
        let spec = JobSpec::Test {
            archive: archive_path,
            password_hint: password.is_some(),
        };
        self.run_job(spec, password, cx);
    }

    /// Shows entry properties in a dialog: full details for a single
    /// selection, totals for a multi-selection. Runs the dialog off the
    /// listener stack (spawn + window handle), like every other dialog.
    fn show_properties(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(window) = self.window else {
            return;
        };
        let selected = self.explorer.read(cx).selected_rows(cx);
        if selected.is_empty() {
            self.status = Some("Select an entry to inspect".into());
            cx.notify();
            return;
        }
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let _ = window.update(cx, |_, window, cx| {
                let rows: Vec<(SharedString, SharedString)> = if selected.len() == 1 {
                    let row = &selected[0];
                    vec![
                        ("Name".into(), row.name.clone().into()),
                        ("Path".into(), row.path.clone().into()),
                        ("Size".into(), explorer::model::format_size(row.size).into()),
                        (
                            "Packed".into(),
                            explorer::model::format_size(row.packed).into(),
                        ),
                        ("Modified".into(), row.modified.clone().into()),
                        (
                            "Attributes".into(),
                            explorer::model::attr_string(row).into(),
                        ),
                        (
                            "CRC".into(),
                            row.crc
                                .map(|crc| format!("{crc:08X}"))
                                .unwrap_or_else(|| "-".into())
                                .into(),
                        ),
                        ("Method".into(), row.method.clone().into()),
                    ]
                } else {
                    let files = selected.iter().filter(|row| !row.is_directory).count();
                    let dirs = selected.len() - files;
                    let total: u64 = selected.iter().map(|row| row.size).sum();
                    let packed: u64 = selected.iter().map(|row| row.packed).sum();
                    vec![
                        ("Entries".into(), format!("{}", selected.len()).into()),
                        ("Files".into(), format!("{files}").into()),
                        ("Folders".into(), format!("{dirs}").into()),
                        (
                            "Total size".into(),
                            explorer::model::format_size(total).into(),
                        ),
                        (
                            "Total packed".into(),
                            explorer::model::format_size(packed).into(),
                        ),
                    ]
                };
                window.open_dialog(cx, move |dialog, window, cx| {
                    let muted = cx.theme().muted_foreground;
                    let rows = rows.clone();
                    let items: Vec<gpui::AnyElement> = rows
                        .into_iter()
                        .map(|(label, value)| {
                            div()
                                .flex()
                                .flex_row()
                                .gap_3()
                                .py_0p5()
                                .child(
                                    div()
                                        .w(px(110.0))
                                        .flex_shrink_0()
                                        .text_xs()
                                        .text_color(muted)
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .truncate()
                                        .text_xs()
                                        .child(value),
                                )
                                .into_any_element()
                        })
                        .collect();
                    let _ = window;
                    dialog
                        .title("Properties")
                        .w(px(440.0))
                        .child(div().p_3().flex().flex_col().children(items))
                });
            });
        })
        .detach();
    }

    /// Offers a picker of the other open archive workspaces and compares the
    /// chosen one against this one (both working views), opening the result
    /// in a diff panel.
    fn compare_with(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            return;
        };
        let others = globals::other_archive_sessions(cx, cx.entity().entity_id());
        if others.is_empty() {
            self.status = Some("Open another archive to compare with".into());
            cx.notify();
            return;
        }
        let weak = cx.weak_entity();
        let window_handle = self.window;
        cx.spawn(async move |_, cx| {
            let Some(window_handle) = window_handle else {
                return;
            };
            let _ = window_handle.update(cx, |_, window, cx| {
                let items = others.clone();
                window.open_dialog(cx, move |dialog, _, _| {
                    let items = items.clone();
                    let weak = weak.clone();
                    let rows = items
                        .into_iter()
                        .map(|(title, target_session)| {
                            let weak = weak.clone();
                            div()
                                .id(SharedString::from(format!("cmp-{title}")))
                                .px_2()
                                .py_1p5()
                                .rounded_sm()
                                .text_sm()
                                .cursor_pointer()
                                .hover(|el| el.bg(hsla(0.0, 0.0, 0.5, 0.15)))
                                .child(SharedString::from(title.clone()))
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let _ = weak.update(cx, |this, cx| {
                                        this.spawn_cross_compare(
                                            target_session.clone(),
                                            title.clone(),
                                            cx,
                                        )
                                    });
                                })
                        })
                        .collect::<Vec<_>>();
                    dialog
                        .title("Compare with…")
                        .w(px(360.0))
                        .child(div().p_2().flex().flex_col().gap_0p5().children(rows))
                });
            });
        })
        .detach();
    }

    /// Runs the two-working-trees comparison against `target` off the UI
    /// thread and opens the diff panel. Both sessions are locked in a fixed
    /// order (by session id) so two concurrent comparisons cannot deadlock.
    fn spawn_cross_compare(
        &mut self,
        target: Arc<Mutex<ArchiveSession>>,
        target_title: String,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.session.clone() else {
            return;
        };
        let Some(engine) = globals::engine(cx) else {
            self.status = Some("7-Zip engine not loaded".into());
            cx.notify();
            return;
        };
        let Some(window_handle) = self.window else {
            return;
        };
        let source_title = self.title.to_string();
        self.status = Some(format!("Comparing with {target_title}…").into());
        cx.notify();
        let weak = cx.weak_entity();
        let provider_source = source.clone();
        let provider_target = target.clone();
        cx.spawn(async move |_, cx| {
            let report = cx
                .background_executor()
                .spawn(async move {
                    // Lock in ascending session-id order.
                    let source_id = source.lock().expect("session lock poisoned").id();
                    let target_id = target.lock().expect("session lock poisoned").id();
                    let (first, second, source_is_first) = if source_id <= target_id {
                        (source, target, true)
                    } else {
                        (target, source, false)
                    };
                    let first = first.lock().expect("session lock poisoned");
                    let second = second.lock().expect("session lock poisoned");
                    let first_tree = first.overlay().working();
                    let second_tree = second.overlay().working();
                    // DiffPanel's Base side is the source (this) workspace,
                    // Working side the target, regardless of lock order.
                    let report = if source_is_first {
                        tree_vs_tree(first_tree, second_tree)
                    } else {
                        tree_vs_tree(second_tree, first_tree)
                    };
                    (report, source_title, target_title)
                })
                .await;
            let (report, source_title, target_title) = report;
            let _ = weak.update(cx, |this, cx| {
                this.status = None;
                cx.notify();
            });
            let provider: Arc<dyn DiffContentProvider> = Arc::new(CrossWorkspaceProvider {
                source: provider_source,
                target: provider_target,
                engine,
            });
            let _ = window_handle.update(cx, |_, window, cx| {
                let _ = weak.update(cx, |_, cx| {
                    let panel = cx.new(|cx| {
                        DiffPanel::new(
                            format!("Compare — {source_title} vs {target_title}"),
                            report,
                            provider,
                            cx,
                        )
                    });
                    globals::open_center_panel(cx, panel, window);
                });
            });
        })
        .detach();
    }

    /// Compares the archive's base tree against the working view and opens
    /// the diff panel with the report.
    fn open_diff_panel(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            self.status = Some("Open an archive first".into());
            cx.notify();
            return;
        };
        let Some(engine) = globals::engine(cx) else {
            self.status = Some("7-Zip engine not loaded".into());
            cx.notify();
            return;
        };
        let Some(window_handle) = self.window else {
            return;
        };
        let session = session.clone();
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let report_session = session.clone();
            let report = cx
                .background_executor()
                .spawn(async move {
                    let session = report_session.lock().expect("session lock poisoned");
                    tree_vs_tree(session.overlay().base(), session.overlay().working())
                })
                .await;
            let title: Option<String> = weak
                .read_with(cx, |this, _| Some(this.title.to_string()))
                .ok()
                .flatten();
            let Some(title) = title else { return };
            let provider: Arc<dyn DiffContentProvider> =
                Arc::new(SessionDiffProvider::new(session, engine));
            let _ = weak.update(cx, |_, cx| {
                let panel =
                    cx.new(|cx| DiffPanel::new(format!("Diff — {title}"), report, provider, cx));
                let _ = window_handle.update(cx, |_, window, cx| {
                    globals::open_center_panel(cx, panel, window);
                });
            });
        })
        .detach();
    }

    /// Runs a job on the shared TaskRunner and reports the outcome in the
    /// status line. Long ops move to the WinRAR-style progress page in P4.
    fn run_job(
        &mut self,
        spec: JobSpec,
        password: Option<password::Password>,
        cx: &mut Context<Self>,
    ) {
        let Some(engine) = globals::engine(cx) else {
            self.status = Some("7-Zip engine not loaded".into());
            cx.notify();
            return;
        };
        let Some(window_handle) = self.window else {
            self.status = Some("No window attached".into());
            cx.notify();
            return;
        };
        let runner = TaskRunner::new(engine);
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let pause = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let rx = runner.run_with_controls(
            spec.clone(),
            password.as_ref(),
            cancel.clone(),
            pause.clone(),
        );

        // The WinRAR-style progress page owns the job: status fields, pause
        // and cancel controls, and the finished-card. This workspace listens
        // for the terminal event to refresh its tree.
        let panel_title: SharedString = match &spec {
            JobSpec::Extract { archive, .. } => format!("Extracting {}", archive.display()).into(),
            JobSpec::Compress { target, .. } => {
                format!("Compressing to {}", target.display()).into()
            }
            JobSpec::Test { archive, .. } => format!("Testing {}", archive.display()).into(),
            JobSpec::Add { archive, .. } => format!("Updating {}", archive.display()).into(),
            JobSpec::Delete { archive, .. } => {
                format!("Deleting from {}", archive.display()).into()
            }
            JobSpec::Rename { archive, .. } => format!("Renaming in {}", archive.display()).into(),
            JobSpec::NewFolder { archive, .. } => format!("Updating {}", archive.display()).into(),
            JobSpec::Checksum { path, .. } => format!("Checksum {}", path.display()).into(),
        };
        let weak = cx.weak_entity();
        let password_was_supplied = password.is_some();
        // In-place archive rewrites change the entry tree behind the
        // overlay's back, so the session must reload before the view refresh;
        // read-only jobs (extract/test/checksum) just re-poll the working dir.
        let rewrites_archive = matches!(
            &spec,
            JobSpec::Delete { .. } | JobSpec::Rename { .. } | JobSpec::NewFolder { .. }
        );
        // Open the progress page from a detached task: this method is often
        // called from a click listener, and the nested window update that a
        // synchronous `window_handle.update` performs there is rejected
        // (the panel then never appears — "clicking Test does nothing").
        cx.spawn(async move |_, cx| {
            let _ = window_handle.update(cx, |_, window, cx| {
                let panel = cx.new(|cx| {
                    let mut panel = crate::progress_panel::ProgressPanel::new(
                        panel_title,
                        rx,
                        cancel,
                        pause,
                        cx,
                    );
                    // Registered inside the constructor's update, before the poll
                    // task can ever run, so no completion is missed.
                    panel.set_on_finished({
                        let weak = weak.clone();
                        move |app, success, message, error| {
                            let _ = weak.update(app, |this, cx| {
                                // The engine now classifies a header-encrypted
                                // archive that a supplied password could not
                                // open as WrongPassword at the boundary; an
                                // open failure with no password is OpenFailed.
                                // Either way this is the retryable-password case
                                // the toolbar needs to surface, keyed on the
                                // typed kind instead of the old message
                                // substring match.
                                let password_issue = !success
                                    && match error {
                                        Some(task::TaskErrorKind::WrongPassword) => true,
                                        Some(task::TaskErrorKind::OpenFailed) => {
                                            password_was_supplied
                                        }
                                        _ => false,
                                    };
                                if password_issue {
                                    this.needs_password = true;
                                    this.status = Some(
                                        "Wrong password — use the toolbar button to retry".into(),
                                    );
                                } else {
                                    this.status =
                                        Some(job_result_message(success, message, error));
                                }
                                if success {
                                    if rewrites_archive {
                                        // Re-read the rewritten archive so the
                                        // overlay's base and indices reflect it.
                                        this.reload_from_archive(cx);
                                    } else {
                                        this.refresh_dirty(cx);
                                        this.explorer
                                            .update(cx, |explorer, cx| explorer.refresh(cx));
                                    }
                                }
                                cx.notify();
                            });
                        }
                    });
                    panel
                });
                globals::open_center_panel(cx, panel, window);
            });
        })
        .detach();
    }

    /// Re-reads the archive into the session overlay after an in-place
    /// rewrite (Delete/Rename/NewFolder ran on the archive file directly, so
    /// the overlay's base tree and entry indices are now stale). The reload
    /// runs on the background executor (it re-lists via the engine) and the
    /// view refreshes on the main thread once it lands.
    fn reload_from_archive(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    session
                        .lock()
                        .expect("session lock poisoned")
                        .reload()
                        .map_err(|err| err.to_string())
                })
                .await;
            let _ = weak.update(cx, |this, cx| {
                this.after_write(outcome, cx);
            });
        })
        .detach();
    }
}

impl Render for ArchiveWorkspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let changes = self.snapshot_changes();
        let staged_count = changes.iter().filter(|row| row.staged).count();
        let unstaged_count = changes.len() - staged_count;

        let mut toolbar = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .h(px(34.0))
            .flex_shrink_0()
            .border_b_1()
            .border_color(border);
        if self.session.is_some() {
            toolbar = toolbar
                .child(
                    Button::new("extract")
                        .label("Extract")
                        .ghost()
                        .xsmall()
                        .on_click(
                            cx.listener(|this, _, window, cx| this.extract_selected(window, cx)),
                        ),
                )
                .child(
                    Button::new("add")
                        .label("Add")
                        .ghost()
                        .xsmall()
                        .on_click(cx.listener(|this, _, _, cx| this.add_files(cx))),
                )
                .child(
                    Button::new("rename")
                        .label("Rename")
                        .ghost()
                        .xsmall()
                        .on_click(
                            cx.listener(|this, _, window, cx| this.rename_selected_in(window, cx)),
                        ),
                )
                .child(
                    Button::new("properties")
                        .label("Properties")
                        .ghost()
                        .xsmall()
                        .on_click(
                            cx.listener(|this, _, window, cx| this.show_properties(window, cx)),
                        ),
                )
                .child(
                    Button::new("test")
                        .label("Test")
                        .ghost()
                        .xsmall()
                        .on_click(cx.listener(|this, _, _, cx| this.test_archive(cx))),
                )
                .child(
                    Button::new("diff").label("Diff").ghost().xsmall().on_click(
                        cx.listener(|this, _, window, cx| this.open_diff_panel(window, cx)),
                    ),
                )
                .child(
                    Button::new("compare")
                        .label("Compare with…")
                        .ghost()
                        .xsmall()
                        .on_click(cx.listener(|this, _, window, cx| this.compare_with(window, cx))),
                )
                .child(
                    Button::new("toggle-changes")
                        .label(if self.changes_open || self.dirty {
                            format!("Changes ({staged_count})")
                        } else {
                            "Changes".into()
                        })
                        .ghost()
                        .xsmall()
                        .selected(self.changes_open || self.dirty)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.changes_open = !this.changes_open;
                            cx.notify();
                        })),
                );
        }
        if self.needs_password {
            toolbar = toolbar.child(
                Button::new("enter-password")
                    .label("Enter password")
                    .primary()
                    .xsmall()
                    .on_click(cx.listener(|this, _, window, cx| this.enter_password(window, cx))),
            );
        }
        if self.dirty {
            toolbar = toolbar
                .child(
                    Button::new("commit")
                        .label(if staged_count > 0 {
                            format!("Commit ({staged_count})")
                        } else {
                            "Commit".into()
                        })
                        .primary()
                        .xsmall()
                        .on_click(cx.listener(|this, _, _, cx| this.commit_and_notify(cx))),
                )
                .child(
                    Button::new("discard")
                        .label("Discard")
                        .ghost()
                        .xsmall()
                        .on_click(cx.listener(|this, _, _, cx| this.discard(cx))),
                );
        }

        let breadcrumb: SharedString = match &self.session {
            Some(_) => {
                let current = self.explorer.read(cx).current_path();
                if current.is_empty() {
                    "/".into()
                } else {
                    format!("/{current}").into()
                }
            }
            None => "Open an archive to begin".into(),
        };
        let toolbar = toolbar.child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_sm()
                .text_color(muted)
                .child(breadcrumb),
        );

        // An explicit status message (operation result / hint) is rendered
        // emphasized; the default line (entry/selection summary) stays muted.
        let (status_line, status_emphasized): (SharedString, bool) = match &self.status {
            Some(message) => (message.clone(), true),
            None => {
                let selection = self.explorer.read(cx).selected_rows(cx);
                let line = if selection.is_empty() {
                    format!("{} entries", self.explorer.read(cx).rows(cx).len()).into()
                } else {
                    let bytes: u64 = selection.iter().map(|row| row.size).sum();
                    format!(
                        "{} selected, {}",
                        selection.len(),
                        explorer::model::format_size(bytes)
                    )
                    .into()
                };
                (line, false)
            }
        };

        // Inline password entry: rendered inside this panel (never a dialog),
        // so focus, typing, and dismissal all behave like any other control.
        let password_row = self.password_entry.clone().map(|entry| {
            let theme = cx.theme();
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_2()
                .py_1p5()
                .flex_shrink_0()
                .border_b_1()
                .border_color(border)
                .bg(hsla(
                    theme.warning.h,
                    theme.warning.s,
                    theme.warning.l,
                    0.08,
                ))
                .child(div().text_xs().text_color(theme.warning).child("Password:"))
                .child(div().w(px(240.0)).child(Input::new(&entry)))
                .child(
                    Button::new("password-ok")
                        .label("OK")
                        .primary()
                        .xsmall()
                        .on_click(cx.listener(|this, _, _, cx| {
                            let Some(entry) = this.password_entry.clone() else {
                                return;
                            };
                            let password = entry.read(cx).value().trim().to_string();
                            if password.is_empty() {
                                return;
                            }
                            this.password_entry = None;
                            this.apply_supplied_password(password, cx);
                        })),
                )
                .child(
                    Button::new("password-dismiss")
                        .label("Dismiss")
                        .ghost()
                        .xsmall()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.password_entry = None;
                            this.status = Some("Password entry dismissed".into());
                            cx.notify();
                        })),
                )
        });

        let content = if self.session.is_none() {
            let recent: Vec<String> = settings::global(cx)
                .recent_archives
                .iter()
                .take(6)
                .cloned()
                .collect();
            let weak = cx.weak_entity();
            let recent_list = if recent.is_empty() {
                None
            } else {
                let rows: Vec<gpui::AnyElement> = recent
                    .iter()
                    .map(|path| {
                        let weak = weak.clone();
                        let path = path.clone();
                        div()
                            .id(SharedString::from(format!("recent-{path}")))
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .cursor_pointer()
                            .hover(|el| el.bg(hsla(0.0, 0.0, 0.5, 0.15)))
                            .truncate()
                            .max_w(px(420.0))
                            .child(SharedString::from(path.clone()))
                            .on_click(move |_, _, cx| {
                                let Some(host) = globals::host(cx) else {
                                    return;
                                };
                                let Some(window) = globals::host_window(cx) else {
                                    return;
                                };
                                let path = PathBuf::from(&path);
                                let _ = window.update(cx, |_, window, cx| {
                                    let _ = host.update(cx, |host, cx| {
                                        host.open_archive(path, window, cx, true)
                                    });
                                });
                            })
                            .into_any_element()
                    })
                    .collect();
                Some(
                    div()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(div().text_xs().text_color(muted).child("Recent archives"))
                        .children(rows),
                )
            };
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child("Open an archive to begin (Ctrl+O)"),
                        )
                        .child(
                            Button::new("open-archive")
                                .label("Open Archive…")
                                .primary()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    // Direct host call: the action bubble is
                                    // the Ctrl+O path, not needed here.
                                    let Some(host) = globals::host(cx) else {
                                        this.status = Some("No window host".into());
                                        cx.notify();
                                        return;
                                    };
                                    let _ = host.update(cx, |host, cx| {
                                        host.open_archive_dialog(window, cx)
                                    });
                                })),
                        )
                        .children(recent_list),
                )
        } else {
            let table = div().flex_1().min_h_0().child(self.explorer.clone());
            if self.dirty || self.changes_open {
                let strip = self.render_changes_strip(&changes, staged_count, unstaged_count, cx);
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(table)
                    .child(strip)
            } else {
                table
            }
        };

        div()
            .id("archive-workspace")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .children(password_row)
            .child(content)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .h(px(24.0))
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(border)
                    .text_xs()
                    .text_color(if status_emphasized {
                        cx.theme().foreground
                    } else {
                        muted
                    })
                    .when(status_emphasized, |el| {
                        el.child(
                            div()
                                .w(px(6.0))
                                .h(px(6.0))
                                .rounded_full()
                                .bg(cx.theme().primary),
                        )
                    })
                    .child(status_line),
            )
    }
}

#[cfg(target_os = "windows")]
fn open_with_association(path: &std::path::Path) {
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", ""])
        .arg(path)
        .spawn();
}

#[cfg(not(target_os = "windows"))]
fn open_with_association(path: &std::path::Path) {
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

/// One-line, user-visible outcome of a background job. Kept in the status
/// line until the next action replaces it, so fast jobs still give feedback.
fn job_result_message(
    success: bool,
    message: &str,
    error: Option<task::TaskErrorKind>,
) -> SharedString {
    if success {
        "Completed successfully".into()
    } else {
        match error {
            Some(task::TaskErrorKind::Cancelled) => "Cancelled".into(),
            _ => format!("Failed: {message}").into(),
        }
    }
}

/// Serves diff-panel content out of two open sessions: the Base side is the
/// source workspace, the Working side the comparison target. Both sides read
/// from their archive (falling back from working to base for unmodified
/// entries), so this also works for entries the target never extracted.
struct CrossWorkspaceProvider {
    source: Arc<Mutex<ArchiveSession>>,
    target: Arc<Mutex<ArchiveSession>>,
    engine: Arc<dyn bit7z_rs::ArchiveEngine>,
}

impl DiffContentProvider for CrossWorkspaceProvider {
    fn content(&self, path: &str, side: Side) -> Option<Vec<u8>> {
        let session = match side {
            Side::Base => &self.source,
            Side::Working => &self.target,
        };
        let (archive_path, index, password) = {
            let session = session.lock().expect("session lock poisoned");
            let overlay = session.overlay();
            let node_id = overlay
                .working()
                .resolve_path(path)
                .or_else(|| overlay.base().resolve_path(path))?;
            let index = overlay
                .working()
                .node(node_id)
                .and_then(|n| n.archive_index())
                .or_else(|| overlay.base().node(node_id).and_then(|n| n.archive_index()))?;
            (
                session.archive_path().to_path_buf(),
                index,
                session.password().cloned(),
            )
        };
        self.engine
            .extract_to_buffer(&archive_path, index, password.as_ref())
            .ok()
    }
}

/// Serves diff-panel content requests out of one session: the base side is
/// extracted from the archive on demand, the working side is read from the
/// session's work directory (falling back to base content for entries that
/// were never extracted, which by definition are unchanged).
struct SessionDiffProvider {
    session: Arc<Mutex<ArchiveSession>>,
    engine: Arc<dyn bit7z_rs::ArchiveEngine>,
}

impl SessionDiffProvider {
    fn new(session: Arc<Mutex<ArchiveSession>>, engine: Arc<dyn bit7z_rs::ArchiveEngine>) -> Self {
        Self { session, engine }
    }

    /// The archive index of `path` in the base tree, if it is a file there.
    fn base_index(&self, path: &str) -> Option<u32> {
        let session = self.session.lock().expect("session lock poisoned");
        let id = session.overlay().base().resolve_path(path)?;
        session.overlay().base().node(id)?.archive_index()
    }
}

impl DiffContentProvider for SessionDiffProvider {
    fn content(&self, path: &str, side: Side) -> Option<Vec<u8>> {
        match side {
            Side::Base => {
                let index = self.base_index(path)?;
                let (archive_path, password) = {
                    let session = self.session.lock().expect("session lock poisoned");
                    (
                        session.archive_path().to_path_buf(),
                        session.password().cloned(),
                    )
                };
                self.engine
                    .extract_to_buffer(&archive_path, index, password.as_ref())
                    .ok()
            }
            Side::Working => {
                // Extracted-but-unedited entries have no work-dir file; their
                // content is identical to base by definition.
                let (work_dir, password) = {
                    let session = self.session.lock().expect("session lock poisoned");
                    (
                        session.work_dir().to_path_buf(),
                        session.password().cloned(),
                    )
                };
                let fs_path = work_dir.join(temp::sanitize_relative(std::path::Path::new(path)));
                if let Ok(bytes) = std::fs::read(&fs_path) {
                    return Some(bytes);
                }
                let index = self.base_index(path)?;
                let archive_path = self
                    .session
                    .lock()
                    .expect("session lock poisoned")
                    .archive_path()
                    .to_path_buf();
                self.engine
                    .extract_to_buffer(&archive_path, index, password.as_ref())
                    .ok()
            }
        }
    }
}
