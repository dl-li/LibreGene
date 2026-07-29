# 3-verify-and-build.ps1
# Verify the Rust + MSVC toolchain, then run a Rust compile-check on
# libregene-core. Run this after 1-install-rust.ps1 and 2-install-vs-buildtools.ps1.
#
# Usage (PowerShell, any working directory):
#   powershell -ExecutionPolicy Bypass -File scripts\setup-windows\3-verify-and-build.ps1

$ErrorActionPreference = 'Stop'

# Resolve the repo root from this script's location (scripts/setup-windows/ -> ../..).
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $ScriptDir '..\..')
$Backend = Join-Path $RepoRoot 'backend'
Write-Host "Repo root: $RepoRoot" -ForegroundColor Cyan

if (-not (Test-Path (Join-Path $Backend 'Cargo.toml'))) {
    throw "backend/Cargo.toml not found under $RepoRoot — run this script from inside the LibreGene checkout."
}

Write-Host "`n=== [1/4] Verify Rust ===" -ForegroundColor Cyan
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (-not ($env:Path -split ';' -contains $cargoBin)) {
    $env:Path = "$cargoBin;$env:Path"
}
& cargo --version
if ($LASTEXITCODE -ne 0) {
    throw "cargo unavailable — run 1-install-rust.ps1 first, then open a NEW shell so PATH is refreshed."
}

Write-Host "`n=== [2/4] Verify MSVC toolchain ===" -ForegroundColor Cyan
$vsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path $vsWhere)) {
    throw "vswhere missing — VS Build Tools not installed. Run 2-install-vs-buildtools.ps1 first."
}
$msvcPath = & $vsWhere -latest -products * `
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
    -property installationPath 2>$null
if (-not $msvcPath) {
    throw "MSVC C++ toolchain not detected — rerun 2-install-vs-buildtools.ps1 (a reboot may be required if it returned exit 3010)."
}
Write-Host "MSVC path: $msvcPath" -ForegroundColor Green
Write-Host "Note: cargo finds MSVC automatically via the 'cc' crate — no need to load vcvars manually." -ForegroundColor DarkGray

Write-Host "`n=== [3/4] cargo check on libregene-core ===" -ForegroundColor Cyan
Write-Host "(First run downloads ~400 crates, ~5-10 min. Progress prints to stderr.)"
Set-Location $Backend
& cargo check -p libregene-core
if ($LASTEXITCODE -ne 0) {
    Write-Host "`nlibregene-core cargo check FAILED (exit $LASTEXITCODE)." -ForegroundColor Red
    Write-Host "If this is a pure Rust compile error, report it to the project. If MSVC is reported missing, rerun 2-install-vs-buildtools.ps1." -ForegroundColor Yellow
    exit $LASTEXITCODE
}
Write-Host "libregene-core compiles cleanly." -ForegroundColor Green

Write-Host "`n=== [4/4] Toolchain ready — next steps ===" -ForegroundColor Cyan
Write-Host "Frontend deps (first time):  npm install --no-audit --no-fund"
Write-Host "Full release build (.exe):   npx tauri build"
Write-Host "Dev mode (hot reload):       npx tauri dev"
Write-Host "`nTip: if cargo progress looks frozen in this window, it is just PowerShell buffering the progress lines — the build is still running." -ForegroundColor DarkGray
