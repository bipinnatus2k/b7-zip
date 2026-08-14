mod arg;

use arg::{Cli, Commands};
use bit7z_rs::{ArchiveEngine, Bit7zEngine, EngineOp};
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

    let engine: Arc<dyn ArchiveEngine> = Arc::new(match Bit7zEngine::new(find_dll().as_deref()) {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("failed to load 7-Zip engine: {error}");
            std::process::exit(1);
        }
    });
    let runner = TaskRunner::new(engine.clone());

    let result = run(command, &runner, &engine);
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

/// Locate the 7-Zip DLL: next to the executable, then VCPKG_ROOT.
fn find_dll() -> Option<String> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    for name in ["7z.dll", "7zip.dll"] {
        let candidate = exe_dir.join(name);
        if candidate.exists() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    if let Ok(vcpkg) = std::env::var("VCPKG_ROOT") {
        for name in ["7zip.dll", "7z.dll"] {
            let candidate = PathBuf::from(&vcpkg)
                .join("installed/x64-windows/bin")
                .join(name);
            if candidate.exists() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }
    None
}

fn run(
    command: Commands,
    runner: &TaskRunner,
    engine: &Arc<dyn ArchiveEngine>,
) -> Result<(), String> {
    match command {
        Commands::Open { path, password } => {
            return list_archive(&PathBuf::from(&path), password, engine);
        }
        Commands::List { path, password } => {
            return list_archive(&path, password, engine);
        }
        Commands::Extract { path, to, indices, password } => {
            let target = to.map(PathBuf::from).unwrap_or_else(|| {
                let mut p = PathBuf::from(&path);
                p.set_extension("");
                p
            });
            let items: Vec<u32> = indices
                .as_deref()
                .unwrap_or("")
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            let job = JobSpec::Extract {
                archive: PathBuf::from(&path),
                items,
                target,
                overwrite: OverwriteSpec::Overwrite,
                password_hint: password.is_some(),
            };
            let pw = password.as_deref().map(password::Password::new);
            drain_events(runner.run(job, pw.as_ref()));
            Ok(())
        }
        Commands::Test { path, password } => {
            let pw = password.as_deref().map(password::Password::new);
            let result = engine.test(&PathBuf::from(&path), pw.as_ref()).map_err(|e| e.to_string())?;
            if result.all_ok {
                println!("ok: {} items", result.total);
                Ok(())
            } else {
                Err(format!("{}/{} items failed: {:?}", result.failed_count, result.total, result.errors))
            }
        }
        Commands::Compress { files, to, format, password } => {
            let target = to.map(PathBuf::from).unwrap_or_else(|| {
                let first = files.first().cloned().unwrap_or_else(|| "archive".into());
                let mut p = PathBuf::from(first);
                p.set_extension(format.as_str());
                p
            });
            let format = parse_format(&format);
            let job = JobSpec::Compress {
                inputs: files.iter().map(PathBuf::from).collect(),
                target,
                format,
                level: LevelSpec::Normal,
                solid: None,
                volume: None,
                threads: None,
                encrypt_headers: false,
                password_hint: password.is_some(),
            };
            let pw = password.as_deref().map(password::Password::new);
            drain_events(runner.run(job, pw.as_ref()));
            Ok(())
        }
        Commands::Preview { path, index, password, max_bytes } => {
            let pw = password.as_deref().map(password::Password::new);
            let bytes = engine
                .extract_to_buffer(&PathBuf::from(&path), index, pw.as_ref())
                .map_err(|e| e.to_string())?;
            let shown = bytes.iter().take(max_bytes).copied().collect::<Vec<u8>>();
            if shown.iter().all(|b| b.is_ascii_graphic() || b.is_ascii_whitespace()) {
                println!("{}", String::from_utf8_lossy(&shown));
            } else {
                for (i, chunk) in shown.chunks(16).enumerate() {
                    let hex: Vec<String> = chunk.iter().map(|b| format!("{:02x}", b)).collect();
                    println!("{:08x}  {}", i * 16, hex.join(" "));
                }
            }
            if bytes.len() > max_bytes {
                println!("... ({} bytes total)", bytes.len());
            }
            Ok(())
        }
        Commands::Add { path, files, password } => {
            let ops: Vec<EngineOp> = files
                .iter()
                .map(|f| EngineOp::Add {
                    fs_path: PathBuf::from(f),
                    archive_path: Path::new(f)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| f.clone()),
                })
                .collect();
            let pw = password.as_deref().map(password::Password::new);
            engine.update(&PathBuf::from(&path), &ops, pw.as_ref()).map_err(|e| e.to_string())
        }
        Commands::Delete { path, indices, password } => {
            let ops: Vec<EngineOp> = indices.iter().map(|i| EngineOp::Delete { archive_index: *i }).collect();
            let pw = password.as_deref().map(password::Password::new);
            engine.update(&PathBuf::from(&path), &ops, pw.as_ref()).map_err(|e| e.to_string())
        }
        Commands::Rename { path, index, name, password } => {
            let ops = vec![EngineOp::Rename { archive_index: index, new_path: name }];
            let pw = password.as_deref().map(password::Password::new);
            engine.update(&PathBuf::from(&path), &ops, pw.as_ref()).map_err(|e| e.to_string())
        }
        Commands::NewFolder { .. } => {
            Err("new-folder is not supported yet".into())
        }
        Commands::Checksum { path, index, algorithm, password } => {
            let pw = password.as_deref().map(password::Password::new);
            let bytes = engine
                .extract_to_buffer(&path, index as u32, pw.as_ref())
                .map_err(|e| e.to_string())?;
            let algo = parse_algorithm(algorithm.as_deref())?;
            let digest = checksum::checksum_bytes(&bytes, algo);
            println!("{}  {}", algo.name(), digest);
            Ok(())
        }
        Commands::ShellInstall | Commands::ShellUninstall => {
            Err("shell integration arrives with the COM DLL (later step)".into())
        },
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

fn drain_events(rx: std::sync::mpsc::Receiver<task::TaskEvent>) {
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
            task::TaskEvent::Finished { success, message } => {
                if success {
                    println!("done");
                } else {
                    eprintln!("error: {message}");
                }
            }
        }
    }
}

fn parse_format(format: &str) -> FormatSpec {
    match format.to_ascii_lowercase().as_str() {
        "zip" => FormatSpec::Zip,
        "tar" => FormatSpec::Tar,
        "gz" | "gzip" => FormatSpec::GZip,
        "bz2" | "bzip2" => FormatSpec::BZip2,
        "xz" => FormatSpec::Xz,
        "wim" => FormatSpec::Wim,
        _ => FormatSpec::SevenZip,
    }
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
