# package.ps1 - assemble a release directory for Bit7zFM.
#
# Requires: cargo (offline cache), the Windows SDK (mt.exe), and a vcpkg
# checkout providing bit7z64.lib + 7zip.lib + 7zip.dll (see README).
#
# Usage:  powershell -ExecutionPolicy Bypass -File scripts/package.ps1
# Output: dist\Bit7zFM-<version>\  (exe + dll + shell plugin)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# ---------------------------------------------------------------------------
# 1. Release build
# ---------------------------------------------------------------------------
Write-Host '[1/5] cargo build --release (offline)...'
cargo build --release --offline
if ($LASTEXITCODE -ne 0) { throw 'release build failed' }

# ---------------------------------------------------------------------------
# 2. Embed DPI manifest into the exes (icon + version come from .res)
# ---------------------------------------------------------------------------
$mt = Get-ChildItem 'C:/Program Files (x86)/Windows Kits/10/bin' -Recurse -Filter mt.exe -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending | Select-Object -First 1 -ExpandProperty FullName
if (-not $mt) { throw 'mt.exe not found (Windows SDK required)' }

$manifest = Join-Path $root 'crates/resources/app.manifest'
foreach ($exe in @('bit7zfm.exe', 'bit7z-executor.exe')) {
    $path = Join-Path $root "target/release/$exe"
    if (Test-Path $path) {
        Write-Host "[2/5] embedding manifest into $exe"
        & $mt -manifest $manifest "-outputresource:$path;1" | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "mt.exe failed for $exe" }
    }
}

# ---------------------------------------------------------------------------
# 3. Locate the 7-Zip runtime DLL (vcpkg or next to the exe)
# ---------------------------------------------------------------------------
$sevenZip = $null
foreach ($candidate in @(
    (Join-Path $root 'target/release/7zip.dll'),
    (Join-Path $root 'target/release/7z.dll'),
    (Join-Path $env:VCPKG_ROOT 'installed/x64-windows/bin/7zip.dll'),
    (Join-Path $env:VCPKG_ROOT 'installed/x64-windows/bin/7z.dll')
)) {
    if ($candidate -and (Test-Path $candidate)) { $sevenZip = $candidate; break }
}
if (-not $sevenZip) { throw '7zip.dll/7z.dll not found (set VCPKG_ROOT or copy it next to target/release)' }

# ---------------------------------------------------------------------------
# 4. Assemble dist directory
# ---------------------------------------------------------------------------
$version = '0.1.0'
$dist = Join-Path $root "dist/Bit7zFM-$version"
New-Item -ItemType Directory -Force -Path $dist | Out-Null

Copy-Item (Join-Path $root 'target/release/bit7zfm.exe') $dist
Copy-Item (Join-Path $root 'target/release/bit7z-executor.exe') $dist
Copy-Item (Join-Path $root 'target/release/shell.dll') $dist
Copy-Item $sevenZip $dist
Copy-Item (Join-Path $root 'crates/resources/bit7z.ico') $dist

Write-Host '[3/5] dist assembled'
Get-ChildItem $dist | ForEach-Object { Write-Host ("    " + $_.Name + "  " + $_.Length + " bytes") }

# ---------------------------------------------------------------------------
# 5. Register the shell extension (HKCU, no admin)
# ---------------------------------------------------------------------------
Write-Host '[4/5] registering shell context menu...'
& (Join-Path $dist 'bit7z.exe') shell-install 2>$null
if (Test-Path (Join-Path $dist 'bit7z.exe')) {
    & (Join-Path $dist 'bit7z.exe') shell-install
} else {
    Write-Host '    (bit7z.exe not shipped; run "cargo run -p cli -- shell-install" to register)'
}

Write-Host '[5/5] done:'
Write-Host ("    " + $dist)
