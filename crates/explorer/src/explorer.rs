//! Archive explorer view: the file table bound to the session's rows signal.
//! The table component is the `ui` crate's [`TableView`] (ported from the
//! original guise project): column widths (drag a header's edge), column
//! order (drag headers onto each other), column visibility (header
//! right-click), and sorting (header clicks) all live there. This view binds
//! it to the session, wraps it in the horizontal scroll container with its
//! custom scrollbar, and handles external file drops, the shift-wheel pan,
//! file-manager keys, and the row context menu.

use crate::model::{
    self, ColumnKind, EntryRow, attr_string, natural_cmp, collect_indices, crc_string,
    format_size, rows_for_path, total_table_width,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action as _, AnyElement, App, AppContext, Context, DragMoveEvent, Entity, EventEmitter,
    ExternalPaths, FocusHandle, Focusable, Hsla, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, ScrollHandle, ScrollWheelEvent, SharedString, StatefulInteractiveElement,
    Styled, WeakEntity, Window, div, hsla, point, px,
};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::ActiveTheme;
use reactive_signals::reactive::Signal;
use session::ArchiveSession;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use ui::components::Responsive;
use ui::data::table_view::{
    self, Align, Column as TableColumn, SelectionMode, TableView, TableViewEvent,
};

gpui::actions!(
    explorer,
    [
        ToggleSizeColumn,
        TogglePackedColumn,
        ToggleModifiedColumn,
        ToggleAttributesColumn,
        ToggleCrcColumn,
        ToggleMethodColumn,
    ]
);

/// Events forwarded to the host.
#[derive(Debug, Clone)]
pub enum ExplorerEvent {
    /// A file entry was activated (double click / Enter).
    Activated(usize),
    SelectionChanged(Vec<usize>),
    /// Keyboard commands the host must perform (they need the engine).
    Command(ExplorerCommand),
}

/// Keyboard commands the host must perform (they need the engine).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerCommand {
    Delete,
    Rename,
}

/// A browsable archive explorer view.
pub struct ArchiveExplorer {
    session: Option<Arc<Mutex<ArchiveSession>>>,
    current_path: String,
    /// The table's data: rows already in `rows_for_path` order (directories
    /// first, natural names). The `TableView` binds to this signal and does
    /// its own sorting on top.
    rows: Signal<Vec<EntryRow>>,
    table: Entity<TableView<EntryRow>>,
    /// Visible data columns in display order. The Name column is implicit —
    /// always first, flexing over the remaining width.
    columns: Vec<model::Column>,
    /// Horizontal pan of the table viewport, shared with the custom
    /// scrollbar strip under the table.
    x_scroll: ScrollHandle,
    focus_handle: FocusHandle,
    _subscriptions: Vec<gpui::Subscription>,
}

impl EventEmitter<ExplorerEvent> for ArchiveExplorer {}

impl Focusable for ArchiveExplorer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Column identity in the `TableView`: the Name column is 0, the data
/// columns follow [`ColumnKind`] declaration order starting at 1.
fn column_id(kind: ColumnKind) -> u64 {
    match kind {
        ColumnKind::Size => 1,
        ColumnKind::Packed => 2,
        ColumnKind::Modified => 3,
        ColumnKind::Attributes => 4,
        ColumnKind::Crc => 5,
        ColumnKind::Method => 6,
    }
}

fn column_kind_by_id(id: u64) -> Option<ColumnKind> {
    Some(match id {
        1 => ColumnKind::Size,
        2 => ColumnKind::Packed,
        3 => ColumnKind::Modified,
        4 => ColumnKind::Attributes,
        5 => ColumnKind::Crc,
        6 => ColumnKind::Method,
        _ => return None,
    })
}

/// The `TableView` column definitions for a visible-column state.
fn build_table_columns(columns: &[model::Column]) -> Vec<TableColumn<EntryRow>> {
    let mut out = vec![TableColumn::<EntryRow>::new("Name")
        .id(0)
        .flex(1.0)
        .min_width(model::NAME_MIN_WIDTH)
        .text(|row: &EntryRow| row.name.clone().into())
        .sortable_by(|a: &EntryRow, b: &EntryRow| natural_cmp(&a.name, &b.name))];
    for state in columns {
        let kind = state.kind;
        let base = TableColumn::<EntryRow>::new(kind.title())
            .id(column_id(kind))
            .width(state.width)
            .min_width(model::COLUMN_MIN_WIDTH);
        let column = match kind {
            ColumnKind::Size => base
                .align(Align::End)
                .text(|row| format_size(row.size).into())
                .sortable_by(|a, b| a.size.cmp(&b.size)),
            ColumnKind::Packed => base
                .align(Align::End)
                .text(|row| format_size(row.packed).into())
                .sortable_by(|a, b| a.packed.cmp(&b.packed)),
            ColumnKind::Modified => base
                .text(|row| row.modified.clone().into())
                .sortable_by(|a, b| a.modified.cmp(&b.modified)),
            ColumnKind::Attributes => base.text(|row| attr_string(row).into()),
            ColumnKind::Crc => base
                .align(Align::End)
                .text(|row| crc_string(row).into())
                .sortable_by(|a, b| a.crc.cmp(&b.crc)),
            ColumnKind::Method => base.text(|row| row.method.clone().into()),
        };
        out.push(column);
    }
    out
}

impl ArchiveExplorer {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let rows: Signal<Vec<EntryRow>> = Signal::new(cx, Vec::new());
        let x_scroll = ScrollHandle::new();
        let columns = model::default_columns();
        let table = {
            let rows = rows.clone();
            let initial = columns.clone();
            cx.new(|cx| {
                TableView::new(cx)
                    .columns(build_table_columns(&initial))
                    .bind_rows(&rows, cx)
                    .selection_mode(SelectionMode::Multi)
                    .highlight_on_hover(true)
                    .fill(true)
                    .reorderable(true)
                    .empty(|_window, cx| {
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("No entries")
                    })
                    .row_menu({
                        let rows = rows.clone();
                        move |menu, source, _window, app| {
                            let is_directory = rows
                                .read(app)
                                .get(source)
                                .is_some_and(|row| row.is_directory);
                            row_context_menu(is_directory, menu)
                        }
                    })
                    .header_menu({
                        let weak = cx.weak_entity();
                        move |popup, _window, app| {
                            let Some(table) = weak.upgrade() else {
                                return popup;
                            };
                            column_menu(table.read(app).column_defs(), popup)
                        }
                    })
            })
        };
        let subscription = cx.subscribe(&table, Self::on_table_event);

        Self {
            session: None,
            current_path: String::new(),
            rows,
            table,
            columns,
            x_scroll,
            focus_handle: cx.focus_handle(),
            _subscriptions: vec![subscription],
        }
    }

    pub fn set_session(&mut self, session: Arc<Mutex<ArchiveSession>>, cx: &mut Context<Self>) {
        self.session = Some(session);
        self.current_path.clear();
        self.refresh(cx);
    }

    pub fn session(&self) -> Option<&Arc<Mutex<ArchiveSession>>> {
        self.session.as_ref()
    }

    pub fn navigate(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            return;
        };
        let exists = {
            let session = session.lock().expect("session lock poisoned");
            session.overlay().working().resolve_path(path).is_some()
        };
        if exists {
            self.current_path = path.to_string();
            self.refresh(cx);
        }
    }

    pub fn navigate_up(&mut self, cx: &mut Context<Self>) {
        if self.current_path.is_empty() {
            return;
        }
        let trimmed = self.current_path.trim_end_matches('/');
        self.current_path = match trimmed.rfind('/') {
            Some(idx) => trimmed[..idx].to_string(),
            None => String::new(),
        };
        self.refresh(cx);
    }

    pub fn current_path(&self) -> &str {
        &self.current_path
    }

    pub fn rows<'a>(&self, cx: &'a App) -> &'a [EntryRow] {
        self.rows.read(cx).as_slice()
    }

    pub fn selected_rows(&self, cx: &App) -> Vec<EntryRow> {
        self.table
            .read(cx)
            .selected()
            .iter()
            .filter_map(|ix| self.rows.read(cx).get(*ix).cloned())
            .collect()
    }

    /// Archive entry indices the host should operate on: the recursive
    /// expansion of the selection, or of every visible row when the selection
    /// is empty (7zFM semantics).
    pub fn target_indices(&self, cx: &App) -> Vec<u32> {
        let Some(session) = &self.session else {
            return Vec::new();
        };
        let rows = self.rows.read(cx);
        let selected: Vec<&EntryRow> = if self.table.read(cx).selected().is_empty() {
            rows.iter().collect()
        } else {
            self.table
                .read(cx)
                .selected()
                .iter()
                .filter_map(|ix| rows.get(*ix))
                .collect()
        };
        let session = session.lock().expect("session lock poisoned");
        let tree = session.overlay().working();
        let mut indices = Vec::new();
        for row in selected {
            collect_indices(tree, row.id, &mut indices);
        }
        indices.sort_unstable();
        indices.dedup();
        indices
    }

    pub fn focused_row(&self, cx: &App) -> Option<EntryRow> {
        let selected = self.table.read(cx).selected();
        if selected.len() != 1 {
            return None;
        }
        self.rows.read(cx).get(selected[0]).cloned()
    }

    /// The most recently selected row index (the "active" entry, e.g. the one
    /// Enter/Open acts on).
    pub fn active_index(&self, cx: &App) -> Option<usize> {
        self.table.read(cx).selected().into_iter().last()
    }

    /// Selects every visible row (the Select-all command).
    pub fn select_all_rows(&mut self, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| table.select_all(cx));
    }

    /// Copies dropped external files into the current directory as unstaged
    /// additions. The copies run on the background executor (same shape as
    /// the workspace's Add-files flow): a large drop must neither block the
    /// UI thread nor hold the session mutex on it.
    fn ingest_dropped(&mut self, paths: &[std::path::PathBuf], cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let current = self.current_path.clone();
        let files: Vec<std::path::PathBuf> = paths
            .iter()
            .filter(|path| !path.is_dir()) // directory recursion lands with the Add-folder flow
            .cloned()
            .collect();
        if files.is_empty() {
            return;
        }
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let added = cx
                .background_executor()
                .spawn(async move {
                    let mut session = session.lock().expect("session lock poisoned");
                    let mut added = 0usize;
                    for file in files {
                        let Some(name) = file.file_name() else {
                            continue;
                        };
                        let rel = if current.is_empty() {
                            name.to_string_lossy().to_string()
                        } else {
                            format!("{current}/{}", name.to_string_lossy())
                        };
                        if session.ingest_file(&file, &rel).is_ok() {
                            added += 1;
                        }
                    }
                    added
                })
                .await;
            if added > 0 {
                let _ = weak.update(cx, |this, cx| this.refresh(cx));
            }
        })
        .detach();
    }

    /// Rebuilds the rows from the session (directories first, natural names)
    /// and publishes them to the table's signal, which repaints the table.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let next = match &self.session {
            Some(session) => rows_for_path(session, &self.current_path),
            None => Vec::new(),
        };
        self.rows.set(cx, next);
        cx.notify();
    }

    fn on_table_event(
        &mut self,
        _table: Entity<TableView<EntryRow>>,
        event: &TableViewEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            TableViewEvent::SelectionChanged(indices) => {
                cx.emit(ExplorerEvent::SelectionChanged(indices.clone()));
                cx.notify();
            }
            TableViewEvent::Activated(source) => cx.emit(ExplorerEvent::Activated(*source)),
            TableViewEvent::Resized(id, width) => {
                let Some(kind) = column_kind_by_id(*id) else {
                    return;
                };
                if let Some(column) = self
                    .columns
                    .iter_mut()
                    .find(|column| column.kind == kind)
                {
                    column.width = *width;
                }
            }
            TableViewEvent::ColumnsReordered => {
                // Adopt the table's display order (and carried widths) so a
                // later show/hide rebuild keeps the user's arrangement.
                let layout = self.table.read(cx).column_layout();
                self.columns = layout
                    .iter()
                    .filter_map(|(id, width)| {
                        column_kind_by_id(*id)
                            .map(|kind| model::Column {
                                kind,
                                width: *width,
                            })
                    })
                    .collect();
            }
            TableViewEvent::Sorted(_) => {}
        }
    }

    /// Shows a hidden column (appended last, at default width) or hides a
    /// visible one, rebuilding the table's columns.
    fn toggle_column(&mut self, kind: ColumnKind, cx: &mut Context<Self>) {
        if let Some(at) = self.columns.iter().position(|column| column.kind == kind) {
            self.columns.remove(at);
        } else {
            self.columns.push(model::Column {
                kind,
                width: kind.default_width(),
            });
        }
        let defs = build_table_columns(&self.columns);
        self.table.update(cx, |table, cx| table.set_columns(defs, cx));
        cx.notify();
    }

    fn on_toggle_size_column(
        &mut self,
        _: &ToggleSizeColumn,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_column(ColumnKind::Size, cx);
    }

    fn on_toggle_packed_column(
        &mut self,
        _: &TogglePackedColumn,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_column(ColumnKind::Packed, cx);
    }

    fn on_toggle_modified_column(
        &mut self,
        _: &ToggleModifiedColumn,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_column(ColumnKind::Modified, cx);
    }

    fn on_toggle_attributes_column(
        &mut self,
        _: &ToggleAttributesColumn,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_column(ColumnKind::Attributes, cx);
    }

    fn on_toggle_crc_column(
        &mut self,
        _: &ToggleCrcColumn,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_column(ColumnKind::Crc, cx);
    }

    fn on_toggle_method_column(
        &mut self,
        _: &ToggleMethodColumn,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_column(ColumnKind::Method, cx);
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        match key {
            "backspace" => self.navigate_up(cx),
            "delete" => cx.emit(ExplorerEvent::Command(ExplorerCommand::Delete)),
            "f2" => cx.emit(ExplorerEvent::Command(ExplorerCommand::Rename)),
            "a" if modifiers.control => self.select_all_rows(cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Render for ArchiveExplorer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, muted, border) = {
            let theme = cx.theme();
            (theme.foreground, theme.muted_foreground, theme.border)
        };
        let columns = self.columns.clone();
        let total = total_table_width(&columns);
        let data_view = self.table.clone();
        let x_scroll = self.x_scroll.clone();
        let weak = cx.weak_entity();

        // Drag&drop: external file drops arrive as `ExternalPaths` drags (the
        // platform layer translates them); the whole view is the drop target.
        div()
            .id("archive-explorer")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key))
            .on_action(cx.listener(Self::on_toggle_size_column))
            .on_action(cx.listener(Self::on_toggle_packed_column))
            .on_action(cx.listener(Self::on_toggle_modified_column))
            .on_action(cx.listener(Self::on_toggle_attributes_column))
            .on_action(cx.listener(Self::on_toggle_crc_column))
            .on_action(cx.listener(Self::on_toggle_method_column))
            .drag_over::<ExternalPaths>(|el, _, _, cx| {
                el.border_color(cx.theme().primary)
            })
            .on_drop(cx.listener(
                move |this, paths: &ExternalPaths, _window, cx| {
                    this.ingest_dropped(paths.paths(), cx);
                },
            ))
            // Shift + wheel pans the table horizontally (Windows mice have no
            // horizontal wheel).
            .on_scroll_wheel({
                let x_scroll = x_scroll.clone();
                let weak = weak.clone();
                move |event: &ScrollWheelEvent, window, cx| {
                    if !event.modifiers.shift {
                        return;
                    }
                    let max = x_scroll.max_offset();
                    if max.x >= px(0.0) {
                        return; // the table fits; nothing to pan
                    }
                    let delta = event.delta.pixel_delta(window.line_height());
                    if delta.x == px(0.0) && delta.y == px(0.0) {
                        return;
                    }
                    let cur = x_scroll.offset();
                    let wanted = (cur.x + delta.x + delta.y).clamp(max.x, px(0.0));
                    x_scroll.set_offset(point(wanted, cur.y));
                    cx.stop_propagation();
                    let _ = weak.update(cx, |_, cx| cx.notify());
                }
            })
            .size_full()
            .flex()
            .flex_col()
            .text_color(fg)
            .bg(cx.theme().background)
            .child(div().flex_1().min_h_0().child(Responsive::new(
                "explorer-table",
                move |size, _window, _cx| {
                    // The table's own measured width decides whether the strip
                    // exists and how wide the thumb is; unmeasured (first
                    // frame) counts as fitting.
                    let viewport = size.width().unwrap_or(total);
                    let overflow = viewport + 0.5 < total;
                    div()
                        .size_full()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .id("table-xscroll")
                                .flex_1()
                                .min_h_0()
                                .overflow_x_scroll()
                                .restrict_scroll_to_axis()
                                .track_scroll(&x_scroll)
                                .child(
                                    div()
                                        .h_full()
                                        .w_full()
                                        .min_w(px(total))
                                        .flex()
                                        .flex_col()
                                        .child(data_view.clone()),
                                ),
                        )
                        .child(x_scrollbar_strip(
                            &x_scroll, viewport, overflow, total, muted, &weak,
                        ))
                        .into_any_element()
                },
            )))
    }
}

/// The per-row context menu. Commands are plain actions: they bubble from the
/// focused menu to the host panel, which resolves them against the current
/// selection (the right-clicked row is part of it by then).
fn row_context_menu(is_directory: bool, menu: PopupMenu) -> PopupMenu {
    menu.menu("Open", Box::new(app_action::OpenSelected))
        .separator()
        .menu("Rename", Box::new(app_action::RenameEntry))
        .menu("Delete", Box::new(app_action::DeleteSelected))
        .separator()
        .menu("Extract…", Box::new(app_action::ExtractSelected))
        .menu("Copy full paths", Box::new(app_action::CopySelectedPaths))
        .when(!is_directory, |menu| {
            menu.menu("Checksum…", Box::new(app_action::ChecksumSelected))
        })
        .separator()
        .menu("Properties", Box::new(app_action::ShowProperties))
}

/// The header context menu: every data column with a check mark, toggling
/// its visibility. Reads the table's live column list, so it always reflects
/// the current arrangement.
fn column_menu(columns: &[TableColumn<EntryRow>], menu: PopupMenu) -> PopupMenu {
    let mut menu = menu;
    for kind in [
        ColumnKind::Size,
        ColumnKind::Packed,
        ColumnKind::Modified,
        ColumnKind::Attributes,
        ColumnKind::Crc,
        ColumnKind::Method,
    ] {
        let id = column_id(kind);
        let visible = columns.iter().any(|column| column.identity() == id);
        let action: Box<dyn gpui::Action> = match kind {
            ColumnKind::Size => ToggleSizeColumn.boxed_clone(),
            ColumnKind::Packed => TogglePackedColumn.boxed_clone(),
            ColumnKind::Modified => ToggleModifiedColumn.boxed_clone(),
            ColumnKind::Attributes => ToggleAttributesColumn.boxed_clone(),
            ColumnKind::Crc => ToggleCrcColumn.boxed_clone(),
            ColumnKind::Method => ToggleMethodColumn.boxed_clone(),
        };
        menu = menu.menu_with_check(kind.title(), visible, action);
    }
    menu
}

/// Drag payload for the horizontal scrollbar thumb.
#[derive(Clone, Copy)]
struct XThumbDrag;

/// Invisible ghost for the thumb drag (gpui requires a rendered preview).
struct ThumbGhost;

impl Render for ThumbGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// The thin horizontal scrollbar under the table: shown only when the
/// container is narrower than the columns' total width. Dragging the thumb
/// maps the pointer's ratio along the track to the scroll offset; touchpad
/// horizontal gestures work on the scroller directly.
fn x_scrollbar_strip(
    handle: &ScrollHandle,
    viewport: f32,
    overflow: bool,
    total: f32,
    muted: Hsla,
    weak: &WeakEntity<ArchiveExplorer>,
) -> AnyElement {
    if !overflow {
        return div().into_any_element();
    }
    // gpui conventions: `max_offset` is the positive scrollable range and
    // `offset` lives in [-max, 0], so progress = -offset / max.
    let max = f32::from(handle.max_offset().x);
    let offset = f32::from(handle.offset().x);
    let frac = if max > 0.0 {
        (-offset / max).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let thumb_w = (viewport * viewport / total).max(24.0).min(viewport);
    let thumb_x = frac * (viewport - thumb_w);

    div()
        .id("x-scrollbar")
        .relative()
        .w_full()
        .h(px(10.0))
        .flex_shrink_0()
        .bg(hsla(muted.h, muted.s, muted.l, 0.08))
        .on_drag_move({
            let weak = weak.clone();
            move |ev: &DragMoveEvent<XThumbDrag>, _window, cx| {
                let vw = f32::from(ev.bounds.size.width);
                if vw <= 0.0 {
                    return;
                }
                let ratio = ((f32::from(ev.event.position.x)
                    - f32::from(ev.bounds.origin.x))
                    / vw)
                    .clamp(0.0, 1.0);
                let offset = px(-(ratio * (total - vw)));
                let _ = weak.update(cx, |this, cx| {
                    this.x_scroll.set_offset(point(offset, px(0.0)));
                    cx.notify();
                });
            }
        })
        .child(
            div()
                .id("x-thumb")
                .absolute()
                .left(px(thumb_x))
                .top(px(2.0))
                .h(px(6.0))
                .w(px(thumb_w))
                .rounded_full()
                .bg(hsla(muted.h, muted.s, muted.l, 0.5))
                .cursor_pointer()
                .on_drag(XThumbDrag, |_, _, _, cx| cx.new(|_| ThumbGhost)),
        )
        .into_any_element()
}
