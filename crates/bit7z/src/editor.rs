
use crate::handle::BitArchiveHandle;
use crate::library::Bit7zLibrary;
use crate::writer::WriterFormat;

// ============================================================================
// Editor
// ============================================================================
pub struct ArchiveEditor {
    raw: BitArchiveHandle,
}

// SAFETY: Editor wraps a raw FFI handle to a C++ archive editor.
// Editor instances are short-lived and used within a single operation.
// All access is serialized through Bit7zRepository's Mutex-protected library lock.
unsafe impl Send for ArchiveEditor {}
unsafe impl Sync for ArchiveEditor {}

impl ArchiveEditor {
    pub fn open(
        lib: &Bit7zLibrary,
        path: &str,
        format: WriterFormat,
        password: Option<&str>,
    ) -> Result<Self, String> {
        let c_path = std::ffi::CString::new(path).map_err(|e| format!("{}", e))?;
        let c_pw = password.and_then(|p| std::ffi::CString::new(p).ok());
        let raw = unsafe {
            bit7z_ffi::bit7z_editor_open(
                lib.raw_handle().as_ptr(),
                c_path.as_ptr(),
                format.into(),
                c_pw.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
            )
        };
        if raw.is_null() {
            Err("failed to open editor".into())
        } else {
            Ok(Self {
                raw: BitArchiveHandle::from_raw(raw),
            })
        }
    }

    pub fn rename(&self, index: u32, new_path: &str) -> Result<(), String> {
        let c_path = std::ffi::CString::new(new_path).map_err(|e| format!("{}", e))?;
        let ret = unsafe { bit7z_ffi::bit7z_editor_rename(self.raw.as_ptr(), index, c_path.as_ptr()) };
        if ret == 0 {
            Ok(())
        } else {
            Err("rename failed".into())
        }
    }

    pub fn delete(&self, index: u32) -> Result<(), String> {
        let ret = unsafe { bit7z_ffi::bit7z_editor_delete(self.raw.as_ptr(), index) };
        if ret == 0 {
            Ok(())
        } else {
            Err("delete failed".into())
        }
    }

    pub fn apply(&self) -> Result<(), String> {
        let ret = unsafe { bit7z_ffi::bit7z_editor_apply(self.raw.as_ptr()) };
        if ret == 0 {
            Ok(())
        } else {
            Err("apply changes failed".into())
        }
    }
}

impl Drop for ArchiveEditor {
    fn drop(&mut self) {
        unsafe {
            bit7z_ffi::bit7z_editor_close(self.raw.as_ptr());
        }
    }
}
