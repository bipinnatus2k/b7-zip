//! Locates the 7-Zip DLL: exe dir (`7z.dll` then `7zip.dll`), then `$VCPKG_ROOT/installed/x64-windows/bin` (`7zip.dll` then `7z.dll`).

use std::path::{Path, PathBuf};

/// Locate the 7-Zip DLL by searching the exe dir, then `VCPKG_ROOT`.
pub fn locate_dll() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let vcpkg_bin = std::env::var("VCPKG_ROOT").ok().map(|root| {
        let triple =
            std::env::var("VCPKG_DEFAULT_TRIPLET").unwrap_or_else(|_| "x64-windows".into());
        PathBuf::from(root)
            .join("installed")
            .join(triple)
            .join("bin")
    });
    locate_dll_in(&exe_dir, vcpkg_bin.as_deref())
}

fn locate_dll_in(exe_dir: &Path, vcpkg_bin: Option<&Path>) -> Option<PathBuf> {
    for name in ["7z.dll", "7zip.dll"] {
        let candidate = exe_dir.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    let vcpkg_bin = vcpkg_bin?;
    for name in ["7zip.dll", "7z.dll"] {
        let candidate = vcpkg_bin.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::write(path, b"").unwrap();
    }

    #[test]
    fn exe_dir_prefers_7z_dll_over_7zip_dll() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("7z.dll"));
        touch(&dir.path().join("7zip.dll"));
        assert_eq!(
            locate_dll_in(dir.path(), None),
            Some(dir.path().join("7z.dll"))
        );
    }

    #[test]
    fn exe_dir_falls_back_to_7zip_dll() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("7zip.dll"));
        assert_eq!(
            locate_dll_in(dir.path(), None),
            Some(dir.path().join("7zip.dll"))
        );
    }

    #[test]
    fn vcpkg_bin_prefers_7zip_dll_over_7z_dll() {
        let exe_dir = tempfile::tempdir().unwrap();
        let vcpkg = tempfile::tempdir().unwrap();
        touch(&vcpkg.path().join("7zip.dll"));
        touch(&vcpkg.path().join("7z.dll"));
        assert_eq!(
            locate_dll_in(exe_dir.path(), Some(vcpkg.path())),
            Some(vcpkg.path().join("7zip.dll"))
        );
    }

    #[test]
    fn vcpkg_bin_falls_back_to_7z_dll() {
        let exe_dir = tempfile::tempdir().unwrap();
        let vcpkg = tempfile::tempdir().unwrap();
        touch(&vcpkg.path().join("7z.dll"));
        assert_eq!(
            locate_dll_in(exe_dir.path(), Some(vcpkg.path())),
            Some(vcpkg.path().join("7z.dll"))
        );
    }

    #[test]
    fn returns_none_when_nowhere_to_be_found() {
        let exe_dir = tempfile::tempdir().unwrap();
        let vcpkg = tempfile::tempdir().unwrap();
        assert_eq!(locate_dll_in(exe_dir.path(), Some(vcpkg.path())), None);
    }
}
