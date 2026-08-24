//! Archive explorer view: the gpui/guise table and keyboard behavior built
//! on the pure `explorer_model` crate.

use explorer_model::{
    EntryRow, collect_indices, crc_string, attr_string, format_size, is_archive_name, natural_cmp,
    rows_for_path,
};
use gpui::{div, px, App, AppContext, Context, Entity, EventEmitter, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render, SharedString, Styled, Window};
use guise::prelude::*;
use session::ArchiveSession;
use std::sync::{Arc, Mutex};

/// Events forwarded to the host.
#[derive(Debug, Clone)]
pub enum ExplorerEvent {
    Activated(usize),
    SelectionChanged(Vec<usize>),
    Command(ExplorerCommand),
}

/// Extra keyboard commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerCommand {
    NavigateUp,
    Delete,
    Rename,
}

/// A browsable archive explorer view.
pub struct ArchiveExplorer {
    session: Arc<Mutex<ArchiveSession>>,
    current_path: String,
    rows: Vec<EntryRow>,
    table: Entity<TableView<EntryRow>>,
}

impl EventEmitter<ExplorerEvent> for ArchiveExplorer {}

impl ArchiveExplorer {
    pub fn new(session: Arc<Mutex<ArchiveSession>>, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            let table = cx.new(|cx| {
                TableView::new(cx)
                    .columns(vec![
                        Column::new("Name")
                            .min_width(160.0)
                            .cell(|row: &EntryRow, _window, cx| name_cell(row, cx))
                            .sortable_by(|a: &EntryRow, b: &EntryRow| {
                                natural_cmp(&a.name, &b.name)
                            }),
                        Column::new("Size")
                            .width(88.0)
                            .align(Align::End)
                            .text(|row: &EntryRow| SharedString::from(format_size(row.size)))
                            .sortable_by(|a: &EntryRow, b: &EntryRow| a.size.cmp(&b.size)),
                        Column::new("Packed")
                            .width(88.0)
                            .align(Align::End)
                            .text(|row: &EntryRow| SharedString::from(format_size(row.packed)))
                            .sortable_by(|a: &EntryRow, b: &EntryRow| a.packed.cmp(&b.packed)),
                        Column::new("Modified")
                            .width(140.0)
                            .text(|row: &EntryRow| SharedString::from(row.modified.clone()))
                            .sortable_by(|a: &EntryRow, b: &EntryRow| a.modified.cmp(&b.modified)),
                        Column::new("Attributes")
                            .width(92.0)
                            .align(Align::Center)
                            .text(|row: &EntryRow| SharedString::from(attr_string(row))),
                        Column::new("CRC")
                            .width(80.0)
                            .align(Align::End)
                            .text(|row: &EntryRow| SharedString::from(crc_string(row)))
                            .sortable_by(|a: &EntryRow, b: &EntryRow| a.crc.cmp(&b.crc)),
                        Column::new("Method")
                            .width(84.0)
                            .text(|row: &EntryRow| SharedString::from(row.method.clone())),
                    ])
                    .selection_mode(SelectionMode::Multi)
                    .striped(true)
                    .highlight_on_hover(true)
                    .height(320.0)
            });
            let mut this = Self {
                session,
                current_path: String::new(),
                rows: Vec::new(),
                table,
            };
            this.refresh(cx);
            cx.subscribe(
                &this.table,
                |this, _table, event: &TableViewEvent, cx| match event {
                    TableViewEvent::Activated(row) => {
                        let Some(entry) = this.rows.get(*row).cloned() else {
                            return;
                        };
                        if entry.is_directory {
                            this.navigate(&entry.path, cx);
                        } else {
                            cx.emit(ExplorerEvent::Activated(*row));
                        }
                    }
                    TableViewEvent::SelectionChanged(rows) => {
                        cx.emit(ExplorerEvent::SelectionChanged(rows.clone()));
                    }
                    TableViewEvent::Sorted(_) => {}
                },
            )
            .detach();
            this
        })
    }

    pub fn set_session(&mut self, session: Arc<Mutex<ArchiveSession>>, cx: &mut Context<Self>) {
        self.session = session;
        self.current_path.clear();
        self.refresh(cx);
    }

    pub fn navigate(&mut self, path: &str, cx: &mut Context<Self>) {
        let session = self.session.lock().expect("session lock poisoned");
        if session.overlay().working().resolve_path(path).is_some() {
            self.current_path = path.to_string();
            drop(session);
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

    pub fn table(&self) -> &Entity<TableView<EntryRow>> {
        &self.table
    }

    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.table.read(cx).focus_handle()
    }

    pub fn rows(&self) -> &[EntryRow] {
        &self.rows
    }

    pub fn selected_rows(&self, cx: &App) -> Vec<EntryRow> {
        self.table
            .read(cx)
            .selected()
            .into_iter()
            .filter_map(|ix| self.rows.get(ix).cloned())
            .collect()
    }

    pub fn target_indices(&self, cx: &App) -> Vec<u32> {
        let selected = self.table.read(cx).selected();
        let rows: Vec<&EntryRow> = if selected.is_empty() {
            self.rows.iter().collect()
        } else {
            selected
                .iter()
                .filter_map(|ix| self.rows.get(*ix))
                .collect()
        };
        let session = self.session.lock().expect("session lock poisoned");
        let tree = session.overlay().working();
        let mut indices = Vec::new();
        for row in rows {
            collect_indices(tree, row.id, &mut indices);
        }
        indices.sort_unstable();
        indices.dedup();
        indices
    }

    pub fn focused_row(&self, cx: &App) -> Option<EntryRow> {
        let selected = self.table.read(cx).selected();
        let ix = if selected.len() == 1 {
            selected[0]
        } else {
            return None;
        };
        self.rows.get(ix).cloned()
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.rows = rows_for_path(&self.session, &self.current_path);
        self.table
            .update(cx, |table, cx| table.set_rows(self.rows.clone(), cx));
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let command = match key {
            "backspace" => Some(ExplorerCommand::NavigateUp),
            "delete" => Some(ExplorerCommand::Delete),
            "f2" => Some(ExplorerCommand::Rename),
            _ => None,
        };
        if let Some(command) = command {
            cx.emit(ExplorerEvent::Command(command));
            cx.stop_propagation();
            return;
        }
        if event.keystroke.modifiers.platform && key == "a" {
            self.table.update(cx, |table, cx| table.select_all(cx));
            cx.stop_propagation();
        }
    }
}

impl Render for ArchiveExplorer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("archive-explorer")
            .size_full()
            .flex()
            .flex_col()
            .on_key_down(cx.listener(Self::on_key))
            .child(self.table.clone())
    }
}

fn name_cell(row: &EntryRow, cx: &App) -> gpui::Div {
    let t = cx.global::<Theme>();
    let icon = if row.is_directory {
        IconName::Folder
    } else if is_archive_name(&row.name) {
        IconName::FileArchive
    } else {
        IconName::File
    };
    let tint = if row.is_directory {
        ColorName::Blue
    } else if is_archive_name(&row.name) {
        ColorName::Grape
    } else {
        ColorName::Gray
    };
    let mut el = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .min_w(px(0.0))
        .child(Icon::new(icon).size(Size::Sm).color(tint));
    el = el.child(
        div()
            .min_w(px(0.0))
            .truncate()
            .text_color(t.text().hsla())
            .child(SharedString::from(row.name.clone())),
    );
    if row.is_encrypted {
        el = el.child(
            Icon::new(IconName::Lock)
                .size(Size::Xs)
                .color(ColorName::Yellow),
        );
    }
    el
}
