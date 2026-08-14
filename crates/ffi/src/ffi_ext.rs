
// C-linkage callback-based extraction (declared in demo.h)
unsafe extern "C" {
    pub fn bit7z_reader_extract_to_cb_c(
        reader: *mut std::ffi::c_void,
        indices: *const u32,
        count: u32,
        dest: *const std::ffi::c_char,
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
    ) -> i32;
}

unsafe extern "C" {
    pub fn bit7z_reader_extract_with_rename_c(
        reader: *mut std::ffi::c_void,
        dest: *const std::ffi::c_char,
        ctx: *mut std::ffi::c_void,
        on_rename: Option<
            unsafe extern "C" fn(
                *const std::ffi::c_char,
                u64,
                i32,
                *mut std::ffi::c_char,
                u32,
                *mut std::ffi::c_void,
            ) -> i32,
        >,
        on_progress: Option<unsafe extern "C" fn(u64, u64, *mut std::ffi::c_void) -> i32>,
        on_file: Option<unsafe extern "C" fn(*const std::ffi::c_char, *mut std::ffi::c_void)>,
    ) -> i32;
}

// ============================================================================
// Writer / Editor FFI declarations (extern "C" linkage in demo.h)
// ============================================================================

unsafe extern "C" {
    // fn bit7z_writer_create(lib: *mut std::ffi::c_void, format: i32) -> *mut std::ffi::c_void;
    // fn bit7z_writer_open(
    //     lib: *mut std::ffi::c_void,
    //     path: *const std::ffi::c_char,
    //     format: i32,
    //     password: *const std::ffi::c_char,
    // ) -> *mut std::ffi::c_void;
    // fn bit7z_writer_close(w: *mut std::ffi::c_void);
    // fn bit7z_writer_set_threads(w: *mut std::ffi::c_void, n: u32);
    // fn bit7z_writer_set_compression_level(w: *mut std::ffi::c_void, level: i32);
    // fn bit7z_writer_set_password(w: *mut std::ffi::c_void, password: *const std::ffi::c_char);
    // fn bit7z_writer_set_update_mode(w: *mut std::ffi::c_void, mode: i32);
    // fn bit7z_writer_add_file(w: *mut std::ffi::c_void, path: *const std::ffi::c_char) -> i32;
    fn bit7z_writer_add_files(
        w: *mut std::ffi::c_void,
        paths: *const *const std::ffi::c_char,
        count: u32,
    ) -> i32;
    // bit7z_writer_add_items uses `const char**` which autocxx cannot bind,
    // so it is declared manually here instead of via generate!() in ffi.rs.
    // fn bit7z_writer_add_items(
    //     w: *mut std::ffi::c_void,
    //     paths: *const *const std::ffi::c_char,
    //     archive_paths: *const *const std::ffi::c_char,
    //     count: u32,
    // ) -> i32;
    // fn bit7z_writer_add_dir(w: *mut std::ffi::c_void, dir: *const std::ffi::c_char) -> i32;
    // fn bit7z_writer_compress_to(w: *mut std::ffi::c_void, out_path: *const std::ffi::c_char)
    //                             -> i32;
    pub fn bit7z_writer_compress_to_cb(
        w: *mut std::ffi::c_void,
        out_path: *const std::ffi::c_char,
        ctx: *mut std::ffi::c_void,
        on_progress: Option<unsafe extern "C" fn(u64, u64, *mut std::ffi::c_void) -> i32>,
        on_file: Option<unsafe extern "C" fn(*const std::ffi::c_char, *mut std::ffi::c_void)>,
    ) -> i32;


    // Test archive integrity — not yet used (inline in demo.h, missing linker symbols)
    // fn bit7z_reader_test(reader: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    // fn bit7z_test_result_total(result: *mut std::ffi::c_void) -> u32;
    // fn bit7z_test_result_failed_count(result: *mut std::ffi::c_void) -> u32;
    // fn bit7z_test_result_all_ok(result: *mut std::ffi::c_void) -> i32;
    // fn bit7z_test_result_error(result: *mut std::ffi::c_void) -> *const std::ffi::c_char;
    // fn bit7z_test_result_free(result: *mut std::ffi::c_void);

    // Encryption detection
    // fn bit7z_reader_has_encrypted_items(reader: *mut std::ffi::c_void) -> i32;
    // Single-call extract to buffer (avoids double-extraction).
    pub fn bit7z_reader_extract_to_buffer_c(
        reader: *mut std::ffi::c_void,
        index: u32,
        out_data: *mut *mut std::ffi::c_void,
        out_size: *mut i64,
    ) -> i32;
}
