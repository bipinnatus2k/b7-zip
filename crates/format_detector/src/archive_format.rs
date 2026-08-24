use serde::{Deserialize, Serialize};

/// Supported archive formats.
///
/// Represents every format the bit7z SDK can handle. Physical formats (with
/// corresponding file_format magic) map directly to a single container type;
/// compound aliases such as `TarGz` logically wrap two layers and delegate
/// `physical()` to the outer compression layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArchiveFormat {
    #[serde(rename = "7z")]
    SevenZip,
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "tar")]
    Tar,
    #[serde(rename = "tar.gz")]
    TarGz,
    #[serde(rename = "tar.xz")]
    TarXz,
    #[serde(rename = "tar.bz2")]
    TarBz2,
    #[serde(rename = "rar")]
    Rar,
    #[serde(rename = "rar5")]
    Rar5,
    #[serde(rename = "gz")]
    GZip,
    #[serde(rename = "bz2")]
    BZip2,
    #[serde(rename = "xz")]
    Xz,
    #[serde(rename = "wim")]
    Wim,
    #[serde(rename = "arj")]
    Arj,
    #[serde(rename = "cab")]
    Cab,
    #[serde(rename = "lzh")]
    Lzh,
    #[serde(rename = "lzma")]
    Lzma,
    #[serde(rename = "lzma86")]
    Lzma86,
    #[serde(rename = "zstd")]
    Zstd,
    #[serde(rename = "ppmd")]
    Ppmd,
    #[serde(rename = "nsis")]
    Nsis,
    #[serde(rename = "iso")]
    Iso,
    #[serde(rename = "chm")]
    Chm,
    #[serde(rename = "cpio")]
    Cpio,
    #[serde(rename = "deb")]
    Deb,
    #[serde(rename = "rpm")]
    Rpm,
    #[serde(rename = "dmg")]
    Dmg,
    #[serde(rename = "vhd")]
    Vhd,
    #[serde(rename = "vmdk")]
    Vmdk,
    #[serde(rename = "vdi")]
    Vdi,
    #[serde(rename = "squashfs")]
    SquashFS,
    #[serde(rename = "compound")]
    Compound,
    #[serde(rename = "other")]
    Other(u32),
}

/// All non-`Other` archive formats for iteration.
pub const ALL_FORMATS: &[ArchiveFormat] = &[
    ArchiveFormat::SevenZip,
    ArchiveFormat::Zip,
    ArchiveFormat::Tar,
    ArchiveFormat::TarGz,
    ArchiveFormat::TarBz2,
    ArchiveFormat::TarXz,
    ArchiveFormat::GZip,
    ArchiveFormat::BZip2,
    ArchiveFormat::Xz,
    ArchiveFormat::Wim,
    ArchiveFormat::Rar,
    ArchiveFormat::Rar5,
    ArchiveFormat::Arj,
    ArchiveFormat::Cab,
    ArchiveFormat::Lzh,
    ArchiveFormat::Lzma,
    ArchiveFormat::Lzma86,
    ArchiveFormat::Zstd,
    ArchiveFormat::Ppmd,
    ArchiveFormat::Nsis,
    ArchiveFormat::Iso,
    ArchiveFormat::Chm,
    ArchiveFormat::Cpio,
    ArchiveFormat::Deb,
    ArchiveFormat::Rpm,
    ArchiveFormat::Dmg,
    ArchiveFormat::Vhd,
    ArchiveFormat::Vmdk,
    ArchiveFormat::Vdi,
    ArchiveFormat::SquashFS,
];

impl ArchiveFormat {
    pub fn extension(&self) -> &str {
        match self {
            ArchiveFormat::SevenZip => "7z",
            ArchiveFormat::Zip => "zip",
            ArchiveFormat::Tar => "tar",
            ArchiveFormat::TarGz => "tar.gz",
            ArchiveFormat::TarXz => "tar.xz",
            ArchiveFormat::TarBz2 => "tar.bz2",
            ArchiveFormat::Rar | ArchiveFormat::Rar5 => "rar",
            ArchiveFormat::GZip => "gz",
            ArchiveFormat::BZip2 => "bz2",
            ArchiveFormat::Xz => "xz",
            ArchiveFormat::Wim => "wim",
            ArchiveFormat::Arj => "arj",
            ArchiveFormat::Cab => "cab",
            ArchiveFormat::Lzh => "lzh",
            ArchiveFormat::Lzma => "lzma",
            ArchiveFormat::Lzma86 => "lzma86",
            ArchiveFormat::Zstd => "zst",
            ArchiveFormat::Ppmd => "ppmd",
            ArchiveFormat::Nsis => "exe",
            ArchiveFormat::Iso => "iso",
            ArchiveFormat::Chm => "chm",
            ArchiveFormat::Cpio => "cpio",
            ArchiveFormat::Deb => "deb",
            ArchiveFormat::Rpm => "rpm",
            ArchiveFormat::Dmg => "dmg",
            ArchiveFormat::Vhd => "vhd",
            ArchiveFormat::Vmdk => "vmdk",
            ArchiveFormat::Vdi => "vdi",
            ArchiveFormat::SquashFS => "squashfs",
            ArchiveFormat::Compound => "compound",
            ArchiveFormat::Other(_) => "other",
        }
    }

    pub fn physical(&self) -> ArchiveFormat {
        match self {
            ArchiveFormat::TarGz => ArchiveFormat::GZip,
            ArchiveFormat::TarBz2 => ArchiveFormat::BZip2,
            ArchiveFormat::TarXz => ArchiveFormat::Xz,
            other => *other,
        }
    }

    pub fn inner_format(&self) -> Option<ArchiveFormat> {
        match self {
            ArchiveFormat::TarGz | ArchiveFormat::GZip => Some(ArchiveFormat::Tar),
            ArchiveFormat::TarBz2 | ArchiveFormat::BZip2 => Some(ArchiveFormat::Tar),
            ArchiveFormat::TarXz | ArchiveFormat::Xz => Some(ArchiveFormat::Tar),
            _ => None,
        }
    }

    pub fn extensions(&self) -> &[&str] {
        match self {
            ArchiveFormat::SevenZip => &["7z"],
            ArchiveFormat::Zip => &["zip"],
            ArchiveFormat::Tar => &["tar"],
            ArchiveFormat::TarGz => &["tar.gz", "tgz"],
            ArchiveFormat::TarXz => &["tar.xz", "txz"],
            ArchiveFormat::TarBz2 => &["tar.bz2", "tbz2", "tbz"],
            ArchiveFormat::Rar | ArchiveFormat::Rar5 => &["rar"],
            ArchiveFormat::GZip => &["gz", "tgz", "tar.gz"],
            ArchiveFormat::BZip2 => &["bz2", "tbz2", "tbz", "tar.bz2"],
            ArchiveFormat::Xz => &["xz", "txz", "tar.xz"],
            ArchiveFormat::Wim => &["wim"],
            ArchiveFormat::Arj => &["arj"],
            ArchiveFormat::Cab => &["cab"],
            ArchiveFormat::Lzh => &["lzh", "lha"],
            ArchiveFormat::Lzma => &["lzma"],
            ArchiveFormat::Lzma86 => &["lzma86"],
            ArchiveFormat::Zstd => &["zst", "zstd"],
            ArchiveFormat::Ppmd => &["ppmd"],
            ArchiveFormat::Nsis => &["exe"],
            ArchiveFormat::Iso => &["iso"],
            ArchiveFormat::Chm => &["chm"],
            ArchiveFormat::Cpio => &["cpio"],
            ArchiveFormat::Deb => &["deb"],
            ArchiveFormat::Rpm => &["rpm"],
            ArchiveFormat::Dmg => &["dmg"],
            ArchiveFormat::Vhd => &["vhd", "vhdx"],
            ArchiveFormat::Vmdk => &["vmdk"],
            ArchiveFormat::Vdi => &["vdi"],
            ArchiveFormat::SquashFS => &["squashfs"],
            ArchiveFormat::Compound => &["compound"],
            ArchiveFormat::Other(_) => &[],
        }
    }

    pub fn sdk_value(&self) -> Option<u32> {
        match self {
            ArchiveFormat::Other(v) => Some(*v),
            _ => None,
        }
    }

    pub fn supports_encryption(&self) -> bool {
        matches!(self, ArchiveFormat::SevenZip | ArchiveFormat::Zip)
    }

    pub fn supports_encrypted_filenames(&self) -> bool {
        matches!(self, ArchiveFormat::SevenZip)
    }

    pub fn is_writable(&self) -> bool {
        matches!(
            self,
            ArchiveFormat::SevenZip
                | ArchiveFormat::Zip
                | ArchiveFormat::Tar
                | ArchiveFormat::GZip
                | ArchiveFormat::BZip2
                | ArchiveFormat::Xz
                | ArchiveFormat::Wim
                | ArchiveFormat::TarGz
                | ArchiveFormat::TarBz2
                | ArchiveFormat::TarXz
        )
    }

    pub fn supports_compression_level(&self) -> bool {
        self.is_writable() && !matches!(self, ArchiveFormat::Tar)
    }

    pub fn display_name(&self) -> &str {
        match self {
            ArchiveFormat::SevenZip => "7z",
            ArchiveFormat::Zip => "Zip",
            ArchiveFormat::Tar => "Tar",
            ArchiveFormat::TarGz => "Tar.gz",
            ArchiveFormat::TarXz => "Tar.xz",
            ArchiveFormat::TarBz2 => "Tar.bz2",
            ArchiveFormat::Rar => "Rar",
            ArchiveFormat::Rar5 => "Rar5",
            ArchiveFormat::GZip => "GZip",
            ArchiveFormat::BZip2 => "BZip2",
            ArchiveFormat::Xz => "Xz",
            ArchiveFormat::Wim => "WIM",
            ArchiveFormat::Arj => "ARJ",
            ArchiveFormat::Cab => "CAB",
            ArchiveFormat::Lzh => "LZH",
            ArchiveFormat::Lzma => "LZMA",
            ArchiveFormat::Lzma86 => "LZMA86",
            ArchiveFormat::Zstd => "Zstd",
            ArchiveFormat::Ppmd => "PPMd",
            ArchiveFormat::Nsis => "NSIS",
            ArchiveFormat::Iso => "ISO",
            ArchiveFormat::Chm => "CHM",
            ArchiveFormat::Cpio => "CPIO",
            ArchiveFormat::Deb => "DEB",
            ArchiveFormat::Rpm => "RPM",
            ArchiveFormat::Dmg => "DMG",
            ArchiveFormat::Vhd => "VHD",
            ArchiveFormat::Vmdk => "VMDK",
            ArchiveFormat::Vdi => "VDI",
            ArchiveFormat::SquashFS => "SquashFS",
            ArchiveFormat::Compound => "Compound",
            ArchiveFormat::Other(_) => "Other",
        }
    }

    pub fn all_support_compress() -> Vec<ArchiveFormat> {
        vec![
            ArchiveFormat::SevenZip,
            ArchiveFormat::Zip,
            ArchiveFormat::Tar,
            ArchiveFormat::GZip,
            ArchiveFormat::BZip2,
            ArchiveFormat::Xz,
            ArchiveFormat::Wim,
            ArchiveFormat::TarGz,
            ArchiveFormat::TarBz2,
            ArchiveFormat::TarXz,
        ]
    }
}
