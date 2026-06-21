# FreeDB Windows MSI Build Script
# Requirements:
#   - Rust toolchain with x86_64-pc-windows-msvc target
#   - cargo-wix: cargo install cargo-wix
#   - WiX Toolset v4: https://wixtoolset.org/

param(
    [string]$Configuration = "release"
)

$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))

Write-Host "=== FreeDB Windows MSI Builder ===" -ForegroundColor Cyan

# ---- Pre-flight checks ----

if (-not (Get-Command "cargo" -ErrorAction SilentlyContinue)) {
    Write-Error "cargo not found. Install Rust from https://rustup.rs/"
    exit 1
}

if (-not (cargo wix --version 2>$null)) {
    Write-Error "cargo-wix not found. Install it with: cargo install cargo-wix"
    exit 1
}

$wixToolset = Get-ChildItem -Path "C:\Program Files*\WiX Toolset v*" -Directory -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $wixToolset) {
    Write-Error "WiX Toolset not found. Install from https://wixtoolset.org/"
    exit 1
}

$env:WIX = $wixToolset.FullName
$env:PATH = "$env:WIX\bin;$env:PATH"

Write-Host "WiX Toolset: $env:WIX" -ForegroundColor Green
Write-Host "cargo-wix version: $(cargo wix --version)" -ForegroundColor Green

# ---- Build ----

$msvcTarget = "x86_64-pc-windows-msvc"

Write-Host ""
Write-Host "=== Building FreeDB ($Configuration, $msvcTarget) ===" -ForegroundColor Cyan

cargo wix `
    --package desktop `
    --bin freedb `
    --$Configuration `
    --target $msvcTarget `
    --output "target/wix/FreeDB-0.1.0-x86_64.msi"

# ---- Output ----

$msiPath = "target/wix/FreeDB-0.1.0-x86_64.msi"

if (Test-Path $msiPath) {
    $size = [math]::Round((Get-Item $msiPath).Length / 1MB, 2)
    Write-Host ""
    Write-Host "=== Done ===" -ForegroundColor Green
    Write-Host "MSI: $((Get-Item $msiPath).FullName)" -ForegroundColor Green
    Write-Host "Size: ${size} MB" -ForegroundColor Green
} else {
    Write-Error "MSI not found at expected path: $msiPath"
    Write-Host "Check cargo build output for errors." -ForegroundColor Yellow
    exit 1
}
