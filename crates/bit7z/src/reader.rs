use crate::handle::BitArchiveHandle;
use crate::library::Bit7zLibrary;
use bit7z_ffi::{
    bit7z_reader_extract_to_buffer_c, bit7z_reader_extract_to_cb_c,
    bit7z_reader_extract_with_rename_c, bit7z_reader_has_encrypted_items, c_void,
};
use password::Password;
use std::ffi::CStr;
use std::ptr;

// ============================================================================
// ArchiveReader
// ============================================================================

pub struct ArchiveReader {
    raw: BitArchiveHandle,
}

// SAFETY: ArchiveReader wraps a raw FFI handle to a C++ archive reader.
// The handle is only used for read operations which are thread-safe in bit7z.
// The handle is stored in Bit7zRepository's HashMap protected by Mutex, and
// all access goes through methods that lock the mutex first.
unsafe impl Send for ArchiveReader {}
unsafe impl Sync for ArchiveReader {}

impl ArchiveReader {
    pub fn open(
        lib: &Bit7zLibrary,
        path: &str,
        password: Option<&Password>,
    ) -> Result<Self, String> {
        let c_path = std::ffi::CString::new(path).map_err(|e| format!("Invalid path: {}", e))?;
        let c_pw = password
            .map(|p| std::ffi::CString::new(p.as_str()))
            .transpose()
            .map_err(|e| format!("Invalid password: {}", e))?;
        let raw = unsafe {
            BitArchiveHandle::from_raw(bit7z_ffi::bit7z_reader_open(
                lib.raw.as_ptr(),
                c_path.as_ptr(),
                c_pw.as_ref().map_or(ptr::null(), |p| p.as_ptr()),
            ))
        };
        if raw.is_null() {
            return Err("Failed to open archive".into());
        }
        Ok(Self { raw })
    }

    pub fn item_count(&self) -> u32 {
        unsafe { bit7z_ffi::bit7z_reader_item_count(self.raw.as_ptr()) }
    }

    pub fn item(&self, index: u32) -> Item<'_> {
        Item {
            reader: self,
            index,
        }
    }

    pub fn extract_to(&self, indices: &[u32], dest: &str) -> Result<(), String> {
        let c_dest = std::ffi::CString::new(dest).map_err(|e| format!("Invalid path: {}", e))?;
        let ret: i32 = unsafe {
            bit7z_ffi::bit7z_reader_extract_to(
                self.raw.as_ptr(),
                indices.as_ptr(),
                indices.len() as u32,
                c_dest.as_ptr(),
            )
        };
        if ret != 0 {
            Err("Extraction failed".into())
        } else {
            Ok(())
        }
    }

    pub fn extract_to_buffer(&self, index: u32) -> Result<Vec<u8>, String> {
        let mut out_data: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut out_size: i64 = 0;
        let ret = unsafe {
            bit7z_reader_extract_to_buffer_c(self.raw.as_ptr(), index, &mut out_data, &mut out_size)
        };
        if ret != 0 || out_size < 0 || (out_size > 0 && out_data.is_null()) {
            return Err("Extraction to buffer failed".into());
        }
        if out_size == 0 {
            return Ok(Vec::new());
        }
        let slice = unsafe { std::slice::from_raw_parts(out_data as *const u8, out_size as usize) };
        let result = slice.to_vec();
        unsafe {
            bit7z_ffi::bit7z_reader_free_buffer(out_data as *mut c_void);
        }
        Ok(result)
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

    /// Create from a raw handle (takes ownership).
    pub unsafe fn from_raw(raw: BitArchiveHandle) -> Self {
        Self { raw }
    }

    /// Test archive integrity.
    pub fn test(&self) -> Result<(bool, u32, u32, Vec<String>, Vec<String>), String> {
        let result = unsafe { bit7z_ffi::bit7z_reader_test(self.raw.as_ptr()) };
        if result.is_null() {
            return Err("test call failed".into());
        }
        let all_ok = unsafe { bit7z_ffi::bit7z_test_result_all_ok(result) } != 0;
        let total = unsafe { bit7z_ffi::bit7z_test_result_total(result) };
        let failed_count = unsafe { bit7z_ffi::bit7z_test_result_failed_count(result) };
        let failed_paths = Vec::new();
        let mut failed_errors = Vec::new();
        // Note: The C++ implementation only stores one error path/error for exception case
        // For per-item failures, we'd need extended C++ API
        if !all_ok && failed_count > 0 {
            let error_msg = unsafe {
                let ptr = bit7z_ffi::bit7z_test_result_error(result);
                if ptr.is_null() {
                    "test failed".to_string()
                } else {
                    std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
                }
            };
            failed_errors.push(error_msg);
        }
        unsafe {
            bit7z_ffi::bit7z_test_result_free(result);
        }
        Ok((all_ok, total, failed_count, failed_paths, failed_errors))
    }

    /// Check if opened archive has any encrypted items.
    pub fn has_encrypted_items(&self) -> bool {
        unsafe { bit7z_reader_has_encrypted_items(self.raw.as_ptr()) != 0 }
    }

    /// Extracts items with per-file overwrite/progress/file callbacks.
    /// `ctx` is passed to every callback as opaque user data.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::not_unsafe_ptr_arg_deref)] // ctx is an opaque token passed through to callbacks
    pub fn extract_to_cb(
        &self,
        indices: &[u32],
        dest: &str,
        ctx: *mut std::ffi::c_void,
        on_overwrite: Option<
            unsafe extern "C" fn(
                *const std::ffi::c_char,
                *const std::ffi::c_char,
                u64,
                u64,
                i64,
                i64,
                *mut std::ffi::c_void,
            ) -> i32,
        >,
        on_progress: Option<unsafe extern "C" fn(u64, u64, *mut std::ffi::c_void) -> i32>,
        on_file: Option<unsafe extern "C" fn(*const std::ffi::c_char, u64, *mut std::ffi::c_void)>,
    ) -> Result<(), String> {
        let c_dest = std::ffi::CString::new(dest).map_err(|e| format!("{}", e))?;
        let ret = unsafe {
            bit7z_reader_extract_to_cb_c(
                self.raw.as_ptr(),
                indices.as_ptr(),
                indices.len() as u32,
                c_dest.as_ptr(),
                ctx,
                on_overwrite,
                on_progress,
                on_file,
            )
        };
        if ret == 0 {
            Ok(())
        } else {
            Err("extraction failed or cancelled".into())
        }
    }

    /// Extract all items with per-file rename/skip/overwrite via RenameCallback.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::not_unsafe_ptr_arg_deref)] // ctx is an opaque token passed through to callbacks
    pub fn extract_with_rename(
        &self,
        dest: &str,
        ctx: *mut std::ffi::c_void,
        on_rename: Option<
            unsafe extern "C" fn(
                *const std::ffi::c_char,
                u32,
                u64,
                i32,
                *mut std::ffi::c_char,
                u32,
                *mut std::ffi::c_void,
            ) -> i32,
        >,
        on_progress: Option<unsafe extern "C" fn(u64, u64, *mut std::ffi::c_void) -> i32>,
        on_file: Option<unsafe extern "C" fn(*const std::ffi::c_char, *mut std::ffi::c_void)>,
    ) -> Result<(), String> {
        let c_dest = std::ffi::CString::new(dest).map_err(|e| format!("{}", e))?;
        let ret = unsafe {
            bit7z_reader_extract_with_rename_c(
                self.raw.as_ptr(),
                c_dest.as_ptr(),
                ctx,
                on_rename,
                on_progress,
                on_file,
            )
        };
        if ret == 0 {
            Ok(())
        } else {
            Err("extraction failed or cancelled".into())
        }
    }
}

impl Drop for ArchiveReader {
    fn drop(&mut self) {
        unsafe {
            bit7z_ffi::bit7z_reader_close(self.raw.as_ptr());
        }
    }
}

// ============================================================================
// Item
// ============================================================================

pub struct Item<'a> {
    reader: &'a ArchiveReader,
    pub index: u32,
}

impl<'a> Item<'a> {
    pub fn path(&self) -> String {
        let p = unsafe { bit7z_ffi::bit7z_item_path(self.reader.raw.as_ptr(), self.index) };
        if p.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() }
        }
    }
    pub fn name(&self) -> String {
        let n = unsafe { bit7z_ffi::bit7z_item_name(self.reader.raw.as_ptr(), self.index) };
        if n.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(n).to_string_lossy().into_owned() }
        }
    }
    pub fn size(&self) -> u64 {
        unsafe { bit7z_ffi::bit7z_item_size(self.reader.raw.as_ptr(), self.index) }
    }
    pub fn packed_size(&self) -> u64 {
        unsafe { bit7z_ffi::bit7z_item_packed_size(self.reader.raw.as_ptr(), self.index) }
    }
    pub fn is_directory(&self) -> bool {
        unsafe { bit7z_ffi::bit7z_item_is_dir(self.reader.raw.as_ptr(), self.index) != 0 }
    }
    pub fn is_encrypted(&self) -> bool {
        unsafe { bit7z_ffi::bit7z_item_is_encrypted(self.reader.raw.as_ptr(), self.index) != 0 }
    }

    fn raw_ptr(&self) -> *mut c_void {
        unsafe { bit7z_ffi::bit7z_item_from_reader(self.reader.raw.as_ptr(), self.index) }
    }

    pub fn mtime(&self) -> Result<u64, String> {
        let result = unsafe { bit7z_ffi::bit7z_item_mtime(self.raw_ptr()) };
        Ok(result)
    }

    pub fn ctime(&self) -> Result<u64, String> {
        let result = unsafe { bit7z_ffi::bit7z_item_ctime(self.raw_ptr()) };
        Ok(result)
    }

    pub fn atime(&self) -> Result<u64, String> {
        let result = unsafe { bit7z_ffi::bit7z_item_atime(self.raw_ptr()) };
        Ok(result)
    }

    pub fn attributes(&self) -> Result<u32, String> {
        let result = unsafe { bit7z_ffi::bit7z_item_attributes(self.raw_ptr()) };
        Ok(result)
    }

    pub fn host_os(&self) -> Result<u8, String> {
        let result = unsafe { bit7z_ffi::bit7z_item_host_os(self.raw_ptr()) };
        Ok(result)
    }

    pub fn compression_method(&self) -> Result<String, String> {
        let buf_size: u32 = 256;
        let mut buf: Vec<u8> = vec![0u8; buf_size as usize];
        let ret = unsafe {
            bit7z_ffi::bit7z_item_compression_method(
                self.raw_ptr(),
                buf.as_mut_ptr() as *mut _,
                buf_size,
            )
        };
        if ret < 0 {
            return Err("failed to get compression method".into());
        }
        let c_str = unsafe { CStr::from_ptr(buf.as_ptr() as *const _) };
        Ok(c_str.to_string_lossy().into_owned())
    }

    pub fn comment(&self) -> Result<String, String> {
        let buf_size: u32 = 256;
        let mut buf: Vec<u8> = vec![0u8; buf_size as usize];
        let ret = unsafe {
            bit7z_ffi::bit7z_item_comment(self.raw_ptr(), buf.as_mut_ptr() as *mut _, buf_size)
        };
        if ret < 0 {
            return Err("failed to get comment".into());
        }
        let c_str = unsafe { CStr::from_ptr(buf.as_ptr() as *const _) };
        Ok(c_str.to_string_lossy().into_owned())
    }

    pub fn user(&self) -> Result<String, String> {
        let buf_size: u32 = 256;
        let mut buf: Vec<u8> = vec![0u8; buf_size as usize];
        let ret = unsafe {
            bit7z_ffi::bit7z_item_user(self.raw_ptr(), buf.as_mut_ptr() as *mut _, buf_size)
        };
        if ret < 0 {
            return Err("failed to get user".into());
        }
        let c_str = unsafe { CStr::from_ptr(buf.as_ptr() as *const _) };
        Ok(c_str.to_string_lossy().into_owned())
    }

    pub fn group(&self) -> Result<String, String> {
        let buf_size: u32 = 256;
        let mut buf: Vec<u8> = vec![0u8; buf_size as usize];
        let ret = unsafe {
            bit7z_ffi::bit7z_item_group(self.raw_ptr(), buf.as_mut_ptr() as *mut _, buf_size)
        };
        if ret < 0 {
            return Err("failed to get group".into());
        }
        let c_str = unsafe { CStr::from_ptr(buf.as_ptr() as *const _) };
        Ok(c_str.to_string_lossy().into_owned())
    }

    pub fn is_symlink(&self) -> Result<bool, String> {
        let result = unsafe { bit7z_ffi::bit7z_item_is_symlink(self.raw_ptr()) };
        Ok(result != 0)
    }

    pub fn posix_attrib(&self) -> Result<u32, String> {
        let result = unsafe { bit7z_ffi::bit7z_item_posix_attrib(self.raw_ptr()) };
        Ok(result)
    }

    pub fn extension(&self) -> Result<String, String> {
        let buf_size: u32 = 256;
        let mut buf: Vec<u8> = vec![0u8; buf_size as usize];
        let ret = unsafe {
            bit7z_ffi::bit7z_item_extension(self.raw_ptr(), buf.as_mut_ptr() as *mut _, buf_size)
        };
        if ret < 0 {
            return Err("failed to get extension".into());
        }
        let c_str = unsafe { CStr::from_ptr(buf.as_ptr() as *const _) };
        Ok(c_str.to_string_lossy().into_owned())
    }

    pub fn hardlink(&self) -> Result<String, String> {
        let buf_size: u32 = 256;
        let mut buf: Vec<u8> = vec![0u8; buf_size as usize];
        let ret = unsafe {
            bit7z_ffi::bit7z_item_hardlink(self.raw_ptr(), buf.as_mut_ptr() as *mut _, buf_size)
        };
        if ret < 0 {
            return Err("failed to get hardlink".into());
        }
        let c_str = unsafe { CStr::from_ptr(buf.as_ptr() as *const _) };
        Ok(c_str.to_string_lossy().into_owned())
    }
}
