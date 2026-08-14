//! Archive format detection by magic bytes and file extension.
//!
//! A small leaf component with no dependencies. Returns a generic
//! [`ArchiveFormat`] identifier that other components (engine, session,
//! task) map to their own format types.

use std::io::Read;
use std::path::Path;

/// A generic archive format identifier.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ArchiveFormat {
    SevenZip,
    Zip,
    Rar,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    GZip,
    BZip2,
    Xz,
    Wim,
    Unknown,
}

impl ArchiveFormat {
    /// Canonical lowercase name (e.g. `7z`, `tar.gz`).
    pub fn name(self) -> &'static str {
        match self {
            ArchiveFormat::SevenZip => "7z",
            ArchiveFormat::Zip => "zip",
            ArchiveFormat::Rar => "rar",
            ArchiveFormat::Tar => "tar",
            ArchiveFormat::TarGz => "tar.gz",
            ArchiveFormat::TarBz2 => "tar.bz2",
            ArchiveFormat::TarXz => "tar.xz",
            ArchiveFormat::GZip => "gz",
            ArchiveFormat::BZip2 => "bz2",
            ArchiveFormat::Xz => "xz",
            ArchiveFormat::Wim => "wim",
            ArchiveFormat::Unknown => "unknown",
        }
    }

    /// Whether this format is known (not [`ArchiveFormat::Unknown`]).
    pub fn is_known(self) -> bool {
        !matches!(self, ArchiveFormat::Unknown)
    }
}

/// Detect the format from the leading bytes of a file.
pub fn detect_from_bytes(header: &[u8]) -> ArchiveFormat {
    if header.starts_with(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
        return ArchiveFormat::SevenZip;
    }
    if header.starts_with(&[0x50, 0x4B, 0x03, 0x04])
        || header.starts_with(&[0x50, 0x4B, 0x05, 0x06])
        || header.starts_with(&[0x50, 0x4B, 0x07, 0x08])
    {
        return ArchiveFormat::Zip;
    }
    // RAR v4: 52 61 72 21 1A 07 00 ; RAR v5: 52 61 72 21 1A 07 01 00
    if header.starts_with(&[0x52, 0x61, 0x72, 0x21, 0x1A, 0x07]) {
        return ArchiveFormat::Rar;
    }
    if header.starts_with(&[0x1F, 0x8B]) {
        return ArchiveFormat::GZip;
    }
    if header.starts_with(&[0x42, 0x5A, 0x68]) {
        return ArchiveFormat::BZip2;
    }
    if header.starts_with(&[0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00]) {
        return ArchiveFormat::Xz;
    }
    // "MSWIM\0\0\0"
    if header.starts_with(&[0x4D, 0x53, 0x57, 0x49, 0x4D, 0x00, 0x00, 0x00]) {
        return ArchiveFormat::Wim;
    }
    // tar: "ustar" at offset 257 (and a non-zero checksum block)
    if header.len() >= 262 && &header[257..262] == b"ustar" {
        return ArchiveFormat::Tar;
    }
    ArchiveFormat::Unknown
}

/// Detect the format from a file name (extension based).
pub fn detect_from_name(name: &str) -> ArchiveFormat {
    let name = name.to_ascii_lowercase();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        return ArchiveFormat::TarGz;
    }
    if name.ends_with(".tar.bz2") || name.ends_with(".tbz2") || name.ends_with(".tbz") {
        return ArchiveFormat::TarBz2;
    }
    if name.ends_with(".tar.xz") || name.ends_with(".txz") {
        return ArchiveFormat::TarXz;
    }
    let ext = name.rsplit('.').next().unwrap_or("");
    match ext {
        "7z" => ArchiveFormat::SevenZip,
        "zip" => ArchiveFormat::Zip,
        "rar" => ArchiveFormat::Rar,
        "tar" => ArchiveFormat::Tar,
        "gz" => ArchiveFormat::GZip,
        "bz2" => ArchiveFormat::BZip2,
        "xz" => ArchiveFormat::Xz,
        "wim" => ArchiveFormat::Wim,
        _ => ArchiveFormat::Unknown,
    }
}

/// Detect from an extension string (without the leading dot).
pub fn detect_from_extension(extension: &str) -> ArchiveFormat {
    detect_from_name(&format!("file.{extension}"))
}

/// Detect the format of a file on disk: magic bytes first, extension as fallback.
pub fn detect_from_path(path: &Path) -> ArchiveFormat {
    if let Ok(mut file) = std::fs::File::open(path) {
        let mut header = [0u8; 512];
        let n = file.read(&mut header).unwrap_or(0);
        let magic = detect_from_bytes(&header[..n]);
        if magic.is_known() {
            return magic;
        }
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .map(detect_from_name)
        .unwrap_or(ArchiveFormat::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_bytes() {
        assert_eq!(detect_from_bytes(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]), ArchiveFormat::SevenZip);
        assert_eq!(detect_from_bytes(&[0x50, 0x4B, 0x03, 0x04]), ArchiveFormat::Zip);
        assert_eq!(detect_from_bytes(&[0x50, 0x4B, 0x05, 0x06]), ArchiveFormat::Zip);
        assert_eq!(detect_from_bytes(&[0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x00]), ArchiveFormat::Rar);
        assert_eq!(detect_from_bytes(&[0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x01, 0x00]), ArchiveFormat::Rar);
        assert_eq!(detect_from_bytes(&[0x1F, 0x8B]), ArchiveFormat::GZip);
        assert_eq!(detect_from_bytes(&[0x42, 0x5A, 0x68]), ArchiveFormat::BZip2);
        assert_eq!(detect_from_bytes(&[0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00]), ArchiveFormat::Xz);
        assert_eq!(detect_from_bytes(b"MSWIM\0\0\0"), ArchiveFormat::Wim);
        assert_eq!(detect_from_bytes(&[0x01, 0x02, 0x03]), ArchiveFormat::Unknown);
    }

    #[test]
    fn tar_ustar_magic() {
        let mut header = vec![0u8; 512];
        header[257..262].copy_from_slice(b"ustar");
        assert_eq!(detect_from_bytes(&header), ArchiveFormat::Tar);
    }

    #[test]
    fn extensions() {
        assert_eq!(detect_from_name("backup.7z"), ArchiveFormat::SevenZip);
        assert_eq!(detect_from_name("backup.zip"), ArchiveFormat::Zip);
        assert_eq!(detect_from_name("data.tar.gz"), ArchiveFormat::TarGz);
        assert_eq!(detect_from_name("data.tgz"), ArchiveFormat::TarGz);
        assert_eq!(detect_from_name("data.tar.bz2"), ArchiveFormat::TarBz2);
        assert_eq!(detect_from_name("data.tar.xz"), ArchiveFormat::TarXz);
        assert_eq!(detect_from_name("data.bz2"), ArchiveFormat::BZip2);
        assert_eq!(detect_from_name("ARCHIVE.ZIP"), ArchiveFormat::Zip);
        assert_eq!(detect_from_name("notes.txt"), ArchiveFormat::Unknown);
        assert_eq!(detect_from_name("noext"), ArchiveFormat::Unknown);
    }

    #[test]
    fn path_with_magic_wins_over_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fake.zip");
        // 7z magic in a file named .zip: magic detection wins.
        std::fs::write(&path, [0x37u8, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]).unwrap();
        assert_eq!(detect_from_path(&path), ArchiveFormat::SevenZip);
    }

    #[test]
    fn path_extension_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.tar");
        std::fs::write(&path, []).unwrap();
        assert_eq!(detect_from_path(&path), ArchiveFormat::Tar);
    }
}
