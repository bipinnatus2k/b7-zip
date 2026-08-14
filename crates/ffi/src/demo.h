#pragma once

#define BIT7Z_AUTO_FORMAT

#include "bit7z/bitformat.hpp"
#include "bit7z/bit7zlibrary.hpp"
#include "bit7z/bitinputarchive.hpp"
#include "bit7z/bitarchivereader.hpp"
#include "bit7z/bitarchivewriter.hpp"
#include "bit7z/bitarchiveeditor.hpp"
#include "bit7z/bitarchiveiteminfo.hpp"
#include "bit7z/biterror.hpp"
#include "bit7z/bittypes.hpp"
#include <bit7z/bitwindows.hpp>
#include <sys/stat.h>
#include <chrono>
#include <memory>
#include <type_traits>
#include <unordered_set>

// Type aliases for ergonomic Rust naming
using ArchiveFormatFeatures = bit7z::FormatFeatures;
using ArchiveInFormat = bit7z::BitInFormat;
using ArchiveInOutFormat = bit7z::BitInOutFormat;
using ArchiveItem = bit7z::BitArchiveItem;
using ArchiveItemInfo = bit7z::BitArchiveItemInfo;
using ArchiveProperties = bit7z::BitProperty;
using ArchivePropVariant = bit7z::BitPropVariant;
using ArchiveItemOffset = bit7z::BitArchiveItemOffset;
using ArchiveLibrary = bit7z::Bit7zLibrary;
using ArchiveStartOffset = bit7z::ArchiveStartOffset;
using InputArchive = bit7z::BitInputArchive;
using ArchiveReader = bit7z::BitArchiveReader;
using ArchiveWriter = bit7z::BitArchiveWriter;
using buffer_t = bit7z::buffer_t;
using time_type = bit7z::time_type;
using ArchiveError = bit7z::BitError;

// ===== Test function =====
extern "C" inline int multiply(int a, int b) { return a * b; }

// ===== Library wrappers =====
extern "C" inline void* bit7z_create_library(const char* dll_path) {
    try {
        auto* lib = new bit7z::Bit7zLibrary(dll_path ? std::string(dll_path) : "");
        return static_cast<void*>(lib);
    } catch (...) { return nullptr; }
}
extern "C" inline void bit7z_destroy_library(void* lib) {
    delete static_cast<bit7z::Bit7zLibrary*>(lib);
}

// ===== Reader wrappers =====

extern "C" inline void* bit7z_reader_open(void* lib_ptr, const char* path, const char* password) {
    try {
        auto& lib = *static_cast<bit7z::Bit7zLibrary*>(lib_ptr);
        return new bit7z::BitArchiveReader(lib,
            bit7z::tstring(path ? path : ""),
            bit7z::BitFormat::Auto,
            bit7z::tstring(password ? password : ""));
    } catch (...) { return nullptr; }
}
extern "C" inline void bit7z_reader_close(void* reader_ptr) {
    delete static_cast<bit7z::BitArchiveReader*>(reader_ptr);
}
extern "C" inline uint32_t bit7z_reader_item_count(void* reader_ptr) {
    try {
        return static_cast<bit7z::BitArchiveReader*>(reader_ptr)->itemsCount();
    } catch (...) { return 0; }
}

// ===== Item accessor wrappers (thread_local buffers, thread-safe) =====
extern "C" inline const char* bit7z_item_path(void* reader_ptr, uint32_t index) {
    try {
        thread_local std::string s;
        s = static_cast<bit7z::BitArchiveReader*>(reader_ptr)->items()[index].path();
        return s.c_str();
    } catch (...) { return ""; }
}
extern "C" inline const char* bit7z_item_name(void* reader_ptr, uint32_t index) {
    try {
        thread_local std::string s;
        s = static_cast<bit7z::BitArchiveReader*>(reader_ptr)->items()[index].name();
        return s.c_str();
    } catch (...) { return ""; }
}
extern "C" inline uint64_t bit7z_item_size(void* reader_ptr, uint32_t index) {
    try {
        return static_cast<bit7z::BitArchiveReader*>(reader_ptr)->items()[index].size();
    } catch (...) { return 0; }
}
extern "C" inline uint64_t bit7z_item_packed_size(void* reader_ptr, uint32_t index) {
    try {
        return static_cast<bit7z::BitArchiveReader*>(reader_ptr)->items()[index].packSize();
    } catch (...) { return 0; }
}
extern "C" inline int32_t bit7z_item_is_dir(void* reader_ptr, uint32_t index) {
    try {
        return static_cast<bit7z::BitArchiveReader*>(reader_ptr)->items()[index].isDir() ? 1 : 0;
    } catch (...) { return 0; }
}
extern "C" inline int32_t bit7z_item_is_encrypted(void* reader_ptr, uint32_t index) {
    try {
        return static_cast<bit7z::BitArchiveReader*>(reader_ptr)->items()[index].isEncrypted() ? 1 : 0;
    } catch (...) { return 0; }
}
extern "C" inline uint32_t bit7z_item_crc(void* reader_ptr, uint32_t index) {
    try {
        return static_cast<bit7z::BitArchiveReader*>(reader_ptr)->items()[index].crc();
    } catch (...) { return 0; }
}

// Helper: copy bit7z::tstring to a UTF-8 char buffer (caller-owned).
// Returns byte count written (excluding null terminator), or -1 on error.
inline int32_t tstring_to_utf8(const bit7z::tstring& src, char* out_buf, uint32_t buf_size) {
    if (!out_buf || buf_size == 0) return 0;
    size_t len = src.size();
    if (len >= (size_t)buf_size) len = (size_t)buf_size - 1;
    std::copy(src.begin(), src.begin() + len, out_buf);
    out_buf[len] = '\0';
    return (int32_t)len;
}

// ===== Item property wrappers (direct BitArchiveItem pointer) =====

extern "C" inline uint64_t bit7z_item_mtime(void* ptr) {
    try {
        auto* item = static_cast<bit7z::BitArchiveItem*>(ptr);
        auto tp = item->lastWriteTime();
        return static_cast<uint64_t>(std::chrono::system_clock::to_time_t(tp));
    } catch (...) { return 0; }
}

extern "C" inline uint64_t bit7z_item_ctime(void* ptr) {
    try {
        auto* item = static_cast<bit7z::BitArchiveItem*>(ptr);
        auto tp = item->creationTime();
        return static_cast<uint64_t>(std::chrono::system_clock::to_time_t(tp));
    } catch (...) { return 0; }
}

extern "C" inline uint64_t bit7z_item_atime(void* ptr) {
    try {
        auto* item = static_cast<bit7z::BitArchiveItem*>(ptr);
        auto tp = item->lastAccessTime();
        return static_cast<uint64_t>(std::chrono::system_clock::to_time_t(tp));
    } catch (...) { return 0; }
}

extern "C" inline uint32_t bit7z_item_attributes(void* ptr) {
    try {
        auto* item = static_cast<bit7z::BitArchiveItem*>(ptr);
        return item->attributes();
    } catch (...) { return 0; }
}

extern "C" inline uint8_t bit7z_item_host_os(void* ptr) {
    try {
        auto* item = static_cast<bit7z::BitArchiveItem*>(ptr);
        return item->itemProperty(bit7z::BitProperty::HostOS).getUInt8();
    } catch (...) { return 0; }
}

extern "C" inline int32_t bit7z_item_compression_method(void* ptr, char* out_buf, uint32_t buf_size) {
    try {
        auto* item = static_cast<bit7z::BitArchiveItem*>(ptr);
        auto prop = item->itemProperty(bit7z::BitProperty::Method).getString();
        return tstring_to_utf8(prop, out_buf, buf_size);
    } catch (...) { if (out_buf && buf_size > 0) out_buf[0] = '\0'; return -1; }
}

extern "C" inline int32_t bit7z_item_comment(void* ptr, char* out_buf, uint32_t buf_size) {
    try {
        auto* item = static_cast<bit7z::BitArchiveItem*>(ptr);
        auto prop = item->itemProperty(bit7z::BitProperty::Comment).getString();
        return tstring_to_utf8(prop, out_buf, buf_size);
    } catch (...) { if (out_buf && buf_size > 0) out_buf[0] = '\0'; return -1; }
}

extern "C" inline int32_t bit7z_item_user(void* ptr, char* out_buf, uint32_t buf_size) {
    try {
        auto* item = static_cast<bit7z::BitArchiveItem*>(ptr);
        auto prop = item->itemProperty(bit7z::BitProperty::User).getString();
        return tstring_to_utf8(prop, out_buf, buf_size);
    } catch (...) { if (out_buf && buf_size > 0) out_buf[0] = '\0'; return -1; }
}

extern "C" inline int32_t bit7z_item_group(void* ptr, char* out_buf, uint32_t buf_size) {
    try {
        auto* item = static_cast<bit7z::BitArchiveItem*>(ptr);
        auto prop = item->itemProperty(bit7z::BitProperty::Group).getString();
        return tstring_to_utf8(prop, out_buf, buf_size);
    } catch (...) { if (out_buf && buf_size > 0) out_buf[0] = '\0'; return -1; }
}

extern "C" inline int32_t bit7z_item_is_symlink(void* ptr) {
    try {
        auto* item = static_cast<bit7z::BitArchiveItem*>(ptr);
        return item->isSymLink() ? 1 : 0;
    } catch (...) { return 0; }
}

extern "C" inline uint32_t bit7z_item_posix_attrib(void* ptr) {
    try {
        auto* item = static_cast<bit7z::BitArchiveItem*>(ptr);
        return item->itemProperty(bit7z::BitProperty::PosixAttrib).getUInt32();
    } catch (...) { return 0; }
}

extern "C" inline int32_t bit7z_item_extension(void* ptr, char* out_buf, uint32_t buf_size) {
    try {
        auto* item = static_cast<bit7z::BitArchiveItem*>(ptr);
        auto ext = item->extension();
        return tstring_to_utf8(ext, out_buf, buf_size);
    } catch (...) { if (out_buf && buf_size > 0) out_buf[0] = '\0'; return -1; }
}


extern "C" inline int32_t bit7z_item_hardlink(void* ptr, char* out_buf, uint32_t buf_size) {
    try {
        auto* item = static_cast<bit7z::BitArchiveItem*>(ptr);
        auto prop = item->itemProperty(ArchiveProperties::HardLink).getString();
        return tstring_to_utf8(prop, out_buf, buf_size);
    } catch (...) { if (out_buf && buf_size > 0) out_buf[0] = '\0'; return -1; }
}

// Retrieve raw BitArchiveItem* from reader + index (for property wrappers above)
extern "C" inline void* bit7z_item_from_reader(void* reader_ptr, uint32_t index) {
    try {
        auto& reader = *static_cast<bit7z::BitArchiveReader*>(reader_ptr);
        return (void*)&reader.items()[index];
    } catch (...) { return nullptr; }
}

// ===== Extract wrappers =====
extern "C" inline int32_t bit7z_reader_extract_to(void* reader_ptr, const uint32_t* indices, uint32_t count, const char* dest_path) {
    try {
        auto& reader = *static_cast<bit7z::BitArchiveReader*>(reader_ptr);
        std::vector<uint32_t> idxs(indices, indices + count);
        reader.extractTo(bit7z::tstring(dest_path ? dest_path : ""), idxs);
        return 0;
    } catch (...) { return -1; }
}

extern "C" inline int32_t bit7z_reader_extract_item_to_buffer(void* reader_ptr, uint32_t index, void** out_data, int64_t* out_size) {
    try {
        auto& reader = *static_cast<bit7z::BitArchiveReader*>(reader_ptr);
        bit7z::buffer_t buf;
        reader.extractTo(buf, index);
        *out_size = static_cast<int64_t>(buf.size());
        if (buf.empty()) { *out_data = nullptr; return -1; }
        auto* data = new unsigned char[buf.size()];
        std::copy(buf.begin(), buf.end(), data);
        *out_data = data;
        return 0;
    } catch (...) { *out_data = nullptr; *out_size = 0; return -1; }
}
extern "C" inline void bit7z_reader_free_buffer(void* data) {
    delete[] static_cast<unsigned char*>(data);
}

// C-linkage wrapper that extracts item to buffer in a single call
// (avoids double-extract from calling size + data separately).
extern "C" int32_t bit7z_reader_extract_to_buffer_c(
    void* reader_ptr, uint32_t index, void** out_data, int64_t* out_size
) {
    return bit7z_reader_extract_item_to_buffer(reader_ptr, index, out_data, out_size);
}

inline int64_t bit7z_reader_extract_item_size(void* reader_ptr, uint32_t index) {
    try {
        auto& reader = *static_cast<bit7z::BitArchiveReader*>(reader_ptr);
        bit7z::buffer_t buf;
        reader.extractTo(buf, index);
        return static_cast<int64_t>(buf.size());
    } catch (...) { return -1; }
}
extern "C" inline void* bit7z_reader_extract_item_data(void* reader_ptr, uint32_t index) {
    try {
        auto& reader = *static_cast<bit7z::BitArchiveReader*>(reader_ptr);
        bit7z::buffer_t buf;
        reader.extractTo(buf, index);
        if (buf.empty()) return nullptr;
        auto* data = new unsigned char[buf.size()];
        std::copy(buf.begin(), buf.end(), data);
        return data;
    } catch (...) { return nullptr; }
}

// ===== Test archive integrity =====

struct TestResult {
    bool all_ok;
    uint32_t total;
    uint32_t failed_count;
    std::vector<std::string> failed_paths;
    std::vector<std::string> failed_errors;
};

extern "C" inline void* bit7z_reader_test(void* reader_ptr) {
    try {
        auto& reader = *static_cast<bit7z::BitArchiveReader*>(reader_ptr);
        reader.test();  // throws BitException if any item fails
        // If no exception, all items passed
        auto* result = new TestResult();
        result->all_ok = true;
        result->total = reader.itemsCount();
        result->failed_count = 0;
        return static_cast<void*>(result);
    } catch (const bit7z::BitException& e) {
        auto* result = new TestResult();
        result->all_ok = false;
        result->total = 0;
        result->failed_count = 1;
        result->failed_paths.push_back("");
        result->failed_errors.push_back(e.what());
        return static_cast<void*>(result);
    } catch (...) {
        return nullptr;
    }
}

extern "C" inline uint32_t bit7z_test_result_total(void* result_ptr) {
    return static_cast<TestResult*>(result_ptr)->total;
}

extern "C" inline uint32_t bit7z_test_result_failed_count(void* result_ptr) {
    return static_cast<TestResult*>(result_ptr)->failed_count;
}

extern "C" inline int32_t bit7z_test_result_all_ok(void* result_ptr) {
    return static_cast<TestResult*>(result_ptr)->all_ok ? 1 : 0;
}

extern "C" inline const char* bit7z_test_result_error(void* result_ptr) {
    auto* r = static_cast<TestResult*>(result_ptr);
    if (r->failed_errors.empty()) return "";
    return r->failed_errors[0].c_str();
}

extern "C" inline void bit7z_test_result_free(void* result_ptr) {
    delete static_cast<TestResult*>(result_ptr);
}

// ===== Encryption detection =====

static inline const bit7z::BitInFormat& detect_format_from_path(const char*) {
    return bit7z::BitFormat::Auto;
}

extern "C" inline int32_t bit7z_is_header_encrypted(void* lib_ptr, const char* path) {
    try {
        auto& lib = *static_cast<bit7z::Bit7zLibrary*>(lib_ptr);
        const auto& fmt = detect_format_from_path(path);
        return bit7z::BitArchiveReader::isHeaderEncrypted(lib, path ? path : "", fmt) ? 1 : 0;
    } catch (...) { return 0; }
}

extern "C" inline int32_t bit7z_is_encrypted(void* lib_ptr, const char* path) {
    try {
        auto& lib = *static_cast<bit7z::Bit7zLibrary*>(lib_ptr);
        const auto& fmt = detect_format_from_path(path);
        return bit7z::BitArchiveReader::isEncrypted(lib, path ? path : "", fmt) ? 1 : 0;
    } catch (...) { return 0; }
}

extern "C" inline int32_t bit7z_reader_has_encrypted_items(void* reader_ptr) {
    try {
        auto& reader = *static_cast<bit7z::BitArchiveReader*>(reader_ptr);
        return reader.hasEncryptedItems() ? 1 : 0;
    } catch (...) { return 0; }
}

extern "C" inline int32_t bit7z_reader_is_solid(void* reader_ptr) {
    try {
        auto& reader = *static_cast<bit7z::BitArchiveReader*>(reader_ptr);
        auto prop = reader.archiveProperty(ArchiveProperties::Solid);
        return (!prop.isEmpty() && prop.getBool()) ? 1 : 0;
    } catch (...) { return 0; }
}

extern "C" inline int32_t bit7z_reader_is_multi_volume(void* reader_ptr) {
    try {
        auto& reader = *static_cast<bit7z::BitArchiveReader*>(reader_ptr);
        auto prop = reader.archiveProperty(ArchiveProperties::IsVolume);
        return (!prop.isEmpty() && prop.getBool()) ? 1 : 0;
    } catch (...) { return 0; }
}

extern "C" inline uint32_t bit7z_reader_volumes_count(void* reader_ptr) {
    try {
        auto& reader = *static_cast<bit7z::BitArchiveReader*>(reader_ptr);
        auto prop = reader.archiveProperty(ArchiveProperties::NumVolumes);
        if (prop.isEmpty()) return 0;
        if (prop.isUInt32()) return prop.getUInt32();
        if (prop.isUInt64()) return static_cast<uint32_t>(prop.getUInt64());
        return 0;
    } catch (...) { return 0; }
}

extern "C" inline uint64_t bit7z_reader_headers_size(void* reader_ptr) {
    try {
        auto& reader = *static_cast<bit7z::BitArchiveReader*>(reader_ptr);
        auto prop = reader.archiveProperty(ArchiveProperties::HeadersSize);
        if (prop.isEmpty()) return 0;
        return prop.getUInt64();
    } catch (...) { return 0; }
}

extern "C" inline int32_t bit7z_reader_has_comment(void* reader_ptr) {
    try {
        auto& reader = *static_cast<bit7z::BitArchiveReader*>(reader_ptr);
        auto prop = reader.archiveProperty(ArchiveProperties::Commented);
        return (!prop.isEmpty() && prop.getBool()) ? 1 : 0;
    } catch (...) { return 0; }
}

extern "C" inline uint64_t bit7z_reader_dictionary_size(void* reader_ptr) {
    try {
        auto& reader = *static_cast<bit7z::BitArchiveReader*>(reader_ptr);
        auto prop = reader.archiveProperty(ArchiveProperties::DictionarySize);
        if (prop.isEmpty()) return 0;
        return prop.getUInt64();
    } catch (...) { return 0; }
}

// ===== Directory listing (opaque handle + accessors) =====

struct ItemList {
    std::vector<bit7z::BitArchiveItemInfo> items;
    std::vector<std::string> paths;
    std::vector<std::string> dirs;
    std::vector<uint32_t> dir_indices;
    std::string prefix;
};

extern "C" inline void* bit7z_reader_list_directory(void* reader_ptr, const char* path) {
    try {
        auto& reader = *static_cast<bit7z::BitArchiveReader*>(reader_ptr);
        std::string prefix = path ? path : "";
        size_t plen = prefix.size();

        auto* list = new ItemList();
        list->prefix = prefix;

        std::unordered_set<std::string> seen_dirs;
        uint32_t count = reader.itemsCount();

        for (uint32_t i = 0; i < count; i++) {
            auto itemOffset = reader.itemAt(i);
            std::string itemPath = itemOffset.path();
            for (auto& c : itemPath) if (c == '\\') c = '/';

            if (itemPath.substr(0, plen) != prefix) continue;

            auto tail = itemPath.substr(plen);
            if (tail.empty()) continue;

            auto slashPos = tail.find('/');
            if (slashPos == std::string::npos) {
                list->paths.push_back(itemPath);
                list->items.push_back(bit7z::BitArchiveItemInfo(itemOffset));
            } else {
                std::string subdir = tail.substr(0, slashPos + 1);
                std::string dirPath = prefix + subdir;
                if (seen_dirs.find(dirPath) == seen_dirs.end()) {
                    seen_dirs.insert(dirPath);
                    list->dirs.push_back(dirPath);
                }
            }
        }

        for (size_t i = 0; i < list->dirs.size(); i++) {
            list->dir_indices.push_back(count + static_cast<uint32_t>(i));
        }

        return static_cast<void*>(list);
    } catch (...) { return nullptr; }
}

extern "C" inline uint32_t bit7z_item_list_count(void* list_ptr) {
    auto* list = static_cast<ItemList*>(list_ptr);
    return static_cast<uint32_t>(list->items.size() + list->dirs.size());
}

extern "C" inline uint32_t bit7z_item_list_index(void* list_ptr, uint32_t index) {
    auto* list = static_cast<ItemList*>(list_ptr);
    if (index < list->items.size())
        return list->items[index].index();
    return list->dir_indices[index - list->items.size()];
}

extern "C" inline const char* bit7z_item_list_path(void* list_ptr, uint32_t index) {
    auto* list = static_cast<ItemList*>(list_ptr);
    if (index < list->items.size())
        return list->paths[index].c_str();
    return list->dirs[index - list->items.size()].c_str();
}

extern "C" inline uint64_t bit7z_item_list_size(void* list_ptr, uint32_t index) {
    auto* list = static_cast<ItemList*>(list_ptr);
    if (index < list->items.size())
        return list->items[index].size();
    return 0;
}

extern "C" inline uint64_t bit7z_item_list_packed_size(void* list_ptr, uint32_t index) {
    auto* list = static_cast<ItemList*>(list_ptr);
    if (index < list->items.size())
        return list->items[index].packSize();
    return 0;
}

extern "C" inline int32_t bit7z_item_list_is_dir(void* list_ptr, uint32_t index) {
    auto* list = static_cast<ItemList*>(list_ptr);
    if (index < list->items.size())
        return list->items[index].isDir() ? 1 : 0;
    return 1;
}

extern "C" inline int32_t bit7z_item_list_is_encrypted(void* list_ptr, uint32_t index) {
    auto* list = static_cast<ItemList*>(list_ptr);
    if (index < list->items.size())
        return list->items[index].isEncrypted() ? 1 : 0;
    return 0;
}

extern "C" inline uint32_t bit7z_item_list_crc(void* list_ptr, uint32_t index) {
    auto* list = static_cast<ItemList*>(list_ptr);
    if (index < list->items.size())
        return list->items[index].crc();
    return 0;
}

extern "C" inline void* bit7z_item_list_item(void* list_ptr, uint32_t index) {
    auto* list = static_cast<ItemList*>(list_ptr);
    if (index < list->items.size())
        return static_cast<void*>(&list->items[index]);
    return nullptr;
}

extern "C" inline void bit7z_item_list_free(void* list_ptr) {
    delete static_cast<ItemList*>(list_ptr);
}

// ===== Batch items (all archive items returned as a single ItemList) =====

extern "C" inline void* bit7z_reader_items(void* reader_ptr) {
    try {
        auto& reader = *static_cast<bit7z::BitArchiveReader*>(reader_ptr);
        auto all_items = reader.items();

        auto* list = new ItemList();
        list->items = std::move(all_items);
        list->prefix.clear();

        for (auto& item : list->items) {
            std::string itemPath = item.path();
            for (auto& c : itemPath) if (c == '\\') c = '/';
            list->paths.push_back(std::move(itemPath));
        }
        return static_cast<void*>(list);
    } catch (...) { return nullptr; }
}

// ===== Writer wrappers =====

enum WriterFormat : int {
    BIT7Z_FORMAT_7Z = 0,
    BIT7Z_FORMAT_ZIP = 1,
    BIT7Z_FORMAT_TAR = 2,
    BIT7Z_FORMAT_GZIP = 3,
    BIT7Z_FORMAT_BZIP2 = 4,
    BIT7Z_FORMAT_XZ = 5,
    BIT7Z_FORMAT_WIM = 6,
};

enum WriterCompressionLevel : int {
    BIT7Z_COMPRESS_NONE = 0,
    BIT7Z_COMPRESS_FASTEST = 1,
    BIT7Z_COMPRESS_FAST = 2,
    BIT7Z_COMPRESS_NORMAL = 3,
    BIT7Z_COMPRESS_MAX = 4,
    BIT7Z_COMPRESS_ULTRA = 5,
};

extern "C" void* bit7z_writer_create(void* lib_ptr, int format) {
    try {
        auto& lib = *static_cast<bit7z::Bit7zLibrary*>(lib_ptr);
        const auto& fmt = [format]() -> const bit7z::BitInOutFormat& {
            switch (format) {
                case 1: return bit7z::BitFormat::Zip;
                case 2: return bit7z::BitFormat::Tar;
                case 3: return bit7z::BitFormat::GZip;
                case 4: return bit7z::BitFormat::BZip2;
                case 5: return bit7z::BitFormat::Xz;
                case 6: return bit7z::BitFormat::Wim;
                default: return bit7z::BitFormat::SevenZip;
            }
        }();
        auto* writer = new bit7z::BitArchiveWriter(lib, fmt);
        return static_cast<void*>(writer);
    } catch (...) { return nullptr; }
}

extern "C" void* bit7z_writer_open(void* lib_ptr, const char* path, int format, const char* password) {
    try {
        auto& lib = *static_cast<bit7z::Bit7zLibrary*>(lib_ptr);
        const auto& fmt = [format]() -> const bit7z::BitInOutFormat& {
            switch (format) {
                case 1: return bit7z::BitFormat::Zip;
                case 2: return bit7z::BitFormat::Tar;
                case 3: return bit7z::BitFormat::GZip;
                case 4: return bit7z::BitFormat::BZip2;
                case 5: return bit7z::BitFormat::Xz;
                case 6: return bit7z::BitFormat::Wim;
                default: return bit7z::BitFormat::SevenZip;
            }
        }();
        auto* writer = new bit7z::BitArchiveWriter(
            lib, bit7z::tstring(path ? path : ""), fmt,
            bit7z::tstring(password ? password : ""));
        return static_cast<void*>(writer);
    } catch (...) { return nullptr; }
}

extern "C" void bit7z_writer_close(void* writer_ptr) {
    delete static_cast<bit7z::BitArchiveWriter*>(writer_ptr);
}

extern "C" void bit7z_writer_set_threads(void* writer_ptr, uint32_t n) {
    static_cast<bit7z::BitArchiveWriter*>(writer_ptr)->setThreadsCount(n);
}

extern "C" void bit7z_writer_set_compression_level(void* writer_ptr, int level) {
    auto lvl = bit7z::BitCompressionLevel::Normal;
    switch (level) {
        case 0: lvl = bit7z::BitCompressionLevel::None; break;
        case 1: lvl = bit7z::BitCompressionLevel::Fastest; break;
        case 2: lvl = bit7z::BitCompressionLevel::Fast; break;
        case 3: lvl = bit7z::BitCompressionLevel::Normal; break;
        case 4: lvl = bit7z::BitCompressionLevel::Max; break;
        case 5: lvl = bit7z::BitCompressionLevel::Ultra; break;
    }
    static_cast<bit7z::BitArchiveWriter*>(writer_ptr)->setCompressionLevel(lvl);
}

extern "C" void bit7z_writer_set_password(void* writer_ptr, const char* password) {
    static_cast<bit7z::BitArchiveWriter*>(writer_ptr)->setPassword(
        bit7z::tstring(password ? password : ""));
}

extern "C" void bit7z_writer_set_update_mode(void* writer_ptr, int mode) {
    auto modeEnum = bit7z::UpdateMode::None;
    if (mode == 1) modeEnum = bit7z::UpdateMode::Append;
    else if (mode == 2) modeEnum = bit7z::UpdateMode::Update;
    static_cast<bit7z::BitArchiveWriter*>(writer_ptr)->setUpdateMode(modeEnum);
}

extern "C" void bit7z_writer_set_compression_method(void* writer_ptr, int method) {
    static_cast<bit7z::BitArchiveWriter*>(writer_ptr)->setCompressionMethod(
        static_cast<bit7z::BitCompressionMethod>(method));
}

extern "C" void bit7z_writer_set_dictionary_size(void* writer_ptr, uint32_t bytes) {
    static_cast<bit7z::BitArchiveWriter*>(writer_ptr)->setDictionarySize(bytes);
}

extern "C" void bit7z_writer_set_word_size(void* writer_ptr, uint32_t bytes) {
    static_cast<bit7z::BitArchiveWriter*>(writer_ptr)->setWordSize(bytes);
}

extern "C" void bit7z_writer_set_solid_mode(void* writer_ptr, int solid) {
    static_cast<bit7z::BitArchiveWriter*>(writer_ptr)->setSolidMode(solid != 0);
}

extern "C" void bit7z_writer_set_volume_size(void* writer_ptr, uint64_t bytes) {
    static_cast<bit7z::BitArchiveWriter*>(writer_ptr)->setVolumeSize(bytes);
}

extern "C" void bit7z_writer_set_password_ex(void* writer_ptr, const char* password, int encrypt_header) {
    auto& writer = *static_cast<bit7z::BitArchiveWriter*>(writer_ptr);
    if (encrypt_header) {
        writer.setPassword(
            bit7z::tstring(password ? password : ""),
            bit7z::EncryptionScope::DataAndHeaders);
    } else {
        writer.setPassword(
            bit7z::tstring(password ? password : ""));
    }
}

extern "C" void bit7z_writer_set_store_timestamps(void* writer_ptr, int modified, int created, int accessed) {
    auto& writer = *static_cast<bit7z::BitArchiveWriter*>(writer_ptr);
    writer.setStoreLastWriteTime(modified != 0);
    writer.setStoreCreationTime(created != 0);
    writer.setStoreLastAccessTime(accessed != 0);
}

extern "C" int32_t bit7z_writer_add_dir_filtered(void* writer_ptr, const char* dir, const char* filter, int policy, int recursive) {
    try {
        auto& writer = *static_cast<bit7z::BitArchiveWriter*>(writer_ptr);
        writer.addFiles(
            bit7z::tstring(dir ? dir : ""),
            bit7z::tstring(filter ? filter : ""),
            policy == 0 ? bit7z::FilterPolicy::Include : bit7z::FilterPolicy::Exclude,
            recursive != 0);
        return 0;
    } catch (...) { return -1; }
}

extern "C" int32_t bit7z_writer_add_items(void* writer_ptr, const char** paths, const char** archive_paths, uint32_t count) {
    try {
        auto& writer = *static_cast<bit7z::BitArchiveWriter*>(writer_ptr);
        std::vector<std::pair<bit7z::tstring, bit7z::tstring>> pairs;
        pairs.reserve(count);
        for (uint32_t i = 0; i < count; ++i) {
            pairs.emplace_back(
                bit7z::tstring(paths[i] ? paths[i] : ""),
                bit7z::tstring(archive_paths[i] ? archive_paths[i] : ""));
        }
        writer.addItems(pairs);
        return 0;
    } catch (...) { return -1; }
}

extern "C" int32_t bit7z_writer_add_file(void* writer_ptr, const char* path) {
    try {
        static_cast<bit7z::BitArchiveWriter*>(writer_ptr)->addFile(
            bit7z::tstring(path ? path : ""));
        return 0;
    } catch (...) { return -1; }
}

extern "C" int32_t bit7z_writer_add_files(void* writer_ptr, const char* const* paths, uint32_t count) {
    try {
        std::vector<bit7z::tstring> vec;
        vec.reserve(count);
        for (uint32_t i = 0; i < count; ++i) {
            if (paths[i]) vec.emplace_back(paths[i]);
        }
        static_cast<bit7z::BitArchiveWriter*>(writer_ptr)->addItems(vec);
        return 0;
    } catch (...) { return -1; }
}

extern "C" int32_t bit7z_writer_add_dir(void* writer_ptr, const char* dir) {
    try {
        static_cast<bit7z::BitArchiveWriter*>(writer_ptr)->addDirectory(
            bit7z::tstring(dir ? dir : ""));
        return 0;
    } catch (...) { return -1; }
}

extern "C" int32_t bit7z_writer_compress_to(void* writer_ptr, const char* out_path) {
    try {
        static_cast<bit7z::BitArchiveWriter*>(writer_ptr)->compressTo(
            bit7z::tstring(out_path ? out_path : ""));
        return 0;
    } catch (...) { return -1; }
}

extern "C" int32_t bit7z_writer_compress_to_cb(
    void* writer_ptr,
    const char* out_path,
    void* ctx,
    int32_t (*on_progress)(uint64_t processed, uint64_t total, void* ctx),
    void   (*on_file)(const char* path, void* ctx)
) {
    try {
        auto& writer = *static_cast<bit7z::BitArchiveWriter*>(writer_ptr);
        auto sharedTotal = std::make_shared<uint64_t>(0);

        if (on_progress) {
            writer.setTotalCallback([sharedTotal](uint64_t total) {
                *sharedTotal = total;
            });
            writer.setProgressCallback([ctx, on_progress, sharedTotal](uint64_t processed) -> bool {
                return on_progress(processed, *sharedTotal, ctx) != 0;
            });
        }

        if (on_file) {
            writer.setFileCallback([ctx, on_file](const bit7z::tstring& path) {
                on_file(path.c_str(), ctx);
            });
        }

        writer.compressTo(bit7z::tstring(out_path ? out_path : ""));

        writer.setFileCallback(nullptr);
        writer.setProgressCallback(nullptr);
        writer.setTotalCallback(nullptr);

        return 0;
    } catch (...) { return -1; }
}

// ===== Editor wrappers =====

extern "C" void* bit7z_editor_open(void* lib_ptr, const char* path, int format, const char* password) {
    try {
        auto& lib = *static_cast<bit7z::Bit7zLibrary*>(lib_ptr);
        const auto& fmt = [format]() -> const bit7z::BitInOutFormat& {
            switch (format) {
                case 1: return bit7z::BitFormat::Zip;
                case 2: return bit7z::BitFormat::Tar;
                case 3: return bit7z::BitFormat::GZip;
                case 4: return bit7z::BitFormat::BZip2;
                case 5: return bit7z::BitFormat::Xz;
                case 6: return bit7z::BitFormat::Wim;
                default: return bit7z::BitFormat::SevenZip;
            }
        }();
        auto* editor = new bit7z::BitArchiveEditor(
            lib, bit7z::tstring(path ? path : ""), fmt,
            bit7z::tstring(password ? password : ""));
        return static_cast<void*>(editor);
    } catch (...) { return nullptr; }
}

extern "C" void bit7z_editor_close(void* editor_ptr) {
    delete static_cast<bit7z::BitArchiveEditor*>(editor_ptr);
}

extern "C" int32_t bit7z_editor_rename(void* editor_ptr, uint32_t index, const char* new_path) {
    try {
        static_cast<bit7z::BitArchiveEditor*>(editor_ptr)->renameItem(
            index, bit7z::tstring(new_path ? new_path : ""));
        return 0;
    } catch (...) { return -1; }
}

extern "C" int32_t bit7z_editor_delete(void* editor_ptr, uint32_t index) {
    try {
        static_cast<bit7z::BitArchiveEditor*>(editor_ptr)->deleteItem(index);
        return 0;
    } catch (...) { return -1; }
}

extern "C" int32_t bit7z_editor_apply(void* editor_ptr) {
    try {
        static_cast<bit7z::BitArchiveEditor*>(editor_ptr)->applyChanges();
        return 0;
    } catch (...) { return -1; }
}

// ===== Callback-based extract (supports per-file overwrite, progress, cancel) =====

// C callback types — passed from Rust via function pointers.
// on_overwrite: return 0=Overwrite, 1=Skip
// on_progress:  return 0=cancel, non-zero=continue

inline int32_t bit7z_reader_extract_to_cb(
    void* reader_ptr,
    const uint32_t* indices,
    uint32_t count,
    const char* dest_path,
    void* ctx,
    int32_t (*on_overwrite)(const char* src, const char* dest, uint64_t existing_size, uint64_t src_size, int64_t src_mtime, int64_t dest_mtime, void* ctx),
    int32_t (*on_progress)(uint64_t processed, uint64_t total, void* ctx),
    void      (*on_file)(const char* path, uint64_t file_size, void* ctx)
) {
    try {
        auto& reader = *static_cast<bit7z::BitArchiveReader*>(reader_ptr);
        std::string destDir(dest_path ? dest_path : "");
        if (!destDir.empty() && destDir.back() != '/' && destDir.back() != '\\') {
            destDir += '/';
        }

        // Shared state for total size (set by TotalCallback, read by ProgressCallback)
        auto sharedTotal = std::make_shared<uint64_t>(0);

        if (on_progress) {
            reader.setTotalCallback([sharedTotal](uint64_t total) {
                *sharedTotal = total;
            });
            reader.setProgressCallback([ctx, on_progress, sharedTotal](uint64_t processed) -> bool {
                return on_progress(processed, *sharedTotal, ctx) != 0;
            });
        }

        if (on_overwrite || on_file) {
            auto fileIndex = std::make_shared<uint32_t>(0);
            reader.setFileCallback([&reader, &destDir, ctx, on_overwrite, on_file, fileIndex, indices, count](const bit7z::tstring& path) {
                uint32_t idx = (*fileIndex)++;

                // Look up source item info
                uint64_t fileSize = 0;
                int64_t srcMtime = 0;
                if (idx < count) {
                    auto itemPtr = bit7z_item_from_reader(&reader, indices[idx]);
                    if (itemPtr) {
                        auto* item = static_cast<bit7z::BitArchiveItem*>(itemPtr);
                        fileSize = item->size();
                        srcMtime = static_cast<int64_t>(std::chrono::system_clock::to_time_t(item->lastWriteTime()));
                    }
                }

                if (on_file) {
                    on_file(path.c_str(), fileSize, ctx);
                }

                if (!on_overwrite) return;

                std::string fullDest = destDir + path;

                struct stat st;
                if (stat(fullDest.c_str(), &st) == 0) {
                    int64_t destMtime = static_cast<int64_t>(st.st_mtime);
                    int32_t action = on_overwrite(path.c_str(), fullDest.c_str(), static_cast<uint64_t>(st.st_size), fileSize, srcMtime, destMtime, ctx);
                    switch (action) {
                        case 0: reader.setOverwriteMode(bit7z::OverwriteMode::Overwrite); break;
                        case 1: reader.setOverwriteMode(bit7z::OverwriteMode::Skip); break;
                        default: reader.setOverwriteMode(bit7z::OverwriteMode::Skip); break;
                    }
                } else {
                    reader.setOverwriteMode(bit7z::OverwriteMode::Overwrite);
                }
            });
        }

        std::vector<uint32_t> idxs(indices, indices + count);
        reader.extractTo(bit7z::tstring(dest_path ? dest_path : ""), idxs);

        // Reset callbacks to avoid accidental reuse
        reader.setFileCallback(nullptr);
        reader.setProgressCallback(nullptr);
        reader.setTotalCallback(nullptr);

        return 0;
    } catch (...) { return -1; }
}

// ===== RenameCallback-based extract (per-file overwrite/skip/rename + progress) =====

// on_rename: fill out_buf with desired path (empty = skip).
// Return 0 = continue, -1 = abort.

inline int32_t bit7z_reader_extract_with_rename(
    void* reader_ptr,
    const char* dest_path,
    void* ctx,
    int32_t (*on_rename)(const char* src, uint64_t size, int32_t is_dir, char* out, uint32_t out_size, void* ctx),
    int32_t (*on_progress)(uint64_t processed, uint64_t total, void* ctx),
    void   (*on_file)(const char* path, void* ctx)
) {
    try {
        auto& reader = *static_cast<bit7z::BitArchiveReader*>(reader_ptr);
        auto sharedTotal = std::make_shared<uint64_t>(0);

        if (on_progress) {
            reader.setTotalCallback([sharedTotal](uint64_t total) { *sharedTotal = total; });
            reader.setProgressCallback([ctx, on_progress, sharedTotal](uint64_t processed) -> bool {
                return on_progress(processed, *sharedTotal, ctx) != 0;
            });
        }
        if (on_file) {
            reader.setFileCallback([ctx, on_file](const bit7z::tstring& path) {
                on_file(path.c_str(), ctx);
            });
        }

        reader.extractTo(
            bit7z::tstring(dest_path ? dest_path : ""),
            [ctx, on_rename](const bit7z::BitArchiveItem& item) -> bit7z::tstring {
                if (!on_rename) return item.path();
                char buf[4096];
                buf[0] = '\0';
                auto p = item.path();
                if (on_rename(p.c_str(), item.size(), item.isDir() ? 1 : 0, buf, (uint32_t)sizeof(buf), ctx) != 0) {
                    return {};   // abort operation
                }
                if (buf[0] == '\0') return {};  // skip this item
                return bit7z::tstring(buf);
            }
        );

        reader.setFileCallback(nullptr);
        reader.setProgressCallback(nullptr);
        reader.setTotalCallback(nullptr);
        return 0;
    } catch (...) { return -1; }
}

// C-linkage wrapper for extract_to_cb
extern "C" int32_t bit7z_reader_extract_to_cb_c(
    void* reader_ptr,
    const uint32_t* indices,
    uint32_t count,
    const char* dest_path,
    void* ctx,
    int32_t (*on_overwrite)(const char* src, const char* dest, uint64_t existing_size, uint64_t src_size, int64_t src_mtime, int64_t dest_mtime, void* ctx),
    int32_t (*on_progress)(uint64_t processed, uint64_t total, void* ctx),
    void   (*on_file)(const char* path, uint64_t file_size, void* ctx)
) {
    return bit7z_reader_extract_to_cb(reader_ptr, indices, count, dest_path, ctx, on_overwrite, on_progress, on_file);
}

// C-linkage wrapper for compress callback.
extern "C" int32_t bit7z_writer_compress_to_cb_c(
    void* writer_ptr,
    const char* out_path,
    void* ctx,
    int32_t (*on_progress)(uint64_t processed, uint64_t total, void* ctx),
    void   (*on_file)(const char* path, void* ctx)
) {
    return bit7z_writer_compress_to_cb(writer_ptr, out_path, ctx, on_progress, on_file);
}

// C-linkage wrapper for extract_with_rename.
extern "C" int32_t bit7z_reader_extract_with_rename_c(
    void* reader_ptr,
    const char* dest_path,
    void* ctx,
    int32_t (*on_rename)(const char* src, uint64_t size, int32_t is_dir, char* out, uint32_t out_size, void* ctx),
    int32_t (*on_progress)(uint64_t processed, uint64_t total, void* ctx),
    void   (*on_file)(const char* path, void* ctx)
) {
    return bit7z_reader_extract_with_rename(reader_ptr, dest_path, ctx, on_rename, on_progress, on_file);
}