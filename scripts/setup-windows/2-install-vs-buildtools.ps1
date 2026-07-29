# 2-install-vs-buildtools.ps1
# Install VS 2022 Build Tools (C++ desktop + Windows 11 SDK).
# Largest piece of the toolchain (~3-4GB).
# Usage: right-click -> Run with PowerShell (MUST be admin).
# Silent install using .vsconfig.

$ErrorActionPreference = 'Stop'

Write-Host "=== Check MSVC cl.exe ===" -ForegroundColor Cyan
$vsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$needInstall = $true
if (Test-Path $vsWhere) {
    $msvc = & $vsWhere -latest -products * `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -property installationPath 2>$null
    if ($msvc) {
        Write-Host "MSVC toolchain already present: $msvc" -ForegroundColor Green
        Write-Host "Skipping install." -ForegroundColor Green
        $needInstall = $false
    }
}

if ($needInstall) {
    Write-Host "MSVC not found, downloading VS Build Tools bootstrapper..." -ForegroundColor Yellow
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

    $bootstrapper = Join-Path $env:TEMP 'vs_BuildTools.exe'
    $url = 'https://aka.ms/vs/17/release/vs_BuildTools.exe'
    Write-Host "Downloading: $url (4MB bootstrapper, then ~3-4GB of components)"
    Invoke-WebRequest -Uri $url -OutFile $bootstrapper -UseBasicParsing

    $vsconfig = Join-Path $PSScriptRoot '.vsconfig'
    Write-Host "=== Silent install (using $vsconfig) ===" -ForegroundColor Cyan
    Write-Host "This takes a long time (20-40 min). Do NOT close the window."

    $installPath = "${env:ProgramFiles}\Microsoft Visual Studio\2022\BuildTools"
    $args = @(
        '--quiet', '--wait', '--norestart',
        '--config', $vsconfig,
        '--installPath', $installPath
    )
    & $bootstrapper @args

    $exitCode = $LASTEXITCODE
    if ($exitCode -eq 0) {
        Write-Host "VS Build Tools install succeeded." -ForegroundColor Green
    } elseif ($exitCode -eq 3010) {
        Write-Host "VS Build Tools install succeeded, but a Windows restart is required." -ForegroundColor Yellow
    } else {
        Write-Host "VS Build Tools install FAILED, exit code: $exitCode" -ForegroundColor Red
        Write-Host "Try running manually: $bootstrapper"
        exit $exitCode
    }
}

Write-Host "`n=== Final verification ===" -ForegroundColor Cyan
if (Test-Path $vsWhere) {
    $msvcPath = & $vsWhere -latest -products * `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -property installationPath 2>$null
    if ($msvcPath) {
        Write-Host "MSVC install path: $msvcPath" -ForegroundColor Green
    }
}

Write-Host "`nToolchain install complete!" -ForegroundColor Green
Write-Host "Next step: run 3-verify-and-build.ps1 to build LibreGene." -ForegroundColor Cyan
