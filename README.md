# Bit7zFM

A 7-Zip/WinRAR-class compressed-file manager for Windows, built with
[GPUI](https://github.com/zed-industries/zed) and the bit7z C++ library
(which wraps 7-Zip itself). The project is a set of small, decoupled crates
so each component can be developed, tested, and reused independently.

## Components

| Crate / bin     | Responsibility                                              |
|-----------------|-------------------------------------------------------------|
| `crates/ffi`     | Hand-written C++ bridge (`demo.h` + `bridge.cc`) exposing bit7z as `extern "C"`, compiled with the `cc` crate |
| `crates/bit7z`   | Safe Rust wrapper: `ArchiveEngine` trait + `Bit7zEngine` (Mutex-serialized FFI) |
| `crates/vfs`     | Archive-agnostic VFS: `VfsNode`, `Tree`, `Overlay` (base+working+dirty), `diff::build_changeset`, `Changeset`, `EditQueue` |
| `crates/archive_vfs` | Builds a `Tree` from `ArchiveEntry` lists (synthetic directories) |
| `crates/fs`      | Physical `FsTree` (scan + watcher events) and attribute filling |
| `crates/fs_watcher` | Independent file-system watcher (`FsEvent` stream, notify crate) |
| `crates/temp`    | Temp-root management with runtime re-registration (default: system temp) |
| `crates/session` | `ArchiveSession`: lazy extraction to temp, overlay sync, commit/discard |
| `crates/task`    | Serialized job contracts (`JobFile`/serde) + `TaskRunner` with progress events |
| `crates/checksum`| CRC32/CRC64/MD5/SHA-1/SHA-256/SHA-512 |
| `crates/format_detector` | Sniff archive formats |
| `crates/password`| Password entry abstraction (secrets never stored in job files) |
| `crates/ui_kit`  | GPUI component library: buttons, text fields, tabs, tables, tree view, combo box, dialogs, toasts, status bar, split panes |
| `crates/explorer`| `ArchiveExplorer` browse component (breadcrumb + file table) |
| `app`           | `bit7zfm` – the file manager (GPUI shell + workspace) |
| `executor`      | `bit7z-executor` – standalone GUI task runner driven by job files |
| `cli`           | `bit7z` – full command-line surface (list/extract/compress/test/…, `shell-install`/`shell-uninstall`) |
| `crates/shell`  | Explorer context-menu COM DLL (IShellExtInit + IContextMenu) |
| `crates/resources` | Windows resources: app icon, DPI manifest, version info |

## Architecture

```
vfs → fs → fs_watcher        (three independent layers)
archive_vfs → engine        (bit7z via FFI)

manager (app)
  └─ explorer (browse component)
      └─ session (edit overlay on top of archive_vfs tree)
          └─ task (job files) → executor (GUI runner) | cli | shell (COM)
```

The VFS layer has **no notion of archives**: a `Tree` of `VfsNode`s is a
plain hierarchy of `id`, `parent`, `name`, `is_directory`, and an
`attrs` map using shared cross-platform attribute names
(`attr::SIZE`, `attr::PACKED_SIZE`, `attr::MODIFIED`, `attr::CRC`,
`attr::ARCHIVE_INDEX`, `attr::FS_PATH`, …). Edits are tracked by an
`Overlay` (`base` + `working` + `dirty`), collapsed into a
`Changeset` by the diff engine, and applied to the real archive by the
engine (or to the filesystem by a commit).

### Cross-process job contract

`executor`, `cli` and `shell` never talk directly to the engine. They
exchange **job files** (`{version, id, spec}`, JSON) under the temp jobs
directory. **Passwords never enter job files** – a spec only carries
`password_hint`; the real secret is supplied out-of-band (CLI flag,
dialog). See `crates/task/src/job.rs`.

### Shell extension

`crates/shell` builds `shell.dll`, a COM in-process server loaded by
Explorer.exe. It adds context-menu verbs (Extract here / Extract to… / Add
to archive… / Test / Open with Bit7zFM), writes a job file, and launches
`bit7z-executor.exe` hidden. Registration is HKCU-only and symmetric:

```
regsvr32  shell.dll        # or: bit7z shell-install
regsvr32 /u shell.dll      # or: bit7z shell-uninstall
```

Every exported entry point is wrapped in `catch_unwind`; the DLL never
touches archives itself.

## Prerequisites

- Rust stable (MSVC target), Windows 10/11 SDK (for `rc.exe`/`mt.exe`)
- [vcpkg](https://github.com/microsoft/vcpkg) with the bit7z port:
  `vcpkg install bit7z:x64-windows` – produces `bit7z64.lib`,
  `7zip.lib`, `7zip.dll` under `<VCPKG_ROOT>/installed/x64-windows`
- Set `VCPKG_ROOT` accordingly (the FFI build script locates the libs)

## Build

```
cargo build --offline            # debug (uses local registry cache)
cargo build --release --offline
```

## Test

```
cargo test --workspace --offline
```

## Package

```
powershell -ExecutionPolicy Bypass -File scripts/package.ps1
```

Assembles `dist/Bit7zFM-0.1.0/` with `bit7zfm.exe`,
`bit7z-executor.exe`, `shell.dll`, `7zip.dll`, embeds the DPI manifest
into the exes, and registers the shell extension.

## CLI examples

```
bit7z list archive.7z
bit7z extract archive.7z --to out/
bit7z extract archive.7z --indices 0,2 --to out/ --password secret
bit7z compress a.txt b/ --to out.7z
bit7z test archive.zip
bit7z checksum archive.7z 3 --algorithm sha256
bit7z shell-install
bit7z shell-uninstall
```

## Design rules

- Each crate is standalone: no crate outside `task` knows job files; no
  crate outside `vfs` knows archive-agnostic trees; `fs_watcher` knows
  nothing about VFS.
- 7-Zip is single-threaded and accessed through one Mutex; long operations
  run on background threads and report progress via channels.
- Zip-slip is prevented by `temp::sanitize_relative`; long paths keep
  `\\?\` prefixes stripped.
- No panics cross FFI boundaries (shell DLL, C++ bridge callbacks).

## License

GPL-3.0-or-later (see LICENSE-GPL). The bundled bit7z wrapper and 7-Zip
runtime are distributed under their respective licenses (7-Zip is LGPL /
public-domain-licensed portions).
