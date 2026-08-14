//! The concrete [`ArchiveEngine`] implementation backed by the bit7z C++
//! library (via the C FFI in `bit7z-ffi`).
//!
//! All FFI calls are serialized through an internal mutex: bit7z handles are
//! not thread-safe, and the 7-Zip DLL is designed for single-threaded
//! sequential access per handle. Strings are passed as UTF-8 (bit7z's
//! default narrow-string mode converts to UTF-16 internally on Windows).

use crate::engine::{
    ArchiveEngine, ArchiveEntry, ArchiveError, CompressOptions, EngineOp, ExtractOptions,
    OverwriteMode, TestResult,
};
use crate::handle::BitArchiveHandle;
use crate::library::Bit7zLibrary;
use crate::reader::ArchiveReader;
use crate::writer::{ArchiveWriter, WriterFormat};
use crate::editor::ArchiveEditor;
use std::ffi::{CStr, CString};
use std::sync::Mutex;
use std::path::{Path, PathBuf};

/// Engine implementation backed by the bit7z C++ library.
pub struct Bit7zEngine {
    lib: Mutex<Bit7zLibrary>,
}

impl Bit7zEngine {
    /// Create the engine, loading the 7-Zip DLL.
    ///
    /// `dll_path` — optional explicit path to `7z.dll` (or `7zip.dll`);
    /// `None` lets bit7z search its default locations.
    pub fn new(dll_path: Option<&str>) -> Result<Self, ArchiveError> {
        let lib = match dll_path {
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
        let reader = ArchiveReader::open(&lib, &path_to_cstring(path)?, password).map_err(ArchiveError::Engine)?;
        let result = f(&reader);
        drop(reader);
        result
    }
}

fn path_to_cstring(path: &Path) -> Result<String, ArchiveError> {
    path.to_str().map(str::to_string).ok_or_else(|| ArchiveError::Engine("path is not valid UTF-8".into()))
}

impl ArchiveEngine for Bit7zEngine {
    fn list(&self, path: &Path, password: Option<&password::Password>) -> Result<Vec<ArchiveEntry>, ArchiveError> {
        self.with_reader(path, password, |reader| {
            let raw = unsafe { bit7z_ffi::bit7z_reader_items(reader.raw_handle().as_ptr()) };
            if raw.is_null() {
                return Err(ArchiveError::OpenFailed("failed to list archive items".into()));
            }
            let count = unsafe { bit7z_ffi::bit7z_item_list_count(raw) };
            let mut entries = Vec::with_capacity(count as usize);
            for i in 0..count {
                let index = unsafe { bit7z_ffi::bit7z_item_list_index(raw, i) };
                let path_c = unsafe { bit7z_ffi::bit7z_item_list_path(raw, i) };
                let path = if path_c.is_null() { String::new() } else { unsafe { CStr::from_ptr(path_c).to_string_lossy().into_owned() } };
                let size = unsafe { bit7z_ffi::bit7z_item_list_size(raw, i) };
                let packed = unsafe { bit7z_ffi::bit7z_item_list_packed_size(raw, i) };
                let is_dir = unsafe { bit7z_ffi::bit7z_item_list_is_dir(raw, i) != 0 };
                let is_enc = unsafe { bit7z_ffi::bit7z_item_list_is_encrypted(raw, i) != 0 };
                let crc = unsafe { bit7z_ffi::bit7z_item_list_crc(raw, i) };
                let name = path.rsplit('/').next().unwrap_or(&path).to_string();

                // Per-item deep properties via the raw BitArchiveItem pointer.
                let item_ptr = unsafe { bit7z_ffi::bit7z_item_from_reader(reader.raw_handle().as_ptr(), index) };
                let modified = if item_ptr.is_null() { None } else {
                    let secs = unsafe { bit7z_ffi::bit7z_item_mtime(item_ptr) };
                    epoch_to_datetime(secs)
                };
                let created = if item_ptr.is_null() { None } else {
                    let secs = unsafe { bit7z_ffi::bit7z_item_ctime(item_ptr) };
                    epoch_to_datetime(secs)
                };
                let accessed = if item_ptr.is_null() { None } else {
                    let secs = unsafe { bit7z_ffi::bit7z_item_atime(item_ptr) };
                    epoch_to_datetime(secs)
                };
                let attributes = if item_ptr.is_null() { None } else {
                    Some(unsafe { bit7z_ffi::bit7z_item_attributes(item_ptr) })
                };
                let posix_attrib = if item_ptr.is_null() { None } else {
                    Some(unsafe { bit7z_ffi::bit7z_item_posix_attrib(item_ptr) })
                };
                let host_os = if item_ptr.is_null() { None } else {
                    Some(unsafe { bit7z_ffi::bit7z_item_host_os(item_ptr) })
                };
                let is_symlink = if item_ptr.is_null() { false } else {
                    unsafe { bit7z_ffi::bit7z_item_is_symlink(item_ptr) != 0 }
                };
                let compression_method = if item_ptr.is_null() { None } else {
                    let mut buf = vec![0u8; 64];
                    let n = unsafe { bit7z_ffi::bit7z_item_compression_method(item_ptr, buf.as_mut_ptr() as *mut _, 64) };
                    if n < 0 { None } else {
                        let s = unsafe { CStr::from_ptr(buf.as_ptr() as *const _).to_string_lossy().into_owned() };
                        if s.is_empty() { None } else { Some(s) }
                    }
                };
                let extension = if item_ptr.is_null() { None } else {
                    let mut buf = vec![0u8; 64];
                    let n = unsafe { bit7z_ffi::bit7z_item_extension(item_ptr, buf.as_mut_ptr() as *mut _, 64) };
                    if n < 0 { None } else {
                        let s = unsafe { CStr::from_ptr(buf.as_ptr() as *const _).to_string_lossy().into_owned() };
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
                    crc: if crc == 0 && !is_enc { None } else { Some(crc) },
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

    fn extract(&self, path: &Path, indices: &[u32], dest: &Path, password: Option<&password::Password>, options: &ExtractOptions) -> Result<(), ArchiveError> {
        let dest_str = path_to_cstring(dest)?;
        self.with_reader(path, password, |reader| {
            let c_dest = CString::new(dest_str.as_str()).map_err(|e| ArchiveError::Engine(e.to_string()))?;
            // on_overwrite trampoline: 0 = overwrite, 1 = skip
            let mode = match options.overwrite {
                OverwriteMode::Overwrite => 0i32,
                OverwriteMode::Skip | OverwriteMode::AutoRename => 1i32,
                OverwriteMode::Ask => 1i32, // Ask is handled at a higher level via extract_with_callback
            };
            let ret = unsafe {
                bit7z_ffi::bit7z_reader_extract_to(
                    reader.raw_handle().as_ptr(),
                    indices.as_ptr(),
                    indices.len() as u32,
                    c_dest.as_ptr(),
                )
            };
            let _ = mode;
            if ret != 0 {
                Err(ArchiveError::Engine("extraction failed".into()))
            } else {
                Ok(())
            }
        })
    }

    fn extract_to_buffer(&self, path: &Path, index: u32, password: Option<&password::Password>) -> Result<Vec<u8>, ArchiveError> {
        self.with_reader(path, password, |reader| {
            reader.extract_to_buffer(index).map_err(ArchiveError::Engine)
        })
    }

    fn test(&self, path: &Path, password: Option<&password::Password>) -> Result<TestResult, ArchiveError> {
        self.with_reader(path, password, |reader| {
            let (all_ok, total, failed_count, _, failed_errors) = reader.test().map_err(ArchiveError::Engine)?;
            Ok(TestResult { all_ok, total, failed_count, errors: failed_errors })
        })
    }

    fn compress(&self, inputs: &[PathBuf], target: &Path, options: &CompressOptions) -> Result<(), ArchiveError> {
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
            .map(|p| p.to_str().ok_or_else(|| ArchiveError::Engine("input path not UTF-8".into())))
            .collect::<Result<_, _>>()?;
        writer.add_files(&c_inputs).map_err(ArchiveError::Engine)?;
        writer.compress_to(target.to_str().ok_or_else(|| ArchiveError::Engine("target path not UTF-8".into()))? )
            .map_err(ArchiveError::Engine)
    }

    fn update(&self, path: &Path, ops: &[EngineOp], password: Option<&password::Password>) -> Result<(), ArchiveError> {
        let lib = self.lib.lock().unwrap();
        let path_str = path_to_cstring(path)?;

        // Step 1: structural edits (delete/rename) through the archive
        // editor. Modify is handled as delete (here) + re-add (step 2).
        let has_structural = ops.iter().any(|op| matches!(op, EngineOp::Delete { .. } | EngineOp::Rename { .. } | EngineOp::Modify { .. }));
        if has_structural {
            let editor = ArchiveEditor::open(&lib, &path_str, WriterFormat::SevenZip, password.map(|p| p.as_str())).map_err(ArchiveError::Engine)?;
            for op in ops {
                match op {
                    EngineOp::Delete { archive_index } => {
                        editor.delete(*archive_index).map_err(ArchiveError::Engine)?;
                    }
                    EngineOp::Rename { archive_index, new_path } => {
                        editor.rename(*archive_index, new_path).map_err(ArchiveError::Engine)?;
                    }
                    EngineOp::Modify { archive_index, .. } => {
                        editor.delete(*archive_index).map_err(ArchiveError::Engine)?;
                    }
                    _ => {}
                }
            }
            editor.apply().map_err(ArchiveError::Engine)?;
        }

        // Step 2: additions and re-additions through a writer opened in
        // update mode. The editor from step 1 is closed by now, so the
        // archive is not held by two handles at once.
        let has_content = ops.iter().any(|op| matches!(op, EngineOp::Add { .. } | EngineOp::Modify { .. }));
        if has_content {
            let writer = ArchiveWriter::open(&lib, &path_str, WriterFormat::SevenZip, password).map_err(ArchiveError::Engine)?;
            writer.set_update_mode(crate::writer::UpdateMode::Update);
            for op in ops {
                match op {
                    EngineOp::Add { fs_path, archive_path } => {
                        let fs = path_to_cstring(fs_path)?;
                        writer.add_item_with_path(&fs, archive_path).map_err(ArchiveError::Engine)?;
                    }
                    EngineOp::Modify { archive_path, fs_path, .. } => {
                        let fs = path_to_cstring(fs_path)?;
                        writer.add_item_with_path(&fs, archive_path).map_err(ArchiveError::Engine)?;
                    }
                    _ => {}
                }
            }
            writer.compress_to(&path_str).map_err(ArchiveError::Engine)?;
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
