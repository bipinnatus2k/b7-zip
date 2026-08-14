//! Auto-generated extern "C" declarations matching the exported functions
//! in demo.h (the `extern "C"`/`extern "C" inline` wrappers compiled by
//! bridge.cc). Do not edit by hand.

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

unsafe extern "C" {
    pub fn bit7z_create_library(dll_path: *const std::ffi::c_char) -> *mut std::ffi::c_void;
    pub fn bit7z_destroy_library(lib: *mut std::ffi::c_void) -> ();
    pub fn bit7z_reader_open(lib_ptr: *mut std::ffi::c_void, path: *const std::ffi::c_char, password: *const std::ffi::c_char) -> *mut std::ffi::c_void;
    pub fn bit7z_reader_close(reader_ptr: *mut std::ffi::c_void) -> ();
    pub fn bit7z_reader_item_count(reader_ptr: *mut std::ffi::c_void) -> u32;
    pub fn bit7z_item_path(reader_ptr: *mut std::ffi::c_void, index: u32) -> *const std::ffi::c_char;
    pub fn bit7z_item_name(reader_ptr: *mut std::ffi::c_void, index: u32) -> *const std::ffi::c_char;
    pub fn bit7z_item_size(reader_ptr: *mut std::ffi::c_void, index: u32) -> u64;
    pub fn bit7z_item_packed_size(reader_ptr: *mut std::ffi::c_void, index: u32) -> u64;
    pub fn bit7z_item_is_dir(reader_ptr: *mut std::ffi::c_void, index: u32) -> i32;
    pub fn bit7z_item_is_encrypted(reader_ptr: *mut std::ffi::c_void, index: u32) -> i32;
    pub fn bit7z_item_crc(reader_ptr: *mut std::ffi::c_void, index: u32) -> u32;
    pub fn bit7z_item_mtime(ptr: *mut std::ffi::c_void) -> u64;
    pub fn bit7z_item_ctime(ptr: *mut std::ffi::c_void) -> u64;
    pub fn bit7z_item_atime(ptr: *mut std::ffi::c_void) -> u64;
    pub fn bit7z_item_attributes(ptr: *mut std::ffi::c_void) -> u32;
    pub fn bit7z_item_host_os(ptr: *mut std::ffi::c_void) -> u8;
    pub fn bit7z_item_compression_method(ptr: *mut std::ffi::c_void, out_buf: *mut std::ffi::c_char, buf_size: u32) -> i32;
    pub fn bit7z_item_comment(ptr: *mut std::ffi::c_void, out_buf: *mut std::ffi::c_char, buf_size: u32) -> i32;
    pub fn bit7z_item_user(ptr: *mut std::ffi::c_void, out_buf: *mut std::ffi::c_char, buf_size: u32) -> i32;
    pub fn bit7z_item_group(ptr: *mut std::ffi::c_void, out_buf: *mut std::ffi::c_char, buf_size: u32) -> i32;
    pub fn bit7z_item_is_symlink(ptr: *mut std::ffi::c_void) -> i32;
    pub fn bit7z_item_posix_attrib(ptr: *mut std::ffi::c_void) -> u32;
    pub fn bit7z_item_extension(ptr: *mut std::ffi::c_void, out_buf: *mut std::ffi::c_char, buf_size: u32) -> i32;
    pub fn bit7z_item_hardlink(ptr: *mut std::ffi::c_void, out_buf: *mut std::ffi::c_char, buf_size: u32) -> i32;
    pub fn bit7z_item_from_reader(reader_ptr: *mut std::ffi::c_void, index: u32) -> *mut std::ffi::c_void;
    pub fn bit7z_reader_extract_to(reader_ptr: *mut std::ffi::c_void, indices: *const u32, count: u32, dest_path: *const std::ffi::c_char) -> i32;
    pub fn bit7z_reader_extract_item_to_buffer(reader_ptr: *mut std::ffi::c_void, index: u32, out_data: *mut *mut std::ffi::c_void, out_size: *mut i64) -> i32;
    pub fn bit7z_reader_free_buffer(data: *mut std::ffi::c_void) -> ();
    pub fn bit7z_reader_extract_item_data(reader_ptr: *mut std::ffi::c_void, index: u32) -> *mut std::ffi::c_void;
    pub fn bit7z_reader_test(reader_ptr: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    pub fn bit7z_test_result_total(result_ptr: *mut std::ffi::c_void) -> u32;
    pub fn bit7z_test_result_failed_count(result_ptr: *mut std::ffi::c_void) -> u32;
    pub fn bit7z_test_result_all_ok(result_ptr: *mut std::ffi::c_void) -> i32;
    pub fn bit7z_test_result_error(result_ptr: *mut std::ffi::c_void) -> *const std::ffi::c_char;
    pub fn bit7z_test_result_free(result_ptr: *mut std::ffi::c_void) -> ();
    pub fn bit7z_is_header_encrypted(lib_ptr: *mut std::ffi::c_void, path: *const std::ffi::c_char) -> i32;
    pub fn bit7z_is_encrypted(lib_ptr: *mut std::ffi::c_void, path: *const std::ffi::c_char) -> i32;
    pub fn bit7z_reader_has_encrypted_items(reader_ptr: *mut std::ffi::c_void) -> i32;
    pub fn bit7z_reader_is_solid(reader_ptr: *mut std::ffi::c_void) -> i32;
    pub fn bit7z_reader_is_multi_volume(reader_ptr: *mut std::ffi::c_void) -> i32;
    pub fn bit7z_reader_volumes_count(reader_ptr: *mut std::ffi::c_void) -> u32;
    pub fn bit7z_reader_headers_size(reader_ptr: *mut std::ffi::c_void) -> u64;
    pub fn bit7z_reader_has_comment(reader_ptr: *mut std::ffi::c_void) -> i32;
    pub fn bit7z_reader_dictionary_size(reader_ptr: *mut std::ffi::c_void) -> u64;
    pub fn bit7z_reader_list_directory(reader_ptr: *mut std::ffi::c_void, path: *const std::ffi::c_char) -> *mut std::ffi::c_void;
    pub fn bit7z_item_list_count(list_ptr: *mut std::ffi::c_void) -> u32;
    pub fn bit7z_item_list_index(list_ptr: *mut std::ffi::c_void, index: u32) -> u32;
    pub fn bit7z_item_list_path(list_ptr: *mut std::ffi::c_void, index: u32) -> *const std::ffi::c_char;
    pub fn bit7z_item_list_size(list_ptr: *mut std::ffi::c_void, index: u32) -> u64;
    pub fn bit7z_item_list_packed_size(list_ptr: *mut std::ffi::c_void, index: u32) -> u64;
    pub fn bit7z_item_list_is_dir(list_ptr: *mut std::ffi::c_void, index: u32) -> i32;
    pub fn bit7z_item_list_is_encrypted(list_ptr: *mut std::ffi::c_void, index: u32) -> i32;
    pub fn bit7z_item_list_crc(list_ptr: *mut std::ffi::c_void, index: u32) -> u32;
    pub fn bit7z_item_list_item(list_ptr: *mut std::ffi::c_void, index: u32) -> *mut std::ffi::c_void;
    pub fn bit7z_item_list_free(list_ptr: *mut std::ffi::c_void) -> ();
    pub fn bit7z_reader_items(reader_ptr: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    pub fn bit7z_writer_create(lib_ptr: *mut std::ffi::c_void, format: i32) -> *mut std::ffi::c_void;
    pub fn bit7z_writer_open(lib_ptr: *mut std::ffi::c_void, path: *const std::ffi::c_char, format: i32, password: *const std::ffi::c_char) -> *mut std::ffi::c_void;
    pub fn bit7z_writer_close(writer_ptr: *mut std::ffi::c_void) -> ();
    pub fn bit7z_writer_set_threads(writer_ptr: *mut std::ffi::c_void, n: u32) -> ();
    pub fn bit7z_writer_set_compression_level(writer_ptr: *mut std::ffi::c_void, level: i32) -> ();
    pub fn bit7z_writer_set_password(writer_ptr: *mut std::ffi::c_void, password: *const std::ffi::c_char) -> ();
    pub fn bit7z_writer_set_update_mode(writer_ptr: *mut std::ffi::c_void, mode: i32) -> ();
    pub fn bit7z_writer_set_compression_method(writer_ptr: *mut std::ffi::c_void, method: i32) -> ();
    pub fn bit7z_writer_set_dictionary_size(writer_ptr: *mut std::ffi::c_void, bytes: u32) -> ();
    pub fn bit7z_writer_set_word_size(writer_ptr: *mut std::ffi::c_void, bytes: u32) -> ();
    pub fn bit7z_writer_set_solid_mode(writer_ptr: *mut std::ffi::c_void, solid: i32) -> ();
    pub fn bit7z_writer_set_volume_size(writer_ptr: *mut std::ffi::c_void, bytes: u64) -> ();
    pub fn bit7z_writer_set_password_ex(writer_ptr: *mut std::ffi::c_void, password: *const std::ffi::c_char, encrypt_header: i32) -> ();
    pub fn bit7z_writer_set_store_timestamps(writer_ptr: *mut std::ffi::c_void, modified: i32, created: i32, accessed: i32) -> ();
    pub fn bit7z_writer_add_dir_filtered(writer_ptr: *mut std::ffi::c_void, dir: *const std::ffi::c_char, filter: *const std::ffi::c_char, policy: i32, recursive: i32) -> i32;
    pub fn bit7z_writer_add_items(writer_ptr: *mut std::ffi::c_void, paths: *const *const std::ffi::c_char, archive_paths: *const *const std::ffi::c_char, count: u32) -> i32;
    pub fn bit7z_writer_add_file(writer_ptr: *mut std::ffi::c_void, path: *const std::ffi::c_char) -> i32;
    pub fn bit7z_writer_add_dir(writer_ptr: *mut std::ffi::c_void, dir: *const std::ffi::c_char) -> i32;
    pub fn bit7z_writer_compress_to(writer_ptr: *mut std::ffi::c_void, out_path: *const std::ffi::c_char) -> i32;
    pub fn bit7z_editor_open(lib_ptr: *mut std::ffi::c_void, path: *const std::ffi::c_char, format: i32, password: *const std::ffi::c_char) -> *mut std::ffi::c_void;
    pub fn bit7z_editor_close(editor_ptr: *mut std::ffi::c_void) -> ();
    pub fn bit7z_editor_rename(editor_ptr: *mut std::ffi::c_void, index: u32, new_path: *const std::ffi::c_char) -> i32;
    pub fn bit7z_editor_delete(editor_ptr: *mut std::ffi::c_void, index: u32) -> i32;
    pub fn bit7z_editor_apply(editor_ptr: *mut std::ffi::c_void) -> i32;
}
