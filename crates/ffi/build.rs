use std::path::{Path, PathBuf};

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

fn main() -> miette::Result<()> {
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
    let mut include_paths = vec![manifest_dir.join("src")];
    if let Some(ref dir) = installed {
        include_paths.push(dir.join("include"));
    }
    let source = manifest_dir.join("src/lib.rs");
    let mut b = autocxx_build::Builder::new(&source, &include_paths)
        .auto_allowlist(true)
        .build()?;
    b.flag_if_supported("-std=c++17").compile("bit7z-autocxx");

    // Remove include! from inside extern block (Rust 1.72+ compat)
    if let Ok(out_dir) = std::env::var("OUT_DIR") {
        let gen_path = Path::new(&out_dir)
            .join("autocxx-build-dir")
            .join("rs")
            .join("autocxx-ffi-default-gen.rs");
        if gen_path.exists() {
            let content = std::fs::read_to_string(&gen_path).unwrap_or_default();
            let pattern = "include ! (\"demo.h\") ; include ! (\"autocxxgen_ffi.h\") ; ";
            if content.contains(pattern) {
                let fixed = content.replace(pattern, "");
                std::fs::write(&gen_path, &fixed).ok();
                println!("cargo:warning=PATCHED: removed include! from inside extern block");
            }
        }
    }

    if let Some(ref dir) = installed {
        println!(
            "cargo:rustc-link-search=native={}",
            dir.join("lib").display()
        );
        println!("cargo:rustc-link-lib=bit7z64");
        println!("cargo:rustc-link-lib=7zip");
    }
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rustc-link-lib=oleaut32");
        println!("cargo:rustc-link-lib=ole32");
        println!("cargo:rustc-link-lib=user32");
    }
    println!("cargo:rerun-if-changed=src/lib.rs");
    Ok(())
}
