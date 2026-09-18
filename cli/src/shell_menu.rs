//! Sparse MSIX registration for the Windows 11 Explorer context menu
//! (`bit7z shell-menu-install` / `shell-menu-uninstall`).
//!
//! Two parts:
//! 1. `HKCU\Software\Classes\CLSID\<root clsid>\InprocServer32` pointing at
//!    `b7zmenu.dll` (Explorer CoActivates the verb's CLSID in-process).
//! 2. A sparse MSIX package (`appxmanifest.xml` next to the DLL) added via
//!    `PackageManager.AddPackageByUriAsync` with `AllowUnsigned` and an
//!    external-location URI so the DLL stays beside the app binaries.
//!
//! Unsigned sparse packages require Developer Mode (or a trusted self-signed
//! certificate). The menu itself is a Windows 11 feature.

use windows::core::{HSTRING, PCWSTR};
use windows::Foundation::Uri;
use windows::Management::Deployment::{AddPackageOptions, PackageManager};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::UI::Shell::SHChangeNotify;
use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_FLAGS};

/// The registry key name is derived from `explorer_menu::CLSID_ROOT` — the
/// one source shared with the host DLL and the manifest Verb Clsid.
const PACKAGE_NAME: &str = "Bit7zFM.ShellMenu";

/// Locates `b7zmenu.dll` beside the CLI (or in the usual target dirs).
fn find_menu_dll() -> Option<std::path::PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let candidates = [
        exe_dir.join("b7zmenu.dll"),
        std::path::PathBuf::from("target/debug/b7zmenu.dll"),
        std::path::PathBuf::from("target/release/b7zmenu.dll"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

fn pcw(s: &HSTRING) -> PCWSTR {
    PCWSTR(s.as_ptr())
}


/// UTF-16 LE bytes with a null terminator — the wire format of REG_SZ.
fn registry_sz(value: &str) -> Vec<u8> {
    let mut bytes: Vec<u8> = value
        .encode_utf16()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    bytes.extend_from_slice(&[0, 0]);
    bytes
}

fn write_clsid(dll_path: &std::path::Path) -> Result<(), String> {
    let root = explorer_menu::clsid_root_string();
    unsafe {
        let clsid_key = HSTRING::from(format!("Software\\Classes\\CLSID\\{root}"));
        let inproc_key =
            HSTRING::from(format!("Software\\Classes\\CLSID\\{root}\\InprocServer32"));

        let mut key = HKEY::default();
        let err = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            pcw(&clsid_key),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        );
        if err != ERROR_SUCCESS {
            return Err(format!("RegCreateKeyExW(CLSID): {:?}", err));
        }
        RegCloseKey(key).ok();

        let mut key = HKEY::default();
        let err = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            pcw(&inproc_key),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        );
        if err != ERROR_SUCCESS {
            return Err(format!("RegCreateKeyExW(InprocServer32): {:?}", err));
        }

        let dll_bytes = registry_sz(&dll_path.display().to_string());
        let _ = RegSetValueExW(key, None, None, REG_SZ, Some(&dll_bytes));
        let apartment_bytes = registry_sz("Apartment");
        let threading = HSTRING::from("ThreadingModel");
        let _ = RegSetValueExW(
            key,
            pcw(&threading),
            None,
            REG_SZ,
            Some(&apartment_bytes),
        );
        RegCloseKey(key).ok();
    }
    Ok(())
}

fn delete_clsid() -> Result<(), String> {
    let root = explorer_menu::clsid_root_string();
    unsafe {
        let clsid_key = HSTRING::from(format!("Software\\Classes\\CLSID\\{root}"));
        let err = RegDeleteTreeW(HKEY_CURRENT_USER, pcw(&clsid_key));
        // 2 = not found: fine when uninstalling twice.
        if err != ERROR_SUCCESS && err != windows::Win32::Foundation::WIN32_ERROR(2) {
            return Err(format!("RegDeleteTreeW: {:?}", err));
        }
    }
    Ok(())
}

/// Registers the sparse package (Win11). The manifest must sit next to the
/// DLL; the DLL's directory becomes the package's external content location.
fn register_sparse_package(dll_path: &std::path::Path) -> Result<(), String> {
    let manifest = dll_path.with_file_name("appxmanifest.xml");
    if !manifest.is_file() {
        return Err(format!(
            "appxmanifest.xml not found next to {} — ship it with the DLL",
            dll_path.display()
        ));
    }
    let external = dll_path
        .parent()
        .ok_or("dll has no parent dir")?
        .to_path_buf();

    let package_manager = PackageManager::new().map_err(|e| e.to_string())?;
    let options = AddPackageOptions::new().map_err(|e| e.to_string())?;
    options.SetAllowUnsigned(true).map_err(|e| e.to_string())?;
    let external_uri = uri_from_path(&external)?;
    options
        .SetExternalLocationUri(&external_uri)
        .map_err(|e| e.to_string())?;

    let manifest_uri = uri_from_path(&manifest)?;
    let deployment = package_manager
        .AddPackageByUriAsync(&manifest_uri, &options)
        .map_err(|e| e.to_string())?;
    deployment.get().map_err(|e| e.to_string())?;
    Ok(())
}

fn unregister_sparse_package() -> Result<(), String> {
    let manager = PackageManager::new().map_err(|e| e.to_string())?;
    let removal = manager
        .RemovePackageAsync(&HSTRING::from(PACKAGE_NAME))
        .map_err(|e| e.to_string())?;
    removal.get().map_err(|e| e.to_string())?;
    Ok(())
}

fn uri_from_path(path: &std::path::Path) -> Result<Uri, String> {
    let absolute = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    let text = format!("file:///{}", absolute.to_string_lossy().replace('\\', "/"));
    Uri::CreateUri(&HSTRING::from(text)).map_err(|e| e.to_string())
}

fn notify_shell() {
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_FLAGS(0), None, None);
    }
}

/// `bit7z shell-menu-install`
pub fn install() -> Result<(), String> {
    let dll = find_menu_dll()
        .ok_or("b7zmenu.dll not found (build the explorer-menu-host crate first)")?;
    write_clsid(&dll)?;
    register_sparse_package(&dll)?;
    notify_shell();
    println!("Explorer context menu registered (restart Explorer or reboot if the menu does not appear)");
    Ok(())
}

/// `bit7z shell-menu-uninstall`
pub fn uninstall() -> Result<(), String> {
    let result = unregister_sparse_package();
    delete_clsid()?;
    notify_shell();
    result?;
    println!("Explorer context menu unregistered");
    Ok(())
}
