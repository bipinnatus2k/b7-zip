use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::archive_format::ArchiveFormat;
use crate::validator::FormatValidator;

/// Structural validator for Zip archives.
///
/// Reads the End of Central Directory record (which lives at the end of the
/// file), then validates central directory and local file header signatures
/// and rejects path traversal in entry names.
pub struct ZipValidator;

impl FormatValidator for ZipValidator {
    fn format(&self) -> ArchiveFormat {
        ArchiveFormat::Zip
    }

    fn validate(&self, path: &Path) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        let mut file = File::open(path).map_err(|e| vec![e.to_string()])?;
        let file_len = file.metadata().map_err(|e| vec![e.to_string()])?.len();

        if file_len < 22 {
            errors.push("file too small for a Zip archive".to_string());
            return Err(errors);
        }

        // The EOCD is within the last 65,557 bytes (comment length is u16).
        let tail_start = file_len.saturating_sub(65_557);
        file.seek(SeekFrom::Start(tail_start))
            .map_err(|e| vec![e.to_string()])?;
        let mut tail = Vec::new();
        file.read_to_end(&mut tail)
            .map_err(|e| vec![e.to_string()])?;

        let eocd_rel = tail
            .windows(4)
            .enumerate()
            .rev()
            .find(|(_, w)| *w == [0x50, 0x4B, 0x05, 0x06])
            .map(|(i, _)| i);
        let Some(eocd_rel) = eocd_rel else {
            errors.push("EOCD signature not found".to_string());
            return Err(errors);
        };
        let eocd_pos = tail_start + eocd_rel as u64;
        if eocd_pos + 22 > file_len {
            errors.push("EOCD extends beyond file".to_string());
            return Err(errors);
        }

        let cd_entries =
            u16::from_le_bytes(tail[eocd_rel + 10..eocd_rel + 12].try_into().unwrap()) as u64;
        let cd_size =
            u32::from_le_bytes(tail[eocd_rel + 12..eocd_rel + 16].try_into().unwrap()) as u64;
        let cd_offset =
            u32::from_le_bytes(tail[eocd_rel + 16..eocd_rel + 20].try_into().unwrap()) as u64;

        if cd_offset
            .checked_add(cd_size)
            .is_none_or(|end| end > file_len)
        {
            errors.push("Central Directory extends beyond file".to_string());
        }

        let mut pos = cd_offset;
        for _ in 0..cd_entries {
            let mut fixed = [0u8; 46];
            if read_exact_at(&mut file, pos, &mut fixed).is_err() {
                errors.push("CD entry truncated".to_string());
                break;
            }
            if fixed[0..4] != [0x50, 0x4B, 0x01, 0x02] {
                errors.push("invalid CD entry signature".to_string());
                break;
            }

            let name_len = u16::from_le_bytes(fixed[28..30].try_into().unwrap()) as u64;
            let extra_len = u16::from_le_bytes(fixed[30..32].try_into().unwrap()) as u64;
            let comment_len = u16::from_le_bytes(fixed[32..34].try_into().unwrap()) as u64;
            let local_offset = u32::from_le_bytes(fixed[42..46].try_into().unwrap()) as u64;

            if name_len > 0 {
                let mut name = vec![0u8; name_len as usize];
                if read_exact_at(&mut file, pos + 46, &mut name).is_ok() {
                    let name_str = String::from_utf8_lossy(&name);
                    let normalized = name_str.replace('\\', "/");
                    if normalized.split('/').any(|part| part == "..") {
                        errors.push(format!("path traversal detected in entry: {}", name_str));
                    }
                }
            }

            if local_offset
                .checked_add(30)
                .is_none_or(|end| end > file_len)
            {
                errors.push(format!(
                    "LFH at offset {} extends beyond file",
                    local_offset
                ));
            } else {
                let mut lfh = [0u8; 30];
                if read_exact_at(&mut file, local_offset, &mut lfh).is_err()
                    || lfh[0..4] != [0x50, 0x4B, 0x03, 0x04]
                {
                    errors.push(format!("invalid LFH signature at offset {}", local_offset));
                } else {
                    let lfh_name_len = u16::from_le_bytes(lfh[26..28].try_into().unwrap()) as u64;
                    let lfh_extra_len = u16::from_le_bytes(lfh[28..30].try_into().unwrap()) as u64;
                    if local_offset
                        .checked_add(30)
                        .and_then(|v| v.checked_add(lfh_name_len))
                        .and_then(|v| v.checked_add(lfh_extra_len))
                        .is_none_or(|end| end > file_len)
                    {
                        errors.push(format!(
                            "LFH data at offset {} extends beyond file",
                            local_offset
                        ));
                    }
                }
            }

            pos = pos
                .checked_add(46)
                .and_then(|v| v.checked_add(name_len))
                .and_then(|v| v.checked_add(extra_len))
                .and_then(|v| v.checked_add(comment_len))
                .ok_or_else(|| {
                    errors.push("CD entry offset overflow".to_string());
                    errors.clone()
                })?;
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn read_exact_at(file: &mut File, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(buf)
}
