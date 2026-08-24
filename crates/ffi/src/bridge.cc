// Compilation unit for the C++ wrapper layer (demo.h).
//
// demo.h declares the exported functions as `extern "C" inline`. MSVC only
// emits COMDAT symbols for inline functions that are odr-used, so we take
// the address of every exported function here to force symbol emission.
// The addresses themselves are never used.

#include "demo.h"

extern "C" {
void* (*force_bit7z_create_library)(void) = (void* (*)(void)) &bit7z_create_library;
void* (*force_bit7z_destroy_library)(void) = (void* (*)(void)) &bit7z_destroy_library;
void* (*force_bit7z_reader_open)(void) = (void* (*)(void)) &bit7z_reader_open;
void* (*force_bit7z_reader_close)(void) = (void* (*)(void)) &bit7z_reader_close;
void* (*force_bit7z_reader_item_count)(void) = (void* (*)(void)) &bit7z_reader_item_count;
void* (*force_bit7z_item_path)(void) = (void* (*)(void)) &bit7z_item_path;
void* (*force_bit7z_item_name)(void) = (void* (*)(void)) &bit7z_item_name;
void* (*force_bit7z_item_size)(void) = (void* (*)(void)) &bit7z_item_size;
void* (*force_bit7z_item_packed_size)(void) = (void* (*)(void)) &bit7z_item_packed_size;
void* (*force_bit7z_item_is_dir)(void) = (void* (*)(void)) &bit7z_item_is_dir;
void* (*force_bit7z_item_is_encrypted)(void) = (void* (*)(void)) &bit7z_item_is_encrypted;
void* (*force_bit7z_item_crc)(void) = (void* (*)(void)) &bit7z_item_crc;
void* (*force_bit7z_item_crc_defined)(void) = (void* (*)(void)) &bit7z_item_crc_defined;
void* (*force_bit7z_item_mtime)(void) = (void* (*)(void)) &bit7z_item_mtime;
void* (*force_bit7z_item_ctime)(void) = (void* (*)(void)) &bit7z_item_ctime;
void* (*force_bit7z_item_atime)(void) = (void* (*)(void)) &bit7z_item_atime;
void* (*force_bit7z_item_attributes)(void) = (void* (*)(void)) &bit7z_item_attributes;
void* (*force_bit7z_item_host_os)(void) = (void* (*)(void)) &bit7z_item_host_os;
void* (*force_bit7z_item_compression_method)(void) = (void* (*)(void)) &bit7z_item_compression_method;
void* (*force_bit7z_item_comment)(void) = (void* (*)(void)) &bit7z_item_comment;
void* (*force_bit7z_item_user)(void) = (void* (*)(void)) &bit7z_item_user;
void* (*force_bit7z_item_group)(void) = (void* (*)(void)) &bit7z_item_group;
void* (*force_bit7z_item_is_symlink)(void) = (void* (*)(void)) &bit7z_item_is_symlink;
void* (*force_bit7z_item_posix_attrib)(void) = (void* (*)(void)) &bit7z_item_posix_attrib;
void* (*force_bit7z_item_extension)(void) = (void* (*)(void)) &bit7z_item_extension;
void* (*force_bit7z_item_hardlink)(void) = (void* (*)(void)) &bit7z_item_hardlink;
void* (*force_bit7z_item_from_reader)(void) = (void* (*)(void)) &bit7z_item_from_reader;
void* (*force_bit7z_reader_extract_to)(void) = (void* (*)(void)) &bit7z_reader_extract_to;
void* (*force_bit7z_reader_extract_item_to_buffer)(void) = (void* (*)(void)) &bit7z_reader_extract_item_to_buffer;
void* (*force_bit7z_reader_free_buffer)(void) = (void* (*)(void)) &bit7z_reader_free_buffer;
void* (*force_bit7z_reader_extract_to_buffer_c)(void) = (void* (*)(void)) &bit7z_reader_extract_to_buffer_c;
void* (*force_bit7z_reader_extract_item_data)(void) = (void* (*)(void)) &bit7z_reader_extract_item_data;
void* (*force_bit7z_reader_test)(void) = (void* (*)(void)) &bit7z_reader_test;
void* (*force_bit7z_test_result_total)(void) = (void* (*)(void)) &bit7z_test_result_total;
void* (*force_bit7z_test_result_failed_count)(void) = (void* (*)(void)) &bit7z_test_result_failed_count;
void* (*force_bit7z_test_result_all_ok)(void) = (void* (*)(void)) &bit7z_test_result_all_ok;
void* (*force_bit7z_test_result_error)(void) = (void* (*)(void)) &bit7z_test_result_error;
void* (*force_bit7z_test_result_free)(void) = (void* (*)(void)) &bit7z_test_result_free;
void* (*force_bit7z_is_header_encrypted)(void) = (void* (*)(void)) &bit7z_is_header_encrypted;
void* (*force_bit7z_is_encrypted)(void) = (void* (*)(void)) &bit7z_is_encrypted;
void* (*force_bit7z_reader_has_encrypted_items)(void) = (void* (*)(void)) &bit7z_reader_has_encrypted_items;
void* (*force_bit7z_reader_is_solid)(void) = (void* (*)(void)) &bit7z_reader_is_solid;
void* (*force_bit7z_reader_is_multi_volume)(void) = (void* (*)(void)) &bit7z_reader_is_multi_volume;
void* (*force_bit7z_reader_volumes_count)(void) = (void* (*)(void)) &bit7z_reader_volumes_count;
void* (*force_bit7z_reader_headers_size)(void) = (void* (*)(void)) &bit7z_reader_headers_size;
void* (*force_bit7z_reader_has_comment)(void) = (void* (*)(void)) &bit7z_reader_has_comment;
void* (*force_bit7z_reader_dictionary_size)(void) = (void* (*)(void)) &bit7z_reader_dictionary_size;
void* (*force_bit7z_reader_list_directory)(void) = (void* (*)(void)) &bit7z_reader_list_directory;
void* (*force_bit7z_item_list_count)(void) = (void* (*)(void)) &bit7z_item_list_count;
void* (*force_bit7z_item_list_index)(void) = (void* (*)(void)) &bit7z_item_list_index;
void* (*force_bit7z_item_list_path)(void) = (void* (*)(void)) &bit7z_item_list_path;
void* (*force_bit7z_item_list_size)(void) = (void* (*)(void)) &bit7z_item_list_size;
void* (*force_bit7z_item_list_packed_size)(void) = (void* (*)(void)) &bit7z_item_list_packed_size;
void* (*force_bit7z_item_list_is_dir)(void) = (void* (*)(void)) &bit7z_item_list_is_dir;
void* (*force_bit7z_item_list_is_encrypted)(void) = (void* (*)(void)) &bit7z_item_list_is_encrypted;
void* (*force_bit7z_item_list_crc)(void) = (void* (*)(void)) &bit7z_item_list_crc;
void* (*force_bit7z_item_list_item)(void) = (void* (*)(void)) &bit7z_item_list_item;
void* (*force_bit7z_item_list_free)(void) = (void* (*)(void)) &bit7z_item_list_free;
void* (*force_bit7z_reader_items)(void) = (void* (*)(void)) &bit7z_reader_items;
void* (*force_bit7z_writer_create)(void) = (void* (*)(void)) &bit7z_writer_create;
void* (*force_bit7z_writer_open)(void) = (void* (*)(void)) &bit7z_writer_open;
void* (*force_bit7z_writer_close)(void) = (void* (*)(void)) &bit7z_writer_close;
void* (*force_bit7z_writer_set_threads)(void) = (void* (*)(void)) &bit7z_writer_set_threads;
void* (*force_bit7z_writer_set_compression_level)(void) = (void* (*)(void)) &bit7z_writer_set_compression_level;
void* (*force_bit7z_writer_set_password)(void) = (void* (*)(void)) &bit7z_writer_set_password;
void* (*force_bit7z_writer_set_update_mode)(void) = (void* (*)(void)) &bit7z_writer_set_update_mode;
void* (*force_bit7z_writer_set_compression_method)(void) = (void* (*)(void)) &bit7z_writer_set_compression_method;
void* (*force_bit7z_writer_set_dictionary_size)(void) = (void* (*)(void)) &bit7z_writer_set_dictionary_size;
void* (*force_bit7z_writer_set_word_size)(void) = (void* (*)(void)) &bit7z_writer_set_word_size;
void* (*force_bit7z_writer_set_solid_mode)(void) = (void* (*)(void)) &bit7z_writer_set_solid_mode;
void* (*force_bit7z_writer_set_volume_size)(void) = (void* (*)(void)) &bit7z_writer_set_volume_size;
void* (*force_bit7z_writer_set_password_ex)(void) = (void* (*)(void)) &bit7z_writer_set_password_ex;
void* (*force_bit7z_writer_set_store_timestamps)(void) = (void* (*)(void)) &bit7z_writer_set_store_timestamps;
void* (*force_bit7z_writer_add_dir_filtered)(void) = (void* (*)(void)) &bit7z_writer_add_dir_filtered;
void* (*force_bit7z_writer_add_items)(void) = (void* (*)(void)) &bit7z_writer_add_items;
void* (*force_bit7z_writer_add_file)(void) = (void* (*)(void)) &bit7z_writer_add_file;
void* (*force_bit7z_writer_add_files)(void) = (void* (*)(void)) &bit7z_writer_add_files;
void* (*force_bit7z_writer_add_dir)(void) = (void* (*)(void)) &bit7z_writer_add_dir;
void* (*force_bit7z_writer_compress_to)(void) = (void* (*)(void)) &bit7z_writer_compress_to;
void* (*force_bit7z_writer_compress_to_cb)(void) = (void* (*)(void)) &bit7z_writer_compress_to_cb;
void* (*force_bit7z_editor_open)(void) = (void* (*)(void)) &bit7z_editor_open;
void* (*force_bit7z_editor_close)(void) = (void* (*)(void)) &bit7z_editor_close;
void* (*force_bit7z_editor_rename)(void) = (void* (*)(void)) &bit7z_editor_rename;
void* (*force_bit7z_editor_delete)(void) = (void* (*)(void)) &bit7z_editor_delete;
void* (*force_bit7z_editor_apply)(void) = (void* (*)(void)) &bit7z_editor_apply;
void* (*force_bit7z_reader_extract_to_cb_c)(void) = (void* (*)(void)) &bit7z_reader_extract_to_cb_c;
void* (*force_bit7z_writer_compress_to_cb_c)(void) = (void* (*)(void)) &bit7z_writer_compress_to_cb_c;
void* (*force_bit7z_reader_extract_with_rename_c)(void) = (void* (*)(void)) &bit7z_reader_extract_with_rename_c;
}
