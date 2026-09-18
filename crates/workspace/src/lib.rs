//! Workspace layer: the GPUI view tree built on `gpui-kit` docks.
//!
//! [`multi_workspace::MultiWorkspace`] hosts one [`archive_workspace::ArchiveWorkspace`]
//! per tab — each wrapping a 7zFM-style [`explorer`](bit7z_explorer) file table
//! over an editing session — with the staging strip, progress panel, diff panel,
//! settings panel and devtools panel as dockable `Panel`s.

pub mod multi_workspace;
pub mod constants;
pub mod archive_workspace;
pub mod diff_panel;
pub mod progress_panel;
pub mod devtools_panel;
pub mod settings_panel;
pub mod globals;
pub(crate) mod tab_bar;
pub mod panels;

use gpui::{App, Global};
use std::path::PathBuf;

/// Archives requested for opening before any window exists (CLI argv paths).
/// The window root drains this when it is created; paths that fail to open
/// stay the caller's problem to report.
#[derive(Debug, Default)]
pub struct PendingOpen(pub Vec<PathBuf>);

impl Global for PendingOpen {}

/// Takes the pending open list, leaving it empty.
pub fn take_pending_open(cx: &mut App) -> Vec<PathBuf> {
    std::mem::take(&mut cx.global_mut::<PendingOpen>().0)
}
