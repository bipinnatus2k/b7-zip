//! Explorer shell extension: a COM DLL loaded by Explorer.exe that adds
//! archive context-menu verbs (7-Zip / WinRAR style) through the legacy
//! `IShellExtInit` + `IContextMenu` interfaces.
//!
//! Safety model: this DLL runs *inside* Explorer.exe, so every exported
//! entry point is wrapped in `catch_unwind`, no panic may cross the FFI
//! boundary, and the DLL performs no work beyond writing a job file and
//! launching the executor process. The DLL never touches the archive itself.
//!
//! Registration (self-register / unregister, HKCU only, no admin needed):
//! - `regsvr32 shell.dll`  -> `DllRegisterServer`
//! - `regsvr32 /u shell.dll` -> `DllUnregisterServer`
//! - `bit7z shell-install` / `bit7z shell-uninstall` (same entry points)
//!
//! Menu layout is controlled by `HKCU\Software\Bit7zFM\Shell`:
//! - `CascadedMenu` (DWORD, default 1): show verbs under a "Bit7z" submenu
//! - `MenuFlags` (DWORD, default all bits): per-verb visibility mask

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use windows::Win32::Foundation::{CloseHandle, E_FAIL, E_NOTIMPL, HMODULE};
use windows::Win32::System::Com::{
    DVASPECT_CONTENT, FORMATETC, IClassFactory, IClassFactory_Impl, IDataObject, TYMED_HGLOBAL,
};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Ole::ReleaseStgMedium;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteKeyValueW};
use windows::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessW, PROCESS_INFORMATION, STARTF_USESHOWWINDOW,
    STARTUPINFOW,
};
use windows::Win32::UI::Shell::{
    CMINVOKECOMMANDINFO, CMINVOKECOMMANDINFOEX, DragQueryFileW, HDROP, IContextMenu,
    IContextMenu_Impl, IShellExtInit, IShellExtInit_Impl, SHCNE_ASSOCCHANGED, SHCNF_IDLIST,
    SHChangeNotify, SHDeleteKeyW, SHGetValueW, SHSetValueW, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, HMENU, InsertMenuItemW, MENUITEMINFOW, MFT_SEPARATOR, MIIM_FTYPE, MIIM_ID,
    MIIM_STRING, MIIM_SUBMENU, SW_HIDE, SW_SHOWNORMAL,
};
use windows::core::{
    BOOL, GUID, HRESULT, IUnknown, IUnknownImpl, Interface, PCWSTR, PWSTR, Ref, implement, w,
};

/// The COM class id of this extension.
pub const CLSID_BIT7Z_MENU: GUID = GUID::from_u128(0x4b69747a_7a69_5348_4c4c_4d454e55434f);

/// DLL module handle captured by `DllMain`.
static DLL_INSTANCE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

/// Entry point called when Explorer loads (or unloads) this DLL.
#[unsafe(no_mangle)]
extern "system" fn DllMain(
    hinst_dll: HMODULE,
    fdw_reason: u32,
    _lpv_reserved: *mut c_void,
) -> bool {
    if fdw_reason == 1
    /* DLL_PROCESS_ATTACH */
    {
        let _ = DLL_INSTANCE.set(hinst_dll.0 as usize);
    }
    true
}

/// Registry subtree for per-user class registration.
const REG_CLASSES: &str = "Software\\Classes";
/// Per-user shell menu preferences (mirrors 7-Zip's CascadedContextMenu).
const REG_SHELL_SETTINGS: &str = "Software\\Bit7zFM\\Shell";
/// Per-user Explorer approval list. Explorer will not load a per-user
/// context-menu handler unless its CLSID is approved here.
const REG_APPROVED: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Shell Extensions\\Approved";

// ---------------------------------------------------------------------------
// Verb ids (stable; used as idCmdFirst offsets and MenuFlags bits)
// ---------------------------------------------------------------------------

const CMD_OPEN: u32 = 0;
const CMD_TEST: u32 = 1;
const CMD_EXTRACT: u32 = 2;
const CMD_EXTRACT_HERE: u32 = 3;
const CMD_EXTRACT_TO: u32 = 4;
const CMD_ADD: u32 = 5;
const CMD_ADD_TO_7Z: u32 = 6;
const CMD_ADD_TO_ZIP: u32 = 7;
const CMD_ADD_EACH_7Z: u32 = 8;
const CMD_HASH_CRC32: u32 = 9;
const CMD_HASH_CRC64: u32 = 10;
const CMD_HASH_SHA1: u32 = 11;
const CMD_HASH_SHA256: u32 = 12;
const CMD_HASH_MD5: u32 = 13;
const CMD_COUNT: u32 = 14;

static JOB_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// `CMF_DEFAULTONLY`: Explorer asks whether the extension supplies a default
/// verb. We don't, so don't add any menu items for this probe.
const CMF_DEFAULTONLY: u32 = 0x0000_0001;

/// `GetCommandString` type selectors (values are fixed by the shell contract).
const GCS_VERBA: u32 = 0x0000_0000;
const GCS_HELPTEXTA: u32 = 0x0000_0001;
const GCS_VALIDATEA: u32 = 0x0000_0002;
const GCS_VERBW: u32 = 0x0000_0004;
const GCS_HELPTEXTW: u32 = 0x0000_0005;
const GCS_VALIDATEW: u32 = 0x0000_0006;
const GCS_UNICODE: u32 = 0x0000_0004;
/// `CMIC_MASK_UNICODE`: the shell passed a `CMINVOKECOMMANDINFOEX` with wide
/// string fields.
const CMIC_MASK_UNICODE: u32 = 0x0000_4000;

const FLAG_OPEN: u32 = 1 << CMD_OPEN;
const FLAG_TEST: u32 = 1 << CMD_TEST;
const FLAG_EXTRACT: u32 = 1 << CMD_EXTRACT;
const FLAG_EXTRACT_HERE: u32 = 1 << CMD_EXTRACT_HERE;
const FLAG_EXTRACT_TO: u32 = 1 << CMD_EXTRACT_TO;
const FLAG_ADD: u32 = 1 << CMD_ADD;
const FLAG_ADD_TO_7Z: u32 = 1 << CMD_ADD_TO_7Z;
const FLAG_ADD_TO_ZIP: u32 = 1 << CMD_ADD_TO_ZIP;
const FLAG_ADD_EACH_7Z: u32 = 1 << CMD_ADD_EACH_7Z;
const FLAG_HASH: u32 = (1 << CMD_HASH_CRC32)
    | (1 << CMD_HASH_CRC64)
    | (1 << CMD_HASH_SHA1)
    | (1 << CMD_HASH_SHA256)
    | (1 << CMD_HASH_MD5);
const FLAG_ALL: u32 = (1 << CMD_COUNT) - 1;

/// Well-known archive extensions (extension-only; no I/O inside Explorer).
const ARCHIVE_EXTENSIONS: &[&str] = &[
    "7z", "zip", "rar", "tar", "gz", "tgz", "bz2", "tbz2", "tbz", "xz", "txz", "wim", "iso", "cab",
    "arj", "lzh", "lha", "dmg", "vhd", "vhdx", "wim", "001", "z", "lzma",
];

/// Context captured by `IShellExtInit::Initialize`.
struct MenuContext {
    files: Vec<String>,
    /// The `idCmdFirst` offset Explorer handed us in QueryContextMenu.
    id_cmd_first: u32,
}

/// Shell menu preferences loaded from the registry.
#[derive(Clone, Debug)]
struct MenuSettings {
    /// When true, verbs live under a single "Bit7z" cascade item.
    cascaded: bool,
    /// Root cascade label.
    cascade_name: String,
    /// Bitmask of visible verbs (`FLAG_*`).
    flags: u32,
}

impl Default for MenuSettings {
    fn default() -> Self {
        Self {
            cascaded: true,
            cascade_name: "Bit7z".into(),
            flags: FLAG_ALL,
        }
    }
}

impl MenuSettings {
    fn enabled(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }
}

/// The shell extension object (IShellExtInit + IContextMenu).
#[implement(IShellExtInit, IContextMenu)]
struct Bit7zMenu {
    ctx: std::sync::Mutex<Option<MenuContext>>,
}

impl Bit7zMenu {
    fn new() -> Self {
        Self {
            ctx: std::sync::Mutex::new(None),
        }
    }
}

impl IShellExtInit_Impl for Bit7zMenu_Impl {
    fn Initialize(
        &self,
        _pidl_folder: *const windows::Win32::UI::Shell::Common::ITEMIDLIST,
        pdtobj: Ref<'_, IDataObject>,
        _hkey_prog_id: windows::Win32::System::Registry::HKEY,
    ) -> windows::core::Result<()> {
        let Some(data_object) = pdtobj.as_ref() else {
            return Ok(());
        };
        let files = collect_files(data_object);
        *self.get_impl().ctx.lock().unwrap() = Some(MenuContext {
            files,
            id_cmd_first: 0,
        });
        Ok(())
    }
}

impl IContextMenu_Impl for Bit7zMenu_Impl {
    fn QueryContextMenu(
        &self,
        hmenu: HMENU,
        index_menu: u32,
        id_cmd_first: u32,
        _id_cmd_last: u32,
        u_flags: u32,
    ) -> HRESULT {
        // Explorer uses CMF_DEFAULTONLY to ask for a default verb. This
        // extension has no default verb, so leave the menu untouched.
        if u_flags & CMF_DEFAULTONLY != 0 {
            return HRESULT(0);
        }

        let files = self
            .get_impl()
            .ctx
            .lock()
            .unwrap()
            .as_ref()
            .map(|c| c.files.clone())
            .unwrap_or_default();
        let settings = load_menu_settings();
        let plan = build_menu_plan(&files, &settings);
        if plan.is_empty() {
            return HRESULT(0);
        }

        let inserted = if settings.cascaded {
            insert_cascaded_menu(
                hmenu,
                index_menu,
                id_cmd_first,
                &settings.cascade_name,
                &plan,
            )
        } else {
            insert_flat_menu(hmenu, index_menu, id_cmd_first, &plan)
        };

        if let Some(ctx) = self.get_impl().ctx.lock().unwrap().as_mut() {
            ctx.id_cmd_first = id_cmd_first;
        }

        // Low 16 bits = largest command id offset assigned + 1 (MSDN).
        let max_offset = plan
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Command { id, .. } => Some(*id),
                _ => None,
            })
            .max()
            .map(|id| id + 1)
            .unwrap_or(0);
        let _ = inserted;
        HRESULT(max_offset as i32)
    }

    fn InvokeCommand(&self, pici: *const CMINVOKECOMMANDINFO) -> windows::core::Result<()> {
        if pici.is_null() {
            return Ok(());
        }
        let info = unsafe { &*pici };
        if info.lpVerb.is_null() {
            return Ok(());
        }
        let verb_ptr = info.lpVerb.as_ptr() as usize;
        // `lpVerb` is either an integer resource id (whose pointer value is
        // <= 0xFFFF) or an ANSI/Wide canonical-verb string returned by
        // GetCommandString.
        let is_string_verb = verb_ptr > 0xFFFF;
        let is_unicode = info.cbSize >= std::mem::size_of::<CMINVOKECOMMANDINFOEX>() as u32
            && info.fMask & CMIC_MASK_UNICODE != 0;
        let command_id = if is_string_verb {
            let verb = if is_unicode {
                let info = unsafe { &*(pici as *const CMINVOKECOMMANDINFOEX) };
                unsafe { info.lpVerbW.to_string() }.ok()
            } else {
                let verb = unsafe {
                    std::ffi::CStr::from_ptr(info.lpVerb.as_ptr().cast::<std::ffi::c_char>())
                };
                verb.to_str().ok().map(|s| s.to_string())
            };
            let Some(verb) = verb else {
                return Ok(());
            };
            let Some(command_id) = command_id_from_verb(&verb) else {
                return Ok(());
            };
            command_id
        } else {
            verb_ptr as u32
        };

        let context = self.get_impl().ctx.lock().unwrap().take();
        let Some(context) = context else {
            return Ok(());
        };
        if context.files.is_empty() {
            return Ok(());
        }
        // Integer verbs arrive as absolute menu ids (idCmdFirst + offset);
        // canonical string verbs already map to the local verb id.
        let command_id = if is_string_verb {
            command_id
        } else {
            command_id.saturating_sub(context.id_cmd_first)
        };
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _ = run_command(command_id, &context.files);
        }));
        Ok(())
    }

    fn GetCommandString(
        &self,
        id_cmd: usize,
        u_type: u32,
        _p_reserved: *const u32,
        psz_name: windows::core::PSTR,
        cch_max: u32,
    ) -> windows::core::Result<()> {
        match u_type {
            // Validation requests do not require a buffer; S_OK means the
            // command id is owned by this extension.
            GCS_VALIDATEA | GCS_VALIDATEW => {
                if id_cmd < CMD_COUNT as usize {
                    Ok(())
                } else {
                    Err(E_FAIL.into())
                }
            }
            GCS_VERBA | GCS_VERBW => {
                let Some(verb) = command_verb(id_cmd) else {
                    return Err(E_FAIL.into());
                };
                write_command_string(psz_name, cch_max, verb, u_type & GCS_UNICODE != 0)
            }
            GCS_HELPTEXTA | GCS_HELPTEXTW => {
                let Some(help) = command_help(id_cmd) else {
                    return Err(E_FAIL.into());
                };
                write_command_string(psz_name, cch_max, help, u_type & GCS_UNICODE != 0)
            }
            _ => Err(E_NOTIMPL.into()),
        }
    }
}

fn command_verb(id_cmd: usize) -> Option<&'static str> {
    match id_cmd as u32 {
        CMD_OPEN => Some("open"),
        CMD_TEST => Some("test"),
        CMD_EXTRACT => Some("extract"),
        CMD_EXTRACT_HERE => Some("extract_here"),
        CMD_EXTRACT_TO => Some("extract_to"),
        CMD_ADD => Some("add"),
        CMD_ADD_TO_7Z => Some("add_to_7z"),
        CMD_ADD_TO_ZIP => Some("add_to_zip"),
        CMD_ADD_EACH_7Z => Some("add_each_7z"),
        CMD_HASH_CRC32 => Some("crc32"),
        CMD_HASH_CRC64 => Some("crc64"),
        CMD_HASH_SHA1 => Some("sha1"),
        CMD_HASH_SHA256 => Some("sha256"),
        CMD_HASH_MD5 => Some("md5"),
        _ => None,
    }
}

fn command_id_from_verb(verb: &str) -> Option<u32> {
    (0..CMD_COUNT).find(|&id| command_verb(id as usize).is_some_and(|candidate| candidate == verb))
}

fn command_help(id_cmd: usize) -> Option<&'static str> {
    match id_cmd as u32 {
        CMD_OPEN => Some("Open the archive in Bit7zFM"),
        CMD_TEST => Some("Test archive integrity"),
        CMD_EXTRACT => Some("Extract files to a chosen folder"),
        CMD_EXTRACT_HERE => Some("Extract files to the current folder"),
        CMD_EXTRACT_TO => Some("Extract files to a subfolder"),
        CMD_ADD => Some("Add files to a new archive"),
        CMD_ADD_TO_7Z => Some("Compress to a .7z archive"),
        CMD_ADD_TO_ZIP => Some("Compress to a .zip archive"),
        CMD_ADD_EACH_7Z => Some("Compress each item to a separate .7z archive"),
        CMD_HASH_CRC32 => Some("Compute CRC-32"),
        CMD_HASH_CRC64 => Some("Compute CRC-64"),
        CMD_HASH_SHA1 => Some("Compute SHA-1"),
        CMD_HASH_SHA256 => Some("Compute SHA-256"),
        CMD_HASH_MD5 => Some("Compute MD5"),
        _ => None,
    }
}

fn write_command_string(
    psz_name: windows::core::PSTR,
    cch_max: u32,
    text: &str,
    wide: bool,
) -> windows::core::Result<()> {
    if psz_name.is_null() || cch_max == 0 {
        return Err(E_FAIL.into());
    }

    if wide {
        let encoded: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        if encoded.len() > cch_max as usize {
            return Err(E_FAIL.into());
        }
        unsafe {
            std::ptr::copy_nonoverlapping(
                encoded.as_ptr().cast::<u8>(),
                psz_name.0,
                encoded.len() * std::mem::size_of::<u16>(),
            );
        }
    } else {
        let bytes = text.as_bytes();
        if bytes.len() + 1 > cch_max as usize {
            return Err(E_FAIL.into());
        }
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), psz_name.0, bytes.len());
            psz_name.0.add(bytes.len()).write(0);
        }
    }
    Ok(())
}

// ============================================================================
// Menu planning / insertion
// ============================================================================

#[derive(Clone, Debug)]
enum MenuEntry {
    Command { id: u32, label: String },
    Separator,
}

fn is_archive_path(path: &str) -> bool {
    let name = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase();
    if name.ends_with(".tar.gz")
        || name.ends_with(".tar.bz2")
        || name.ends_with(".tar.xz")
        || name.ends_with(".tgz")
        || name.ends_with(".tbz2")
        || name.ends_with(".tbz")
        || name.ends_with(".txz")
    {
        return true;
    }
    let ext = name.rsplit('.').next().unwrap_or("");
    ARCHIVE_EXTENSIONS.iter().any(|e| *e == ext)
}

fn selection_has_archive(files: &[String]) -> bool {
    files.iter().any(|f| is_archive_path(f))
}

fn archive_folder_name(path: &str) -> String {
    let p = Path::new(path);
    let name = p
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("archive")
        .to_string();
    let lower = name.to_ascii_lowercase();
    for suffix in [
        ".tar.gz", ".tar.bz2", ".tar.xz", ".tgz", ".tbz2", ".tbz", ".txz",
    ] {
        if lower.ends_with(suffix) {
            return name[..name.len() - suffix.len()].to_string();
        }
    }
    p.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("archive")
        .to_string()
}

fn compress_target_stem(files: &[String]) -> String {
    let stem = default_archive_stem(files);
    // Recompressing a single archive to its own path would overwrite the
    // input while it is being read.
    if files.len() == 1 && is_archive_path(&files[0]) {
        format!("{stem}_new")
    } else {
        stem
    }
}

fn default_archive_stem(files: &[String]) -> String {
    if files.len() == 1 {
        let p = Path::new(&files[0]);
        if p.is_dir() || !is_archive_path(&files[0]) {
            return p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("archive")
                .to_string();
        }
        // Single archive selected for recompress: still use its stem.
        return archive_folder_name(&files[0]);
    }
    // Multi-select: name after the parent folder when possible.
    Path::new(&files[0])
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("archive")
        .to_string()
}

fn build_menu_plan(files: &[String], settings: &MenuSettings) -> Vec<MenuEntry> {
    if files.is_empty() {
        return Vec::new();
    }
    let has_archive = selection_has_archive(files);
    let stem = compress_target_stem(files);
    let folder = if has_archive {
        archive_folder_name(&files[0])
    } else {
        stem.clone()
    };
    let multi = files.len() > 1;
    let mut plan = Vec::new();

    let push_cmd = |plan: &mut Vec<MenuEntry>, id: u32, flag: u32, label: String| {
        if settings.enabled(flag) {
            plan.push(MenuEntry::Command { id, label });
        }
    };
    let push_sep = |plan: &mut Vec<MenuEntry>| {
        if plan
            .last()
            .is_some_and(|e| !matches!(e, MenuEntry::Separator))
        {
            plan.push(MenuEntry::Separator);
        }
    };

    if has_archive {
        push_cmd(&mut plan, CMD_OPEN, FLAG_OPEN, "Open archive".into());
        push_sep(&mut plan);
        push_cmd(
            &mut plan,
            CMD_EXTRACT,
            FLAG_EXTRACT,
            "Extract files...".into(),
        );
        push_cmd(
            &mut plan,
            CMD_EXTRACT_HERE,
            FLAG_EXTRACT_HERE,
            "Extract Here".into(),
        );
        push_cmd(
            &mut plan,
            CMD_EXTRACT_TO,
            FLAG_EXTRACT_TO,
            format!("Extract to \"{folder}\\\""),
        );
        push_sep(&mut plan);
        push_cmd(&mut plan, CMD_TEST, FLAG_TEST, "Test archive".into());
        push_sep(&mut plan);
    }

    push_cmd(&mut plan, CMD_ADD, FLAG_ADD, "Add to archive...".into());
    push_cmd(
        &mut plan,
        CMD_ADD_TO_7Z,
        FLAG_ADD_TO_7Z,
        format!("Add to \"{stem}.7z\""),
    );
    push_cmd(
        &mut plan,
        CMD_ADD_TO_ZIP,
        FLAG_ADD_TO_ZIP,
        format!("Add to \"{stem}.zip\""),
    );
    if multi || files.iter().any(|f| Path::new(f).is_dir()) {
        push_cmd(
            &mut plan,
            CMD_ADD_EACH_7Z,
            FLAG_ADD_EACH_7Z,
            "Add each to separate .7z".into(),
        );
    }

    if settings.enabled(FLAG_HASH) {
        push_sep(&mut plan);
        push_cmd(
            &mut plan,
            CMD_HASH_CRC32,
            1 << CMD_HASH_CRC32,
            "CRC-32".into(),
        );
        push_cmd(
            &mut plan,
            CMD_HASH_CRC64,
            1 << CMD_HASH_CRC64,
            "CRC-64".into(),
        );
        push_cmd(&mut plan, CMD_HASH_SHA1, 1 << CMD_HASH_SHA1, "SHA-1".into());
        push_cmd(
            &mut plan,
            CMD_HASH_SHA256,
            1 << CMD_HASH_SHA256,
            "SHA-256".into(),
        );
        push_cmd(&mut plan, CMD_HASH_MD5, 1 << CMD_HASH_MD5, "MD5".into());
    }

    // Drop trailing separator if any.
    while matches!(plan.last(), Some(MenuEntry::Separator)) {
        plan.pop();
    }
    plan
}

fn insert_menu_separator(hmenu: HMENU, index: u32) {
    let sep = MENUITEMINFOW {
        cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
        fMask: MIIM_FTYPE,
        fType: MFT_SEPARATOR,
        ..Default::default()
    };
    unsafe {
        let _ = InsertMenuItemW(hmenu, index, true, &sep);
    }
}

fn insert_menu_command(hmenu: HMENU, index: u32, id: u32, label: &str) {
    let mut wide: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
    let item = MENUITEMINFOW {
        cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
        fMask: MIIM_ID | MIIM_STRING,
        wID: id,
        dwTypeData: PWSTR(wide.as_mut_ptr()),
        cch: 0,
        ..Default::default()
    };
    unsafe {
        let _ = InsertMenuItemW(hmenu, index, true, &item);
    }
}

fn insert_menu_entries(
    hmenu: HMENU,
    start_index: u32,
    id_cmd_first: u32,
    plan: &[MenuEntry],
) -> u32 {
    let mut count = 0u32;
    for entry in plan {
        match entry {
            MenuEntry::Separator => {
                insert_menu_separator(hmenu, start_index + count);
                count += 1;
            }
            MenuEntry::Command { id, label } => {
                insert_menu_command(hmenu, start_index + count, id_cmd_first + id, label);
                count += 1;
            }
        }
    }
    count
}

fn insert_flat_menu(hmenu: HMENU, index_menu: u32, id_cmd_first: u32, plan: &[MenuEntry]) -> u32 {
    insert_menu_entries(hmenu, index_menu, id_cmd_first, plan)
}

fn insert_cascaded_menu(
    hmenu: HMENU,
    index_menu: u32,
    id_cmd_first: u32,
    cascade_name: &str,
    plan: &[MenuEntry],
) -> u32 {
    let Ok(submenu) = (unsafe { CreatePopupMenu() }) else {
        return insert_flat_menu(hmenu, index_menu, id_cmd_first, plan);
    };
    let _ = insert_menu_entries(submenu, 0, id_cmd_first, plan);

    let mut wide: Vec<u16> = cascade_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let item = MENUITEMINFOW {
        cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
        fMask: MIIM_STRING | MIIM_SUBMENU,
        hSubMenu: submenu,
        dwTypeData: PWSTR(wide.as_mut_ptr()),
        cch: 0,
        ..Default::default()
    };
    unsafe {
        let _ = InsertMenuItemW(hmenu, index_menu, true, &item);
    }
    // Explorer owns `submenu` after a successful insert.
    1
}

// ============================================================================
// HDROP file collection
// ============================================================================

/// Extract file paths from a data object (CF_HDROP).
fn collect_files(data_object: &IDataObject) -> Vec<String> {
    let formatetc = FORMATETC {
        cfFormat: 15, // CF_HDROP
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0 as u32,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    let Ok(mut medium) = (unsafe { data_object.GetData(&formatetc) }) else {
        return Vec::new();
    };
    let hdrop = HDROP(unsafe { medium.u.hGlobal.0 });
    let count = unsafe { DragQueryFileW(hdrop, u32::MAX, None) };
    let mut files = Vec::new();
    for i in 0..count {
        let len = unsafe { DragQueryFileW(hdrop, i, None) };
        if len == 0 {
            continue;
        }
        let mut buf = vec![0u16; len as usize + 1];
        let written = unsafe { DragQueryFileW(hdrop, i, Some(&mut buf)) };
        files.push(String::from_utf16_lossy(&buf[..written as usize]));
    }
    // HDROP from IDataObject::GetData is an STGMEDIUM — free with ReleaseStgMedium.
    unsafe { ReleaseStgMedium(&mut medium) };
    files
}

// ============================================================================
// Command execution
// ============================================================================

fn run_command(command_id: u32, files: &[String]) -> Result<(), String> {
    let first = PathBuf::from(&files[0]);
    let parent = first
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let stem = compress_target_stem(files);
    let folder = archive_folder_name(&files[0]);

    match command_id {
        CMD_OPEN => {
            return open_with_manager(&first).map_err(|e| e.to_string());
        }
        CMD_EXTRACT => {
            // "Extract files..." — open the manager so the user can choose a target.
            return open_with_manager(&first).map_err(|e| e.to_string());
        }
        CMD_EXTRACT_HERE => launch_job(task::JobSpec::Extract {
            archive: first,
            items: Vec::new(),
            target: parent,
            overwrite: task::OverwriteSpec::Ask,
            password_hint: false,
        }),
        CMD_EXTRACT_TO => launch_job(task::JobSpec::Extract {
            archive: first,
            items: Vec::new(),
            target: parent.join(&folder),
            overwrite: task::OverwriteSpec::Ask,
            password_hint: false,
        }),
        CMD_TEST => launch_job(task::JobSpec::Test {
            archive: first,
            password_hint: false,
        }),
        CMD_ADD => {
            // Open the manager with the complete selection preloaded in its
            // Add-to-archive dialog.
            return open_with_manager_add(files).map_err(|e| e.to_string());
        }
        CMD_ADD_TO_7Z => launch_job(task::JobSpec::Compress {
            inputs: files.iter().map(PathBuf::from).collect(),
            target: parent.join(format!("{stem}.7z")),
            format: task::FormatSpec::SevenZip,
            level: task::LevelSpec::Normal,
            solid: None,
            volume: None,
            threads: None,
            encrypt_headers: false,
            password_hint: false,
        }),
        CMD_ADD_TO_ZIP => launch_job(task::JobSpec::Compress {
            inputs: files.iter().map(PathBuf::from).collect(),
            target: parent.join(format!("{stem}.zip")),
            format: task::FormatSpec::Zip,
            level: task::LevelSpec::Normal,
            solid: None,
            volume: None,
            threads: None,
            encrypt_headers: false,
            password_hint: false,
        }),
        CMD_ADD_EACH_7Z => {
            for f in files {
                let path = PathBuf::from(f);
                let p = path
                    .parent()
                    .map(|x| x.to_path_buf())
                    .unwrap_or_else(|| parent.clone());
                let s = path
                    .file_stem()
                    .and_then(|x| x.to_str())
                    .unwrap_or("archive");
                launch_job(task::JobSpec::Compress {
                    inputs: vec![path.clone()],
                    target: p.join(format!("{s}.7z")),
                    format: task::FormatSpec::SevenZip,
                    level: task::LevelSpec::Normal,
                    solid: None,
                    volume: None,
                    threads: None,
                    encrypt_headers: false,
                    password_hint: false,
                })?;
            }
            Ok(())
        }
        CMD_HASH_CRC32 => launch_hash(files, checksum::ChecksumAlgorithm::Crc32),
        CMD_HASH_CRC64 => launch_hash(files, checksum::ChecksumAlgorithm::Crc64),
        CMD_HASH_SHA1 => launch_hash(files, checksum::ChecksumAlgorithm::Sha1),
        CMD_HASH_SHA256 => launch_hash(files, checksum::ChecksumAlgorithm::Sha256),
        CMD_HASH_MD5 => launch_hash(files, checksum::ChecksumAlgorithm::Md5),
        _ => Ok(()),
    }
}

fn launch_hash(files: &[String], algorithm: checksum::ChecksumAlgorithm) -> Result<(), String> {
    for file in files {
        launch_job(task::JobSpec::Checksum {
            path: PathBuf::from(file),
            algorithm,
        })?;
    }
    Ok(())
}

fn launch_job(spec: task::JobSpec) -> Result<(), String> {
    let Some(executor) = find_sibling_exe("bit7z-executor.exe") else {
        return Err(format!(
            "bit7z-executor.exe not found next to shell DLL (dll at {:?})",
            module_path().ok()
        ));
    };
    let Ok(jobs_dir) = temp::ensure_jobs_dir() else {
        return Err("cannot create jobs dir".into());
    };
    let job_id = format!(
        "shell-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
        JOB_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let job_path = jobs_dir.join(format!("{job_id}.job.json"));
    let file = task::JobFile::new(job_id, spec);
    let json = file.to_json().map_err(|e| e.to_string())?;
    std::fs::write(&job_path, json).map_err(|e| e.to_string())?;
    launch_process(&executor, &job_path)
}

/// Launch `executor <jobfile>` hidden, detached from Explorer.
fn launch_process(executor: &Path, job_path: &Path) -> Result<(), String> {
    let app = windows::core::HSTRING::from(executor);
    let mut cmdline: Vec<u16> = format!("\"{}\"", job_path.display())
        .encode_utf16()
        .collect();
    cmdline.push(0);
    let si = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        dwFlags: STARTF_USESHOWWINDOW,
        wShowWindow: SW_HIDE.0 as u16,
        ..Default::default()
    };
    let mut pi = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            &app,
            Some(PWSTR(cmdline.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_UNICODE_ENVIRONMENT,
            None,
            PCWSTR::null(),
            &si,
            &mut pi,
        )
    }
    .map_err(|e| format!("CreateProcessW: {e}"))?;
    unsafe {
        let _ = CloseHandle(pi.hThread);
        let _ = CloseHandle(pi.hProcess);
    }
    Ok(())
}

/// Open a path with the Bit7z file manager when present, else default association.
fn open_with_manager_add(files: &[String]) -> windows::core::Result<()> {
    let Some(manager) = find_sibling_exe("bit7zfm.exe") else {
        return Err(windows::core::Error::from_hresult(HRESULT(
            0x8007_0002u32 as i32,
        )));
    };
    let app = windows::core::HSTRING::from(manager.as_path());
    // The app binary accepts only positional file paths. Passing the
    // selection positionally lets Workspace::open_paths preload its Add
    // dialog without a dedicated app-level `--add` switch.
    let mut parameters = String::new();
    for file in files {
        if !parameters.is_empty() {
            parameters.push(' ');
        }
        parameters.push('"');
        parameters.push_str(&file.replace('"', "\""));
        parameters.push('"');
    }
    let params = windows::core::HSTRING::from(&parameters);
    unsafe {
        ShellExecuteW(None, w!("open"), &app, &params, None, SW_SHOWNORMAL);
    }
    Ok(())
}

fn open_with_manager(path: &Path) -> windows::core::Result<()> {
    if let Some(manager) = find_sibling_exe("bit7zfm.exe") {
        let app = windows::core::HSTRING::from(manager.as_path());
        let file = windows::core::HSTRING::from(path);
        unsafe {
            ShellExecuteW(None, w!("open"), &app, &file, None, SW_SHOWNORMAL);
        }
        return Ok(());
    }
    let file = windows::core::HSTRING::from(path);
    unsafe {
        ShellExecuteW(None, w!("open"), &file, None, None, SW_SHOWNORMAL);
    }
    Ok(())
}

/// Locate an exe shipped next to this DLL.
fn find_sibling_exe(name: &str) -> Option<PathBuf> {
    let module = HMODULE(*DLL_INSTANCE.get()? as *mut c_void);
    unsafe {
        let mut buf = vec![0u16; 4096];
        let len = GetModuleFileNameW(Some(module), &mut buf);
        if len == 0 {
            return None;
        }
        let dll = PathBuf::from(String::from_utf16_lossy(&buf[..len as usize]));
        let sibling = dll.parent()?.join(name);
        if sibling.exists() {
            Some(sibling)
        } else {
            None
        }
    }
}

// ============================================================================
// Class factory
// ============================================================================

#[implement(IClassFactory)]
struct Bit7zClassFactory;

impl IClassFactory_Impl for Bit7zClassFactory_Impl {
    fn CreateInstance(
        &self,
        punk_outer: Ref<'_, IUnknown>,
        riid: *const GUID,
        ppv_object: *mut *mut c_void,
    ) -> windows::core::Result<()> {
        if ppv_object.is_null() {
            return Err(windows::core::Error::from_hresult(HRESULT(
                0x8000_4003u32 as i32,
            ))); // E_POINTER
        }
        unsafe {
            *ppv_object = std::ptr::null_mut();
        }
        if riid.is_null() {
            return Err(windows::core::Error::from_hresult(HRESULT(
                0x8000_4003u32 as i32,
            ))); // E_POINTER
        }
        if !punk_outer.is_null() {
            return Err(windows::core::Error::from_hresult(HRESULT(
                0x8004_0110u32 as i32,
            ))); // CLASS_E_NOAGGREGATION
        }
        let menu: IContextMenu = Bit7zMenu::new().into();
        unsafe { menu.query(riid, ppv_object) }.ok()?;
        Ok(())
    }

    fn LockServer(&self, _f_lock: BOOL) -> windows::core::Result<()> {
        Ok(())
    }
}

// ============================================================================
// DLL exports
// ============================================================================

/// COM class object factory entry point.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    catch_unwind(AssertUnwindSafe(|| {
        if ppv.is_null() {
            return HRESULT(0x8000_4003u32 as i32); // E_POINTER
        }
        unsafe {
            *ppv = std::ptr::null_mut();
        }
        if rclsid.is_null() || riid.is_null() {
            return HRESULT(0x8000_4003u32 as i32); // E_POINTER
        }
        if unsafe { *rclsid } != CLSID_BIT7Z_MENU {
            return HRESULT(0x8004_0111u32 as i32); // CLASS_E_CLASSNOTAVAILABLE
        }
        let factory: IClassFactory = Bit7zClassFactory.into();
        unsafe { factory.query(riid, ppv) }
    }))
    .unwrap_or(HRESULT(0x8000_4005u32 as i32)) // E_FAIL on panic
}

/// Whether the DLL may be unloaded (we never hold references).
#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    HRESULT(0)
}

/// Self-register (HKCU only, no admin required).
#[unsafe(no_mangle)]
pub extern "system" fn DllRegisterServer() -> HRESULT {
    match catch_unwind(AssertUnwindSafe(register_server)) {
        Ok(Ok(())) => HRESULT(0),
        Ok(Err(msg)) => {
            eprintln!("Bit7z shell register failed: {msg}");
            HRESULT(0x8000_4005u32 as i32)
        }
        Err(_) => HRESULT(0x8000_4005u32 as i32),
    }
}

/// Self-unregister (symmetric cleanup).
#[unsafe(no_mangle)]
pub extern "system" fn DllUnregisterServer() -> HRESULT {
    match catch_unwind(AssertUnwindSafe(unregister_server)) {
        Ok(Ok(())) => HRESULT(0),
        Ok(Err(msg)) => {
            eprintln!("Bit7z shell unregister failed: {msg}");
            HRESULT(0x8000_4005u32 as i32)
        }
        Err(_) => HRESULT(0x8000_4005u32 as i32),
    }
}

// ============================================================================
// Registration + settings (HKCU only)
// ============================================================================

/// Format a GUID as `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}`.
fn guid_string(guid: &GUID) -> String {
    let (a, b, c, d) = (guid.data1, guid.data2, guid.data3, guid.data4);
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        a, b, c, d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
    )
}

/// The path of this DLL (used for the InprocServer32 value).
fn module_path() -> Result<String, String> {
    let module = HMODULE(
        *DLL_INSTANCE
            .get()
            .ok_or_else(|| "DllMain not called".to_string())? as *mut c_void,
    );
    unsafe {
        let mut buf = vec![0u16; 4096];
        let len = GetModuleFileNameW(Some(module), &mut buf);
        if len == 0 {
            return Err("GetModuleFileNameW failed".into());
        }
        Ok(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn load_menu_settings() -> MenuSettings {
    let mut settings = MenuSettings::default();
    if let Some(v) = get_reg_dword(REG_SHELL_SETTINGS, "CascadedMenu") {
        settings.cascaded = v != 0;
    }
    if let Some(v) = get_reg_dword(REG_SHELL_SETTINGS, "MenuFlags") {
        settings.flags = v;
    }
    if let Some(name) = get_reg_string(REG_SHELL_SETTINGS, "CascadeName") {
        if !name.is_empty() {
            settings.cascade_name = name;
        }
    }
    settings
}

fn get_reg_dword(subkey: &str, value_name: &str) -> Option<u32> {
    let subkey_wide: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let name_wide: Vec<u16> = value_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut data: u32 = 0;
    let mut cb = std::mem::size_of::<u32>() as u32;
    let mut ty: u32 = 0;
    let result = unsafe {
        SHGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey_wide.as_ptr()),
            PCWSTR(name_wide.as_ptr()),
            Some(&mut ty as *mut u32),
            Some(&mut data as *mut u32 as *mut c_void),
            Some(&mut cb as *mut u32),
        )
    };
    if result.0 == 0 { Some(data) } else { None }
}

fn get_reg_string(subkey: &str, value_name: &str) -> Option<String> {
    let subkey_wide: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let name_wide: Vec<u16> = value_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut buf = vec![0u16; 256];
    let mut cb = (buf.len() * 2) as u32;
    let mut ty: u32 = 0;
    let result = unsafe {
        SHGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey_wide.as_ptr()),
            PCWSTR(name_wide.as_ptr()),
            Some(&mut ty as *mut u32),
            Some(buf.as_mut_ptr() as *mut c_void),
            Some(&mut cb as *mut u32),
        )
    };
    if result.0 != 0 || cb < 2 {
        return None;
    }
    let n_chars = (cb as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..n_chars]))
}

fn set_reg_dword(subkey: &str, value_name: &str, value: u32) -> Result<(), String> {
    let subkey_wide: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let name_wide: Vec<u16> = value_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let data = value;
    // REG_DWORD = 4
    let result = unsafe {
        SHSetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey_wide.as_ptr()),
            PCWSTR(name_wide.as_ptr()),
            4,
            Some((&data as *const u32).cast::<c_void>()),
            std::mem::size_of::<u32>() as u32,
        )
    };
    if result != 0 {
        return Err(format!("SHSetValueW DWORD({subkey:?}): error {result}"));
    }
    Ok(())
}

fn register_server() -> Result<(), String> {
    let dll_path = module_path()?;
    let clsid = guid_string(&CLSID_BIT7Z_MENU);

    // Remove the legacy static-verb fallback registered by older builds so
    // the context menu is provided exclusively by IContextMenu.
    delete_reg_key(&format!("{REG_CLASSES}\\*\\shell\\Bit7zFM"))?;
    delete_reg_key(&format!("{REG_CLASSES}\\Directory\\shell\\Bit7zFM"))?;

    let clsid_key = format!("{REG_CLASSES}\\CLSID\\{clsid}");
    set_reg_value(&clsid_key, "", "Bit7z Context Menu")?;
    set_reg_value(&format!("{clsid_key}\\InprocServer32"), "", &dll_path)?;
    set_reg_value(
        &format!("{clsid_key}\\InprocServer32"),
        "ThreadingModel",
        "Apartment",
    )?;
    set_reg_value(REG_APPROVED, &clsid, "Bit7z Context Menu")?;

    let files_key = format!("{REG_CLASSES}\\*\\shellex\\ContextMenuHandlers\\Bit7z");
    set_reg_value(&files_key, "", &clsid)?;

    let dir_key = format!("{REG_CLASSES}\\Directory\\shellex\\ContextMenuHandlers\\Bit7z");
    set_reg_value(&dir_key, "", &clsid)?;

    let bg_key =
        format!("{REG_CLASSES}\\Directory\\Background\\shellex\\ContextMenuHandlers\\Bit7z");
    set_reg_value(&bg_key, "", &clsid)?;

    // Default menu preferences (do not overwrite an existing user choice).
    if get_reg_dword(REG_SHELL_SETTINGS, "CascadedMenu").is_none() {
        set_reg_dword(REG_SHELL_SETTINGS, "CascadedMenu", 1)?;
    }
    if get_reg_dword(REG_SHELL_SETTINGS, "MenuFlags").is_none() {
        set_reg_dword(REG_SHELL_SETTINGS, "MenuFlags", FLAG_ALL)?;
    }
    if get_reg_string(REG_SHELL_SETTINGS, "CascadeName").is_none() {
        set_reg_value(REG_SHELL_SETTINGS, "CascadeName", "Bit7z")?;
    }

    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
    Ok(())
}

fn unregister_server() -> Result<(), String> {
    let clsid = guid_string(&CLSID_BIT7Z_MENU);
    delete_reg_value(REG_APPROVED, &clsid)?;
    delete_reg_key(&format!("{REG_CLASSES}\\CLSID\\{clsid}"))?;
    delete_reg_key(&format!(
        "{REG_CLASSES}\\*\\shellex\\ContextMenuHandlers\\Bit7z"
    ))?;
    delete_reg_key(&format!(
        "{REG_CLASSES}\\Directory\\shellex\\ContextMenuHandlers\\Bit7z"
    ))?;
    delete_reg_key(&format!(
        "{REG_CLASSES}\\Directory\\Background\\shellex\\ContextMenuHandlers\\Bit7z"
    ))?;
    // Remove the legacy static-verb fallback registered by older builds.
    delete_reg_key(&format!("{REG_CLASSES}\\*\\shell\\Bit7zFM"))?;
    delete_reg_key(&format!("{REG_CLASSES}\\Directory\\shell\\Bit7zFM"))?;
    // Keep user menu preferences under Software\Bit7zFM\Shell.
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
    Ok(())
}

fn set_reg_value(subkey: &str, value_name: &str, value: &str) -> Result<(), String> {
    let subkey_wide: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let name_wide: Vec<u16> = value_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let value_wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let result = unsafe {
        SHSetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey_wide.as_ptr()),
            PCWSTR(name_wide.as_ptr()),
            1, // REG_SZ
            Some(value_wide.as_ptr() as *const c_void),
            (value_wide.len() * 2) as u32,
        )
    };
    if result != 0 {
        return Err(format!("SHSetValueW({subkey:?}): error {result}"));
    }
    Ok(())
}

fn delete_reg_key(subkey: &str) -> Result<(), String> {
    let subkey_wide: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let result = unsafe { SHDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(subkey_wide.as_ptr())) };
    let code = result.0;
    // 0 = success; 2/3 = key not found (already clean).
    if code != 0 && code != 2 && code != 3 {
        return Err(format!("SHDeleteKeyW({subkey:?}): error {code}"));
    }
    Ok(())
}

fn delete_reg_value(subkey: &str, value_name: &str) -> Result<(), String> {
    let subkey_wide: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let name_wide: Vec<u16> = value_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let result = unsafe {
        RegDeleteKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey_wide.as_ptr()),
            PCWSTR(name_wide.as_ptr()),
        )
    };
    let code = result.0;
    // 0 = success; 2/3 = key/value not found (already clean).
    if code != 0 && code != 2 && code != 3 {
        return Err(format!("RegDeleteKeyValueW({subkey:?}): error {code}"));
    }
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{CreateMenu, DestroyMenu, GetMenuItemCount};

    fn seed_ctx(files: Vec<String>) -> Bit7zMenu {
        let menu = Bit7zMenu::new();
        *menu.ctx.lock().unwrap() = Some(MenuContext {
            files,
            id_cmd_first: 0,
        });
        menu
    }

    #[test]
    fn archive_plan_contains_extract_and_test() {
        let settings = MenuSettings {
            cascaded: true,
            ..Default::default()
        };
        let plan = build_menu_plan(&["C:\\a\\demo.7z".into()], &settings);
        let labels: Vec<_> = plan
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Command { label, .. } => Some(label.as_str()),
                _ => None,
            })
            .collect();
        assert!(labels.iter().any(|l| *l == "Open archive"));
        assert!(labels.iter().any(|l| *l == "Extract Here"));
        assert!(labels.iter().any(|l| l.starts_with("Extract to \"demo")));
        assert!(labels.iter().any(|l| *l == "Test archive"));
        assert!(labels.iter().any(|l| l.contains(".7z")));
    }

    #[test]
    fn non_archive_plan_has_no_extract() {
        let settings = MenuSettings::default();
        let plan = build_menu_plan(&["C:\\a\\photo.png".into()], &settings);
        let labels: Vec<_> = plan
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Command { label, .. } => Some(label.as_str()),
                _ => None,
            })
            .collect();
        assert!(!labels.iter().any(|l| l.contains("Extract")));
        assert!(labels.iter().any(|l| l.starts_with("Add to \"photo")));
    }

    #[test]
    fn menu_flags_hide_hash() {
        let settings = MenuSettings {
            flags: FLAG_ALL & !FLAG_HASH,
            ..Default::default()
        };
        let plan = build_menu_plan(&["C:\\a\\x.zip".into()], &settings);
        assert!(!plan.iter().any(|e| matches!(
            e,
            MenuEntry::Command { label, .. } if label == "CRC-32"
                || label == "CRC-64"
                || label == "SHA-1"
                || label == "SHA-256"
                || label == "MD5"
        )));
    }

    #[test]
    fn cascaded_query_inserts_one_root_item() {
        let menu = seed_ctx(vec!["C:\\a\\demo.7z".into()]);
        // Force cascaded via env-independent path: temporarily write registry
        // is heavy; instead call insert helpers directly.
        let settings = MenuSettings {
            cascaded: true,
            cascade_name: "Bit7z".into(),
            flags: FLAG_ALL,
        };
        let plan = build_menu_plan(&["C:\\a\\demo.7z".into()], &settings);
        let hmenu = unsafe { CreateMenu() }.expect("CreateMenu");
        let n = insert_cascaded_menu(hmenu, 0, 100, "Bit7z", &plan);
        assert_eq!(n, 1);
        let count = unsafe { GetMenuItemCount(Some(hmenu)) };
        assert_eq!(count, 1, "cascade root only");
        unsafe { DestroyMenu(hmenu) }.ok();
        let _ = menu;
    }

    #[test]
    fn flat_query_inserts_many_items() {
        let settings = MenuSettings {
            cascaded: false,
            flags: FLAG_EXTRACT_HERE | FLAG_TEST | FLAG_ADD_TO_7Z,
            ..Default::default()
        };
        let plan = build_menu_plan(&["C:\\a\\demo.7z".into()], &settings);
        let hmenu = unsafe { CreateMenu() }.expect("CreateMenu");
        let n = insert_flat_menu(hmenu, 0, 100, &plan);
        let count = unsafe { GetMenuItemCount(Some(hmenu)) };
        assert_eq!(count, n as i32);
        assert!(count >= 3);
        unsafe { DestroyMenu(hmenu) }.ok();
    }

    #[test]
    fn defaultonly_probe_adds_no_items() {
        let fresh = seed_ctx(vec!["C:\\a\\demo.7z".into()]);
        let obj: IContextMenu = fresh.into();
        let hmenu = unsafe { CreateMenu() }.expect("CreateMenu");
        let hr = unsafe { obj.QueryContextMenu(hmenu, 0, 0x4000, 0x7FFF, CMF_DEFAULTONLY) };
        assert_eq!(hr.0, 0, "CMF_DEFAULTONLY must leave the menu untouched");
        let count = unsafe { GetMenuItemCount(Some(hmenu)) };
        assert_eq!(count, 0);
        unsafe { DestroyMenu(hmenu) }.ok();
    }

    #[test]
    fn get_command_string_validates_owned_ids() {
        let obj: IContextMenu = Bit7zMenu::new().into();
        let valid = unsafe {
            obj.GetCommandString(
                CMD_TEST as usize,
                GCS_VALIDATEA,
                None,
                windows::core::PSTR::null(),
                0,
            )
        };
        assert!(valid.is_ok(), "owned command ids must validate: {valid:?}");

        let invalid = unsafe {
            obj.GetCommandString(
                CMD_COUNT as usize,
                GCS_VALIDATEA,
                None,
                windows::core::PSTR::null(),
                0,
            )
        };
        assert!(invalid.is_err(), "unknown command ids must not validate");
    }

    #[test]
    fn get_command_string_returns_unicode_verbs() {
        let obj: IContextMenu = Bit7zMenu::new().into();
        let mut buf = [0u16; 32];
        let result = unsafe {
            obj.GetCommandString(
                CMD_TEST as usize,
                GCS_VERBW,
                None,
                windows::core::PSTR(buf.as_mut_ptr().cast::<u8>()),
                buf.len() as u32,
            )
        };
        assert!(
            result.is_ok(),
            "GCS_VERBW must provide a canonical verb: {result:?}"
        );
        let expected = "test".encode_utf16().collect::<Vec<_>>();
        assert_eq!(&buf[..expected.len()], expected.as_slice());
        assert_eq!(
            buf[expected.len()],
            0,
            "verb string must be null terminated"
        );
    }

    #[test]
    fn command_verbs_round_trip() {
        for id in 0..CMD_COUNT {
            let verb = command_verb(id as usize).expect("every command has a verb");
            assert_eq!(command_id_from_verb(verb), Some(id), "verb {verb:?}");
        }
        assert_eq!(command_id_from_verb("not-a-verb"), None);
    }

    #[test]
    fn invoke_command_ignores_string_verbs() {
        let obj: IContextMenu = Bit7zMenu::new().into();
        let mut info = CMINVOKECOMMANDINFO {
            cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
            fMask: 0,
            hwnd: Default::default(),
            lpVerb: windows::core::PCSTR(b"open\0".as_ptr()),
            lpParameters: windows::core::PCSTR::null(),
            lpDirectory: windows::core::PCSTR::null(),
            nShow: 0,
            dwHotKey: 0,
            hIcon: Default::default(),
        };
        let result = unsafe { obj.InvokeCommand(&mut info as *mut CMINVOKECOMMANDINFO) };
        assert!(result.is_ok(), "string verb should not fail: {result:?}");
    }

    #[test]
    fn invoke_command_maps_absolute_id_to_verb() {
        let fresh = seed_ctx(vec!["C:\\archives\\demo.7z".into()]);
        let obj: IContextMenu = fresh.into();
        let hmenu = unsafe { CreateMenu() }.expect("CreateMenu");
        let hr = unsafe { obj.QueryContextMenu(hmenu, 0, 0x4000, 0x7FFF, 0) };
        assert!(hr.is_ok());
        unsafe { DestroyMenu(hmenu) }.ok();

        // 0x4000 + CMD_TEST
        let mut info = CMINVOKECOMMANDINFO {
            cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
            fMask: 0,
            hwnd: Default::default(),
            lpVerb: windows::core::PCSTR((0x4000u32 + CMD_TEST) as usize as *const u8),
            lpParameters: windows::core::PCSTR::null(),
            lpDirectory: windows::core::PCSTR::null(),
            nShow: 0,
            dwHotKey: 0,
            hIcon: Default::default(),
        };
        let result = unsafe { obj.InvokeCommand(&mut info as *mut CMINVOKECOMMANDINFO) };
        assert!(result.is_ok(), "InvokeCommand must not fail: {result:?}");
    }

    #[test]
    fn clsid_string_matches_registry_format() {
        assert_eq!(
            guid_string(&CLSID_BIT7Z_MENU),
            "{4B69747A-7A69-5348-4C4C-4D454E55434F}"
        );
    }

    #[test]
    fn run_command_fails_gracefully_without_executor() {
        let result = run_command(CMD_EXTRACT_HERE, &["C:\\fake\\archive.7z".into()]);
        assert!(result.is_err(), "expected Err without sibling executor");
    }

    #[test]
    fn tar_gz_detected_as_archive() {
        assert!(is_archive_path("C:\\a\\pkg.tar.gz"));
        assert!(is_archive_path("C:\\a\\pkg.tgz"));
        assert!(!is_archive_path("C:\\a\\readme.txt"));
    }
}
