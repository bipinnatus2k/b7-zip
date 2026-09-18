use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "bit7z_archiver",
    version,
    about = "Cross-platform compressed file viewer and editor"
)]
pub struct Cli {
    /// Enable debug output in processes launched by this CLI.
    #[arg(long, global = true)]
    pub debug: bool,

    /// Optional config file handed to launched GUI processes.
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    /// Run long tasks in a detached bit7z-executor window instead of the
    /// current process.
    #[arg(long, global = true)]
    pub gui: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
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
        /// Encrypt also the file names (7z only, like 7-Zip's -mhe).
        #[arg(long)]
        encrypt_headers: bool,
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
    /// Register shell context menu entries (legacy COM, Windows 10+)
    ShellInstall,
    /// Unregister shell context menu entries (legacy COM)
    ShellUninstall,
    /// Register the Windows 11 Explorer context menu (sparse MSIX)
    ShellMenuInstall,
    /// Unregister the Windows 11 Explorer context menu (sparse MSIX)
    ShellMenuUninstall,
}
