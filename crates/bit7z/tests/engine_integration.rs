//! Integration tests against the real bit7z C++ engine.
//!
//! These tests require the 7-Zip DLL (7zip.dll from vcpkg) to be
//! discoverable via `VCPKG_ROOT`; they are skipped otherwise.

use bit7z_rs::{ArchiveEngine, Bit7zEngine, CompressOptions, WriterFormat};
use std::path::{Path, PathBuf};

fn find_dll() -> Option<PathBuf> {
    let vcpkg = std::env::var("VCPKG_ROOT").ok()?;
    let candidates = [
        format!("{vcpkg}/installed/x64-windows/bin/7zip.dll"),
        format!("{vcpkg}/installed/x64-windows/bin/7z.dll"),
    ];
    candidates.iter().map(PathBuf::from).find(|p| p.exists())
}

fn engine() -> Option<Bit7zEngine> {
    let dll = find_dll()?;
    Bit7zEngine::new(Some(dll.to_str()?)).ok()
}

#[test]
fn roundtrip_7z_with_chinese_names() {
    let Some(engine) = engine() else {
        eprintln!("skipped: 7zip.dll not found");
        return;
    };
    let dir = tempfile::tempdir().unwrap();

    // Source files (including a Chinese file name and nested content).
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let chinese = src_dir.join("中文 文件.txt");
    std::fs::write(&chinese, "hello 中文 content\nline2").unwrap();
    let plain = src_dir.join("plain.txt");
    std::fs::write(&plain, "plain content").unwrap();

    let archive = dir.path().join("test.7z");
    engine
        .compress(&[chinese.clone(), plain.clone()], &archive, &CompressOptions::default())
        .expect("compress");

    // List and verify entries.
    let entries = engine.list(&archive, None).expect("list");
    assert_eq!(entries.len(), 2);
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"中文 文件.txt"), "names: {names:?}");
    assert!(names.contains(&"plain.txt"), "names: {names:?}");
    let chinese_entry = entries.iter().find(|e| e.name == "中文 文件.txt").unwrap();
    assert_eq!(chinese_entry.size, "hello 中文 content\nline2".len() as u64);
    assert!(!chinese_entry.is_directory);

    // Extract and verify contents.
    let dest = dir.path().join("out");
    std::fs::create_dir_all(&dest).unwrap();
    let indices: Vec<u32> = entries.iter().map(|e| e.index).collect();
    engine
        .extract(&archive, &indices, &dest, None, &Default::default())
        .expect("extract");
    let extracted = dest.join("中文 文件.txt");
    assert_eq!(
        std::fs::read_to_string(&extracted).unwrap(),
        "hello 中文 content\nline2"
    );
    assert_eq!(
        std::fs::read_to_string(dest.join("plain.txt")).unwrap(),
        "plain content"
    );
}

#[test]
fn roundtrip_zip() {
    let Some(engine) = engine() else {
        eprintln!("skipped: 7zip.dll not found");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("data.bin");
    std::fs::write(&src, vec![0u8; 4096]).unwrap();

    let archive = dir.path().join("test.zip");
    let mut options = CompressOptions::default();
    options.format = WriterFormat::Zip;
    engine.compress(&[src.clone()], &archive, &options).expect("compress zip");

    let entries = engine.list(&archive, None).expect("list zip");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "data.bin");

    let dest = dir.path().join("out");
    std::fs::create_dir_all(&dest).unwrap();
    engine
        .extract(&archive, &[entries[0].index], &dest, None, &Default::default())
        .expect("extract zip");
    assert_eq!(std::fs::read(dest.join("data.bin")).unwrap(), vec![0u8; 4096]);
}

#[test]
fn extract_to_buffer() {
    let Some(engine) = engine() else {
        eprintln!("skipped: 7zip.dll not found");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("mem.txt");
    std::fs::write(&src, b"buffer content").unwrap();
    let archive = dir.path().join("mem.7z");
    engine.compress(&[src], &archive, &CompressOptions::default()).expect("compress");
    let entries = engine.list(&archive, None).unwrap();
    let bytes = engine.extract_to_buffer(&archive, entries[0].index, None).expect("buffer");
    assert_eq!(bytes, b"buffer content");
}

#[test]
fn update_add_delete_rename() {
    let Some(engine) = engine() else {
        eprintln!("skipped: 7zip.dll not found");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.txt");
    std::fs::write(&a, "aaa").unwrap();
    let b = dir.path().join("b.txt");
    std::fs::write(&b, "bbb").unwrap();
    let archive = dir.path().join("edit.7z");
    engine.compress(&[a.clone(), b.clone()], &archive, &CompressOptions::default()).expect("compress");

    let entries = engine.list(&archive, None).unwrap();
    assert_eq!(entries.len(), 2);
    let a_entry = entries.iter().find(|e| e.name == "a.txt").unwrap();
    let b_entry = entries.iter().find(|e| e.name == "b.txt").unwrap();

    // Delete a.txt, rename b.txt -> c.txt, add new.txt.
    let new = dir.path().join("new.txt");
    std::fs::write(&new, "new content").unwrap();
    let ops = vec![
        bit7z_rs::EngineOp::Delete { archive_index: a_entry.index },
        bit7z_rs::EngineOp::Rename { archive_index: b_entry.index, new_path: "c.txt".into() },
        bit7z_rs::EngineOp::Add { fs_path: new.clone(), archive_path: "new.txt".into() },
    ];
    engine.update(&archive, &ops, None).expect("update");

    let after = engine.list(&archive, None).unwrap();
    let names: Vec<&str> = after.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names.len(), 2, "names: {names:?}");
    assert!(names.contains(&"c.txt"), "names: {names:?}");
    assert!(names.contains(&"new.txt"), "names: {names:?}");
}

#[test]
fn password_protected_roundtrip() {
    let Some(engine) = engine() else {
        eprintln!("skipped: 7zip.dll not found");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("secret.txt");
    std::fs::write(&src, "top secret").unwrap();
    let archive = dir.path().join("secret.7z");
    let mut options = CompressOptions::default();
    options.password = Some("hunter2".into());
    options.encrypt_headers = true;
    engine.compress(&[src], &archive, &options).expect("compress");

    // Header encryption should be detectable without a password.
    assert!(engine.is_header_encrypted(&archive).unwrap());
    assert!(engine.is_encrypted(&archive).unwrap());

    // Listing requires the password.
    let pw = bit7z_rs::password::Password::new("hunter2");
    let entries = engine.list(&archive, Some(&pw)).expect("list with password");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "secret.txt");
}
