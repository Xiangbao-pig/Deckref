# DeckRef Windows 打包脚本 (v0.1.1)
# 在项目根目录执行: .\scripts\build_windows.ps1
# 产物: ANzhuang\ 文件夹，内含 deckref.exe，可直接运行或整体拷贝给他人使用。

$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not (Test-Path (Join-Path $ProjectRoot "Cargo.toml"))) {
    $ProjectRoot = (Get-Location).Path
}
Set-Location $ProjectRoot

$OutDir = Join-Path $ProjectRoot "ANzhuang"
$ExeName = "deckref.exe"
$ExePath = Join-Path (Join-Path $ProjectRoot "target\release") $ExeName

Write-Host "[build_windows] Building release binary..." -ForegroundColor Cyan
cargo build --release
if (-not (Test-Path $ExePath)) {
    Write-Host "[build_windows] ERROR: $ExePath not found." -ForegroundColor Red
    exit 1
}

$BinSize = (Get-Item $ExePath).Length
$MinSize = 2 * 1024 * 1024
if ($BinSize -lt $MinSize) {
    Write-Host "[build_windows] WARNING: Binary size $([math]::Round($BinSize/1KB)) KB (< 2 MB). Expected ~5-15 MB." -ForegroundColor Yellow
}
Write-Host "[build_windows] Binary size: $([math]::Round($BinSize/1KB)) KB" -ForegroundColor Green

if (-not (Test-Path $OutDir)) {
    New-Item -ItemType Directory -Path $OutDir | Out-Null
    Write-Host "[build_windows] Created folder: ANzhuang" -ForegroundColor Cyan
}

Copy-Item -Path $ExePath -Destination (Join-Path $OutDir $ExeName) -Force
Write-Host "[build_windows] Copied $ExeName to ANzhuang\" -ForegroundColor Green

$AssetsSrc = Join-Path $ProjectRoot "assets"
if (Test-Path $AssetsSrc) {
    $AssetsDst = Join-Path $OutDir "assets"
    if (-not (Test-Path $AssetsDst)) { New-Item -ItemType Directory -Path $AssetsDst | Out-Null }
    Copy-Item -Path (Join-Path $AssetsSrc "*") -Destination $AssetsDst -Recurse -Force
    Write-Host "[build_windows] Copied assets (e.g. icon) to ANzhuang\assets\" -ForegroundColor Green
}

$ReadmePath = Join-Path $OutDir "使用说明.txt"
$ReadmeSrc = Join-Path $ProjectRoot "scripts\install_readme_win.txt"
if (Test-Path $ReadmeSrc) {
    Copy-Item -Path $ReadmeSrc -Destination $ReadmePath -Force
    Write-Host "[build_windows] Wrote ANzhuang\使用说明.txt" -ForegroundColor Green
} else {
    Set-Content -Path $ReadmePath -Value "DeckRef v0.1.1 - Run deckref.exe" -Encoding UTF8
    Write-Host "[build_windows] Wrote ANzhuang\使用说明.txt (fallback)" -ForegroundColor Yellow
}

$UninstallSrc = Join-Path $ProjectRoot "scripts\uninstall_win.bat"
if (Test-Path $UninstallSrc) {
    Copy-Item -Path $UninstallSrc -Destination (Join-Path $OutDir "卸载.bat") -Force
    Write-Host "[build_windows] Wrote ANzhuang\卸载.bat" -ForegroundColor Green
}

$ConfigReadmeSrc = Join-Path $ProjectRoot "scripts\config_readme_win.txt"
if (Test-Path $ConfigReadmeSrc) {
    $ConfigReadmeDst = Join-Path $OutDir "config_readme.txt"
    Copy-Item -Path $ConfigReadmeSrc -Destination $ConfigReadmeDst -Force
    Write-Host "[build_windows] Wrote ANzhuang\config_readme.txt" -ForegroundColor Green
}

Write-Host "[build_windows] Done. Install files are in: $OutDir" -ForegroundColor Cyan
