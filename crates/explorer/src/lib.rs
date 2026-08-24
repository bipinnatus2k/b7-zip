//! Archive explorer facade.
//!
//! The explorer is split into:
//! * `explorer-model` — pure row/path/sort/format logic (no GUI)
//! * `explorer-view`  — gpui/guise table, navigation, keyboard behavior
//!
//! Existing consumers keep importing `bit7z_explorer::...`.

pub use explorer_model::{
    EntryRow, attr_string, collect_indices, crc_string, format_size, is_archive_name, natural_cmp,
    row_of, rows_for_path,
};
pub use explorer_view::{ArchiveExplorer, ExplorerCommand, ExplorerEvent};
