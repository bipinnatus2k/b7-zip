use std::path::{Path, PathBuf};

/// Find the vcpkg installed tree: from VCPKG_ROOT, or by walking up from
/// the manifest dir looking for a `vcpkg_installed` directory.
fn find_vcpkg_installed(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start.to_path_buf());
    while let Some(ref d) = dir {
        let candidate = d.join("vcpkg_installed");
        if candidate.exists() {
            return Some(candidate);
        }
        dir = d.parent().map(|p| p.to_path_buf());
    }
    None
}

fn main() {
    let manifest_str = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let manifest_dir = Path::new(&manifest_str);

    let vcpkg_root = std::env::var("VCPKG_ROOT").ok();
    let triple = std::env::var("VCPKG_DEFAULT_TRIPLET").unwrap_or_else(|_| "x64-windows".into());
    let installed = vcpkg_root
        .map(|r| Path::new(&r).join("installed").join(&triple))
        .filter(|p| p.exists())
        .or_else(|| {
            let vcpkg = find_vcpkg_installed(manifest_dir)?;
            Some(vcpkg.join(&triple))
        });

    let mut build = cc::Build::new();
    build.cpp(true).file(manifest_dir.join("src/bridge.cc"));
    build.include(manifest_dir.join("src"));
    if let Some(ref dir) = installed {
        build.include(dir.join("include"));
    }
    // bit7z requires C++17.
    build.flag_if_supported("/std:c++17");
    build.flag_if_supported("-std=c++17");
    build.compile("bit7z-bridge");

    if let Some(ref dir) = installed {
        println!("cargo:rustc-link-search=native={}", dir.join("lib").display());
        println!("cargo:rustc-link-lib=bit7z64");
        println!("cargo:rustc-link-lib=7zip");
    }
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rustc-link-lib=oleaut32");
        println!("cargo:rustc-link-lib=ole32");
        println!("cargo:rustc-link-lib=user32");
    }
    println!("cargo:rerun-if-changed=src/demo.h");
    println!("cargo:rerun-if-changed=src/bridge.cc");
    println!("cargo:rerun-if-changed=src/ffi_gen.rs");
}
