//! Detached job launching for out-of-process hosts (shell extensions, the
//! CLI's `--gui` mode): serialize a [`JobSpec`] into the shared jobs
//! directory and start `bit7z-executor.exe` on it, detached from the caller.
//!
//! The caller supplies the directory that holds `bit7z-executor.exe` — for a
//! shell DLL that is its own module directory, for the CLI the running exe's
//! directory. Windows-only by nature (`CreateProcessW`); other platforms get
//! compile-time stubs.

use crate::job::{JobFile, JobSpec};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static JOB_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Writes `spec` as a job file into the shared jobs directory and launches a
/// detached `bit7z-executor.exe` on it.
#[cfg(windows)]
pub fn launch_job(executor_dir: &Path, spec: JobSpec) -> Result<(), String> {
    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::System::Threading::{
        PROCESS_INFORMATION, STARTF_USESHOWWINDOW, STARTUPINFOW, CREATE_UNICODE_ENVIRONMENT,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let executor = executor_dir.join("bit7z-executor.exe");
    if !executor.is_file() {
        return Err(format!("executor not found: {}", executor.display()));
    }
    let jobs_dir = temp::ensure_jobs_dir().map_err(|e| e.to_string())?;
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
    let file = JobFile::new(job_id, spec);
    let json = file.to_json().map_err(|e| e.to_string())?;
    std::fs::write(&job_path, json).map_err(|e| e.to_string())?;

    let app = HSTRING::from(executor.as_os_str());
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
        windows::Win32::System::Threading::CreateProcessW(
            &app,
            Some(PWSTR(cmdline.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_UNICODE_ENVIRONMENT,
            None,
            None,
            &si,
            &mut pi,
        )
    }
    .map_err(|e| format!("CreateProcessW: {e}"))?;
    unsafe {
        let _ = windows::Win32::Foundation::CloseHandle(pi.hThread);
        let _ = windows::Win32::Foundation::CloseHandle(pi.hProcess);
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn launch_job(_executor_dir: &Path, _spec: JobSpec) -> Result<(), String> {
    Err("job launching is Windows-only".into())
}

/// Convenience wrapper that builds a `Checksum` job for one file.
pub fn launch_checksum(
    executor_dir: &Path,
    path: &Path,
    algorithm: checksum::ChecksumAlgorithm,
) -> Result<(), String> {
    launch_job(
        executor_dir,
        JobSpec::Checksum {
            path: path.to_path_buf(),
            algorithm,
        },
    )
}
