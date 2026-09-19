use std::io;
use std::io::IsTerminal;
use anyhow::Context;
use util::ResultExt;

pub(crate) fn stdout_is_a_pty() -> bool {
     io::stdout().is_terminal()
}

/// Points stdout/stderr at NUL when they are pipes.
///
/// This process can outlive whoever launched it (updaters, job runners,
/// shells). When the parent's pipe read end closes, every later
/// `eprintln!` — including Rust's own panic reporting — fails, and the
/// failure *panics*, taking the app down. Real logging lives in zlog's file
/// sink, so a pipe that can no longer be trusted is replaced with the
/// always-writable NUL device instead. Console handles are left alone so
/// terminal launches keep their output.
#[cfg(target_os = "windows")]
pub(crate) fn redirect_pipe_std_streams_to_nul() {
    use windows::Win32::Foundation::GENERIC_WRITE;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, GetFileType, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
        FILE_TYPE_PIPE, OPEN_EXISTING,
    };
    use windows::Win32::System::Console::{
        GetStdHandle, SetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
    };
    use windows::core::w;

    for handle_id in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        let Ok(handle) = (unsafe { GetStdHandle(handle_id) }) else {
            continue;
        };
        if handle.is_invalid() || unsafe { GetFileType(handle) } != FILE_TYPE_PIPE {
            continue;
        }
        // The NUL device: writes always succeed, whatever the other end does.
        let Ok(nul) = (unsafe {
            CreateFileW(
                w!(r"\\.\NUL"),
                GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        }) else {
            continue;
        };
        // Rust std re-queries `GetStdHandle` on every write, so this lands
        // even if something already printed.
        let _ = unsafe { SetStdHandle(handle_id, nul) };
    }
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
