//! Archive explorer view: the virtualized, multi-select file table and
//! keyboard behavior. The rows live in a `Signal` that the `ui` crate's
//! [`DataView`] observes (and repaints on every refresh), virtualized with a
//! fill-the-parent body; the column budget comes from [`Responsive`], which
//! measures the table's own container width, not the window. Sorting and
//! multi-selection stay view behavior here: rows enter the signal already in
//! display order, and the `DataView` stays a plain collection shell.

use crate::model::{
    self, EntryRow, Sort, SortColumn, SortDirection, VisibleColumns, apply_sort, attr_string,
    collect_indices, crc_string, format_size, rows_for_path,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext, ClickEvent, Context, Div, Entity, EventEmitter, ExternalPaths, FocusHandle,
    Focusable, Hsla, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Pixels, Render,
    ScrollStrategy, SharedString, Stateful, StatefulInteractiveElement, Styled,
    UniformListScrollHandle, WeakEntity, Window, div, hsla, px,
};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable};
use reactive_signals::reactive::Signal;
use session::ArchiveSession;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use ui::components::Responsive;
use ui::data::dataview::DataView;

/// Row height of the file table; uniform so the list can virtualize.
const ROW_HEIGHT: Pixels = px(24.0);

/// Fixed column widths, resolved from the model's budget constants so the
/// header, the rows, and the responsive fit calculation never disagree.
const COL_SIZE: Pixels = px(model::SIZE_WIDTH);
const COL_PACKED: Pixels = px(model::PACKED_WIDTH);
const COL_MODIFIED: Pixels = px(model::MODIFIED_WIDTH);
const COL_ATTRIBUTES: Pixels = px(model::ATTRIBUTES_WIDTH);
const COL_CRC: Pixels = px(model::CRC_WIDTH);
const COL_METHOD: Pixels = px(model::METHOD_WIDTH);
const COL_NAME_MIN: Pixels = px(model::NAME_MIN_WIDTH);

/// Events forwarded to the host.
#[derive(Debug, Clone)]
pub enum ExplorerEvent {
    /// A file entry was activated (double click / Enter).
    Activated(usize),
    SelectionChanged(Vec<usize>),
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
    /// The table's data: rows already in display order. One source — the
    /// `DataView` observes it, every accessor below reads it; the view never
    /// keeps a second copy.
    rows: Signal<Vec<EntryRow>>,
    /// Column visibility, published from the measured container width during
    /// render; the row templates read it when painting, so header and rows
    /// agree within one frame.
    cols: Signal<VisibleColumns>,
    data_view: Entity<DataView<EntryRow>>,
    sort: Option<Sort>,
    /// Selected row indices, kept ascending.
    selection: Vec<usize>,
    /// Row a shift-click extends from.
    anchor: Option<usize>,
    focus_handle: FocusHandle,
    scroll_handle: UniformListScrollHandle,
}

impl EventEmitter<ExplorerEvent> for ArchiveExplorer {}

impl Focusable for ArchiveExplorer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ArchiveExplorer {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let rows: Signal<Vec<EntryRow>> = Signal::new(cx, Vec::new());
        let cols = Signal::new(cx, VisibleColumns::MINIMAL);
        let scroll_handle = UniformListScrollHandle::new();
        let weak = cx.weak_entity();
        let data_view = cx.new(|cx| {
            DataView::new(cx, &rows)
                .fill()
                .track_scroll(&scroll_handle)
                .empty(|_window, cx| {
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("No entries")
                })
                .item({
                    // Selection is multi-select (ctrl/shift/extension), which
                    // the shell's single-select model does not cover, so the
                    // row owns its interaction: the template reads the live
                    // selection here and handles the click below.
                    let weak = weak.clone();
                    let cols = cols.clone();
                    move |row: &EntryRow, ix, _window, app| {
                        let visible = *cols.read(app);
                        let selected = weak.upgrade().is_some_and(|explorer| {
                            explorer.read(app).selection.contains(&ix)
                        });
                        render_row(&weak, ix, row, selected, visible, app)
                    }
                })
        });
        Self {
            session: None,
            current_path: String::new(),
            rows,
            cols,
            data_view,
            sort: None,
            selection: Vec::new(),
            anchor: None,
            focus_handle: cx.focus_handle(),
            scroll_handle,
        }
    }

    pub fn set_session(&mut self, session: Arc<Mutex<ArchiveSession>>, cx: &mut Context<Self>) {
        self.session = Some(session);
        self.current_path.clear();
        self.selection.clear();
        self.anchor = None;
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
            self.selection.clear();
            self.anchor = None;
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
        self.selection.clear();
        self.anchor = None;
        self.refresh(cx);
    }

    pub fn current_path(&self) -> &str {
        &self.current_path
    }

    pub fn rows<'a>(&self, cx: &'a App) -> &'a [EntryRow] {
        self.rows.read(cx).as_slice()
    }

    pub fn selected_rows(&self, cx: &App) -> Vec<EntryRow> {
        self.selection
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
        let selected: Vec<&EntryRow> = if self.selection.is_empty() {
            rows.iter().collect()
        } else {
            self.selection
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
        if self.selection.len() != 1 {
            return None;
        }
        self.rows.read(cx).get(self.selection[0]).cloned()
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

    /// Rebuilds the rows from the session (sorted into display order) and
    /// publishes them: the `DataView` repaints off the signal, and the view
    /// holds no second copy.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let next = match &self.session {
            Some(session) => {
                let mut rows = rows_for_path(session, &self.current_path);
                if let Some(sort) = self.sort {
                    apply_sort(&mut rows, sort);
                }
                rows
            }
            None => Vec::new(),
        };
        let len = next.len();
        self.selection.retain(|ix| *ix < len);
        self.anchor = self.anchor.filter(|ix| *ix < len);
        self.rows.set(cx, next);
        cx.notify();
    }

    fn on_row_click(&mut self, ix: usize, event: &ClickEvent, cx: &mut Context<Self>) {
        let modifiers = event.modifiers();
        if event.click_count() >= 2 {
            if let Some(row) = self.rows.read(cx).get(ix).cloned() {
                if row.is_directory {
                    self.navigate(&row.path, cx);
                } else {
                    cx.emit(ExplorerEvent::Activated(ix));
                }
            }
            return;
        }

        if modifiers.shift {
            let anchor = self.anchor.unwrap_or(ix);
            let (start, end) = if anchor <= ix {
                (anchor, ix)
            } else {
                (ix, anchor)
            };
            self.selection = (start..=end).collect();
        } else if modifiers.control {
            if let Some(pos) = self.selection.iter().position(|s| *s == ix) {
                self.selection.remove(pos);
            } else {
                self.selection.push(ix);
                self.selection.sort_unstable();
            }
            self.anchor = Some(ix);
        } else {
            self.selection = vec![ix];
            self.anchor = Some(ix);
        }
        cx.emit(ExplorerEvent::SelectionChanged(self.selection.clone()));
        cx.notify();
    }

    fn select_all(&mut self, cx: &mut Context<Self>) {
        self.selection = (0..self.rows.read(cx).len()).collect();
        cx.emit(ExplorerEvent::SelectionChanged(self.selection.clone()));
        cx.notify();
    }

    fn move_cursor(&mut self, delta: isize, extend: bool, cx: &mut Context<Self>) {
        let len = self.rows.read(cx).len() as isize;
        if len == 0 {
            return;
        }
        let anchor = self.anchor.unwrap_or(0) as isize;
        let current = self.selection.last().copied().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, len - 1) as usize;
        if extend {
            let (start, end) = if anchor <= next as isize {
                (anchor as usize, next)
            } else {
                (next, anchor as usize)
            };
            self.selection = (start..=end).collect();
        } else {
            self.selection = vec![next];
            self.anchor = Some(next);
        }
        // The handle feeds the `DataView`'s internal uniform list (track_scroll).
        self.scroll_handle.scroll_to_item(next, ScrollStrategy::Top);
        cx.emit(ExplorerEvent::SelectionChanged(self.selection.clone()));
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        match key {
            "backspace" => self.navigate_up(cx),
            "delete" => cx.emit(ExplorerEvent::Command(ExplorerCommand::Delete)),
            "f2" => cx.emit(ExplorerEvent::Command(ExplorerCommand::Rename)),
            "up" => self.move_cursor(-1, modifiers.shift, cx),
            "down" => self.move_cursor(1, modifiers.shift, cx),
            "enter" => {
                let active = self.selection.last().copied();
                if let Some(ix) = active {
                    if let Some(row) = self.rows.read(cx).get(ix).cloned() {
                        if row.is_directory {
                            self.navigate(&row.path, cx);
                        } else {
                            cx.emit(ExplorerEvent::Activated(ix));
                        }
                    }
                }
            }
            "a" if modifiers.control => self.select_all(cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn set_sort(&mut self, column: SortColumn, cx: &mut Context<Self>) {
        self.sort = Some(match self.sort {
            Some(sort) => sort.toggled(column),
            None => Sort {
                column,
                direction: SortDirection::Ascending,
            },
        });
        self.refresh(cx);
    }
}

impl Render for ArchiveExplorer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, muted, border) = {
            let theme = cx.theme();
            (theme.foreground, theme.muted_foreground, theme.border)
        };
        let on_sort: Rc<dyn Fn(&SortColumn, &mut Window, &mut App)> =
            Rc::new(cx.listener(|this, column: &SortColumn, _window, cx| {
                this.set_sort(*column, cx);
            }));
        let sort = self.sort;
        let cols = self.cols.clone();
        let data_view = self.data_view.clone();

        // Drag&drop: external file drops arrive as `ExternalPaths` drags (the
        // platform layer translates them); the whole view is the drop target.
        div()
            .id("archive-explorer")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key))
            .drag_over::<ExternalPaths>(|el, _, _, cx| {
                el.border_color(cx.theme().primary)
            })
            .on_drop(cx.listener(
                move |this, paths: &ExternalPaths, _window, cx| {
                    this.ingest_dropped(paths.paths(), cx);
                },
            ))
            .size_full()
            .flex()
            .flex_col()
            .text_color(fg)
            .bg(cx.theme().background)
            .child(div().flex_1().min_h_0().child(Responsive::new(
                "explorer-table",
                move |size, _window, cx| {
                    // The table's own width — not the window's — drives the
                    // column budget. Publish it for the row templates (which
                    // paint later in the frame) and use it for the header now.
                    let visible = VisibleColumns::for_width(size.width());
                    cols.set_if_changed(cx, visible);
                    div()
                        .size_full()
                        .flex()
                        .flex_col()
                        .child(header_row(sort, muted, border, visible, on_sort))
                        .child(data_view)
                        .into_any_element()
                },
            )))
    }
}

fn header_arrow(sort: Option<Sort>, column: SortColumn) -> &'static str {
    match sort {
        Some(s) if s.column == column => match s.direction {
            SortDirection::Ascending => " ↑",
            SortDirection::Descending => " ↓",
        },
        _ => "",
    }
}

fn header_row(
    sort: Option<Sort>,
    muted: Hsla,
    border: Hsla,
    visible: VisibleColumns,
    on_sort: Rc<dyn Fn(&SortColumn, &mut Window, &mut App)>,
) -> Stateful<Div> {
    let column = |title: &'static str,
                  sortable: Option<SortColumn>,
                  width: Pixels,
                  align_end: bool,
                  on_sort: Rc<dyn Fn(&SortColumn, &mut Window, &mut App)>| {
        let label = match sortable {
            Some(col) => format!("{title}{}", header_arrow(sort, col)),
            None => title.to_string(),
        };
        let mut cell = div()
            .w(width)
            .flex_shrink_0()
            .h_full()
            .flex()
            .items_center()
            .px_2()
            .child(div().text_xs().text_color(muted).child(label));
        if align_end {
            cell = cell.justify_end();
        }
        let cell = cell.id(title).cursor_pointer();
        cell.when_some(sortable, |cell, col| {
            cell.hover(|this| this.text_color(gpui::black()))
                .on_click(move |_, window, cx| on_sort(&col, window, cx))
        })
    };

    div()
        .id("header")
        .flex()
        .flex_row()
        .h(px(28.0))
        .flex_shrink_0()
        .border_b_1()
        .border_color(border)
        .child(
            column(
                "Name",
                Some(SortColumn::Name),
                px(0.0),
                false,
                on_sort.clone(),
            )
            .w_auto()
            .flex_1()
            .min_w(COL_NAME_MIN),
        )
        .child(column(
            "Size",
            Some(SortColumn::Size),
            COL_SIZE,
            true,
            on_sort.clone(),
        ))
        .when(visible.packed, |el| {
            el.child(column(
                "Packed",
                None,
                COL_PACKED,
                true,
                on_sort.clone(),
            ))
        })
        .when(visible.modified, |el| {
            el.child(column(
                "Modified",
                Some(SortColumn::Modified),
                COL_MODIFIED,
                false,
                on_sort.clone(),
            ))
        })
        .when(visible.attributes, |el| {
            el.child(column(
                "Attributes",
                None,
                COL_ATTRIBUTES,
                false,
                on_sort.clone(),
            ))
        })
        .when(visible.crc, |el| {
            el.child(column(
                "CRC",
                Some(SortColumn::Crc),
                COL_CRC,
                true,
                on_sort.clone(),
            ))
        })
        .when(visible.method, |el| {
            el.child(column("Method", None, COL_METHOD, false, on_sort))
        })
}

fn render_row(
    weak: &WeakEntity<ArchiveExplorer>,
    ix: usize,
    row: &EntryRow,
    selected: bool,
    visible: VisibleColumns,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, border, secondary, primary) = (
        theme.muted_foreground,
        theme.border,
        theme.secondary,
        theme.primary,
    );
    let selected_bg = hsla(primary.h, primary.s, primary.l, 0.25);

    let weak = weak.clone();
    let icon = if row.is_directory {
        IconName::Folder
    } else {
        IconName::File
    };
    let name_cell = div()
        .flex_1()
        .min_w(px(0.0))
        .h_full()
        .flex()
        .items_center()
        .gap(px(6.0))
        .px_2()
        .child(Icon::new(icon).xsmall().text_color(muted))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .child(SharedString::from(row.name.clone())),
        );

    let cell = |text: String, width: Pixels, end: bool| {
        let mut el = div()
            .w(width)
            .flex_shrink_0()
            .h_full()
            .flex()
            .items_center()
            .px_2()
            .text_xs()
            .child(text);
        if end {
            el = el.justify_end();
        }
        el
    };

    div()
        .id(("row", ix))
        .h(ROW_HEIGHT)
        .w_full()
        .flex()
        .flex_row()
        .border_b_1()
        .border_color(hsla(border.h, border.s, border.l, 0.35))
        .when(selected, |el| el.bg(selected_bg))
        .when(!selected, |el| el.hover(|el| el.bg(secondary)))
        .on_click(move |event, _window, cx| {
            weak.update(cx, |this, cx| this.on_row_click(ix, event, cx))
                .ok();
        })
        .child(name_cell)
        .child(cell(format_size(row.size), COL_SIZE, true))
        .when(visible.packed, |el| {
            el.child(cell(format_size(row.packed), COL_PACKED, true))
        })
        .when(visible.modified, |el| {
            el.child(cell(row.modified.clone(), COL_MODIFIED, false))
        })
        .when(visible.attributes, |el| {
            el.child(cell(attr_string(row), COL_ATTRIBUTES, false))
        })
        .when(visible.crc, |el| {
            el.child(cell(crc_string(row), COL_CRC, true))
        })
        .when(visible.method, |el| {
            el.child(cell(row.method.clone(), COL_METHOD, false))
        })
}
