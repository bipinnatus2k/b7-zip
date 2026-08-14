//! Compile Windows resources (icon, manifest, version info) into the exe.
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../crates/resources/app.rc");
    println!("cargo:rerun-if-changed=../crates/resources/app.manifest");
    println!("cargo:rerun-if-changed=../crates/resources/bit7z.ico");

    // Locate rc.exe from the Windows SDK.
    let Some(rc) = find_rc() else {
        println!("cargo:warning=rc.exe not found; skipping Windows resources for bit7zfm");
        return;
    };

    let resources = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/resources");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let res_out = out_dir.join("app.rc".replace(".rc", ".res"));

    let status = Command::new(&rc)
        .current_dir(&resources)
        .arg("/fo")
        .arg(&res_out)
        .arg(&resources.join("app.rc"))
        .status()
        .expect("failed to spawn rc.exe");
    if !status.success() {
        panic!("rc.exe failed for app.rc");
    }

    // Link the .res into the binary (MSVC linker accepts .res as input).
    println!("cargo:rustc-link-arg-bins={}", res_out.display());
}

/// Find rc.exe under the Windows 10/11 SDK.
fn find_rc() -> Option<PathBuf> {
    let kits = [
        "C:/Program Files (x86)/Windows Kits/10/bin",
        "C:/Program Files/Windows Kits/10/bin",
    ];
    for kit in kits {
        let root = PathBuf::from(kit);
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        // Pick the highest SDK version directory.
        let mut versions: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
            .filter(|s| s.chars().all(|c| c.is_ascii_digit() || c == '.'))
            .collect();
        versions.sort();
        for version in versions.iter().rev() {
            let candidate = root.join(version).join("x64/rc.exe");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}