# AGENTS.md

Guidance for AI coding agents working in this repository.

## What this is

**Bit7zFM** — a 7-Zip/WinRAR-class archive manager for Windows, built on GPUI (the `gpui-pre` fork of zed's gpui, 0.3.4, plus `gpui-kit`) and the bit7z C++ library via a hand-written FFI bridge. Rust workspace, edition 2024, resolver 3. `README.md` has the full component table, but its UI-stack description is partly stale — see "Refactor in flight" below. Phase history and the pending feature backlog live in `docs/ROADMAP.md`.

## Build, check, test

Prerequisites: Rust stable MSVC target, Windows 10/11 SDK, and vcpkg with the bit7z port (`vcpkg install bit7z:x64-windows`) and `VCPKG_ROOT` set. `crates/ffi/build.rs` also walks up from the crate looking for a `vcpkg_installed/` dir.

```bash
cargo build --offline                       # bare commands build only `app` (default-members = ["app"])
cargo check --workspace --offline           # all crates
cargo test  --workspace --offline
cargo clippy --workspace --offline
powershell -ExecutionPolicy Bypass -File scripts/package.ps1   # assembles dist/Bit7zFM-*/
```

- `--offline` is the documented workflow (local registry cache).
- `crates/thirdparties/guise` and the `gpui-router` root dir are `exclude`d from the workspace; only `crates/thirdparties/gpui-router/crates/router` and `router-macros` are members.

## Layout

- **Binaries**: `app` (`bit7zfm` GUI), `executor` (`bit7z-executor` GUI job runner), `cli` (`bit7z`).
- **Archive core**: `crates/ffi` (C++ bridge, compiled with `cc`) → `crates/bit7z` (`ArchiveEngine`, Mutex-serialized) → `crates/archive_vfs` → `crates/vfs` (archive-agnostic `Tree`/`VfsNode`/`Overlay`/`Changeset`). `crates/fs`, `crates/fs_watcher`, `crates/temp`, `crates/session` (overlay sync, commit), `crates/task` (JSON job files + `TaskRunner`).
- **UI**: `crates/ui` (the design system: `components/`, `layout/`, `styles/`, `foundation/`, `data/`), `crates/workspace` (`MultiWorkspace`, `panels/` editor+sidebar, `tab_bar`), `crates/explorer` (package `bit7z-explorer`, the 7zFM-style file table), `crates/platform_title_bar` (Windows + Linux window controls), `crates/tray`, `crates/signals` (reactive-signals lib).
- **Shell integration**: `crates/windows_shell_extension` (COM DLL `shell.dll`), `crates/explorer-menu` (Windows 11 native context menus via MSIX + `IExplorerCommand`), `crates/resources`.
- **Infra**: `app_action`, `app_constants`, `assets` (embedded via rust-embed), `paths`, `release_channel`, `crashes`, `etw_tracing`/`etw_tracing_ui`, `net`, `system_specs`, `checksum`, `format_detector`, `password`.
- **Vendored** (`crates/thirdparties/`): `zed/` (path, util, util_macros, perf), `gpui-router`, `gpui-tray`, `scap` (patched). `guise/` is retired.

## Architecture boundaries (do not violate)

- Crate isolation: only `task` knows job files; only `vfs` knows VFS trees; `fs_watcher` knows nothing about VFS; `multi-workspace-core` stays framework-agnostic (GPUI mapping lives in the host via its `Adapter` trait).
- Cross-process contract: `executor`, `cli`, and the shell extensions never touch the engine directly — they exchange JSON job files (`crates/task/src/job.rs`). **Passwords never enter job files**; specs carry only `password_hint`, the secret is supplied out-of-band (dialog/CLI flag).
- 7-Zip is single-threaded: all engine access goes through one Mutex; long operations run on background threads and report progress via channels/events.
- No panics may cross FFI boundaries (C++ bridge callbacks, shell DLL exports — wrapped in `catch_unwind`).
- Zip-slip protection lives in `temp::sanitize_relative`; keep `\\?\` long-path prefixes stripped at the edges.

## Conventions

- Member crates set `[lints] workspace = true`; workspace clippy denies `dbg_macro` and `todo`, allows the `style` group. Don't relax these per-crate.
- The binary name must equal `app_constants::APP_NAME_LOWERCASE` (compile-time assert in `app/src/main.rs`) — if you rename the binary, update `app_constants`.
- Shared deps go in `[workspace.dependencies]`; use the gpui-pre package aliases already defined there (`gpui`, `collections`, `zlog`, `ztracing`, `util`, `path`…) rather than adding crates.io originals.
- App boots through `app/src/init/` (dirs, crash handler, fonts, environment); global allocator is mimalloc.

## GPUI work

Before writing or debugging GPUI code, load the matching skill in `.agents/skills/`: `gpui-state-management` (where state lives, `cx.notify`/observe/emit), `gpui-async-tasks` (`cx.spawn`, cancellation, background work), `gpui-progress-task` (long-running ops with progress/cancel), `gpui-test` (`gpui::test`, `TestAppContext`). These encode zed's patterns, which this codebase follows.

## Refactor in flight (as of 2026-09)

The UI stack is mid-migration from the vendored `guise` library to `gpui-pre` + `gpui-kit` + the local `ui` crate. Consequences:

- `README.md`'s guise references and some crate descriptions are stale; trust the code and this file over the README for UI questions.
- `crates/explorer_model`, `crates/explorer_view`, and the local `zlog`/`ztracing` crates are retired (commented out of workspace members); `crates/thirdparties/guise/` is gitignored — do not resurrect them.
- `crates/workspace`'s old `workspace.rs`/`welcome.rs`/`task.rs` are deleted; the live code is `multi_workspace.rs`, `panels/`, and `tab_bar.rs`.
- `crates/settings` is an empty placeholder directory, not a workspace member.

## Platform

Windows-first (Win10/11, MSVC). `platform_title_bar` and parts of `ui` carry Linux implementations; keep platform-specific code behind the existing `platforms/` split rather than inline `#[cfg]` in shared UI code.
