//! Explorer shell extension: a COM DLL loaded by Explorer.exe that adds
//! archive context-menu verbs (extract here, extract to, add to archive,
//! test, open).
//!
//! Safety model: this DLL runs *inside* Explorer.exe, so every exported
//! entry point is wrapped in `catch_unwind`, no panic may cross the FFI
//! boundary, and the DLL performs no work beyond writing a job file and
//! launching the executor process. The DLL never touches the archive itself.
//!
//! Registration (self-register / unregister, HKCU only, no admin needed):
//! - `regsvr32 bit7z_shell.dll`  -> `DllRegisterServer`
//! - `regsvr32 /u bit7z_shell.dll` -> `DllUnregisterServer`
//! - `bit7z shell-install` / `bit7z shell-uninstall` (same entry points)

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use windows::core::{GUID, HRESULT, PCWSTR, PWSTR, Ref, BOOL, IUnknown, Interface, IUnknownImpl, implement, w};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::Com::{
    DVASPECT_CONTENT, FORMATETC, IClassFactory, IClassFactory_Impl, IDataObject, TYMED_HGLOBAL,
};
use windows::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GetModuleFileNameW, GetModuleHandleExW,
};
use windows::Win32::System::Registry::HKEY_CURRENT_USER;
use windows::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessW, PROCESS_INFORMATION, STARTF_USESHOWWINDOW,
    STARTUPINFOW,
};
use windows::Win32::UI::Shell::{
    CMINVOKECOMMANDINFO, DragFinish, DragQueryFileW, HDROP, IContextMenu, IContextMenu_Impl,
    IShellExtInit, IShellExtInit_Impl, SHDeleteKeyW, SHSetValueW, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    HMENU, InsertMenuItemW, MENUITEMINFOW, MIIM_STRING, SW_HIDE, SW_SHOWNORMAL,
};

/// The COM class id of this extension.
pub const CLSID_BIT7Z_MENU: GUID = GUID::from_u128(0x4b69747a_7a69_5348_4c4c_4d454e55434f);

/// Registry subtree for per-user class registration.
const REG_CLASSES: &str = "Software\\Classes";

/// Context captured by `IShellExtInit::Initialize`.
struct MenuContext {
    files: Vec<String>,
}

/// The shell extension object (single COM class, two interfaces).
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
        *self.get_impl().ctx.lock().unwrap() = Some(MenuContext { files });
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
        _u_flags: u32,
    ) -> HRESULT {
        let items = [
            (0u32, "Extract here"),
            (1u32, "Extract to..."),
            (2u32, "Add to archive..."),
            (3u32, "Test archive"),
            (4u32, "Open with Bit7zFM"),
        ];
        let mut count = 0u32;
        for (i, (cmd, label)) in items.iter().enumerate() {
            if i > 0 {
                // Separator between the first item and the rest.
                let sep = MENUITEMINFOW {
                    cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
                    fMask: MIIM_STRING,
                    fType: Default::default(),
                    fState: Default::default(),
                    wID: 0,
                    hSubMenu: HMENU::default(),
                    hbmpChecked: Default::default(),
                    hbmpUnchecked: Default::default(),
                    dwItemData: 0,
                    dwTypeData: PWSTR::null(),
                    cch: 0,
                    hbmpItem: Default::default(),
                };
                unsafe {
                    let _ = InsertMenuItemW(hmenu, index_menu + count, true, &sep);
                }
                count += 1;
            }
            let mut wide: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
            let item = MENUITEMINFOW {
                cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
                fMask: MIIM_STRING,
                fType: Default::default(),
                fState: Default::default(),
                wID: id_cmd_first + cmd,
                hSubMenu: HMENU::default(),
                hbmpChecked: Default::default(),
                hbmpUnchecked: Default::default(),
                dwItemData: 0,
                dwTypeData: PWSTR(wide.as_mut_ptr()),
                cch: wide.len() as u32,
                hbmpItem: Default::default(),
            };
            unsafe {
                let _ = InsertMenuItemW(hmenu, index_menu + count, true, &item);
            }
            count += 1;
        }
        // Low 16 bits carry the number of items added.
        HRESULT(count as i32)
    }

    fn InvokeCommand(&self, pici: *const CMINVOKECOMMANDINFO) -> windows::core::Result<()> {
        if pici.is_null() {
            return Ok(());
        }
        let info = unsafe { &*pici };
        // `lpVerb` is either an integer resource id (low 16 bits) or a
        // string; we only support integer ids.
        let verb = info.lpVerb.as_ptr() as usize;
        if (verb >> 16) != 0 {
            return Ok(());
        }
        let command_id = verb as u32;
        let context = self.get_impl().ctx.lock().unwrap().take();
        let Some(context) = context else {
            return Ok(());
        };
        if context.files.is_empty() {
            return Ok(());
        }
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _ = run_command(command_id, &context.files);
        }));
        Ok(())
    }

    fn GetCommandString(
        &self,
        _id_cmd: usize,
        _u_type: u32,
        _p_reserved: *const u32,
        _psz_name: windows::core::PSTR,
        _cch_max: u32,
    ) -> windows::core::Result<()> {
        Ok(())
    }
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
    let Ok(medium) = (unsafe { data_object.GetData(&formatetc) }) else {
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
    // Release the HGLOBAL (and any data-object-owned release).
    unsafe { DragFinish(hdrop) };
    let unk = std::mem::ManuallyDrop::into_inner(medium.pUnkForRelease);
    if let Some(unk) = unk {
        drop(unk);
    }
    files
}

// ============================================================================
// Command execution: write a job file, launch the executor
// ============================================================================

/// Execute a context-menu command.
fn run_command(command_id: u32, files: &[String]) -> Result<(), String> {
    let Some(executor) = find_sibling_exe("bit7z-executor.exe") else {
        return Err("bit7z-executor.exe not found next to shell DLL".into());
    };
    let first = PathBuf::from(&files[0]);
    let parent = first
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let spec = match command_id {
        // Extract here -> into the archive's directory.
        0 => {
            let archive = first.clone();
            let stem = archive
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "extracted".into());
            task::JobSpec::Extract {
                archive,
                items: Vec::new(),
                target: parent.join(&stem),
                overwrite: task::OverwriteSpec::Ask,
                password_hint: false,
            }
        }
        // Extract to -> current directory of the selection.
        1 => {
            let archive = first.clone();
            task::JobSpec::Extract {
                archive,
                items: Vec::new(),
                target: parent,
                overwrite: task::OverwriteSpec::Ask,
                password_hint: false,
            }
        }
        // Add to archive -> sibling `.7z` named after the first file.
        2 => {
            let stem = first
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "archive".into());
            task::JobSpec::Compress {
                inputs: files.iter().map(PathBuf::from).collect(),
                target: parent.join(format!("{stem}.7z")),
                format: task::FormatSpec::SevenZip,
                level: task::LevelSpec::Normal,
                solid: None,
                volume: None,
                threads: None,
                encrypt_headers: false,
                password_hint: false,
            }
        }
        // Test -> validate the archive integrity.
        3 => task::JobSpec::Test {
            archive: first.clone(),
            password_hint: false,
        },
        // Open with the file manager.
        4 => {
            return open_with_manager(&first).map_err(|e| e.to_string());
        }
        _ => return Ok(()),
    };

    // Persist the job file and hand it to the executor process.
    let Ok(jobs_dir) = temp::ensure_jobs_dir() else {
        return Err("cannot create jobs dir".into());
    };
    let job_id = format!("shell-{}-{}", std::process::id(), command_id);
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
    Ok(())
}

/// Open a path with the default association (or the file manager if present).
fn open_with_manager(path: &Path) -> windows::core::Result<()> {
    let file = windows::core::HSTRING::from(path);
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &file,
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    Ok(())
}

/// Locate an exe shipped next to this DLL.
fn find_sibling_exe(name: &str) -> Option<PathBuf> {
    let mut module = HMODULE(std::ptr::null_mut());
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
            PCWSTR(find_sibling_exe as *const () as usize as *const u16),
            &mut module,
        )
        .ok()?;
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
        if !punk_outer.is_null() {
            return Err(windows::core::Error::from_hresult(HRESULT(
                0x8000_4001u32 as i32,
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
        if rclsid.is_null() || riid.is_null() || ppv.is_null() {
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
// Registration (HKCU only: symmetric, no admin)
// ============================================================================

/// Format a GUID as `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}`.
fn guid_string(guid: &GUID) -> String {
    let (a, b, c, d) = (
        guid.data1,
        guid.data2,
        guid.data3,
        guid.data4,
    );
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        a, b, c, d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
    )
}

/// The path of this DLL (used for the InprocServer32 value).
fn module_path() -> Result<String, String> {
    let mut module = HMODULE(std::ptr::null_mut());
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
            PCWSTR(module_path as *const () as usize as *const u16),
            &mut module,
        )
        .map_err(|e| format!("GetModuleHandleExW: {e}"))?;
        let mut buf = vec![0u16; 4096];
        let len = GetModuleFileNameW(Some(module), &mut buf);
        if len == 0 {
            return Err("GetModuleFileNameW failed".into());
        }
        Ok(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn register_server() -> Result<(), String> {
    let dll_path = module_path()?;
    let clsid = guid_string(&CLSID_BIT7Z_MENU);

    // HKCU\\Software\\Classes\\CLSID\\{guid}
    let clsid_key = format!("{REG_CLASSES}\\CLSID\\{clsid}");
    set_reg_value(&clsid_key, "", "Bit7z Context Menu")?;
    set_reg_value(&format!("{clsid_key}\\InprocServer32"), "", &dll_path)?;
    set_reg_value(&format!("{clsid_key}\\InprocServer32"), "ThreadingModel", "Apartment")?;

    // File context menu handler.
    let files_key = format!("{REG_CLASSES}\\*\\shellex\\ContextMenuHandlers\\Bit7z");
    set_reg_value(&files_key, "", &clsid)?;

    // Directory context menu handler (background + items).
    let dir_key = format!("{REG_CLASSES}\\Directory\\shellex\\ContextMenuHandlers\\Bit7z");
    set_reg_value(&dir_key, "", &clsid)?;

    Ok(())
}

fn unregister_server() -> Result<(), String> {
    let clsid = guid_string(&CLSID_BIT7Z_MENU);
    // Delete the CLSID tree (recursive delete of everything we wrote).
    delete_reg_key(&format!("{REG_CLASSES}\\CLSID\\{clsid}"))?;
    delete_reg_key(&format!("{REG_CLASSES}\\*\\shellex\\ContextMenuHandlers\\Bit7z"))?;
    delete_reg_key(&format!("{REG_CLASSES}\\Directory\\shellex\\ContextMenuHandlers\\Bit7z"))?;
    Ok(())
}

fn set_reg_value(subkey: &str, value_name: &str, value: &str) -> Result<(), String> {
    let subkey_wide: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let name_wide: Vec<u16> = value_name.encode_utf16().chain(std::iter::once(0)).collect();
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{CreateMenu, DestroyMenu, GetMenuItemCount};

    /// QueryContextMenu must insert all verbs (4 separators + 5 items = 9).
    #[test]
    fn query_context_menu_adds_verbs() {
        let menu = unsafe { CreateMenu() }.expect("CreateMenu failed");
        let obj: IContextMenu = Bit7zMenu::new().into();
        let hr = unsafe { obj.QueryContextMenu(menu, 0, 100, 200, 0) };
        assert!(hr.is_ok(), "QueryContextMenu failed: {hr:?}");
        let count = unsafe { GetMenuItemCount(Some(menu)) };
        assert_eq!(count, 9, "expected 4 separators + 5 verbs");
        unsafe { DestroyMenu(menu) }.expect("DestroyMenu failed");
    }

    /// A command id in the low 16 bits maps to a known verb; string verbs are
    /// rejected (InvokeCommand returns Ok without launching).
    #[test]
    fn invoke_command_ignores_string_verbs() {
        let obj: IContextMenu = Bit7zMenu::new().into();
        // No context was initialized, so even a valid id is a no-op, but the
        // call must not panic and must return Ok.
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
        assert!(result.is_ok(), "string verb should be ignored: {result:?}");
    }

    /// An integer command id without context is a safe no-op.
    #[test]
    fn invoke_command_integer_id_without_context_is_noop() {
        let obj: IContextMenu = Bit7zMenu::new().into();
        let mut info = CMINVOKECOMMANDINFO {
            cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
            fMask: 0,
            hwnd: Default::default(),
            lpVerb: windows::core::PCSTR(2usize as *const u8),
            lpParameters: windows::core::PCSTR::null(),
            lpDirectory: windows::core::PCSTR::null(),
            nShow: 0,
            dwHotKey: 0,
            hIcon: Default::default(),
        };
        let result = unsafe { obj.InvokeCommand(&mut info as *mut CMINVOKECOMMANDINFO) };
        assert!(result.is_ok());
    }

    /// run_command for a missing sibling exe must fail gracefully (no panic).
    #[test]
    fn run_command_fails_gracefully_without_executor() {
        let result = run_command(0, &["C:\\fake\\archive.7z".into()]);
        assert!(result.is_err(), "expected Err without sibling executor");
    }
}