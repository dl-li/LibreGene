# 1-install-rust.ps1
# Install Rust (MSVC host) for LibreGene Windows build.
# Usage: right-click -> Run with PowerShell (needs admin to add PATH).

$ErrorActionPreference = 'Stop'

Write-Host "=== [1/4] Check existing Rust ===" -ForegroundColor Cyan
$rustOk = $false
try {
    $rustVer = & cargo --version 2>$null
    if ($LASTEXITCODE -eq 0) {
        Write-Host "Rust already installed: $rustVer" -ForegroundColor Green
        $rustOk = $true
    }
} catch { }
if (-not $rustOk) {
    Write-Host "Rust not found, downloading rustup-init.exe ..." -ForegroundColor Yellow

    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $url = "https://win.rustup.rs/x86_64"
    $installer = Join-Path $env:TEMP 'rustup-init.exe'
    Write-Host "Downloading: $url"
    Invoke-WebRequest -Uri $url -OutFile $installer -UseBasicParsing

    Write-Host "=== [2/4] Run rustup-init ===" -ForegroundColor Cyan
    $args = @(
        '-y',
        '--default-toolchain', 'stable',
        '--default-host', 'x86_64-pc-windows-msvc',
        '--profile', 'default'
    )
    & $installer @args
    if ($LASTEXITCODE -ne 0) { throw "rustup-init failed (exit $LASTEXITCODE)" }
}

Write-Host "=== [3/4] Refresh PATH for this session ===" -ForegroundColor Cyan
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (-not ($env:Path -split ';' -contains $cargoBin)) {
    $env:Path = "$cargoBin;$env:Path"
}

Write-Host "=== [4/4] Verify ===" -ForegroundColor Cyan
& cargo --version
& rustc --version
& rustup target list --installed

if ($rustOk) {
    Write-Host "`nRust was already installed, nothing to do." -ForegroundColor Green
} else {
    Write-Host "`nRust install complete." -ForegroundColor Green
}
Write-Host "Next step: run 2-install-vs-buildtools.ps1 for MSVC toolchain." -ForegroundColor Cyan
