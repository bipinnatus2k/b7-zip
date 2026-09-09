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
| `crates/vfs`     | Archive-agnostic VFS: `VfsNode`, `Tree`, `Overlay` (base+working+dirty), `diff::build_changeset`, `Changeset` |
| `crates/archive_vfs` | Builds a `Tree` from `ArchiveEntry` lists (synthetic directories) |
| `crates/fs`      | Physical `FsTree` (scan + watcher events) and attribute filling |
| `crates/fs_watcher` | Independent file-system watcher (`FsEvent` stream, notify crate) |
| `crates/temp`    | Temp-root management with runtime re-registration (default: system temp) |
| `crates/session` | `ArchiveSession`: lazy extraction to temp, overlay sync, commit/discard |
| `crates/task`    | Serialized job contracts (`JobFile`/serde) + `TaskRunner` with progress events |
| `crates/checksum`| CRC32/CRC64/MD5/SHA-1/SHA-256/SHA-512 |
| `crates/format_detector` | Sniff archive formats |
| `crates/password`| Password entry abstraction (secrets never stored in job files) |
| `crates/thirdparties/guise` | Vendored [guise](https://github.com/wess/guise) v0.12.0 – the Mantine-style component library every UI surface is built on (TableView, Modal, inputs, toasts, …) |
| `crates/explorer`| `ArchiveExplorer` browse component (7zFM-style sortable/multi-select file table + keyboard navigation) |
| `app`           | `bit7zfm` – the 7zFM-style file manager (toolbar, breadcrumb address bar, file table, dialogs, live progress) |
| `executor`      | `bit7z-executor` – standalone GUI task runner driven by job files (live byte progress + cancel) |
| `cli`           | `bit7z` – full command-line surface (list/extract/compress/test/…, `shell-install`/`shell-uninstall`) |
| `crates/windows_shell_extension`  | Explorer context-menu COM DLL (IShellExtInit + IContextMenu) |
| `crates/resources` | Windows resources: app icon, DPI manifest, version info |

## Architecture

```
vfs → fs → fs_watcher        (three independent layers)
archive_vfs → engine        (bit7z via FFI)

manager (app, guise UI)
  └─ explorer (7zFM-style file table)
      └─ session (edit overlay on top of archive_vfs tree)
          └─ task (job files) → executor (GUI runner) | cli | shell (COM)
```

All UI surfaces (manager, explorer, executor) are built on
[guise](https://github.com/wess/guise) v0.12.0 (vendored in
`crates/thirdparties/guise`), a Mantine-style component library for gpui.

## File manager features

`bit7zfm` targets the 7-Zip File Manager workflow:

- **Browse**: virtualized, sortable file table (Name / Size / Packed /
  Modified / Attributes / CRC / Method) with drag-resizable columns,
  multi-select (Ctrl/Shift-click), and keyboard navigation
  (arrows, Enter, Backspace, Delete, F2, Ctrl+A).
- **Open**: from the command line or the native file dialog; nested
  directory navigation through a breadcrumb address bar.
- **Extract**: selected entries (or the whole archive), target-directory
  picker, overwrite/skip policy, live byte progress and cancellation.
- **Add**: new archives (7z/zip/tar/gzip/bzip2/xz/wim, level, solid,
  volumes, threads, password + header encryption) or add files into the
  open archive.
- **Test** archive integrity; **Delete** and **Rename** entries inside the
  archive; per-file **open** (extract to temp + OS association).
- **Password** dialogs for encrypted archives; **Info** dialog with
  archive and selection statistics; toast notifications.

## VFS design

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

`crates/windows_shell_extension` builds `shell.dll`, a COM in-process server loaded by
Explorer.exe. It adds 7-Zip / WinRAR-style context-menu verbs:

- On archives: Open, Extract files…, Extract Here, Extract to `"name\"`,
  Test, plus compress / hash entries
- On any file/folder: Add to archive…, Add to `"name.7z"` / `"name.zip"`,
  Add each to separate `.7z`, CRC-32 / CRC-64 / SHA-1 / SHA-256 / MD5

By default verbs are nested under a **Bit7z** cascade submenu. Preferences
live in `HKCU\Software\Bit7zFM\Shell` (written on first install, preserved
on uninstall):

| Value | Type | Default | Meaning |
| --- | --- | --- | --- |
| `CascadedMenu` | DWORD | `1` | `1` = submenu, `0` = flat top-level items |
| `CascadeName` | SZ | `Bit7z` | Root submenu label |
| `MenuFlags` | DWORD | all bits | Per-verb visibility bitmask |

Registration is HKCU-only and symmetric:

```
regsvr32  shell.dll        # or: bit7z shell-install
regsvr32 /u shell.dll      # or: bit7z shell-uninstall
```

The CLSID is also added to
`HKCU\Software\Microsoft\Windows\CurrentVersion\Shell Extensions\Approved`
so Explorer permits the per-user `IContextMenu` handler to load.

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
`bit7z-executor.exe`, `bit7z.exe`, `shell.dll`, `7zip.dll`, embeds the DPI
manifest into the exes, and registers the shell extension.

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
