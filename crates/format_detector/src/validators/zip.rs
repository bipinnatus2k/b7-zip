use std::path::Path;

use crate::archive_format::ArchiveFormat;
use crate::validator::FormatValidator;

/// Structural validator for Zip archives.
///
/// Validates:
/// 1. EOCD (End of Central Directory) signature exists at expected offset
/// 2. Central Directory entries have valid signatures
/// 3. Local File Headers have valid signatures
/// 4. No path traversal in filenames
pub struct ZipValidator;

#[async_trait::async_trait]
impl FormatValidator for ZipValidator {
    fn format(&self) -> ArchiveFormat {
        ArchiveFormat::Zip
    }

    async fn validate(&self, _path: &Path, data: &[u8]) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if data.len() < 22 {
            errors.push("file too small for a Zip archive".to_string());
            return Err(errors);
        }

        // Search for EOCD signature (0x06054b50) from end of file
        let eocd_pos = data
            .windows(4)
            .enumerate()
            .rev()
            .find(|(_, w)| *w == [0x50, 0x4B, 0x05, 0x06])
            .map(|(i, _)| i);

        let eocd_pos = match eocd_pos {
            Some(p) => p,
            None => {
                errors.push("EOCD signature not found".to_string());
                return Err(errors);
            }
        };

        // Parse EOCD fields
        if eocd_pos + 22 > data.len() {
            errors.push("EOCD extends beyond file".to_string());
            return Err(errors);
        }

        let cd_offset =
            u32::from_le_bytes(data[eocd_pos + 16..eocd_pos + 20].try_into().unwrap()) as usize;
        let cd_entries =
            u16::from_le_bytes(data[eocd_pos + 10..eocd_pos + 12].try_into().unwrap()) as usize;
        let cd_size =
            u32::from_le_bytes(data[eocd_pos + 12..eocd_pos + 16].try_into().unwrap()) as usize;

        // Validate Central Directory bounds
        if cd_offset + cd_size > data.len() {
            errors.push("Central Directory extends beyond file".to_string());
        }

        // Iterate central directory entries
        let mut pos = cd_offset;
        for _ in 0..cd_entries {
            if pos + 46 > data.len() {
                errors.push("CD entry truncated".to_string());
                break;
            }
            if data[pos..pos + 4] != [0x50, 0x4B, 0x01, 0x02] {
                errors.push("invalid CD entry signature".to_string());
                break;
            }

            let name_len =
                u16::from_le_bytes(data[pos + 28..pos + 30].try_into().unwrap()) as usize;
            let extra_len =
                u16::from_le_bytes(data[pos + 30..pos + 32].try_into().unwrap()) as usize;
            let comment_len =
                u16::from_le_bytes(data[pos + 32..pos + 34].try_into().unwrap()) as usize;
            let local_offset =
                u32::from_le_bytes(data[pos + 42..pos + 46].try_into().unwrap()) as usize;

            // Check filename for path traversal
            if name_len > 0 && pos + 46 + name_len <= data.len() {
                let name = &data[pos + 46..pos + 46 + name_len];
                let name_str = String::from_utf8_lossy(name);
                if name_str.contains("..") {
                    errors.push(format!(
                        "path traversal detected in entry: {}",
                        name_str
                    ));
                }
            }

            // Validate Local File Header
            if local_offset + 30 > data.len() {
                errors.push(format!(
                    "LFH at offset {} extends beyond file",
                    local_offset
                ));
            } else if data[local_offset..local_offset + 4] != [0x50, 0x4B, 0x03, 0x04] {
                errors.push(format!(
                    "invalid LFH signature at offset {}",
                    local_offset
                ));
            } else {
                let lfh_name_len =
                    u16::from_le_bytes(data[local_offset + 26..local_offset + 28].try_into().unwrap())
                        as usize;
                let lfh_extra_len =
                    u16::from_le_bytes(data[local_offset + 28..local_offset + 30].try_into().unwrap())
                        as usize;
                let lfh_total = 30 + lfh_name_len + lfh_extra_len;
                if local_offset + lfh_total > data.len() {
                    errors.push(format!(
                        "LFH data at offset {} extends beyond file",
                        local_offset
                    ));
                }
            }

            let entry_total = 46 + name_len + extra_len + comment_len;
            pos += entry_total;
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}
