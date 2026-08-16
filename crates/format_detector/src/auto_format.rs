
use file_format::FileFormat;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use crate::archive_format::{ArchiveFormat, ALL_FORMATS};
use crate::format_detector::{DetectError, FormatDetector};
use crate::validator::ValidatorRegistry;

/// Result of detecting and validating an archive file.
#[derive(Debug, Clone)]
pub struct Detection {
    pub format: ArchiveFormat,
    pub logical: Option<ArchiveFormat>,
    pub inner: Option<ArchiveFormat>,
    pub extensions: Vec<String>,
    pub is_multi_volume: bool,
    pub total_volumes: Option<u32>,
    pub validated: bool,
}

/// Errors from the detection layer.
#[derive(Debug, thiserror::Error)]
pub enum DetectionError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unknown or unsupported archive format: {path}")]
    UnknownFormat { path: PathBuf },
    #[error(
        "format mismatch: magic bytes indicate {magic:?} \
         but extension suggests {extension:?}"
    )]
    FormatMismatch {
        magic: ArchiveFormat,
        extension: ArchiveFormat,
    },
    #[error("validation failed for {format:?}: {detail}")]
    ValidationFailed {
        format: ArchiveFormat,
        detail: String,
    },
}

/// Detects archive format from magic bytes and filename extension.
///
/// Uses the `file_format` crate (pure Rust) for magic byte matching, then
/// resolves compound formats (e.g. TarGz) from the extension chain.
pub struct AutoFormat {
    validators: Option<Arc<ValidatorRegistry>>,
}

impl AutoFormat {
    pub fn new() -> Self {
        Self { validators: None }
    }

    pub fn with_validators(validators: Arc<ValidatorRegistry>) -> Self {
        Self {
            validators: Some(validators),
        }
    }

    pub fn detect(&self, path: &Path) -> Result<Detection, DetectionError> {
        let mut file = std::fs::File::open(path)?;
        let ff = file_format::FileFormat::from_reader(&mut file)?;
        let physical =
            file_format_to_archive_format(ff).ok_or_else(|| DetectionError::UnknownFormat {
                path: path.to_path_buf(),
            })?;

        file.seek(std::io::SeekFrom::Start(0))?;
        let mut data = Vec::new();
        file.take(1024).read_to_end(&mut data)?;

        let ext_str = path.to_string_lossy().to_lowercase();
        let extensions = parse_extensions(&ext_str);

        let logical = resolve_logical_format(&extensions);
        let inner = logical
            .and_then(|l| l.inner_format())
            .or_else(|| physical.inner_format());

        let is_multi_volume = extensions.iter().any(|e| {
            matches!(e.as_str(), "001" | "002")
                || (e.len() == 3
                    && e.starts_with('r')
                    && e[1..].bytes().all(|b| b.is_ascii_digit()))
        });

        if let Some(logical) = logical {
            if physical != logical.physical() {
                return Err(DetectionError::FormatMismatch {
                    magic: physical,
                    extension: logical,
                });
            }
        }

        let validated = if let Some(registry) = &self.validators {
            registry.validate(physical, path, &data).is_ok()
        } else {
            false
        };

        Ok(Detection {
            format: physical,
            logical,
            inner,
            extensions,
            is_multi_volume,
            total_volumes: None,
            validated,
        })
    }
}

impl Default for AutoFormat {
    fn default() -> Self {
        Self::new()
    }
}

fn file_format_to_archive_format(ff: FileFormat) -> Option<ArchiveFormat> {
    match ff {
        FileFormat::SevenZip => Some(ArchiveFormat::SevenZip),
        FileFormat::Zip |
        FileFormat::ThreeDimensionalManufacturingFormat |
        FileFormat::AdobeIntegratedRuntime|
        FileFormat::AndroidAppBundle|
        FileFormat::AndroidPackage|
        FileFormat::Autodesk123d|
        FileFormat::CircuitDiagramDocument|
        FileFormat::DesignWebFormatXps|
        FileFormat::ElectronicPublication|
        FileFormat::EnterpriseApplicationArchive|
        FileFormat::FictionbookZip|
        FileFormat::FigmaDesign|
        FileFormat::FlashCs5Project |
        FileFormat::Fusion360 |
        FileFormat::IndesignMarkupLanguage|
        FileFormat::JavaArchive |
        FileFormat::KeyholeMarkupLanguageZip |
        FileFormat::MicrosoftVisualStudioExtension |
        FileFormat::MusicxmlZip|
        FileFormat::OfficeOpenXmlDocument |
        FileFormat::OfficeOpenXmlDrawing |
        FileFormat::OfficeOpenXmlPresentation |
        FileFormat::OfficeOpenXmlSpreadsheet |
        FileFormat::OpendocumentDatabase |
        FileFormat::OpendocumentFormula |
        FileFormat::OpendocumentFormulaTemplate |
        FileFormat::OpendocumentGraphics |
        FileFormat::OpendocumentGraphicsTemplate |
        FileFormat::OpendocumentPresentation|
        FileFormat::OpendocumentPresentationTemplate |
        FileFormat::OpendocumentSpreadsheet |
        FileFormat::OpendocumentSpreadsheetTemplate |
        FileFormat::OpendocumentText |
        FileFormat::OpendocumentTextMaster |
        FileFormat::OpendocumentTextMasterTemplate |
        FileFormat::OpendocumentTextTemplate |
        FileFormat::Openraster|
        FileFormat::Openxps|
        FileFormat::Sketch43|
        FileFormat::SpaceclaimDocument|
        FileFormat::SunXmlCalc|
        FileFormat::SunXmlCalcTemplate |
        FileFormat::SunXmlDraw |
        FileFormat::SunXmlDrawTemplate |
        FileFormat::SunXmlImpress |
        FileFormat::SunXmlImpressTemplate|
        FileFormat::SunXmlMath|
        FileFormat::SunXmlWriter |
        FileFormat::SunXmlWriterGlobal|
        FileFormat::SunXmlWriterTemplate |
        FileFormat::UniversalSceneDescriptionZip|
        FileFormat::WebApplicationArchive|
        FileFormat::WindowsAppBundle|
        FileFormat::WindowsAppPackage|
        FileFormat::Xpinstall|
        FileFormat::IosAppStorePackage
        => Some(ArchiveFormat::Zip),
        FileFormat::TapeArchive => Some(ArchiveFormat::Tar),
        FileFormat::Gzip => Some(ArchiveFormat::GZip),
        FileFormat::Bzip2 => Some(ArchiveFormat::BZip2),
        FileFormat::Xz => Some(ArchiveFormat::Xz),
        FileFormat::WindowsImagingFormat => Some(ArchiveFormat::Wim),
        FileFormat::RoshalArchive => Some(ArchiveFormat::Rar),
        FileFormat::ArchivedByRobertJung => Some(ArchiveFormat::Arj),
        FileFormat::Cabinet => Some(ArchiveFormat::Cab),
        FileFormat::Lha => Some(ArchiveFormat::Lzh),
        FileFormat::LempelZivMarkovChainAlgorithm => Some(ArchiveFormat::Lzma),
        FileFormat::Zstandard => Some(ArchiveFormat::Zstd),
        FileFormat::Iso9660 => Some(ArchiveFormat::Iso),
        FileFormat::MicrosoftCompiledHtmlHelp => Some(ArchiveFormat::Chm),
        FileFormat::Cpio => Some(ArchiveFormat::Cpio),
        FileFormat::DebianPackage => Some(ArchiveFormat::Deb),
        FileFormat::RedHatPackageManager => Some(ArchiveFormat::Rpm),
        FileFormat::AppleDiskImage => Some(ArchiveFormat::Dmg),
        FileFormat::MicrosoftVirtualHardDisk => Some(ArchiveFormat::Vhd),
        FileFormat::VirtualMachineDisk => Some(ArchiveFormat::Vmdk),
        FileFormat::VirtualboxVirtualDiskImage => Some(ArchiveFormat::Vdi),
        FileFormat::Squashfs => Some(ArchiveFormat::SquashFS),
        _ => None,
    }
}


fn parse_extensions(path: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut current = path.to_string();
    while let Some(idx) = current.rfind('.') {
        let ext = current[idx..].to_string();
        parts.push(ext.trim_start_matches('.').to_string());
        current = current[..idx].to_string();
    }
    // Extensions are collected right-to-left, reverse for logical order
    parts.reverse();
    parts
}

fn resolve_logical_format(extensions: &[String]) -> Option<ArchiveFormat> {
    let joined = extensions.join(".");
    let known: &[(&str, ArchiveFormat)] = &[
        ("tar.gz", ArchiveFormat::TarGz),
        ("tar.xz", ArchiveFormat::TarXz),
        ("tar.bz2", ArchiveFormat::TarBz2),
        ("tgz", ArchiveFormat::TarGz),
        ("txz", ArchiveFormat::TarXz),
        ("tbz2", ArchiveFormat::TarBz2),
        ("tbz", ArchiveFormat::TarBz2),
    ];
    for (pat, fmt) in known {
        if joined == *pat || joined.ends_with(pat) {
            return Some(*fmt);
        }
    }
    if let Some(last) = extensions.last() {
        for fmt in ALL_FORMATS {
            if fmt.extensions().contains(&last.as_str()) {
                return Some(*fmt);
            }
        }
    }
    None
}

impl FormatDetector for AutoFormat {
    fn detect_format(&self, path: &Path) -> Result<ArchiveFormat, DetectError> {
        let detection = self.detect(path)?;
        Ok(detection.format)
    }
}

impl From<DetectionError> for DetectError {
    fn from(e: DetectionError) -> Self {
        match e {
            DetectionError::Io(err) => DetectError::Io(err),
            _ => DetectError::UnknownFormat,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_header(dir: &Path, name: &str, magic: &[u8], tail_padding: usize) -> PathBuf {
        let path = dir.join(name);
        let mut bytes = magic.to_vec();
        bytes.extend(std::iter::repeat_n(0u8, tail_padding));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    const SEVEN_ZIP: &[u8] = b"7z\xbc\xaf\x27\x1c";
    const RAR4: &[u8] = b"Rar!\x1a\x07\x00";
    const GZIP: &[u8] = &[0x1f, 0x8b];

    #[test]
    fn split_volume_001_is_multi_volume() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_header(dir.path(), "archive.7z.001", SEVEN_ZIP, 64);
        let detection = AutoFormat::new().detect(&path).unwrap();
        assert!(detection.is_multi_volume);
        assert_eq!(detection.format, ArchiveFormat::SevenZip);
    }

    #[test]
    fn plain_rar_is_not_multi_volume() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_header(dir.path(), "archive.rar", RAR4, 64);
        let detection = AutoFormat::new().detect(&path).unwrap();
        assert!(!detection.is_multi_volume);
    }

    #[test]
    fn tar_gz_resolves_compound_layers() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_header(dir.path(), "archive.tar.gz", GZIP, 64);
        let detection = AutoFormat::new().detect(&path).unwrap();
        assert_eq!(detection.format, ArchiveFormat::GZip);
        assert_eq!(detection.logical, Some(ArchiveFormat::TarGz));
        assert_eq!(detection.inner, Some(ArchiveFormat::Tar));
    }

    #[test]
    fn large_file_detects_from_header_without_reading_it_all() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = vec![0x50u8, 0x4b, 0x03, 0x04];
        bytes.extend(std::iter::repeat_n(0u8, 4 << 20));
        bytes.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
        bytes.extend(std::iter::repeat_n(0u8, 18));
        let path = dir.path().join("big.zip");
        std::fs::write(&path, bytes).unwrap();
        let detection = AutoFormat::new().detect(&path).unwrap();
        assert_eq!(detection.format, ArchiveFormat::Zip);
    }
}
