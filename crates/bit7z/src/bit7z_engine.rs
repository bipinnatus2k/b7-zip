//! The concrete [`ArchiveEngine`] implementation backed by the bit7z C++
//! library (via the C FFI in `bit7z-ffi`).
//!
//! All FFI calls are serialized through an internal mutex: bit7z handles are
//! not thread-safe, and the 7-Zip DLL is designed for single-threaded
//! sequential access per handle. Strings are passed as UTF-8 (bit7z's
//! default narrow-string mode converts to UTF-16 internally on Windows).

use crate::editor::ArchiveEditor;
use crate::engine::{
    ArchiveEngine, ArchiveEntry, ArchiveError, CompressOptions, EngineOp, ExtractOptions,
    OverwriteMode, TestResult,
};
use crate::library::Bit7zLibrary;
use crate::reader::ArchiveReader;
use crate::writer::{ArchiveWriter, WriterFormat};
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// Engine implementation backed by the bit7z C++ library.
pub struct Bit7zEngine {
    lib: Mutex<Bit7zLibrary>,
}

impl Bit7zEngine {
    /// Create the engine, loading the 7-Zip DLL.
    ///
    /// `dll_path` — optional explicit path to `7z.dll` (or `7zip.dll`);
    /// `None` lets bit7z search its default locations.
    pub fn new(dll_path: Option<&Path>) -> Result<Self, ArchiveError> {
        let path = dll_path.map(|p| p.to_string_lossy());
        let lib = match &path {
            Some(path) => Bit7zLibrary::open(path)?,
            None => Bit7zLibrary::open("")?,
        };
        Ok(Self {
            lib: Mutex::new(lib),
        })
    }
}

impl Bit7zEngine {
    fn with_reader<T>(
        &self,
        path: &Path,
        password: Option<&password::Password>,
        f: impl FnOnce(&ArchiveReader) -> Result<T, ArchiveError>,
    ) -> Result<T, ArchiveError> {
        let lib = self.lib.lock().unwrap();
        let reader = ArchiveReader::open(&lib, &path_to_cstring(path)?, password)
            .map_err(ArchiveError::Engine)?;
        let result = f(&reader);
        drop(reader);
        result
    }
}

// ============================================================================
// FFI callback trampolines (progress / per-file / cancel / skip-on-overwrite)
// ============================================================================

/// User-data passed through the FFI to the C callbacks.
struct CallbackCtx {
    progress: Option<Arc<dyn Fn(u64, u64) + Send + Sync>>,
    file: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    cancel: Option<Arc<AtomicBool>>,
    /// While set, the progress callback blocks (the pause). Cancellation is
    /// still honored inside the wait so a paused job can be aborted.
    pause: Option<Arc<AtomicBool>>,
    conflict: Option<Arc<dyn Fn(&str) -> bool + Send + Sync>>,
    auto_rename: Option<AutoRenameCtx>,
}

/// Context for the auto-rename extraction path.
struct AutoRenameCtx {
    dest: PathBuf,
    indices: Vec<u32>,
}

/// on_progress: return 0 to cancel, non-zero to continue.
unsafe extern "C" fn progress_trampoline(
    processed: u64,
    total: u64,
    ctx: *mut std::ffi::c_void,
) -> i32 {
    if ctx.is_null() {
        return 1;
    }
    let ctx = unsafe { &*(ctx as *const CallbackCtx) };
    if ctx
        .cancel
        .as_ref()
        .is_some_and(|c| c.load(Ordering::Relaxed))
    {
        return 0;
    }
    if let Some(pause) = &ctx.pause {
        while pause.load(Ordering::Relaxed) {
            if ctx
                .cancel
                .as_ref()
                .is_some_and(|c| c.load(Ordering::Relaxed))
            {
                return 0;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    if let Some(progress) = &ctx.progress {
        progress(processed, total);
    }
    1
}

/// Reader on_file: (path, file_size, ctx).
unsafe extern "C" fn file_trampoline_reader(
    path: *const std::ffi::c_char,
    _size: u64,
    ctx: *mut std::ffi::c_void,
) {
    if ctx.is_null() || path.is_null() {
        return;
    }
    let ctx = unsafe { &*(ctx as *const CallbackCtx) };
    if let Some(file) = &ctx.file {
        let path = unsafe { CStr::from_ptr(path) }.to_string_lossy();
        file(&path);
    }
}

/// Writer on_file: (path, ctx).
unsafe extern "C" fn file_trampoline_writer(
    path: *const std::ffi::c_char,
    ctx: *mut std::ffi::c_void,
) {
    if ctx.is_null() || path.is_null() {
        return;
    }
    let ctx = unsafe { &*(ctx as *const CallbackCtx) };
    if let Some(file) = &ctx.file {
        let path = unsafe { CStr::from_ptr(path) }.to_string_lossy();
        file(&path);
    }
}

/// on_overwrite: 0 = overwrite, 1 = skip. Used for the Skip policy.
unsafe extern "C" fn overwrite_skip_trampoline(
    _src: *const std::ffi::c_char,
    _dest: *const std::ffi::c_char,
    _existing_size: u64,
    _src_size: u64,
    _src_mtime: i64,
    _dest_mtime: i64,
    _ctx: *mut std::ffi::c_void,
) -> i32 {
    1
}

/// on_overwrite backed by the user-provided `Ask` conflict callback.
unsafe extern "C" fn overwrite_ask_trampoline(
    _src: *const std::ffi::c_char,
    dest: *const std::ffi::c_char,
    _existing_size: u64,
    _src_size: u64,
    _src_mtime: i64,
    _dest_mtime: i64,
    ctx: *mut std::ffi::c_void,
) -> i32 {
    if ctx.is_null() {
        return 1;
    }
    let ctx = unsafe { &*(ctx as *const CallbackCtx) };
    let Some(conflict) = &ctx.conflict else {
        return 1;
    };
    if dest.is_null() {
        return 1;
    }
    let dest = unsafe { CStr::from_ptr(dest) }.to_string_lossy();
    if conflict(&dest) { 0 } else { 1 }
}

/// RenameCallback used for `AutoRename`: skip items outside `indices`, keep
/// the original path for non-existing files, and produce `name (n).ext` for
/// files that already exist.
unsafe extern "C" fn auto_rename_trampoline(
    src: *const std::ffi::c_char,
    index: u32,
    _size: u64,
    _is_dir: i32,
    out: *mut std::ffi::c_char,
    out_size: u32,
    ctx: *mut std::ffi::c_void,
) -> i32 {
    if ctx.is_null() || out.is_null() || out_size == 0 {
        return -1;
    }
    let ctx = unsafe { &*(ctx as *const CallbackCtx) };
    let Some(auto) = &ctx.auto_rename else {
        return -1;
    };
    if !auto.indices.contains(&index) {
        unsafe { *out = 0 };
        return 0;
    }
    if src.is_null() {
        unsafe { *out = 0 };
        return 0;
    }
    let src = unsafe { CStr::from_ptr(src) }.to_string_lossy();
    let mut candidate = auto
        .dest
        .join(sanitize_archive_relative(Path::new(src.as_ref())));
    let original = candidate.clone();
    let mut counter = 1u32;
    while candidate.exists() && counter < 100_000 {
        let stem = original
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("archive");
        let ext = original.extension().and_then(|s| s.to_str());
        let name = match ext {
            Some(ext) => format!("{stem} ({counter}).{ext}"),
            None => format!("{stem} ({counter})"),
        };
        candidate = original.with_file_name(name);
        counter += 1;
    }
    let text = match candidate.strip_prefix(&auto.dest) {
        Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
        Err(_) => candidate.to_string_lossy().replace('\\', "/"),
    };
    let bytes = text.as_bytes();
    if bytes.len() + 1 > out_size as usize {
        return -1;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out.cast::<u8>(), bytes.len());
        *out.add(bytes.len()) = 0;
    }
    0
}

/// Strip absolute/`..` components from an archive-relative path before
/// joining it to an extraction root.
fn sanitize_archive_relative(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => out.push(part),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {}
        }
    }
    out
}

fn path_to_cstring(path: &Path) -> Result<String, ArchiveError> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| ArchiveError::Engine("path is not valid UTF-8".into()))
}

/// Pick a writable bit7z format from the archive file extension.
fn writer_format_for_archive(path: &Path) -> WriterFormat {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "zip" => WriterFormat::Zip,
        "tar" => WriterFormat::Tar,
        "gz" | "gzip" | "tgz" => WriterFormat::GZip,
        "bz2" | "bzip2" | "tbz" | "tbz2" => WriterFormat::BZip2,
        "xz" | "txz" => WriterFormat::Xz,
        "wim" => WriterFormat::Wim,
        _ => WriterFormat::SevenZip,
    }
}

impl ArchiveEngine for Bit7zEngine {
    fn list(
        &self,
        path: &Path,
        password: Option<&password::Password>,
    ) -> Result<Vec<ArchiveEntry>, ArchiveError> {
        self.with_reader(path, password, |reader| {
            let raw = unsafe { bit7z_ffi::bit7z_reader_items(reader.raw_handle().as_ptr()) };
            if raw.is_null() {
                return Err(ArchiveError::OpenFailed(
                    "failed to list archive items".into(),
                ));
            }
            let count = unsafe { bit7z_ffi::bit7z_item_list_count(raw) };
            let mut entries = Vec::with_capacity(count as usize);
            for i in 0..count {
                let index = unsafe { bit7z_ffi::bit7z_item_list_index(raw, i) };
                let path_c = unsafe { bit7z_ffi::bit7z_item_list_path(raw, i) };
                let path = if path_c.is_null() {
                    String::new()
                } else {
                    unsafe { CStr::from_ptr(path_c).to_string_lossy().into_owned() }
                };
                let size = unsafe { bit7z_ffi::bit7z_item_list_size(raw, i) };
                let packed = unsafe { bit7z_ffi::bit7z_item_list_packed_size(raw, i) };
                let is_dir = unsafe { bit7z_ffi::bit7z_item_list_is_dir(raw, i) != 0 };
                let is_enc = unsafe { bit7z_ffi::bit7z_item_list_is_encrypted(raw, i) != 0 };
                let crc = unsafe { bit7z_ffi::bit7z_item_list_crc(raw, i) };
                let name = path.rsplit('/').next().unwrap_or(&path).to_string();

                // Per-item deep properties via the raw BitArchiveItem pointer.
                let item_ptr = unsafe {
                    bit7z_ffi::bit7z_item_from_reader(reader.raw_handle().as_ptr(), index)
                };
                let modified = if item_ptr.is_null() {
                    None
                } else {
                    let secs = unsafe { bit7z_ffi::bit7z_item_mtime(item_ptr) };
                    epoch_to_datetime(secs)
                };
                let created = if item_ptr.is_null() {
                    None
                } else {
                    let secs = unsafe { bit7z_ffi::bit7z_item_ctime(item_ptr) };
                    epoch_to_datetime(secs)
                };
                let accessed = if item_ptr.is_null() {
                    None
                } else {
                    let secs = unsafe { bit7z_ffi::bit7z_item_atime(item_ptr) };
                    epoch_to_datetime(secs)
                };
                let attributes = if item_ptr.is_null() {
                    None
                } else {
                    Some(unsafe { bit7z_ffi::bit7z_item_attributes(item_ptr) })
                };
                let posix_attrib = if item_ptr.is_null() {
                    None
                } else {
                    Some(unsafe { bit7z_ffi::bit7z_item_posix_attrib(item_ptr) })
                };
                let host_os = if item_ptr.is_null() {
                    None
                } else {
                    Some(unsafe { bit7z_ffi::bit7z_item_host_os(item_ptr) })
                };
                let is_symlink = if item_ptr.is_null() {
                    false
                } else {
                    unsafe { bit7z_ffi::bit7z_item_is_symlink(item_ptr) != 0 }
                };
                let compression_method = if item_ptr.is_null() {
                    None
                } else {
                    let mut buf = vec![0u8; 64];
                    let n = unsafe {
                        bit7z_ffi::bit7z_item_compression_method(
                            item_ptr,
                            buf.as_mut_ptr() as *mut _,
                            64,
                        )
                    };
                    if n < 0 {
                        None
                    } else {
                        let s = unsafe {
                            CStr::from_ptr(buf.as_ptr() as *const _)
                                .to_string_lossy()
                                .into_owned()
                        };
                        if s.is_empty() { None } else { Some(s) }
                    }
                };
                let extension = if item_ptr.is_null() {
                    None
                } else {
                    let mut buf = vec![0u8; 64];
                    let n = unsafe {
                        bit7z_ffi::bit7z_item_extension(item_ptr, buf.as_mut_ptr() as *mut _, 64)
                    };
                    if n < 0 {
                        None
                    } else {
                        let s = unsafe {
                            CStr::from_ptr(buf.as_ptr() as *const _)
                                .to_string_lossy()
                                .into_owned()
                        };
                        if s.is_empty() { None } else { Some(s) }
                    }
                };

                entries.push(ArchiveEntry {
                    index,
                    name,
                    path,
                    size,
                    packed_size: packed,
                    is_directory: is_dir,
                    is_encrypted: is_enc,
                    is_symlink,
                    crc: if !item_ptr.is_null()
                        && unsafe { bit7z_ffi::bit7z_item_crc_defined(item_ptr) != 0 }
                    {
                        Some(crc)
                    } else {
                        None
                    },
                    modified,
                    created,
                    accessed,
                    attributes,
                    posix_attrib,
                    host_os,
                    compression_method,
                    comment: None,
                    user: None,
                    group: None,
                    extension,
                    hardlink: None,
                });
            }
            unsafe { bit7z_ffi::bit7z_item_list_free(raw) };
            Ok(entries)
        })
    }

    fn extract(
        &self,
        path: &Path,
        indices: &[u32],
        dest: &Path,
        password: Option<&password::Password>,
        options: &ExtractOptions,
    ) -> Result<(), ArchiveError> {
        if indices.is_empty() {
            return Err(ArchiveError::Engine(
                "extraction index list is empty".into(),
            ));
        }
        let dest_str = path_to_cstring(dest)?;

        // AutoRename goes through the rename-callback path because bit7z has
        // no native "rename new file" overwrite mode.
        if options.overwrite == OverwriteMode::AutoRename {
            let result = self.with_reader(path, password, |reader| {
                let mut ctx = CallbackCtx {
                    progress: options.progress.clone(),
                    file: options.file.clone(),
                    cancel: options.cancel.clone(),
                    pause: options.pause.clone(),
                    conflict: None,
                    auto_rename: Some(AutoRenameCtx {
                        dest: dest.to_path_buf(),
                        indices: indices.to_vec(),
                    }),
                };
                reader
                    .extract_with_rename(
                        &dest_str,
                        &mut ctx as *mut CallbackCtx as *mut std::ffi::c_void,
                        Some(auto_rename_trampoline),
                        Some(progress_trampoline),
                        Some(file_trampoline_writer),
                    )
                    .map_err(ArchiveError::Engine)
            });
            if options
                .cancel
                .as_ref()
                .is_some_and(|c| c.load(Ordering::Relaxed))
            {
                return Err(ArchiveError::Cancelled);
            }
            return result;
        }

        if options.overwrite == OverwriteMode::Ask && options.on_conflict.is_none() {
            return Err(ArchiveError::UnsupportedOperation(
                "Ask overwrite mode requires an on_conflict callback".into(),
            ));
        }

        let with_callbacks = options.progress.is_some()
            || options.file.is_some()
            || options.cancel.is_some()
            || options.pause.is_some()
            || options.on_conflict.is_some()
            || matches!(options.overwrite, OverwriteMode::Skip | OverwriteMode::Ask);

        let result = self.with_reader(path, password, |reader| {
            let c_dest =
                CString::new(dest_str.as_str()).map_err(|e| ArchiveError::Engine(e.to_string()))?;
            if !with_callbacks {
                let ret = unsafe {
                    bit7z_ffi::bit7z_reader_extract_to(
                        reader.raw_handle().as_ptr(),
                        indices.as_ptr(),
                        indices.len() as u32,
                        c_dest.as_ptr(),
                    )
                };
                if ret != 0 {
                    return Err(ArchiveError::Engine("extraction failed".into()));
                }
                return Ok(());
            }

            // Callback-driven path: per-file and byte progress flow through
            // the trampolines; the cancel flag aborts the extraction.
            let mut ctx = CallbackCtx {
                progress: options.progress.clone(),
                file: options.file.clone(),
                cancel: options.cancel.clone(),
                pause: options.pause.clone(),
                conflict: options.on_conflict.clone(),
                auto_rename: None,
            };
            let on_overwrite: Option<
                unsafe extern "C" fn(
                    *const std::ffi::c_char,
                    *const std::ffi::c_char,
                    u64,
                    u64,
                    i64,
                    i64,
                    *mut std::ffi::c_void,
                ) -> i32,
            > = match options.overwrite {
                // The C++ reader default already overwrites.
                OverwriteMode::Overwrite => None,
                OverwriteMode::Skip => Some(overwrite_skip_trampoline),
                OverwriteMode::Ask => Some(overwrite_ask_trampoline),
                OverwriteMode::AutoRename => unreachable!("handled above"),
            };
            reader
                .extract_to_cb(
                    indices,
                    &dest_str,
                    &mut ctx as *mut CallbackCtx as *mut std::ffi::c_void,
                    on_overwrite,
                    Some(progress_trampoline),
                    Some(file_trampoline_reader),
                )
                .map_err(ArchiveError::Engine)
        });
        if options
            .cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Relaxed))
        {
            return Err(ArchiveError::Cancelled);
        }
        result
    }

    fn extract_to_buffer(
        &self,
        path: &Path,
        index: u32,
        password: Option<&password::Password>,
    ) -> Result<Vec<u8>, ArchiveError> {
        self.with_reader(path, password, |reader| {
            reader
                .extract_to_buffer(index)
                .map_err(ArchiveError::Engine)
        })
    }

    fn test_with_options(
        &self,
        path: &Path,
        password: Option<&password::Password>,
        options: &crate::TestOptions,
    ) -> Result<TestResult, ArchiveError> {
        let has_callbacks = options.progress.is_some()
            || options.file.is_some()
            || options.cancel.is_some()
            || options.pause.is_some();
        self.with_reader(path, password, |reader| {
            let (all_ok, total, failed_count, _, failed_errors) = if has_callbacks {
                let mut ctx = CallbackCtx {
                    progress: options.progress.clone(),
                    file: options.file.clone(),
                    cancel: options.cancel.clone(),
                    pause: options.pause.clone(),
                    conflict: None,
                    auto_rename: None,
                };
                reader
                    .test_to_cb(
                        &mut ctx as *mut CallbackCtx as *mut std::ffi::c_void,
                        Some(progress_trampoline),
                        Some(file_trampoline_reader),
                    )
                    .map_err(ArchiveError::Engine)?
            } else {
                reader.test().map_err(ArchiveError::Engine)?
            };
            Ok(TestResult {
                all_ok,
                total,
                failed_count,
                errors: failed_errors,
            })
        })
    }

    fn compress(
        &self,
        inputs: &[PathBuf],
        target: &Path,
        options: &CompressOptions,
    ) -> Result<(), ArchiveError> {
        let lib = self.lib.lock().unwrap();
        let writer = ArchiveWriter::create(&lib, options.format).map_err(ArchiveError::Engine)?;
        if options.threads > 0 {
            writer.set_threads(options.threads);
        }
        writer.set_compression_level(options.level);
        if let Some(method) = options.method {
            writer.set_compression_method(method);
        }
        if let Some(size) = options.dictionary_size {
            writer.set_dictionary_size(size);
        }
        if let Some(size) = options.word_size {
            writer.set_word_size(size);
        }
        if let Some(solid) = options.solid {
            writer.set_solid_mode(solid);
        }
        if let Some(volume) = options.volume_size {
            writer.set_volume_size(volume);
        }
        if let Some(password) = &options.password {
            writer.set_password_ex(password, options.encrypt_headers);
        }
        let c_inputs: Vec<&str> = inputs
            .iter()
            .map(|p| {
                p.to_str()
                    .ok_or_else(|| ArchiveError::Engine("input path not UTF-8".into()))
            })
            .collect::<Result<_, _>>()?;
        writer.add_files(&c_inputs).map_err(ArchiveError::Engine)?;
        let target_str = target
            .to_str()
            .ok_or_else(|| ArchiveError::Engine("target path not UTF-8".into()))?;

        let with_callbacks = options.progress.is_some()
            || options.file.is_some()
            || options.cancel.is_some()
            || options.pause.is_some();
        let result = if with_callbacks {
            let mut ctx = CallbackCtx {
                progress: options.progress.clone(),
                file: options.file.clone(),
                cancel: options.cancel.clone(),
                pause: options.pause.clone(),
                conflict: None,
                auto_rename: None,
            };
            // SAFETY: ctx outlives the synchronous compress call.
            unsafe {
                writer.compress_to_cb(
                    target_str,
                    &mut ctx as *mut CallbackCtx as *mut std::ffi::c_void,
                    Some(progress_trampoline),
                    Some(file_trampoline_writer),
                )
            }
            .map_err(ArchiveError::Engine)
        } else {
            writer.compress_to(target_str).map_err(ArchiveError::Engine)
        };
        if options
            .cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Relaxed))
        {
            return Err(ArchiveError::Cancelled);
        }
        result
    }

    fn update(
        &self,
        path: &Path,
        ops: &[EngineOp],
        password: Option<&password::Password>,
    ) -> Result<(), ArchiveError> {
        let lib = self.lib.lock().unwrap();
        let path_str = path_to_cstring(path)?;
        let format = writer_format_for_archive(path);

        // Step 1: structural edits (delete/rename) through the archive
        // editor. Modify is handled as delete (here) + re-add (step 2).
        let has_structural = ops.iter().any(|op| {
            matches!(
                op,
                EngineOp::Delete { .. } | EngineOp::Rename { .. } | EngineOp::Modify { .. }
            )
        });
        if has_structural {
            let editor = ArchiveEditor::open(&lib, &path_str, format, password.map(|p| p.as_str()))
                .map_err(ArchiveError::Engine)?;
            for op in ops {
                match op {
                    EngineOp::Delete { archive_index } => {
                        editor
                            .delete(*archive_index)
                            .map_err(ArchiveError::Engine)?;
                    }
                    EngineOp::Rename {
                        archive_index,
                        new_path,
                    } => {
                        editor
                            .rename(*archive_index, new_path)
                            .map_err(ArchiveError::Engine)?;
                    }
                    EngineOp::Modify { archive_index, .. } => {
                        editor
                            .delete(*archive_index)
                            .map_err(ArchiveError::Engine)?;
                    }
                    _ => {}
                }
            }
            editor.apply().map_err(ArchiveError::Engine)?;
        }

        // Step 2: additions and re-additions through a writer opened in
        // update mode. The editor from step 1 is closed by now, so the
        // archive is not held by two handles at once.
        let has_content = ops
            .iter()
            .any(|op| matches!(op, EngineOp::Add { .. } | EngineOp::Modify { .. }));
        if has_content {
            let writer = ArchiveWriter::open(&lib, &path_str, format, password)
                .map_err(ArchiveError::Engine)?;
            writer.set_update_mode(crate::writer::UpdateMode::Update);
            for op in ops {
                match op {
                    EngineOp::Add {
                        fs_path,
                        archive_path,
                    } => {
                        let fs = path_to_cstring(fs_path)?;
                        writer
                            .add_item_with_path(&fs, archive_path)
                            .map_err(ArchiveError::Engine)?;
                    }
                    EngineOp::Modify {
                        archive_path,
                        fs_path,
                        ..
                    } => {
                        let fs = path_to_cstring(fs_path)?;
                        writer
                            .add_item_with_path(&fs, archive_path)
                            .map_err(ArchiveError::Engine)?;
                    }
                    _ => {}
                }
            }
            writer
                .compress_to(&path_str)
                .map_err(ArchiveError::Engine)?;
        }
        Ok(())
    }

    fn is_encrypted(&self, path: &Path) -> Result<bool, ArchiveError> {
        let lib = self.lib.lock().unwrap();
        let path_str = path_to_cstring(path)?;
        Ok(lib.is_encrypted(&path_str))
    }

    fn is_header_encrypted(&self, path: &Path) -> Result<bool, ArchiveError> {
        let lib = self.lib.lock().unwrap();
        let path_str = path_to_cstring(path)?;
        Ok(lib.is_header_encrypted(&path_str))
    }
}

fn epoch_to_datetime(secs: u64) -> Option<jiff::civil::DateTime> {
    let secs = i64::try_from(secs).ok()?;
    if secs <= 0 {
        return None;
    }
    let ts = jiff::Timestamp::from_second(secs).ok()?;
    let zoned = ts.to_zoned(jiff::tz::TimeZone::UTC);
    Some(zoned.datetime())
}
