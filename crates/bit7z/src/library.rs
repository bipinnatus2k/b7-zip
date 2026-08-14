
use crate::handle::BitArchiveHandle;

// ============================================================================
// Library
// ============================================================================

pub struct Bit7zLibrary {
    pub(crate) raw: BitArchiveHandle,
}

// SAFETY: Library wraps a raw FFI handle to a C++ bit7z library instance.
// The underlying C++ library is thread-safe for concurrent read operations.
// All FFI calls go through the repository's Mutex-protected methods, ensuring
// serialized access to mutable operations.
unsafe impl Send for Bit7zLibrary {}
unsafe impl Sync for Bit7zLibrary {}

impl Bit7zLibrary {
    pub fn open(path: &str) -> Result<Self, String> {
        let c_path = std::ffi::CString::new(path).map_err(|e| format!("Invalid path: {}", e))?;
        let raw = unsafe { BitArchiveHandle::from_raw(bit7z_ffi::bit7z_create_library(c_path.as_ptr())) };
        if raw.is_null() {
            return Err("Failed to load 7-Zip library".into());
        }
        Ok(Self { raw })
    }

    /// Create from a raw handle (takes ownership).
    pub unsafe fn from_raw(raw: BitArchiveHandle) -> Self {
        Self { raw }
    }

    /// Borrow the raw FFI handle.
    pub fn raw_handle(&self) -> BitArchiveHandle {
        self.raw
    }

    /// Check if archive at path has encrypted headers (static check without opening).
    pub fn is_header_encrypted(&self, path: &str) -> bool {
        let c_path = match std::ffi::CString::new(path) {
            Ok(p) => p,
            Err(_) => return false,
        };
        unsafe { bit7z_ffi::bit7z_is_header_encrypted(self.raw.as_ptr(), c_path.as_ptr()) != 0 }
    }

    /// Check if archive at path is encrypted (static check without opening).
    pub fn is_encrypted(&self, path: &str) -> bool {
        let c_path = match std::ffi::CString::new(path) {
            Ok(p) => p,
            Err(_) => return false,
        };
        unsafe { bit7z_ffi::bit7z_is_encrypted(self.raw.as_ptr(), c_path.as_ptr()) != 0 }
    }
}

impl Drop for Bit7zLibrary {
    fn drop(&mut self) {
        unsafe {
            bit7z_ffi::bit7z_destroy_library(self.raw.as_ptr());
        }
    }
}
