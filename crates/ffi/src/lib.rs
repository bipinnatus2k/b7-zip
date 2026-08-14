// Allow non-standard naming for autocxx-generated C/C++ bindings
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use autocxx::prelude::*;

include_cpp! {
    #include "demo.h"
    safety!(unsafe_ffi)

     // 1. 强制告诉 autocxx：这是一个完整定义的类型，不是前置声明
    instantiable!("bit7z::tstring")
    generate!("bit7z::to_tstring")

    generate!("bit7z_create_library")
    generate!("bit7z_destroy_library")

    // generate!("t_string")
    // generate!("StringPair")
    // generate!("PathsPairArray")
    // generate!("std::vector<std::pair<t_string,t_string>>")

    // Directory listing (opaque handle)
    generate!("bit7z_reader_list_directory")
    generate!("bit7z_item_list_count")
    generate!("bit7z_item_list_index")
    generate!("bit7z_item_list_path")
    generate!("bit7z_item_list_size")
    generate!("bit7z_item_list_packed_size")
    generate!("bit7z_item_list_is_dir")
    generate!("bit7z_item_list_is_encrypted")
    generate!("bit7z_item_list_free")

    // Batch items (all items in one call)
    generate!("bit7z_reader_items")
    generate!("bit7z_item_list_crc")
    generate!("bit7z_item_list_item")

    // Test archive integrity
    generate!("bit7z_reader_test")
    generate!("bit7z_test_result_total")
    generate!("bit7z_test_result_failed_count")
    generate!("bit7z_test_result_all_ok")
    generate!("bit7z_test_result_error")
    generate!("bit7z_test_result_free")

    // Encryption detection
    generate!("bit7z_is_header_encrypted")
    generate!("bit7z_is_encrypted")
    generate!("bit7z_reader_has_encrypted_items")

    // Item property accessors (direct BitArchiveItem pointer)
    generate!("bit7z_item_path")
    generate!("bit7z_item_name")
    generate!("bit7z_item_size")
    generate!("bit7z_item_packed_size")
    generate!("bit7z_item_is_dir")
    generate!("bit7z_item_is_encrypted")
    generate!("bit7z_item_crc")
    generate!("bit7z_item_from_reader")
    generate!("bit7z_item_mtime")
    generate!("bit7z_item_ctime")
    generate!("bit7z_item_atime")
    generate!("bit7z_item_attributes")
    generate!("bit7z_item_host_os")
    generate!("bit7z_item_compression_method")
    generate!("bit7z_item_comment")
    generate!("bit7z_item_user")
    generate!("bit7z_item_group")
    generate!("bit7z_item_is_symlink")
    generate!("bit7z_item_posix_attrib")
    generate!("bit7z_item_extension")
    generate!("bit7z_item_hardlink")

    // Reader archive-level properties
    generate!("bit7z_reader_open")
    generate!("bit7z_reader_close")
    generate!("bit7z_reader_item_count")
    generate!("bit7z_reader_extract_to")
    generate!("bit7z_reader_extract_item_size")
    generate!("bit7z_reader_extract_item_data")
    generate!("bit7z_reader_free_buffer")
    generate!("bit7z_reader_is_solid")
    generate!("bit7z_reader_is_multi_volume")
    generate!("bit7z_reader_volumes_count")
    generate!("bit7z_reader_headers_size")
    generate!("bit7z_reader_has_comment")
    generate!("bit7z_reader_has_encrypted_items")
    generate!("bit7z_reader_dictionary_size")

    // Writer advanced settings
    generate!("bit7z_writer_create")
    generate!("bit7z_writer_open")
    generate!("bit7z_writer_close")
    generate!("bit7z_writer_set_threads")
    generate!("bit7z_writer_set_compression_level")
    generate!("bit7z_writer_set_password")
    generate!("bit7z_writer_set_update_mode")

    generate!("bit7z_writer_set_compression_method")
    generate!("bit7z_writer_set_dictionary_size")
    generate!("bit7z_writer_set_word_size")
    generate!("bit7z_writer_set_solid_mode")
    generate!("bit7z_writer_set_volume_size")
    generate!("bit7z_writer_set_password_ex")
    generate!("bit7z_writer_set_store_timestamps")
    generate!("bit7z_writer_add_file")
    // generate!("bit7z_writer_add_files") //TODO
    // generate!("bit7z_writer_add_items") //TODO
    generate!("bit7z_writer_add_dir")
    generate!("bit7z_writer_add_dir_filtered")

    generate!("bit7z_writer_compress_to")

    // bit7z_writer_add_items is NOT generated here because its signature
    // uses `const char**` (pointer-to-pointer), which autocxx cannot bind.
    // It is declared manually as an `extern "C"` in src/adapters/bit7z/mod.rs.

    generate!("bit7z_editor_open")
    generate!("bit7z_editor_close")
    generate!("bit7z_editor_rename")
    generate!("bit7z_editor_delete")
    generate!("bit7z_editor_apply")


    // generate!("std::vector<std::pair<bit7z::tstring, bit7z::tstring>>")
}


mod ffi_ext;

pub use ffi::*;
pub use ffi_ext::*;
pub use autocxx::*;

