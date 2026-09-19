//! `TableView` — a rich, generic data table (gpui entity), ported from the
//! original guise project's `crates/guise/src/data/tableview` and adapted to
//! this workspace (gpui-kit theme, `reactive_signals`).
//!
//! Renders typed rows through per-column cell closures, with sortable
//! headers, click/mod/shift row selection, a sticky header, drag-resizable
//! and drag-reorderable columns, and a virtualized body.
//!
//! Local extensions over the guise original:
//! - [`TableView::fill`] — body fills the parent (virtualized) instead of a
//!   fixed pixel height.
//! - [`TableView::set_columns`] / [`TableViewEvent::Resized`] — the host owns
//!   column identity via [`Column::id`] and persists widths / rebuilds the
//!   column list (show/hide columns).
//! - [`TableView::reorderable`] — drag a header onto another to reorder;
//!   widths travel with their column.
//! - [`TableView::row_menu`] — per-row context menu hook; right-clicking a
//!   row first adopts it into the selection, matching file-manager behavior.
//! - Ctrl-A / [`TableView::select_all`] in `Multi` selection mode.
//!
//! ```ignore
//! let table = cx.new(|cx| {
//!     TableView::new(cx)
//!         .columns(vec![
//!             Column::new("Name").id(1).text(|u: &User| u.name.clone().into()),
//!             Column::new("Age").id(2).width(80.0).align(Align::End),
//!         ])
//!         .rows(users)
//!         .selection_mode(SelectionMode::Multi)
//!         .fill(true)
//! });
//! ```

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::ops::Range;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    div, px, uniform_list, AnyElement, App, Bounds, Context, Div, DragMoveEvent, EntityId,
    EventEmitter, FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent, Pixels,
    ScrollStrategy, SharedString, Subscription, UniformListScrollHandle, WeakEntity, Window,
};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu};
use gpui_kit::component::ActiveTheme;

use super::Content;
use reactive_signals::reactive::Signal;

/// Events emitted by [`TableView`]. Row indices refer to the **source** rows,
/// not the current display order.
#[derive(Debug, Clone)]
pub enum TableViewEvent {
    /// The set of selected source rows changed (ascending indices).
    SelectionChanged(Vec<usize>),
    /// A row was activated by double-click or Enter.
    Activated(usize),
    /// The sort changed: `Some((column id, dir))`, or `None` when cleared.
    Sorted(Option<(u64, SortDir)>),
    /// A column was drag-resized to `width` (column identified by its
    /// [`Column::id`], which survives reordering).
    Resized(u64, f32),
    /// The user reordered columns by dragging headers.
    ColumnsReordered,
}

type Comparator<T> = Rc<dyn Fn(&T, &T) -> Ordering>;
type CellBuilder<T> = Rc<dyn Fn(&T, &mut Window, &mut App) -> AnyElement>;
type RowMenu = Rc<dyn Fn(PopupMenu, usize, &mut Window, &mut App) -> PopupMenu>;
type HeaderMenu = Rc<dyn Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu>;

enum CellContent<T> {
    Text(Rc<dyn Fn(&T) -> SharedString>),
    Element(CellBuilder<T>),
}

/// Horizontal alignment of a column's header and cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

/// One column of a [`TableView`]: header title, width policy, alignment,
/// optional sort comparator, and a cell renderer.
pub struct Column<T> {
    title: SharedString,
    /// Stable host identity: survives reordering, reported by
    /// [`TableViewEvent::Resized`]. Zero means "no identity".
    id: u64,
    width: Option<f32>,
    flex: f32,
    min_width: f32,
    align: Align,
    sort: Option<Comparator<T>>,
    content: Option<CellContent<T>>,
}

impl<T> Column<T> {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Column {
            title: title.into(),
            id: 0,
            width: None,
            flex: 1.0,
            min_width: 60.0,
            align: Align::Start,
            sort: None,
            content: None,
        }
    }

    /// Stable host-side identity for this column.
    pub fn id(mut self, id: u64) -> Self {
        self.id = id;
        self
    }

    /// The column's stable identity.
    pub fn identity(&self) -> u64 {
        self.id
    }

    /// The column's header caption.
    pub fn title(&self) -> SharedString {
        self.title.clone()
    }

    /// Fixed pixel width. Without it the column flexes (see [`Column::flex`]).
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Grow factor for flexing columns (default `1.0`).
    pub fn flex(mut self, flex: f32) -> Self {
        self.flex = flex;
        self
    }

    /// Lower width bound, honored by both flex sizing and drag-resizing
    /// (default `60.0`).
    pub fn min_width(mut self, min_width: f32) -> Self {
        self.min_width = min_width;
        self
    }

    /// Horizontal alignment of the header and cells (default [`Align::Start`]).
    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    /// Make the column sortable. A header click cycles ascending →
    /// descending → unsorted; the sort is a stable reorder of display
    /// indices and never mutates the rows.
    pub fn sortable_by(mut self, cmp: impl Fn(&T, &T) -> Ordering + 'static) -> Self {
        self.sort = Some(Rc::new(cmp));
        self
    }

    /// Custom cell renderer, re-invoked every frame so cells show live data.
    pub fn cell<E>(mut self, cell: impl Fn(&T, &mut Window, &mut App) -> E + 'static) -> Self
    where
        E: IntoElement,
    {
        self.content = Some(CellContent::Element(Rc::new(move |row, window, cx| {
            cell(row, window, cx).into_any_element()
        })));
        self
    }

    /// Text-cell convenience: the string truncates with an ellipsis when the
    /// column is too narrow.
    pub fn text(mut self, text: impl Fn(&T) -> SharedString + 'static) -> Self {
        self.content = Some(CellContent::Text(Rc::new(text)));
        self
    }
}

/// Row storage: an owned snapshot, or a live binding to a `Signal`.
enum Rows<T> {
    Owned(Rc<Vec<T>>),
    Bound(Signal<Vec<T>>),
}

impl<T> Clone for Rows<T> {
    fn clone(&self) -> Self {
        match self {
            Rows::Owned(rows) => Rows::Owned(rows.clone()),
            Rows::Bound(signal) => Rows::Bound(signal.clone()),
        }
    }
}

/// Drag payload for the header resize grips. `owner` scopes `on_drag_move` to
/// the table that started the drag — the listener fires for every active drag
/// of this type in the window, including other tables'.
struct ResizeDrag {
    owner: EntityId,
    column: usize,
    id: u64,
}

/// Drag payload for header reordering (see [`TableView::reorderable`]).
/// Drops are element-scoped (`.on_drop` on the target header), so unlike
/// [`ResizeDrag`] no owner id is needed.
struct HeaderDrag {
    column: usize,
}

/// Resolved width policy for one column.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ColWidth {
    Fixed(f32),
    Flex(f32, f32), // (grow factor, min width)
}

/// A rich data table. Create with
/// `cx.new(|cx| TableView::new(cx).columns(...).rows(...))`.
pub struct TableView<T: 'static> {
    columns: Vec<Column<T>>,
    rows: Rows<T>,
    focus: FocusHandle,
    selection_mode: SelectionMode,
    selection: SelectionState,
    /// The active sort, keyed by [`Column::id`] so it survives reordering
    /// and column-list rebuilds.
    sort: Option<(u64, SortDir)>,
    /// Source index of each visible row, in display order. Recomputed at the
    /// top of every render; listeners map display → source through it.
    display_order: Vec<usize>,
    /// Columns converted to fixed widths by drag-resizing, keyed by
    /// [`Column::id`] so widths travel with their column through reorders
    /// and rebuilds.
    resized: HashMap<u64, f32>,
    /// Header-cell bounds captured after prepaint, for resize math.
    header_bounds: Vec<Bounds<Pixels>>,
    /// The `bind_rows` observer; dropped (cancelled) by `set_rows`/rebinding.
    rows_sub: Option<Subscription>,
    striped: bool,
    highlight_on_hover: bool,
    with_border: bool,
    height: Option<f32>,
    fill: bool,
    reorderable: bool,
    row_menu: Option<RowMenu>,
    header_menu: Option<HeaderMenu>,
    empty: Option<Content>,
    scroll: UniformListScrollHandle,
}

impl<T: 'static> EventEmitter<TableViewEvent> for TableView<T> {}

impl<T: 'static> TableView<T> {
    pub fn new(cx: &mut Context<Self>) -> Self {
        TableView {
            columns: Vec::new(),
            rows: Rows::Owned(Rc::new(Vec::new())),
            focus: cx.focus_handle(),
            selection_mode: SelectionMode::None,
            selection: SelectionState::default(),
            sort: None,
            display_order: Vec::new(),
            resized: HashMap::new(),
            header_bounds: Vec::new(),
            rows_sub: None,
            striped: false,
            highlight_on_hover: false,
            with_border: false,
            height: None,
            fill: false,
            reorderable: false,
            row_menu: None,
            header_menu: None,
            empty: None,
            scroll: UniformListScrollHandle::new(),
        }
    }

    pub fn columns(mut self, columns: Vec<Column<T>>) -> Self {
        self.columns = columns;
        self
    }

    /// Provide the rows as an owned snapshot. Replace later with
    /// [`TableView::set_rows`].
    pub fn rows(mut self, rows: Vec<T>) -> Self {
        self.rows = Rows::Owned(Rc::new(rows));
        self
    }

    /// Bind the rows to a `Signal<Vec<T>>`: the table observes the signal
    /// (signal writes repaint it) and reads the rows at render, so it always
    /// shows the live value. Selection is pruned when rows disappear.
    pub fn bind_rows(mut self, signal: &Signal<Vec<T>>, cx: &mut Context<Self>) -> Self {
        self.rows = Rows::Bound(signal.clone());
        // Held, not detached: `set_rows` (or a rebind) drops the subscription,
        // so a stale observer never prunes against the old signal's length.
        self.rows_sub = Some(cx.observe(signal.entity(), |this, rows, cx| {
            let len = rows.read(cx).len();
            this.prune_selection(len, cx);
            cx.notify();
        }));
        self
    }

    pub fn selection_mode(mut self, mode: SelectionMode) -> Self {
        self.selection_mode = mode;
        self
    }

    pub fn striped(mut self, striped: bool) -> Self {
        self.striped = striped;
        self
    }

    pub fn highlight_on_hover(mut self, highlight: bool) -> Self {
        self.highlight_on_hover = highlight;
        self
    }

    pub fn with_border(mut self, with_border: bool) -> Self {
        self.with_border = with_border;
        self
    }

    /// Fix the body height (px). The body becomes a virtualized
    /// `uniform_list` scroll region — rows must share one height — and the
    /// header stays outside it, so it is sticky for free.
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// Let the body fill its parent instead of sizing to content or a fixed
    /// height (still virtualized). The table root itself fills too.
    pub fn fill(mut self, fill: bool) -> Self {
        self.fill = fill;
        self
    }

    /// Allow dragging headers onto each other to reorder columns. Widths
    /// travel with their column.
    pub fn reorderable(mut self, reorderable: bool) -> Self {
        self.reorderable = reorderable;
        self
    }

    /// Per-row context menu. `source` is the row's source index; right-
    /// clicking a row adopts it into the selection first (file-manager
    /// behavior), so menu commands can operate on the clicked row.
    pub fn row_menu(
        mut self,
        menu: impl Fn(PopupMenu, usize, &mut Window, &mut App) -> PopupMenu + 'static,
    ) -> Self {
        self.row_menu = Some(Rc::new(menu));
        self
    }

    /// Header context menu — the natural place for column show/hide toggles.
    pub fn header_menu(
        mut self,
        menu: impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + 'static,
    ) -> Self {
        self.header_menu = Some(Rc::new(menu));
        self
    }

    /// Rendered instead of the body when there are no rows.
    pub fn empty<E>(mut self, builder: impl Fn(&mut Window, &mut App) -> E + 'static) -> Self
    where
        E: IntoElement,
    {
        self.empty = Some(Box::new(move |window, cx| {
            builder(window, cx).into_any_element()
        }));
        self
    }

    // --- Entity methods ------------------------------------------------------

    /// Replace the columns (host-driven show/hide/rebuild). Per-column widths
    /// the host wants to keep must be carried in [`Column::width`].
    pub fn set_columns(&mut self, columns: Vec<Column<T>>, cx: &mut Context<Self>) {
        self.columns = columns;
        // Widths the host wants to keep travel in `Column::width`; leftover
        // drag widths are keyed by the old column set and would re-attach to
        // whatever column takes over the position if retained.
        self.resized.clear();
        self.sort = self.sort.filter(|&(id, _)| {
            self.columns
                .iter()
                .any(|column| column.id == id && column.sort.is_some())
        });
        cx.notify();
    }

    /// The current columns, in display order.
    pub fn column_defs(&self) -> &[Column<T>] {
        &self.columns
    }

    /// `(Column::id, effective width)` for every fixed-width column, in
    /// display order. Flex columns (no fixed width) are skipped.
    pub fn column_layout(&self) -> Vec<(u64, f32)> {
        self.columns
            .iter()
            .filter_map(|column| {
                let width = self
                    .resized
                    .get(&column.id)
                    .copied()
                    .or(column.width)
                    .map(|width| width.max(column.min_width))?;
                Some((column.id, width))
            })
            .collect()
    }

    /// Replace the rows with a new owned snapshot (drops any signal binding).
    pub fn set_rows(&mut self, rows: Vec<T>, cx: &mut Context<Self>) {
        let len = rows.len();
        self.rows = Rows::Owned(Rc::new(rows));
        self.rows_sub = None;
        self.prune_selection(len, cx);
        cx.notify();
    }

    /// The selected source-row indices, ascending.
    pub fn selected(&self) -> Vec<usize> {
        self.selection.selected()
    }

    /// The active sort, as `(column id, direction)`.
    pub fn sort_state(&self) -> Option<(u64, SortDir)> {
        self.sort
    }

    /// Select every row (`Multi` mode only). Counts the rows directly — the
    /// display order is only refreshed at render and may be stale here.
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.selection_mode, SelectionMode::Multi) {
            return;
        }
        let len = match &self.rows {
            Rows::Owned(rows) => rows.len(),
            Rows::Bound(signal) => signal.read(cx).len(),
        };
        let before = self.selection.selected();
        self.selection.select_all(len);
        let after = self.selection.selected();
        if before != after {
            cx.emit(TableViewEvent::SelectionChanged(after));
        }
        cx.notify();
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    // --- Internals -----------------------------------------------------------

    fn prune_selection(&mut self, len: usize, cx: &mut Context<Self>) {
        if self.selection.retain_below(len) {
            cx.emit(TableViewEvent::SelectionChanged(self.selection.selected()));
        }
    }

    /// The display order for this frame: a stable index sort when a sorted
    /// column is active, identity otherwise. Never touches the source rows.
    fn compute_order(&self, cx: &App) -> Vec<usize> {
        let sort = self.sort.and_then(|(id, dir)| {
            let column = self.columns.iter().find(|column| column.id == id)?;
            let cmp = column.sort.clone()?;
            Some((dir, cmp))
        });
        match &self.rows {
            Rows::Owned(rows) => order_of(rows, sort),
            Rows::Bound(signal) => order_of(signal.read(cx), sort),
        }
    }

    fn col_width(&self, ix: usize) -> ColWidth {
        let col = &self.columns[ix];
        if let Some(&w) = self.resized.get(&col.id) {
            ColWidth::Fixed(w.max(col.min_width))
        } else if let Some(w) = col.width {
            ColWidth::Fixed(w.max(col.min_width))
        } else {
            ColWidth::Flex(col.flex, col.min_width)
        }
    }

    fn toggle_sort(&mut self, column: usize, cx: &mut Context<Self>) {
        let Some(id) = self.columns.get(column).map(|c| c.id) else {
            return;
        };
        self.sort = cycle_sort(self.sort, id);
        cx.emit(TableViewEvent::Sorted(self.sort));
        cx.notify();
    }

    /// Move column `from` to the position `to` currently occupies. Dragged
    /// widths and the sort are keyed by column id, so nothing to remap.
    fn move_column(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if from == to {
            return;
        }
        let column = self.columns.remove(from);
        let to = if from < to { to - 1 } else { to };
        self.columns.insert(to.min(self.columns.len()), column);
        cx.notify();
    }

    /// Header-grip drags: the grip carries its column; the mouse's window x
    /// minus the header cell's left edge is the new fixed width. Stateless —
    /// no drag-start capture, so the width can never jump.
    fn on_resize_drag(
        &mut self,
        ev: &DragMoveEvent<ResizeDrag>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (owner, column, id) = {
            let drag = ev.drag(cx);
            (drag.owner, drag.column, drag.id)
        };
        if owner != cx.entity_id() {
            return;
        }
        let Some(bounds) = self.header_bounds.get(column) else {
            return;
        };
        let min = self.columns.get(column).map(|c| c.min_width).unwrap_or(0.0);
        let width = f32::from(ev.event.position.x - bounds.left()).max(min);
        self.resized.insert(id, width);
        cx.emit(TableViewEvent::Resized(id, width));
        cx.notify();
    }

    fn row_mouse_down(
        &mut self,
        display: usize,
        toggle: bool,
        range: bool,
        click_count: usize,
        cx: &mut Context<Self>,
    ) {
        if click_count == 2 {
            if let Some(&source) = self.display_order.get(display) {
                cx.emit(TableViewEvent::Activated(source));
            }
            return;
        }
        if matches!(self.selection_mode, SelectionMode::None) {
            return;
        }
        let before = self.selection.selected();
        self.selection
            .click(self.selection_mode, &self.display_order, display, toggle, range);
        let after = self.selection.selected();
        if before != after {
            cx.emit(TableViewEvent::SelectionChanged(after));
        }
        cx.notify();
    }

    /// Right-click on a row: adopt it into the selection when it is not part
    /// of it (native file-manager behavior), then the row menu opens.
    fn row_right_click(&mut self, display: usize, cx: &mut Context<Self>) {
        if matches!(self.selection_mode, SelectionMode::None) {
            return;
        }
        let Some(&source) = self.display_order.get(display) else {
            return;
        };
        if self.selection.is_selected(source) {
            return;
        }
        let before = self.selection.selected();
        self.selection
            .click(self.selection_mode, &self.display_order, display, false, false);
        let after = self.selection.selected();
        if before != after {
            cx.emit(TableViewEvent::SelectionChanged(after));
        }
        cx.notify();
    }

    /// Arrow keys: only consume the key when the cursor actually moves —
    /// `SelectionMode::None` (the default) and empty tables are no-ops, and
    /// the host should keep receiving those arrows.
    fn step(&mut self, delta: isize, extend: bool, cx: &mut Context<Self>) {
        let before = self.selection.selected();
        let Some(display) = self
            .selection
            .step(self.selection_mode, &self.display_order, delta, extend)
        else {
            return;
        };
        if self.height.is_some() || self.fill {
            self.scroll.scroll_to_item(display, ScrollStrategy::Center);
        }
        let after = self.selection.selected();
        if before != after {
            cx.emit(TableViewEvent::SelectionChanged(after));
        }
        cx.notify();
        cx.stop_propagation();
    }

    fn on_key(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let shift = ev.keystroke.modifiers.shift;
        match ev.keystroke.key.as_str() {
            "up" => self.step(-1, shift, cx),
            "down" => self.step(1, shift, cx),
            "enter" => {
                if self.activate_cursor(cx) {
                    cx.stop_propagation();
                }
            }
            "escape" => {
                if self.clear_selection(cx) {
                    cx.stop_propagation();
                }
            }
            "a" if ev.keystroke.modifiers.control => {
                self.select_all(cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    /// Drops the whole selection (host-driven navigation etc.). Returns
    /// whether anything was selected, so callers can skip redundant work.
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) -> bool {
        let cleared = self.selection.clear();
        if cleared {
            cx.emit(TableViewEvent::SelectionChanged(Vec::new()));
            cx.notify();
        }
        cleared
    }

    /// Activates the keyboard cursor (or the sole selection), like Enter.
    /// Returns whether a row was activated, so callers can claim the key.
    pub fn activate_cursor(&mut self, cx: &mut Context<Self>) -> bool {
        let target = self.selection.cursor().or_else(|| {
            let selected = self.selection.selected();
            (selected.len() == 1).then(|| selected[0])
        });
        match target {
            Some(source) => {
                cx.emit(TableViewEvent::Activated(source));
                true
            }
            None => false,
        }
    }

    /// Keyboard facade for hosts holding focus above the table: moves the
    /// cursor like the Up/Down keys (`extend` = shift). Consumed moves stop
    /// propagation and scroll the cursor into view.
    pub fn move_cursor(&mut self, delta: isize, extend: bool, cx: &mut Context<Self>) {
        self.step(delta, extend, cx);
    }

    // --- Rendering -----------------------------------------------------------

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let line = theme.border;
        let primary = theme.primary;

        let owner = cx.entity_id();
        let view = cx.weak_entity();
        let reorderable = self.reorderable;
        let mut row = div()
            .flex()
            .w_full()
            .border_b_1()
            .border_color(line)
            // The header cells' painted bounds, for resize math: children map
            // 1:1 to columns (grips are nested inside the cells).
            .on_children_prepainted(move |bounds, _window, app| {
                view.update(app, |this, _| this.header_bounds = bounds).ok();
            })
            .id("guise-tableview-header");
        let header_menu = self.header_menu.clone();

        for ix in 0..self.columns.len() {
            let col = &self.columns[ix];
            let sortable = col.sort.is_some();
            let sort_dir = self.sort.filter(|&(id, _)| id == col.id).map(|(_, d)| d);
            let title = col.title.clone();

            let mut cell = div()
                .relative()
                .flex()
                .items_center()
                .gap(px(6.0))
                .px(px(8.0))
                .py(px(5.0))
                .text_xs()
                .text_color(muted);
            cell = sized(cell, self.col_width(ix));
            cell = aligned(cell, col.align);
            cell = cell.child(div().min_w(px(0.0)).truncate().child(col.title.clone()));
            if let Some(dir) = sort_dir {
                cell = cell.child(div().text_xs().text_color(primary).child(
                    SharedString::new_static(match dir {
                        SortDir::Asc => "\u{25b2}",
                        SortDir::Desc => "\u{25bc}",
                    }),
                ));
            }

            // Drag handle on the cell's right edge. The last column's grip
            // sits fully inside the cell: at the window's right edge (e.g.
            // maximized) the outer pixels belong to the OS edge zone and
            // would never receive the press.
            let grip = div()
                .id(("guise-tableview-grip", ix))
                .absolute()
                .top(px(0.0))
                .bottom(px(0.0))
                .right(if ix + 1 == self.columns.len() {
                    px(6.0)
                } else {
                    px(-3.0)
                })
                .w(px(6.0))
                .cursor_col_resize()
                .hover(move |s| s.bg(primary.alpha(0.6)))
                .on_drag(
                    ResizeDrag {
                        owner,
                        column: ix,
                        id: col.id,
                    },
                    |_, _, _, cx| cx.new(|_| EmptyView),
                )
                // Don't let a stray click on the grip toggle the sort.
                .on_click(|_ev, _window, cx| cx.stop_propagation());
            cell = cell.child(grip);

            let mut cell = if sortable {
                cell.id(("guise-tableview-head", ix))
                    .cursor_pointer()
                    .hover(move |s| s.text_color(theme.foreground))
                    .on_click(cx.listener(move |this, _ev, _window, cx| {
                        this.toggle_sort(ix, cx);
                    }))
            } else {
                cell.id(("guise-tableview-head", ix))
            };

            if reorderable {
                cell = cell
                    .on_drag(
                        HeaderDrag { column: ix },
                        move |_, _offset, _window, cx| {
                            cx.new(|_| HeaderGhost(title.clone()))
                        },
                    )
                    .drag_over::<HeaderDrag>(move |cell, _, _, _| {
                        cell.bg(primary.alpha(0.15))
                    })
                    .on_drop(cx.listener(
                        move |this, drag: &HeaderDrag, _window, cx| {
                            this.move_column(drag.column, ix, cx);
                            cx.emit(TableViewEvent::ColumnsReordered);
                        },
                    ));
            }
            let cell: AnyElement = cell.into_any_element();
            row = row.child(cell);
        }

        if let Some(menu) = header_menu {
            return row
                .context_menu(move |popup, window, cx| (menu)(popup, window, cx))
                .into_any_element();
        }
        row.into_any_element()
    }

    /// Rows for the display range. For signal-bound rows the backing entity is
    /// leased with `Entity::update`, which yields `&Vec<T>` *and* a usable
    /// `&mut App` at once — cell closures need both.
    fn render_rows(
        &self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let view = cx.weak_entity();
        match self.rows.clone() {
            Rows::Owned(rows) => range
                .filter_map(|display| {
                    let source = *self.display_order.get(display)?;
                    let row = rows.get(source)?;
                    Some(self.render_row(&view, display, source, row, window, cx))
                })
                .collect(),
            Rows::Bound(signal) => signal.entity().update(cx, |rows, cx| {
                range
                    .filter_map(|display| {
                        let source = *self.display_order.get(display)?;
                        let row = rows.get(source)?;
                        Some(self.render_row(&view, display, source, row, window, cx))
                    })
                    .collect()
            }),
        }
    }

    fn render_row(
        &self,
        view: &WeakEntity<Self>,
        display: usize,
        source: usize,
        row: &T,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let theme = cx.theme();
        let text = theme.foreground;
        let line = theme.border;
        let stripe = theme.secondary;
        let selected_bg = theme.primary.alpha(0.18);

        let is_selected = self.selection.is_selected(source);

        let mut tr = div()
            .id(("guise-tableview-row", display))
            .flex()
            .w_full()
            .border_b_1()
            .border_color(line)
            .text_xs()
            .text_color(text);

        if is_selected {
            tr = tr.bg(selected_bg);
        } else if self.striped && display % 2 == 1 {
            tr = tr.bg(stripe);
        }
        if self.highlight_on_hover && !is_selected {
            tr = tr.hover(move |s| s.bg(theme.secondary));
        }

        for (ix, col) in self.columns.iter().enumerate() {
            let mut cell = div()
                .flex()
                .items_center()
                .px(px(8.0))
                .py(px(4.0))
                .overflow_hidden();
            cell = sized(cell, self.col_width(ix));
            cell = aligned(cell, col.align);
            cell = match &col.content {
                Some(CellContent::Text(to_text)) => {
                    cell.child(div().min_w(px(0.0)).truncate().child(to_text(row)))
                }
                Some(CellContent::Element(build)) => cell.child(build(row, window, cx)),
                None => cell,
            };
            tr = tr.child(cell);
        }

        tr = tr.on_mouse_down(
            MouseButton::Left,
            {
                let view = view.clone();
                move |ev: &MouseDownEvent, window, app| {
                    let toggle = ev.modifiers.control;
                    let range = ev.modifiers.shift;
                    let count = ev.click_count;
                    view.update(app, |this, cx| {
                        window.focus(&this.focus, cx);
                        this.row_mouse_down(display, toggle, range, count, cx);
                    })
                    .ok();
                }
            },
        );

        // Right-click: adopt the row into the selection first, then let the
        // host's menu (if any) open over it.
        if self.row_menu.is_some() {
            let view = view.clone();
            tr = tr.on_mouse_down(
                MouseButton::Right,
                move |_: &MouseDownEvent, _window, app| {
                    view.update(app, |this, cx| this.row_right_click(display, cx))
                        .ok();
                },
            );
        }

        match self.row_menu.clone() {
            Some(menu) => tr
                .context_menu(move |popup, window, cx| (menu)(popup, source, window, cx))
                .into_any_element(),
            None => tr.into_any_element(),
        }
    }
}

impl<T: 'static> Render for TableView<T> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.display_order = self.compute_order(cx);
        let count = self.display_order.len();

        let line = cx.theme().border;
        let dimmed = cx.theme().muted_foreground;

        let header = self.render_header(cx);

        let body: AnyElement = if count == 0 {
            match &self.empty {
                Some(builder) => builder(window, cx),
                None => div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .py(px(24.0))
                    .text_xs()
                    .text_color(dimmed)
                    .child(SharedString::new_static("No data"))
                    .into_any_element(),
            }
        } else if self.fill {
            uniform_list(
                "guise-tableview-body",
                count,
                cx.processor(|this, range: Range<usize>, window, cx| {
                    this.render_rows(range, window, cx)
                }),
            )
            .flex_1()
            .min_h_0()
            .w_full()
            .track_scroll(&self.scroll.clone())
            .into_any_element()
        } else if let Some(height) = self.height {
            uniform_list(
                "guise-tableview-body",
                count,
                cx.processor(|this, range: Range<usize>, window, cx| {
                    this.render_rows(range, window, cx)
                }),
            )
            .h(px(height))
            .w_full()
            .track_scroll(&self.scroll.clone())
            .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .w_full()
                .children(self.render_rows(0..count, window, cx))
                .into_any_element()
        };

        let mut table = div()
            .id("guise-tableview")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_drag_move(cx.listener(Self::on_resize_drag))
            .flex()
            .flex_col()
            .child(header)
            .child(body);
        if self.fill {
            table = table.size_full();
        }
        if self.with_border {
            table = table
                .border_1()
                .border_color(line)
                .rounded(px(6.0))
                .overflow_hidden();
        }
        table
    }
}

/// The display order given optional sorting: pure index math from `state`.
fn order_of<T>(rows: &[T], sort: Option<(SortDir, Comparator<T>)>) -> Vec<usize> {
    match sort {
        Some((dir, cmp)) => sorted_order(rows, dir, &*cmp),
        None => identity_order(rows.len()),
    }
}

/// Apply a column's width policy. Fixed columns never flex; flexing columns
/// share leftover space by grow factor from a zero basis.
fn sized(cell: Div, width: ColWidth) -> Div {
    match width {
        ColWidth::Fixed(w) => cell.w(px(w)).flex_none(),
        ColWidth::Flex(factor, min) => cell
            .flex_grow(factor)
            .flex_shrink(1.0)
            .flex_basis(px(0.0))
            .min_w(px(min)),
    }
}

/// Horizontal alignment of a cell's content.
fn aligned(cell: Div, align: Align) -> Div {
    match align {
        Align::Start => cell.justify_start(),
        Align::Center => cell.justify_center(),
        Align::End => cell.justify_end(),
    }
}

/// Sort direction of a table column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

/// How rows respond to clicks and arrow keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionMode {
    /// Rows are not selectable.
    #[default]
    None,
    /// At most one row selected at a time.
    Single,
    /// Many rows: mod-click toggles, shift-click selects a range.
    Multi,
}

/// Header-click cycling: none → asc → desc → none on the same column; a click
/// on a different column starts fresh at ascending.
pub fn cycle_sort(current: Option<(u64, SortDir)>, column: u64) -> Option<(u64, SortDir)> {
    match current {
        Some((col, SortDir::Asc)) if col == column => Some((column, SortDir::Desc)),
        Some((col, SortDir::Desc)) if col == column => None,
        _ => Some((column, SortDir::Asc)),
    }
}

/// The unsorted display order: `0..len`.
pub fn identity_order(len: usize) -> Vec<usize> {
    (0..len).collect()
}

/// A stable sort of row indices by `cmp`; the source slice is never mutated.
/// `Desc` flips the comparator arguments (rather than reversing the result),
/// so equal rows keep their source order in both directions.
pub fn sorted_order<T>(rows: &[T], dir: SortDir, cmp: &dyn Fn(&T, &T) -> Ordering) -> Vec<usize> {
    let mut order = identity_order(rows.len());
    order.sort_by(|&a, &b| match dir {
        SortDir::Asc => cmp(&rows[a], &rows[b]),
        SortDir::Desc => cmp(&rows[b], &rows[a]),
    });
    order
}

/// Row selection over **source** indices. The shift anchor and keyboard cursor
/// are also source indices, mapped through the display order at use, so all
/// three survive resorting.
#[derive(Debug, Clone, Default)]
pub struct SelectionState {
    selected: BTreeSet<usize>,
    anchor: Option<usize>,
    cursor: Option<usize>,
}

impl SelectionState {
    /// The selected source indices, ascending.
    pub fn selected(&self) -> Vec<usize> {
        self.selected.iter().copied().collect()
    }

    pub fn is_selected(&self, source: usize) -> bool {
        self.selected.contains(&source)
    }

    /// The keyboard cursor (a source index), if any.
    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    /// Select every index below `len`. Returns whether the set changed.
    pub fn select_all(&mut self, len: usize) -> bool {
        let before = self.selected.len();
        self.selected = (0..len).collect();
        self.selected.len() != before
    }

    /// Drop everything. Returns whether the selected set changed.
    pub fn clear(&mut self) -> bool {
        self.anchor = None;
        self.cursor = None;
        if self.selected.is_empty() {
            return false;
        }
        self.selected.clear();
        true
    }

    /// Prune indices that fell off the end after a row-count change. Returns
    /// whether the selected set changed.
    pub fn retain_below(&mut self, len: usize) -> bool {
        let before = self.selected.len();
        self.selected.retain(|&s| s < len);
        self.anchor = self.anchor.filter(|&s| s < len);
        self.cursor = self.cursor.filter(|&s| s < len);
        self.selected.len() != before
    }

    /// A mouse click on the row at `display` position. `toggle` is mod-click,
    /// `range` is shift-click (range wins when both are held).
    pub fn click(
        &mut self,
        mode: SelectionMode,
        order: &[usize],
        display: usize,
        toggle: bool,
        range: bool,
    ) {
        let Some(&source) = order.get(display) else {
            return;
        };
        match mode {
            SelectionMode::None => {}
            SelectionMode::Single => {
                if toggle && self.selected.contains(&source) {
                    self.selected.clear();
                } else {
                    self.selected.clear();
                    self.selected.insert(source);
                }
                self.anchor = Some(source);
                self.cursor = Some(source);
            }
            SelectionMode::Multi => {
                if range {
                    let from = match self.anchor.and_then(|a| position_of(order, a)) {
                        Some(pos) => pos,
                        // No live anchor (first interaction, or its row is
                        // gone): this click starts and anchors the range.
                        None => {
                            self.anchor = Some(source);
                            display
                        }
                    };
                    self.select_span(order, from, display);
                } else if toggle {
                    if !self.selected.remove(&source) {
                        self.selected.insert(source);
                    }
                    self.anchor = Some(source);
                } else {
                    self.selected.clear();
                    self.selected.insert(source);
                    self.anchor = Some(source);
                }
                self.cursor = Some(source);
            }
        }
    }

    /// Move the cursor by `delta` display positions (arrow keys), selecting
    /// the row it lands on. `extend` (shift) grows the range from the anchor
    /// in `Multi` mode. Returns the new cursor's display position so the
    /// caller can scroll it into view.
    pub fn step(
        &mut self,
        mode: SelectionMode,
        order: &[usize],
        delta: isize,
        extend: bool,
    ) -> Option<usize> {
        if matches!(mode, SelectionMode::None) || order.is_empty() {
            return None;
        }
        let last = order.len() - 1;
        let display = match self.cursor.and_then(|c| position_of(order, c)) {
            Some(pos) => (pos as isize + delta).clamp(0, last as isize) as usize,
            // Nothing focused yet: Down enters at the top, Up at the bottom.
            None if delta < 0 => last,
            None => 0,
        };
        let source = order[display];
        if extend && matches!(mode, SelectionMode::Multi) {
            let from = match self.anchor.and_then(|a| position_of(order, a)) {
                Some(pos) => pos,
                // No live anchor yet: the row this step lands on anchors the
                // range, so further shift-steps grow from it.
                None => {
                    self.anchor = Some(source);
                    display
                }
            };
            self.select_span(order, from, display);
        } else {
            self.selected.clear();
            self.selected.insert(source);
            self.anchor = Some(source);
        }
        self.cursor = Some(source);
        Some(display)
    }

    /// Select exactly the display range `a..=b` (either direction).
    fn select_span(&mut self, order: &[usize], a: usize, b: usize) {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        self.selected.clear();
        self.selected.extend(order[lo..=hi].iter().copied());
    }
}

/// Where `source` currently sits in the display order.
fn position_of(order: &[usize], source: usize) -> Option<usize> {
    order.iter().position(|&s| s == source)
}

/// Invisible drag ghost for resize drags (gpui requires a rendered preview).
struct EmptyView;

impl Render for EmptyView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Ghost shown while a header is dragged to a new position.
struct HeaderGhost(SharedString);

impl Render for HeaderGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_sm()
            .text_xs()
            .bg(cx.theme().secondary)
            .text_color(cx.theme().foreground)
            .child(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_cycles_asc_desc_none() {
        let s1 = cycle_sort(None, 2);
        assert_eq!(s1, Some((2, SortDir::Asc)));
        let s2 = cycle_sort(s1, 2);
        assert_eq!(s2, Some((2, SortDir::Desc)));
        assert_eq!(cycle_sort(s2, 2), None);
    }

    #[test]
    fn sorted_order_never_touches_the_source() {
        let rows = vec![3, 1, 2];
        let order = sorted_order(&rows, SortDir::Asc, &|a, b| a.cmp(b));
        assert_eq!(order, vec![1, 2, 0]);
        assert_eq!(rows, vec![3, 1, 2]);
    }

    #[test]
    fn sorted_order_is_stable_in_both_directions() {
        // Equal keys (by first tuple field) must keep source order.
        let rows = vec![(1, "a"), (0, "b"), (1, "c"), (0, "d")];
        let cmp = |a: &(i32, &str), b: &(i32, &str)| a.0.cmp(&b.0);
        assert_eq!(sorted_order(&rows, SortDir::Asc, &cmp), vec![1, 3, 0, 2]);
        assert_eq!(sorted_order(&rows, SortDir::Desc, &cmp), vec![0, 2, 1, 3]);
    }

    #[test]
    fn multi_mode_mod_click_toggles() {
        let order = identity_order(4);
        let mut sel = SelectionState::default();
        sel.click(SelectionMode::Multi, &order, 0, false, false);
        sel.click(SelectionMode::Multi, &order, 2, true, false);
        assert_eq!(sel.selected(), vec![0, 2]);
        sel.click(SelectionMode::Multi, &order, 0, true, false);
        assert_eq!(sel.selected(), vec![2]);
    }

    #[test]
    fn multi_mode_shift_click_selects_a_range_from_the_anchor() {
        let order = identity_order(6);
        let mut sel = SelectionState::default();
        sel.click(SelectionMode::Multi, &order, 1, false, false);
        sel.click(SelectionMode::Multi, &order, 4, false, true);
        assert_eq!(sel.selected(), vec![1, 2, 3, 4]);
        // Shift again re-ranges from the same anchor, upward this time.
        sel.click(SelectionMode::Multi, &order, 0, false, true);
        assert_eq!(sel.selected(), vec![0, 1]);
    }

    #[test]
    fn selection_survives_resorting() {
        let mut sel = SelectionState::default();
        sel.click(SelectionMode::Single, &identity_order(4), 2, false, false);
        assert_eq!(sel.selected(), vec![2]);
        // Order flips; source 2 is still the selected row.
        assert!(sel.is_selected(2));
        let order = vec![3, 2, 1, 0]; // source 2 is at display 1
        let display = sel.step(SelectionMode::Single, &order, 1, false);
        assert_eq!(display, Some(2));
        assert_eq!(sel.selected(), vec![1]);
    }

    #[test]
    fn select_all_and_clear_report_changes() {
        let mut sel = SelectionState::default();
        assert!(!sel.select_all(0));
        assert!(sel.select_all(3));
        assert_eq!(sel.selected(), vec![0, 1, 2]);
        assert!(!sel.select_all(3));
        assert!(sel.clear());
        assert!(sel.selected().is_empty());
    }

    #[test]
    fn clear_and_retain_report_changes() {
        let order = identity_order(4);
        let mut sel = SelectionState::default();
        assert!(!sel.clear());
        sel.click(SelectionMode::Multi, &order, 1, false, false);
        sel.click(SelectionMode::Multi, &order, 3, true, false);
        assert!(sel.retain_below(2)); // drops source 3
        assert_eq!(sel.selected(), vec![1]);
        assert!(!sel.retain_below(2));
        assert!(sel.clear());
        assert_eq!(sel.cursor(), None);
    }

    #[gpui::test]
    fn set_columns_drops_dragged_widths_and_stale_sorts(cx: &mut gpui::TestAppContext) {
        let column = |id: u64, sortable: bool| {
            let base = Column::<()>::new(format!("col-{id}")).id(id).width(80.0);
            if sortable {
                base.sortable_by(|_: &(), _: &()| Ordering::Equal)
            } else {
                base
            }
        };
        let entity = cx.new(|cx| {
            TableView::<()>::new(cx).columns(vec![column(1, true), column(2, true)])
        });
        entity.update(cx, |table, cx| {
            table.resized.insert(1, 250.0);
            table.sort = Some((1, SortDir::Asc));

            // Rebuild keeping both columns: the sort (keyed by id) survives,
            // the dragged width does not — it must travel via `Column::width`.
            table.set_columns(vec![column(1, true), column(2, true)], cx);
            assert!(table.resized.is_empty(), "stale drag widths are cleared");
            assert_eq!(table.sort, Some((1, SortDir::Asc)));

            // Hide the sorted column: the sort drops instead of sliding onto
            // whichever column takes over the position.
            table.sort = Some((1, SortDir::Asc));
            table.set_columns(vec![column(2, true)], cx);
            assert_eq!(table.sort, None);
        });
    }

    #[gpui::test]
    fn moved_columns_keep_dragged_widths_by_id(cx: &mut gpui::TestAppContext) {
        let entity = cx.new(|cx| {
            TableView::<()>::new(cx).columns(vec![
                Column::<()>::new("A").id(1).width(100.0),
                Column::<()>::new("B").id(2).width(80.0),
            ])
        });
        entity.update(cx, |table, cx| {
            table.resized.insert(1, 250.0);
            // Drag B (display 1) onto A (display 0).
            table.move_column(1, 0, cx);
            assert_eq!(
                table.columns.iter().map(|c| c.id).collect::<Vec<_>>(),
                vec![2, 1]
            );
            // Widths are keyed by id, so the move itself remaps nothing:
            // A keeps its dragged width, B keeps its default.
            assert_eq!(table.resized.get(&1), Some(&250.0));
            assert_eq!(table.col_width(0), ColWidth::Fixed(80.0));
            assert_eq!(table.col_width(1), ColWidth::Fixed(250.0));
        });
    }
}
