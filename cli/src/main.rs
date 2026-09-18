mod arg;

use arg::{Cli, Commands};
use bit7z_rs::{ArchiveEngine, Bit7zEngine, OverwriteMode};
use clap::Parser;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use task::{FormatSpec, JobSpec, LevelSpec, OverwriteSpec, TaskRunner};

fn main() {
    let cli = Cli::parse();
    let Some(command) = cli.command else {
        // No command: point the user at the help output.
        println!("run with --help to see available commands");
        return;
    };

    let engine: Arc<dyn ArchiveEngine> =
        Arc::new(match Bit7zEngine::new(bit7z_rs::locate_dll().as_deref()) {
            Ok(engine) => engine,
            Err(error) => {
                eprintln!("failed to load 7-Zip engine: {error}");
                std::process::exit(1);
            }
        });
    let runner = TaskRunner::new(engine.clone());

    let result = run(command, &runner, &engine, cli.gui);
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run(
    command: Commands,
    runner: &TaskRunner,
    engine: &Arc<dyn ArchiveEngine>,
    gui: bool,
) -> Result<(), String> {
    match command {
        Commands::Open { path, password: _ } => {
            return launch_app(&[PathBuf::from(&path)]);
        }
        Commands::List { path, password } => {
            return list_archive(&path, password, engine);
        }
        Commands::Extract {
            path,
            to,
            indices,
            password,
        } => {
            let target = to.map(PathBuf::from).unwrap_or_else(|| {
                let mut p = PathBuf::from(&path);
                p.set_extension("");
                p
            });
            let items = parse_indices(indices.as_deref())?;
            let job = JobSpec::Extract {
                archive: PathBuf::from(&path),
                items,
                target,
                overwrite: OverwriteSpec::Overwrite,
                password_hint: password.is_some(),
            };
            run_job_or_launch(job, password, runner, gui)
        }
        Commands::Test { path, password } => {
            let job = JobSpec::Test {
                archive: PathBuf::from(&path),
                password_hint: password.is_some(),
            };
            run_job_or_launch(job, password, runner, gui)
        }
        Commands::Compress {
            files,
            to,
            format,
            password,
            encrypt_headers,
        } => {
            let target = to.map(PathBuf::from).unwrap_or_else(|| {
                let first = files.first().cloned().unwrap_or_else(|| "archive".into());
                let mut p = PathBuf::from(first);
                p.set_extension(format.as_str());
                p
            });
            let format = parse_format(&format)?;
            for input in &files {
                let input = PathBuf::from(input);
                if input == target || (input.is_dir() && target.starts_with(&input)) {
                    return Err(format!(
                        "output archive {} is inside an input path; choose a different target",
                        target.display()
                    ));
                }
            }
            let job = JobSpec::Compress {
                inputs: files.iter().map(PathBuf::from).collect(),
                target,
                format,
                level: LevelSpec::Normal,
                solid: None,
                volume: None,
                threads: None,
                encrypt_headers: encrypt_headers,
                password_hint: password.is_some(),
            };
            run_job_or_launch(job, password, runner, gui)
        }
        Commands::Preview {
            path,
            index,
            password,
            max_bytes,
        } => {
            let pw = password.as_deref().map(password::Password::new);
            let (extracted, total_size) =
                extract_entry_to_temp(engine, Path::new(&path), index, pw.as_ref())?;
            let mut file = std::fs::File::open(&extracted).map_err(|e| e.to_string())?;
            let mut shown = vec![0u8; max_bytes];
            let n = std::io::Read::read(&mut file, &mut shown).map_err(|e| e.to_string())?;
            shown.truncate(n);
            if shown
                .iter()
                .all(|b| b.is_ascii_graphic() || b.is_ascii_whitespace())
            {
                println!("{}", String::from_utf8_lossy(&shown));
            } else {
                for (i, chunk) in shown.chunks(16).enumerate() {
                    let hex: Vec<String> = chunk.iter().map(|b| format!("{:02x}", b)).collect();
                    println!("{:08x}  {}", i * 16, hex.join(" "));
                }
            }
            if total_size > max_bytes as u64 {
                println!("... ({} bytes total)", total_size);
            }
            let _ = std::fs::remove_dir_all(extracted.parent().unwrap_or(Path::new(".")));
            Ok(())
        }
        Commands::Add {
            path,
            files,
            password,
        } => {
            let job = JobSpec::Add {
                archive: PathBuf::from(&path),
                items: files
                    .iter()
                    .map(|f| task::AddItem {
                        fs_path: PathBuf::from(f),
                        archive_path: Path::new(f)
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| f.clone()),
                    })
                    .collect(),
                password_hint: password.is_some(),
            };
            run_job_or_launch(job, password, runner, gui)
        }
        Commands::Delete {
            path,
            indices,
            password,
        } => {
            let job = JobSpec::Delete {
                archive: PathBuf::from(&path),
                indices,
                password_hint: password.is_some(),
            };
            run_job_or_launch(job, password, runner, gui)
        }
        Commands::Rename {
            path,
            index,
            name,
            password,
        } => {
            let job = JobSpec::Rename {
                archive: PathBuf::from(&path),
                index,
                new_path: name,
                password_hint: password.is_some(),
            };
            run_job_or_launch(job, password, runner, gui)
        }
        Commands::NewFolder { .. } => Err("new-folder is not supported yet".into()),
        Commands::Checksum {
            path,
            index,
            algorithm,
            password,
        } => {
            let index =
                u32::try_from(index).map_err(|_| "index does not fit in u32".to_string())?;
            let pw = password.as_deref().map(password::Password::new);
            let algo = parse_algorithm(algorithm.as_deref())?;
            let (extracted, _) =
                extract_entry_to_temp(engine, Path::new(&path), index, pw.as_ref())?;
            let result = checksum::checksum_file(&extracted, algo).map_err(|e| e.to_string())?;
            println!("{}  {}", algo.name(), result.digest);
            let _ = std::fs::remove_dir_all(extracted.parent().unwrap_or(Path::new(".")));
            Ok(())
        }
        Commands::ShellInstall => shell_control(true),
        Commands::ShellUninstall => shell_control(false),
    }
}

fn list_archive(
    path: &PathBuf,
    password: Option<String>,
    engine: &Arc<dyn ArchiveEngine>,
) -> Result<(), String> {
    let pw = password.as_deref().map(password::Password::new);
    let entries = engine.list(path, pw.as_ref()).map_err(|e| e.to_string())?;
    println!("{:>6}  {:>12}  {:>12}  {}", "#", "Size", "Packed", "Name");
    for entry in &entries {
        println!(
            "{:>6}  {:>12}  {:>12}  {}{}",
            entry.index,
            entry.size,
            entry.packed_size,
            if entry.is_directory { "[D] " } else { "" },
            entry.path,
        );
    }
    println!("{} entries", entries.len());
    Ok(())
}

/// Load the shell COM DLL and call its self-registration entry point.
fn shell_control(install: bool) -> Result<(), String> {
    use windows::Win32::Foundation::FARPROC;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::core::{PCSTR, PCWSTR};

    // Locate windows_shell_extension.dll: next to the executable, then in target/debug.
    let dll = find_shell_dll().ok_or("windows_shell_extension.dll not found (build the windows_shell_extension crate first)")?;
    let wide: Vec<u16> = dll.encode_utf16().chain(std::iter::once(0)).collect();
    let module = unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())) }
        .map_err(|e| format!("LoadLibraryW({dll:?}): {e}"))?;
    let entry = if install {
        "DllRegisterServer"
    } else {
        "DllUnregisterServer"
    };
    let name: Vec<u8> = entry.bytes().chain(std::iter::once(0)).collect();
    let proc: FARPROC = unsafe { GetProcAddress(module, PCSTR(name.as_ptr())) };
    let Some(func) = proc else {
        return Err(format!("{entry} not exported from {dll:?}"));
    };
    let func: unsafe extern "system" fn() -> i32 = unsafe { std::mem::transmute(func) };
    let hr = unsafe { func() };
    if hr < 0 {
        Err(format!("{entry} failed with HRESULT {hr:#x}"))
    } else {
        println!(
            "{}",
            if install {
                "shell extension registered"
            } else {
                "shell extension unregistered"
            }
        );
        Ok(())
    }
}

/// Locate the shell COM DLL for self-registration.
fn find_shell_dll() -> Option<String> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let candidates = [
        exe_dir.join("windows_shell_extension.dll"),
        PathBuf::from("target/debug/windows_shell_extension.dll"),
        PathBuf::from("target/release/windows_shell_extension.dll"),
    ];
    candidates
        .into_iter()
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
}


/// Run a job in-process, or hand it to the detached executor window when
/// `--gui` was requested. The CLI owns parameter parsing; the executor owns
/// the visible progress/cancel/overwrite/password UI.
fn run_job_or_launch(
    job: JobSpec,
    password: Option<String>,
    runner: &TaskRunner,
    gui: bool,
) -> Result<(), String> {
    if gui {
        launch_job_in_executor(job, password)
    } else {
        let pw = password.as_deref().map(password::Password::new);
        drain_events(runner.run(job, pw.as_ref()))
    }
}

fn launch_job_in_executor(job: JobSpec, password: Option<String>) -> Result<(), String> {
    let jobs_dir = temp::ensure_jobs_dir().map_err(|e| e.to_string())?;
    let id = format!(
        "cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let path = jobs_dir.join(format!("{id}.job.json"));
    let file = task::JobFile::new(id, job);
    std::fs::write(&path, file.to_json().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;

    let Some(executor) = sibling_exe("bit7z-executor.exe") else {
        return Err("bit7z-executor.exe not found next to bit7z.exe".into());
    };
    let mut cmd = std::process::Command::new(executor);
    cmd.arg(&path);
    if let Some(password) = password {
        cmd.arg("--password").arg(password);
    }
    hide_console_window(&mut cmd);
    cmd.spawn().map_err(|e| format!("failed to launch executor: {e}"))?;
    Ok(())
}

/// Launch the file manager with positional file paths only.
fn launch_app(paths: &[PathBuf]) -> Result<(), String> {
    let Some(app) = sibling_exe("bit7zfm.exe") else {
        return Err("bit7zfm.exe not found next to bit7z.exe".into());
    };
    let mut cmd = std::process::Command::new(app);
    cmd.args(paths);
    hide_console_window(&mut cmd);
    cmd.spawn()
        .map_err(|e| format!("failed to launch manager: {e}"))?;
    Ok(())
}

/// Locate an executable shipped next to this CLI.
fn sibling_exe(name: &str) -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let candidate = dir.join(name);
    candidate.exists().then_some(candidate)
}

fn hide_console_window(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
}


fn drain_events(rx: std::sync::mpsc::Receiver<task::TaskEvent>) -> Result<(), String> {
    let mut finished = None;
    for event in rx.iter() {
        match event {
            task::TaskEvent::Progress { processed, total } => {
                if total > 0 {
                    eprintln!("\r{} / {} bytes", processed, total);
                }
            }
            task::TaskEvent::FileStarted { path } => {
                eprintln!("  {path}");
            }
            task::TaskEvent::OverwriteConflict { path: _, reply } => {
                // Non-interactive CLI: refuse to guess on an `Ask` conflict.
                let _ = reply.send(false);
            }
            task::TaskEvent::Finished { success, message } => {
                if success {
                    println!("done");
                } else {
                    eprintln!("error: {message}");
                }
                finished = Some((success, message));
            }
        }
    }
    match finished {
        Some((true, _)) => Ok(()),
        Some((false, message)) => Err(message),
        None => Err("task worker stopped unexpectedly".into()),
    }
}

fn parse_indices(indices: Option<&str>) -> Result<Vec<u32>, String> {
    let Some(indices) = indices else {
        return Ok(Vec::new());
    };
    let text = indices.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    text.split(',')
        .map(|part| {
            part.trim()
                .parse::<u32>()
                .map_err(|_| format!("invalid archive index: {part:?}"))
        })
        .collect()
}

fn parse_format(format: &str) -> Result<FormatSpec, String> {
    match format.to_ascii_lowercase().as_str() {
        "7z" => Ok(FormatSpec::SevenZip),
        "zip" => Ok(FormatSpec::Zip),
        "tar" => Ok(FormatSpec::Tar),
        "gz" | "gzip" => Ok(FormatSpec::GZip),
        "bz2" | "bzip2" => Ok(FormatSpec::BZip2),
        "xz" => Ok(FormatSpec::Xz),
        "wim" => Ok(FormatSpec::Wim),
        other => Err(format!("unknown archive format: {other}")),
    }
}

/// Extract one archive entry to a fresh temp directory without buffering the
/// whole entry in memory. Returns the extracted file path and entry size.
fn extract_entry_to_temp(
    engine: &Arc<dyn ArchiveEngine>,
    archive: &Path,
    index: u32,
    password: Option<&password::Password>,
) -> Result<(PathBuf, u64), String> {
    let entries = engine.list(archive, password).map_err(|e| e.to_string())?;
    let entry = entries
        .iter()
        .find(|e| e.index == index)
        .ok_or_else(|| format!("archive index {index} not found"))?;
    if entry.is_directory {
        return Err(format!("entry {index} is a directory"));
    }
    let dir = temp::temp_root()
        .join("cli")
        .join(format!("extract-{}-{index}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    engine
        .extract(
            archive,
            &[index],
            &dir,
            password,
            &bit7z_rs::ExtractOptions {
                overwrite: OverwriteMode::Overwrite,
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())?;
    let extracted = dir.join(temp::sanitize_relative(Path::new(&entry.path)));
    if !extracted.is_file() {
        return Err("extracted entry is not a file".into());
    }
    Ok((extracted, entry.size))
}

fn parse_algorithm(name: Option<&str>) -> Result<checksum::ChecksumAlgorithm, String> {
    use checksum::ChecksumAlgorithm as A;
    Ok(match name.map(str::to_ascii_lowercase).as_deref() {
        Some("crc32") => A::Crc32,
        Some("crc64") => A::Crc64,
        Some("md5") => A::Md5,
        Some("sha1") => A::Sha1,
        Some("sha256") => A::Sha256,
        Some("sha512") => A::Sha512,
        Some(other) => return Err(format!("unknown algorithm: {other}")),
        None => A::Sha256,
    })
}
