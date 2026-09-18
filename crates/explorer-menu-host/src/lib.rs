//! Host DLL for the Windows 11 Explorer context menu (`b7zmenu.dll`).
//!
//! Loaded in-process by Explorer through a sparse MSIX package registered by
//! `bit7z shell-menu-install`. `DllMain` installs the menu definitions; the
//! `DllGetClassObject` / `DllCanUnloadNow` exports forward to the
//! `explorer-menu` engine. Every action either writes a job file and starts
//! the detached executor, or launches the manager — the DLL itself never
//! touches archives.

#![cfg(windows)]

use checksum::ChecksumAlgorithm;
use explorer_menu::{CommandState, MenuAction, MenuRoot, Registration, Selection};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use task::JobSpec;
use windows::core::{GUID, HSTRING};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// CLSID of the single flyout root ("Bit7zFM") the sparse manifest binds.
/// Defined once in `explorer-menu` so the installer and the manifest can
/// derive the same canonical string from it.
pub use explorer_menu::CLSID_ROOT as CLSID_BIT7ZFM_ROOT;

fn executor_dir() -> Option<&'static Path> {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    Some(
        DIR.get_or_init(|| {
            explorer_menu::module_directory()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        })
        .as_path(),
    )
}

fn launch_job(spec: JobSpec) -> Result<(), String> {
    let dir = executor_dir().ok_or("module directory unknown")?;
    task::launch::launch_job(dir, spec)
}

fn open_in_manager(paths: &[PathBuf]) -> Result<(), String> {
    let exe = executor_dir()
        .ok_or("module directory unknown")?
        .join("bit7zfm.exe");
    if !exe.is_file() {
        return Err(format!("manager not found: {}", exe.display()));
    }
    let mut params = String::new();
    for path in paths {
        if !params.is_empty() {
            params.push(' ');
        }
        params.push('"');
        params.push_str(&path.display().to_string());
        params.push('"');
    }
    let app = HSTRING::from(exe.as_os_str());
    let args = HSTRING::from(params);
    unsafe {
        ShellExecuteW(None, windows::core::w!("open"), &app, &args, None, SW_SHOWNORMAL);
    }
    Ok(())
}

fn archive_extensions() -> &'static Vec<String> {
    static EXTENSIONS: OnceLock<Vec<String>> = OnceLock::new();
    EXTENSIONS.get_or_init(|| {
        [
            "7z", "zip", "rar", "tar", "gz", "tgz", "bz2", "tbz2", "xz", "txz", "zst", "wim",
            "cab", "iso", "lzma",
        ]
        .iter()
        .map(|ext| ext.to_string())
        .collect()
    })
}

fn all_archives(selection: Option<&Selection>) -> bool {
    selection.is_some_and(|sel| {
        !sel.is_empty()
            && sel.paths().iter().all(|p| {
                p.is_file()
                    && p.extension().is_some_and(|ext| {
                        archive_extensions()
                            .contains(&ext.to_string_lossy().to_lowercase())
                    })
            })
    })
}

fn any_selection(selection: Option<&Selection>) -> bool {
    selection.is_some_and(|sel| !sel.is_empty())
}

fn single_file(selection: Option<&Selection>) -> bool {
    selection.is_some_and(|sel| sel.paths().len() == 1 && sel.first().is_some_and(|p| p.is_file()))
}

fn parent_of(path: &Path) -> PathBuf {
    path.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn stem_of(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "archive".into())
}

// ---------------------------------------------------------------------------
// Verbs
// ---------------------------------------------------------------------------

/// Opens the selection in the manager.
struct OpenInManager;

impl MenuAction for OpenInManager {
    fn title(&self, _selection: Option<&Selection>) -> String {
        "Open with Bit7zFM".into()
    }
    fn state(&self, selection: Option<&Selection>) -> CommandState {
        if any_selection(selection) {
            CommandState::Enabled
        } else {
            CommandState::Hidden
        }
    }
    fn invoke(&self, selection: Option<&Selection>) -> Result<(), String> {
        let Some(sel) = selection else {
            return Err("no selection".into());
        };
        open_in_manager(sel.paths())
    }
}

/// Extract the whole archive into the parent directory.
struct ExtractHere;

impl MenuAction for ExtractHere {
    fn title(&self, _selection: Option<&Selection>) -> String {
        "Extract Here".into()
    }
    fn state(&self, selection: Option<&Selection>) -> CommandState {
        if all_archives(selection) {
            CommandState::Enabled
        } else {
            CommandState::Hidden
        }
    }
    fn invoke(&self, selection: Option<&Selection>) -> Result<(), String> {
        let Some(path) = selection.and_then(Selection::first) else {
            return Err("no selection".into());
        };
        launch_job(JobSpec::Extract {
            archive: path.to_path_buf(),
            items: vec![],
            target: parent_of(path),
            overwrite: task::OverwriteSpec::Ask,
            password_hint: false,
        })
    }
}

/// Extract the whole archive into `<parent>/<stem>\`.
struct ExtractToFolder;

impl MenuAction for ExtractToFolder {
    fn title(&self, selection: Option<&Selection>) -> String {
        let stem = selection
            .and_then(Selection::first)
            .map(|path| stem_of(path))
            .unwrap_or_else(|| "archive".into());
        format!("Extract to \"{stem}\\\"")
    }
    fn state(&self, selection: Option<&Selection>) -> CommandState {
        if all_archives(selection) {
            CommandState::Enabled
        } else {
            CommandState::Hidden
        }
    }
    fn invoke(&self, selection: Option<&Selection>) -> Result<(), String> {
        let Some(path) = selection.and_then(Selection::first) else {
            return Err("no selection".into());
        };
        let mut target = parent_of(path);
        target.push(format!("{}\\", stem_of(path)));
        std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        launch_job(JobSpec::Extract {
            archive: path.to_path_buf(),
            items: vec![],
            target,
            overwrite: task::OverwriteSpec::Ask,
            password_hint: false,
        })
    }
}

/// Test the archive's integrity.
struct TestArchive;

impl MenuAction for TestArchive {
    fn title(&self, _selection: Option<&Selection>) -> String {
        "Test archive".into()
    }
    fn state(&self, selection: Option<&Selection>) -> CommandState {
        if all_archives(selection) {
            CommandState::Enabled
        } else {
            CommandState::Hidden
        }
    }
    fn invoke(&self, selection: Option<&Selection>) -> Result<(), String> {
        let Some(path) = selection.and_then(Selection::first) else {
            return Err("no selection".into());
        };
        launch_job(JobSpec::Test {
            archive: path.to_path_buf(),
            password_hint: false,
        })
    }
}

/// Compress the selection into `<parent>/<stem>.<ext>`.
struct CompressTo {
    extension: &'static str,
}

impl MenuAction for CompressTo {
    fn title(&self, selection: Option<&Selection>) -> String {
        let stem = selection
            .and_then(Selection::first)
            .map(|path| stem_of(path))
            .unwrap_or_else(|| "archive".into());
        format!("Add to \"{stem}.{}\"", self.extension)
    }
    fn state(&self, selection: Option<&Selection>) -> CommandState {
        if any_selection(selection) {
            CommandState::Enabled
        } else {
            CommandState::Hidden
        }
    }
    fn invoke(&self, selection: Option<&Selection>) -> Result<(), String> {
        let Some(sel) = selection else {
            return Err("no selection".into());
        };
        let Some(first) = sel.first() else {
            return Err("no selection".into());
        };
        let mut target = parent_of(first);
        target.push(format!(
            "{}.{}",
            stem_of(first),
            self.extension
        ));
        launch_job(JobSpec::Compress {
            inputs: sel.paths().to_vec(),
            target,
            format: if self.extension == "zip" {
                task::FormatSpec::Zip
            } else {
                task::FormatSpec::SevenZip
            },
            level: task::LevelSpec::Normal,
            solid: None,
            volume: None,
            threads: None,
            encrypt_headers: false,
            password_hint: false,
        })
    }
}

/// Checksum a single file.
struct Checksum {
    algorithm: ChecksumAlgorithm,
    label: &'static str,
}

impl MenuAction for Checksum {
    fn title(&self, _selection: Option<&Selection>) -> String {
        self.label.into()
    }
    fn state(&self, selection: Option<&Selection>) -> CommandState {
        if single_file(selection) {
            CommandState::Enabled
        } else {
            CommandState::Hidden
        }
    }
    fn invoke(&self, selection: Option<&Selection>) -> Result<(), String> {
        let Some(path) = selection.and_then(Selection::first) else {
            return Err("no selection".into());
        };
        let dir = executor_dir().ok_or("module directory unknown")?;
        task::launch::launch_checksum(dir, path, self.algorithm)
    }
}

/// Builds the verb set fresh on every enumeration.
fn build_actions() -> Vec<Arc<dyn MenuAction>> {
    vec![
        Arc::new(OpenInManager),
        Arc::new(ExtractHere),
        Arc::new(ExtractToFolder),
        Arc::new(TestArchive),
        Arc::new(CompressTo { extension: "7z" }),
        Arc::new(CompressTo { extension: "zip" }),
        Arc::new(Checksum {
            algorithm: ChecksumAlgorithm::Crc32,
            label: "CRC-32",
        }),
        Arc::new(Checksum {
            algorithm: ChecksumAlgorithm::Crc64,
            label: "CRC-64",
        }),
        Arc::new(Checksum {
            algorithm: ChecksumAlgorithm::Md5,
            label: "MD5",
        }),
        Arc::new(Checksum {
            algorithm: ChecksumAlgorithm::Sha1,
            label: "SHA-1",
        }),
        Arc::new(Checksum {
            algorithm: ChecksumAlgorithm::Sha256,
            label: "SHA-256",
        }),
    ]
}

static INSTANCE: OnceLock<usize> = OnceLock::new();

/// Installs the verb set; called from DllMain with the module handle.
fn install(instance: HMODULE) {
    let registration = Registration::new(
        vec![MenuRoot::new(
            CLSID_BIT7ZFM_ROOT,
            "Bit7zFM",
            Box::new(build_actions),
        )],
        vec![],
    );
    explorer_menu::init(
        windows::Win32::Foundation::HINSTANCE(instance.0 as *mut _),
        registration,
    );
}

/// DLL entry point (MSVC CRT forwards here from the real DllMain stub).
#[unsafe(no_mangle)]
extern "system" fn DllMain(instance: HMODULE, reason: u32, _reserved: *mut core::ffi::c_void) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    if reason == DLL_PROCESS_ATTACH {
        let _ = INSTANCE.set(instance.0 as usize);
        // DllMain runs under the loader lock in the host (Explorer) process.
        // A panic escaping `install` must never unwind into the loader — it
        // would abort Explorer. Swallow it: the verbs simply stay
        // unregistered and Explorer degrades instead of crashing.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            install(instance);
        }));
    }
    1 // TRUE
}

/// COM activation entry — forwarded to the engine.
/// # Safety
/// Pointers must originate from the unmodified COM activation call.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    object: *mut *mut core::ffi::c_void,
) -> i32 {
    explorer_menu::dll_get_class_object(rclsid, riid, object).0
}

/// Kept loaded inside Explorer to avoid reload churn.
#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> i32 {
    explorer_menu::dll_can_unload_now().0
}
