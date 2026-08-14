use std::path::PathBuf;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "bit7z_archiver",
    version,
    about = "Cross-platform compressed file viewer and editor"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    // /// Run as a background worker process (spawned by parent for IPC operations)
    // #[arg(long, global = true, hide = true)]
    // pub worker: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    Open {
        path: String,
        #[arg(long)]
        password: Option<String>,
    },
    Extract {
        path: String,
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        indices: Option<String>,
        #[arg(long)]
        password: Option<String>,
    },
    Test {
        path: String,
        #[arg(long)]
        password: Option<String>,
    },
    Compress {
        #[arg(num_args = 1..)]
        files: Vec<String>,
        #[arg(long)]
        to: Option<String>,
        #[arg(long, default_value = "7z")]
        format: String,
        #[arg(long)]
        password: Option<String>,
    },
    Preview {
        path: String,
        index: u32,
        #[arg(long)]
        password: Option<String>,
        #[arg(long, default_value = "4096")]
        max_bytes: usize,
    },
    Add {
        path: String,
        #[arg(num_args = 1..)]
        files: Vec<String>,
        #[arg(long)]
        password: Option<String>,
    },
    Delete {
        path: String,
        #[arg(num_args = 1..)]
        indices: Vec<u32>,
        #[arg(long)]
        password: Option<String>,
    },
    Rename {
        path: String,
        index: u32,
        name: String,
        #[arg(long)]
        password: Option<String>,
    },
    /// List contents of an archive
    List {
        path: PathBuf,
        #[arg(long)]
        password: Option<String>,
    },
    /// Compute checksum of an entry
    Checksum {
        path: PathBuf,
        index: usize,
        #[arg(long)]
        algorithm: Option<String>,
        #[arg(long)]
        password: Option<String>,
    },
    /// Create a new folder in an archive
    NewFolder {
        path: PathBuf,
        folder_path: String,
        #[arg(long)]
        password: Option<String>,
    },
    /// Register shell context menu entries
    ShellInstall,
    /// Unregister shell context menu entries
    ShellUninstall,
}