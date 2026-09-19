//! Checksum computation for files and byte streams.
//!
//! A small leaf component with no dependencies on archives, VFS, or the GUI.
//! Supports the algorithms most commonly needed by archive managers:
//! CRC32, CRC64, MD5, SHA-1, SHA-256 and SHA-512.

use crc_fast::CrcAlgorithm;
use serde::{Deserialize, Serialize};
use std::io::{self, Read};
use std::path::Path;

/// Checksum algorithms supported by this component.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum ChecksumAlgorithm {
    Crc32,
    Crc64,
    Md5,
    Sha1,
    Sha256,
    Sha512,
}

impl ChecksumAlgorithm {
    /// Human-readable name, e.g. `CRC-32`.
    pub fn name(self) -> &'static str {
        match self {
            ChecksumAlgorithm::Crc32 => "CRC-32",
            ChecksumAlgorithm::Crc64 => "CRC-64",
            ChecksumAlgorithm::Md5 => "MD5",
            ChecksumAlgorithm::Sha1 => "SHA-1",
            ChecksumAlgorithm::Sha256 => "SHA-256",
            ChecksumAlgorithm::Sha512 => "SHA-512",
        }
    }
}

/// The result of computing a checksum over a single file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecksumResult {
    pub path: String,
    pub size: u64,
    pub algorithm: ChecksumAlgorithm,
    /// Hex-encoded digest (lowercase).
    pub digest: String,
}

/// Incremental hasher state. Each algorithm is a separate implementation,
/// but they share this uniform interface so callers can stream data.
pub trait IncrementalHasher: Send {
    fn update(&mut self, data: &[u8]);
    fn finalize_hex(&self) -> String;
}

/// Create a fresh hasher for the given algorithm.
pub fn new_hasher(algorithm: ChecksumAlgorithm) -> Box<dyn IncrementalHasher> {
    match algorithm {
        ChecksumAlgorithm::Crc32 => Box::new(Crc32Hasher(crc32fast::Hasher::new())),
        ChecksumAlgorithm::Crc64 => Box::new(Crc64Hasher::new()),
        ChecksumAlgorithm::Md5 => {
            use md5::Digest;
            Box::new(Md5Hasher(md5::Md5::new()))
        }
        ChecksumAlgorithm::Sha1 => {
            use sha1::Digest;
            Box::new(Sha1Hasher(sha1::Sha1::new()))
        }
        ChecksumAlgorithm::Sha256 => {
            use sha2::Digest;
            Box::new(Sha256Hasher(sha2::Sha256::new()))
        }
        ChecksumAlgorithm::Sha512 => {
            use sha2::Digest;
            Box::new(Sha512Hasher(sha2::Sha512::new()))
        }
    }
}

/// Compute a checksum over an in-memory byte slice.
pub fn checksum_bytes(data: &[u8], algorithm: ChecksumAlgorithm) -> String {
    let mut hasher = new_hasher(algorithm);
    hasher.update(data);
    hasher.finalize_hex()
}

/// Compute a checksum by streaming from any reader; returns (bytes read, hex digest).
pub fn checksum_reader(reader: impl Read, algorithm: ChecksumAlgorithm) -> io::Result<(u64, String)> {
    let mut hasher = new_hasher(algorithm);
    let mut buffer = [0u8; 64 * 1024];
    let mut total: u64 = 0;
    let mut reader = reader;
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        hasher.update(&buffer[..n]);
    }
    Ok((total, hasher.finalize_hex()))
}

/// Compute a checksum over a file on disk.
pub fn checksum_file(path: impl AsRef<Path>, algorithm: ChecksumAlgorithm) -> io::Result<ChecksumResult> {
    let path = path.as_ref();
    let file = std::fs::File::open(path)?;
    let (size, digest) = checksum_reader(file, algorithm)?;
    Ok(ChecksumResult {
        path: path.display().to_string(),
        size,
        algorithm,
        digest,
    })
}

/// Compute digests for several algorithms over a **single** read pass of the
/// file. A multi-algorithm listing must not reread the file per algorithm —
/// for large selections that multiplies I/O by the algorithm count.
pub fn checksum_file_multi(
    path: impl AsRef<Path>,
    algorithms: &[ChecksumAlgorithm],
) -> io::Result<Vec<ChecksumResult>> {
    let path = path.as_ref();
    let mut file = std::fs::File::open(path)?;
    let mut hashers: Vec<(ChecksumAlgorithm, Box<dyn IncrementalHasher>)> = algorithms
        .iter()
        .copied()
        .map(|algorithm| (algorithm, new_hasher(algorithm)))
        .collect();
    let mut buffer = [0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        for (_, hasher) in &mut hashers {
            hasher.update(&buffer[..n]);
        }
    }
    let path = path.display().to_string();
    Ok(hashers
        .into_iter()
        .map(|(algorithm, hasher)| ChecksumResult {
            path: path.clone(),
            size: total,
            algorithm,
            digest: hasher.finalize_hex(),
        })
        .collect())
}

struct Crc32Hasher(crc32fast::Hasher);
impl IncrementalHasher for Crc32Hasher {
    fn update(&mut self, data: &[u8]) { self.0.update(data); }
    fn finalize_hex(&self) -> String { format!("{:08x}", self.0.clone().finalize()) }
}

/// CRC-64/ECMA-182 (poly 0x42F0E1EBA9EA3693, non-reflected).
///
/// Implemented here (table-driven, dependency-free) instead of using the
/// `crc` crate because its `Digest` borrows the `Crc` instance, which makes
/// incremental hashing inside a boxed trait object awkward.
struct Crc64Hasher(crc_fast::Digest);

impl Crc64Hasher {
    fn new() -> Self {
        let digest = crc_fast::Digest::new(CrcAlgorithm::Crc64Ecma182);
        Self { 0: digest }
    }
}

impl IncrementalHasher for Crc64Hasher {
    fn update(&mut self, data: &[u8]) { self.0.update(data); }
    fn finalize_hex(&self) -> String {
        format!("{:016x}", self.0.finalize())
    }
}

struct Md5Hasher(md5::Md5);
impl IncrementalHasher for Md5Hasher {
    fn update(&mut self, data: &[u8]) {
        use md5::Digest;
        self.0.update(data); }
    fn finalize_hex(&self) -> String {
        use md5::Digest;
        hex(&self.0.clone().finalize()) }
}

struct Sha1Hasher(sha1::Sha1);
impl IncrementalHasher for Sha1Hasher {
    fn update(&mut self, data: &[u8]) {
        use sha1::Digest;
        self.0.update(data); }
    fn finalize_hex(&self) -> String {
        use sha1::Digest;
        hex(&self.0.clone().finalize()) }
}

struct Sha256Hasher(sha2::Sha256);
impl IncrementalHasher for Sha256Hasher {
    fn update(&mut self, data: &[u8]) {
        use sha2::Digest;
        self.0.update(data); }
    fn finalize_hex(&self) -> String {
        use sha2::Digest;
        hex(&self.0.clone().finalize()) }
}

struct Sha512Hasher(sha2::Sha512);
impl IncrementalHasher for Sha512Hasher {
    fn update(&mut self, data: &[u8]) {
        use sha2::Digest;
        self.0.update(data); }
    fn finalize_hex(&self) -> String {
        use sha2::Digest;
        hex(&self.0.clone().finalize()) }
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_empty_vectors() {
        assert_eq!(checksum_bytes(b"", ChecksumAlgorithm::Crc32), "00000000");
        assert_eq!(checksum_bytes(b"", ChecksumAlgorithm::Crc64), "0000000000000000");
        assert_eq!(checksum_bytes(b"", ChecksumAlgorithm::Md5), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(checksum_bytes(b"", ChecksumAlgorithm::Sha1), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(checksum_bytes(b"", ChecksumAlgorithm::Sha256), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(checksum_bytes(b"", ChecksumAlgorithm::Sha512), "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e");
    }

    #[test]
    fn known_crc64_ecma182_vector() {
        // Standard check value for CRC-64/ECMA-182 of "123456789".
        assert_eq!(
            checksum_bytes(b"123456789", ChecksumAlgorithm::Crc64),
            "6c40df5f0b497347"
        );
    }

    #[test]
    fn known_abc_vectors() {
        assert_eq!(checksum_bytes(b"abc", ChecksumAlgorithm::Md5), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(checksum_bytes(b"abc", ChecksumAlgorithm::Sha1), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(checksum_bytes(b"abc", ChecksumAlgorithm::Sha256), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        // CRC-32 of "abc" = 0x352441c2
        assert_eq!(checksum_bytes(b"abc", ChecksumAlgorithm::Crc32), "352441c2");
    }

    #[test]
    fn streaming_matches_bytes() {
        let data = b"The quick brown fox jumps over the lazy dog";
        let (size, digest) = checksum_reader(&data[..], ChecksumAlgorithm::Sha256).unwrap();
        assert_eq!(size as usize, data.len());
        assert_eq!(digest, checksum_bytes(data, ChecksumAlgorithm::Sha256));
    }

    #[test]
    fn checksum_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.bin");
        std::fs::write(&path, b"hello tempfile").unwrap();
        let result = checksum_file(&path, ChecksumAlgorithm::Sha1).unwrap();
        assert_eq!(result.size, 14);
        assert_eq!(result.digest, checksum_bytes(b"hello tempfile", ChecksumAlgorithm::Sha1));
    }

    #[test]
    fn multi_pass_matches_single_algorithm_results() {
        let all = [
            ChecksumAlgorithm::Crc32,
            ChecksumAlgorithm::Crc64,
            ChecksumAlgorithm::Md5,
            ChecksumAlgorithm::Sha1,
            ChecksumAlgorithm::Sha256,
            ChecksumAlgorithm::Sha512,
        ];
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.bin");
        std::fs::write(&path, b"one read, many digests").unwrap();
        let multi = checksum_file_multi(&path, &all).unwrap();
        assert_eq!(multi.len(), all.len());
        for (algorithm, result) in all.iter().zip(&multi) {
            assert_eq!(result.algorithm, *algorithm);
            assert_eq!(result.size, 22);
            assert_eq!(
                result.digest,
                checksum_file(&path, *algorithm).unwrap().digest
            );
        }
    }
}
