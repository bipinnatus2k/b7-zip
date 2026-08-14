

/// Opaque handle wrapping a raw C++ pointer stored as usize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitArchiveHandle(usize);

impl BitArchiveHandle {
    #[inline]
    pub fn from_raw<T>(ptr: *mut T) -> Self {
        BitArchiveHandle(ptr as usize)
    }

    #[inline]
    pub fn as_ptr<T>(self) -> *mut T {
        self.0 as *mut T
    }

    #[inline]
    pub fn null() -> Self {
        BitArchiveHandle(0)
    }

    #[inline]
    pub fn is_null(self) -> bool {
        self.0 == 0
    }
}

// ============================================================================
// FfiHandle — RAII wrapper for C++ resource lifecycle
// ============================================================================

/// Identifies the kind of C++ resource held by an [`FfiArchiveHandle`], so that
/// `Drop` can call the correct C destructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveHandleKind {
    Reader,
    Writer,
    Editor,
}

/// RAII wrapper around a raw C++ pointer stored in `Bit7zRepository.handles`.
///
/// When an `FfiHandle` is dropped it automatically calls the appropriate
/// C destructor (`bit7z_reader_close`, `bit7z_writer_close`, or
/// `bit7z_editor_close`), preventing resource leaks even on panic paths.
pub struct FfiArchiveHandle {
    ptr: *mut std::ffi::c_void,
    kind: ArchiveHandleKind,
}

// SAFETY: FfiHandle wraps a raw FFI pointer. All access is serialized
// through the repository's Mutex, which ensures only one thread calls
// into the C++ bit7z library at a time on the same handle.
unsafe impl Send for FfiArchiveHandle {}
unsafe impl Sync for FfiArchiveHandle {}

impl FfiArchiveHandle {
    pub fn reader(ptr: *mut std::ffi::c_void) -> Self {
        Self {
            ptr,
            kind: ArchiveHandleKind::Reader,
        }
    }

    pub fn writer(ptr: *mut std::ffi::c_void) -> Self {
        Self {
            ptr,
            kind: ArchiveHandleKind::Writer,
        }
    }

    pub fn editor(ptr: *mut std::ffi::c_void) -> Self {
        Self {
            ptr,
            kind: ArchiveHandleKind::Editor,
        }
    }

    pub fn ptr(&self) -> *mut std::ffi::c_void {
        self.ptr
    }

    pub fn kind(&self) -> ArchiveHandleKind {
        self.kind
    }

    pub fn is_null(&self) -> bool {
        self.ptr.is_null()
    }
}

impl Drop for FfiArchiveHandle {
    fn drop(&mut self) {
        if self.ptr.is_null() {
            return;
        }
        unsafe {
            match self.kind {
                ArchiveHandleKind::Reader => bit7z_ffi::bit7z_reader_close(self.ptr as *mut _),
                ArchiveHandleKind::Writer => bit7z_ffi::bit7z_writer_close(self.ptr as *mut _),
                ArchiveHandleKind::Editor => bit7z_ffi::bit7z_editor_close(self.ptr as *mut _),
            }
        }
    }
}
