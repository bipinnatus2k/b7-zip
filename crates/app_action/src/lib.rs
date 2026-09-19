//! Application-level actions for Bit7zFM.
//!
//! Actions defined here are the single vocabulary shared by keybindings,
//! menus, and the tab bar. Payload actions carry their data as tuple fields;
//! unit actions go through [`actions!`].

use gpui::{Action, actions};
use schemars::JsonSchema;
use serde::Deserialize;
use std::path::PathBuf;

// If nothing references this crate the linker may drop the action
// registrations entirely; calling `init` from main keeps them alive.
pub fn init() {}

/// Opens an archive in a new workspace tab.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema, Action)]
#[action(namespace = bit7zfm, no_json)]
pub struct OpenArchive(pub PathBuf);

actions!(
    bit7zfm,
    [
        /// Shows a file dialog and opens the chosen archive.
        OpenArchiveDialog,
        /// Commits the staged entries of the active workspace.
        CommitStaged,
        /// Discards the unstaged edits of the active workspace.
        DiscardUnstaged,
        /// Stages the selected entries.
        StageSelected,
        /// Unstages the selected entries.
        UnstageSelected,
        /// Tests the active archive's integrity.
        TestActiveArchive,
        /// Extracts the selected entries to a chosen directory.
        ExtractSelected,
        /// Adds files into the active archive.
        AddFiles,
        /// Deletes the selected entries.
        DeleteSelected,
        /// Renames the selected entry.
        RenameEntry,
        /// Navigates the active workspace to its parent directory.
        NavigateUp,
        /// Activates the selected entry: enters a folder, views a file.
        OpenSelected,
        /// Selects every entry of the active directory.
        SelectAllEntries,
        /// Shows or hides the changes (staging) panel.
        ToggleChanges,
        /// Compares the active workspace against its base.
        DiffWithBase,
        /// Compares two open workspaces with each other.
        DiffWorkspaces,
        /// Shows the properties dialog for the selection.
        ShowProperties,
        /// Copies the selected entries' full archive paths to the clipboard.
        CopySelectedPaths,
        /// Computes checksums for the selected files.
        ChecksumSelected,
        /// Shows or hides the DevTools panel (dev builds only).
        ToggleDevTools,
        /// Opens the settings panel.
        ToggleSettings,
    ]
);
