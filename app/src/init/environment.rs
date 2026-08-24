use std::io;
use std::io::IsTerminal;
use anyhow::Context;
use util::ResultExt;

pub(crate) fn stdout_is_a_pty() -> bool {
     io::stdout().is_terminal()
}

#[cfg(target_os = "windows")]
pub(crate) fn check_for_conpty_dll() {
    use windows::{
        Win32::{Foundation::FreeLibrary, System::LibraryLoader::LoadLibraryW},
        core::w,
    };

    if let Ok(hmodule) = unsafe { LoadLibraryW(w!("conpty.dll")) } {
        unsafe {
            FreeLibrary(hmodule)
                .context("Failed to free conpty.dll")
                .log_err();
        }
    } else {
        log::warn!("Failed to load conpty.dll. Terminal will work with reduced functionality.");
    }
}
