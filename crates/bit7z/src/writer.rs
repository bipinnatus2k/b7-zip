
use bit7z_ffi::{bit7z_writer_add_files, bit7z_writer_add_items, bit7z_writer_close, bit7z_writer_compress_to_cb, bit7z_writer_create, bit7z_writer_open, bit7z_writer_set_threads};
use password::Password;
use crate::handle::BitArchiveHandle;
use crate::library::Bit7zLibrary;

// ============================================================================
// Writer
// ============================================================================

/// Formats supported for writing (compressing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum WriterFormat {
    SevenZip = 0,
    Zip = 1,
    Tar = 2,
    GZip = 3,
    BZip2 = 4,
    Xz = 5,
    Wim = 6,
}

/// Compression levels mapped to bit7z BitCompressionLevel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum WriterCompressionLevel {
    None = 0,
    Fastest = 1,
    Fast = 2,
    Normal = 3,
    Max = 4,
    Ultra = 5,
}

/// Update mode for modifying existing archives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum UpdateMode {
    None = 0,
    Append = 1,
    Update = 2,
}

/// Compression method for writing archives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum WriterCompressionMethod {
    Copy = 0,
    Deflate = 1,
    Deflate64 = 2,
    BZip2 = 3,
    Lzma = 4,
    Lzma2 = 5,
    Ppmd = 6,
}

/// Encryption scope for archive passwords.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionScope {
    DataOnly = 0,
    DataAndHeaders = 1,
}

/// Filter policy for directory enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum FilterPolicy {
    Include = 0,
    Exclude = 1,
}

impl From<WriterFormat> for i32 {
    fn from(value: WriterFormat) -> i32 {
        value as i32
    }
}

impl From<WriterCompressionLevel> for i32 {
    fn from(value: WriterCompressionLevel) -> i32 {
        value as i32
    }
}

impl From<UpdateMode> for i32 {
    fn from(value: UpdateMode) -> i32 {
        value as i32
    }
}

impl From<WriterCompressionMethod> for i32 {
    fn from(value: WriterCompressionMethod) -> i32 {
        value as i32
    }
}

impl From<FilterPolicy> for i32 {
    fn from(value: FilterPolicy) -> i32 {
        value as i32
    }
}

pub struct ArchiveWriter {
    raw: BitArchiveHandle,
}

// SAFETY: Writer wraps a raw FFI handle to a C++ archive writer.
// Writer instances are short-lived and used within a single operation.
// All access is serialized through Bit7zRepository's Mutex-protected library lock.
unsafe impl Send for ArchiveWriter {}
unsafe impl Sync for ArchiveWriter {}

impl ArchiveWriter {
    pub fn create(lib: &Bit7zLibrary, format: WriterFormat) -> Result<Self, String> {
        let raw = unsafe { bit7z_writer_create(lib.raw_handle().as_ptr(), format.into()) };
        if raw.is_null() {
            Err("failed to create writer".into())
        } else {
            Ok(Self {
                raw: BitArchiveHandle::from_raw(raw),
            })
        }
    }

    pub unsafe fn from_raw(raw: BitArchiveHandle) -> Self {
        Self { raw }
    }

    pub fn open(
        lib: &Bit7zLibrary,
        path: &str,
        format: WriterFormat,
        password: Option<&Password>,
    ) -> Result<Self, String> {
        let c_path = std::ffi::CString::new(path).map_err(|e| format!("{}", e))?;
        let c_pw = password.and_then(|p| std::ffi::CString::new(p.as_str()).ok());
        let raw = unsafe {
            bit7z_writer_open(
                lib.raw_handle().as_ptr(),
                c_path.as_ptr(),
                format.into(),
                c_pw.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
            )
        };
        if raw.is_null() {
            Err("failed to open writer".into())
        } else {
            Ok(Self {
                raw: BitArchiveHandle::from_raw(raw),
            })
        }
    }

    pub fn set_threads(&self, n: u32) {
        unsafe {
            bit7z_writer_set_threads(self.raw.as_ptr(), n);
        }
    }

    pub fn set_compression_level(&self, level: WriterCompressionLevel) {
        unsafe {
            bit7z_ffi::bit7z_writer_set_compression_level(self.raw.as_ptr(), level.into());
        }
    }

    pub fn set_password(&self, password: &str) {
        let c_pw = std::ffi::CString::new(password).unwrap();
        unsafe {
            bit7z_ffi::bit7z_writer_set_password(self.raw.as_ptr(), c_pw.as_ptr());
        }
    }

    pub fn set_update_mode(&self, mode: UpdateMode) {
        unsafe {
            bit7z_ffi::bit7z_writer_set_update_mode(self.raw.as_ptr(), mode.into());
        }
    }

    pub fn add_file(&self, path: &str) -> Result<(), String> {
        let c_path = std::ffi::CString::new(path).map_err(|e| format!("{}", e))?;
        let ret = unsafe { bit7z_ffi::bit7z_writer_add_file(self.raw.as_ptr(), c_path.as_ptr()) };
        if ret == 0 {
            Ok(())
        } else {
            Err("add_file failed".into())
        }
    }

    pub fn add_files(&self, paths: &[&str]) -> Result<(), String> {
        let c_paths: Vec<std::ffi::CString> = paths
            .iter()
            .filter_map(|p| std::ffi::CString::new(*p).ok())
            .collect();
        let ptrs: Vec<*const std::ffi::c_char> = c_paths.iter().map(|s| s.as_ptr()).collect();
        let ret =
            unsafe { bit7z_writer_add_files(self.raw.as_ptr(), ptrs.as_ptr(), ptrs.len() as u32) };
        if ret == 0 {
            Ok(())
        } else {
            Err("add_files failed".into())
        }
    }

    /// Add a single file at `fs_path` into the archive under `archive_path`.",
    pub fn add_item_with_path(&self, fs_path: &str, archive_path: &str) -> Result<(), String> {
        let c_fs = std::ffi::CString::new(fs_path).map_err(|e| format!("{}", e))?;
        let c_arch = std::ffi::CString::new(archive_path).map_err(|e| format!("{}", e))?;
        let fs_ptr = c_fs.as_ptr();
        let arch_ptr = c_arch.as_ptr();
        let ret = unsafe {
            bit7z_writer_add_items(
                self.raw.as_ptr(),
                &fs_ptr,
                &arch_ptr,
                1,
            )
        };
        if ret == 0 {
            Ok(())
        } else {
            Err("add_item_with_path failed".into())
        }
    }

    pub fn add_directory(&self, dir: &str) -> Result<(), String> {
        let c_dir = std::ffi::CString::new(dir).map_err(|e| format!("{}", e))?;
        let ret = unsafe { bit7z_ffi::bit7z_writer_add_dir(self.raw.as_ptr(), c_dir.as_ptr()) };
        if ret == 0 {
            Ok(())
        } else {
            Err("add_directory failed".into())
        }
    }

    pub fn compress_to(&self, out_path: &str) -> Result<(), String> {
        let c_out = std::ffi::CString::new(out_path).map_err(|e| format!("{}", e))?;
        let ret = unsafe { bit7z_ffi::bit7z_writer_compress_to(self.raw.as_ptr(), c_out.as_ptr()) };
        if ret == 0 {
            Ok(())
        } else {
            Err("compress_to failed".into())
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub unsafe fn compress_to_cb(
        &self,
        out_path: &str,
        ctx: *mut std::ffi::c_void,
        on_progress: Option<unsafe extern "C" fn(u64, u64, *mut std::ffi::c_void) -> i32>,
        on_file: Option<unsafe extern "C" fn(*const std::ffi::c_char, *mut std::ffi::c_void)>,
    ) -> Result<(), String> {
        let c_out = std::ffi::CString::new(out_path).map_err(|e| format!("{}", e))?;
        let ret = unsafe {
            bit7z_writer_compress_to_cb(
                self.raw.as_ptr(),
                c_out.as_ptr(),
                ctx,
                on_progress,
                on_file,
            )
        };
        if ret == 0 {
            Ok(())
        } else {
            Err("compress_to failed or cancelled".into())
        }
    }

    pub fn set_compression_method(&self, method: WriterCompressionMethod) {
        unsafe {
            bit7z_ffi::bit7z_writer_set_compression_method(self.raw.as_ptr(), method.into());
        }
    }

    pub fn set_dictionary_size(&self, bytes: u32) {
        unsafe {
            bit7z_ffi::bit7z_writer_set_dictionary_size(self.raw.as_ptr(), bytes);
        }
    }

    pub fn set_word_size(&self, bytes: u32) {
        unsafe {
            bit7z_ffi::bit7z_writer_set_word_size(self.raw.as_ptr(), bytes);
        }
    }

    pub fn set_solid_mode(&self, solid: bool) {
        unsafe {
            bit7z_ffi::bit7z_writer_set_solid_mode(self.raw.as_ptr(), solid.into());
        }
    }

    pub fn set_volume_size(&self, bytes: u64) {
        unsafe {
            bit7z_ffi::bit7z_writer_set_volume_size(self.raw.as_ptr(), bytes);
        }
    }

    pub fn set_password_ex(&self, password: &str, encrypt_header: bool) {
        let c_pw = std::ffi::CString::new(password).unwrap();
        unsafe {
            bit7z_ffi::bit7z_writer_set_password_ex(
                self.raw.as_ptr(),
                c_pw.as_ptr(),
                encrypt_header as i32,
            );
        }
    }

    pub fn set_store_timestamps(&self, modified: bool, created: bool, accessed: bool) {
        unsafe {
            bit7z_ffi::bit7z_writer_set_store_timestamps(
                self.raw.as_ptr(),
                modified as i32,
                created as i32,
                accessed as i32,
            );
        }
    }

    pub fn add_dir_filtered(
        &self,
        dir: &str,
        filter: &str,
        policy: FilterPolicy,
        recursive: bool,
    ) -> Result<(), String> {
        let c_dir = std::ffi::CString::new(dir).map_err(|e| format!("{}", e))?;
        let c_filter = std::ffi::CString::new(filter).map_err(|e| format!("{}", e))?;
        let ret = unsafe {
            bit7z_ffi::bit7z_writer_add_dir_filtered(
                self.raw.as_ptr(),
                c_dir.as_ptr(),
                c_filter.as_ptr(),
                policy as i32,
                recursive as i32,
            )
        };
        if ret == 0 {
            Ok(())
        } else {
            Err("add_dir_filtered failed".into())
        }
    }

    pub fn add_items(&self, paths_and_names: &[(&str, &str)]) -> Result<(), String> {
        let c_paths: Vec<std::ffi::CString> = paths_and_names
            .iter()
            .filter_map(|(p, _)| std::ffi::CString::new(*p).ok())
            .collect();
        let c_names: Vec<std::ffi::CString> = paths_and_names
            .iter()
            .filter_map(|(_, n)| std::ffi::CString::new(*n).ok())
            .collect();
        let path_ptrs: Vec<*const std::ffi::c_char> = c_paths.iter().map(|s| s.as_ptr()).collect();
        let name_ptrs: Vec<*const std::ffi::c_char> = c_names.iter().map(|s| s.as_ptr()).collect();
        let ret = unsafe {
            bit7z_writer_add_items(
                self.raw.as_ptr(),
                path_ptrs.as_ptr(),
                name_ptrs.as_ptr(),
                c_paths.len() as u32,
            )
        };
        if ret == 0 {
            Ok(())
        } else {
            Err("add_items failed".into())
        }
    }

    /// Borrow the raw FFI handle.
    pub fn raw_handle(&self) -> BitArchiveHandle {
        self.raw
    }

    /// Take ownership of the raw handle (prevents Drop from closing).
    pub fn into_raw(self) -> BitArchiveHandle {
        let h = self.raw;
        std::mem::forget(self);
        h
    }
}

impl Drop for ArchiveWriter {
    fn drop(&mut self) {
        unsafe {
            bit7z_writer_close(self.raw.as_ptr());
        }
    }
}