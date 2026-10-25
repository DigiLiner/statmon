param(
    [string]$Version = "0.1.0",
    [switch]$Publish
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

Write-Host "Building release..." -ForegroundColor Cyan
cargo build --release

New-Item -ItemType Directory -Force dist | Out-Null
Copy-Item target\release\statmon.exe dist\ -Force
Copy-Item README.md, LICENSE dist\ -Force

$zip = "dist\statmon-$Version-win64.zip"
Compress-Archive -Path dist\* -DestinationPath $zip -Force
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash
Write-Host "Created $zip" -ForegroundColor Green
Write-Host "SHA256: $hash"

if (-not $Publish) {
    Write-Host ""
    Write-Host "Publish with:" -ForegroundColor Yellow
    Write-Host "  gh release create v$Version `"$zip`" --title `"StatMon $Version`" --generate-notes"
    Write-Host "or upload the zip at https://github.com/DigiLiner/statmon/releases/new"
    exit 0
}

if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    Write-Host "GitHub CLI (gh) not found. Install it with:" -ForegroundColor Red
    Write-Host "  winget install --id GitHub.cli"
    Write-Host "Then run: gh auth login"
    exit 1
}

gh release create "v$Version" $zip --title "StatMon $Version" --generate-notes
