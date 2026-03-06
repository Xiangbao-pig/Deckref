# DeckRef Windows 打包脚本
# 在项目根目录执行: .\scripts\build_windows.ps1
# 产物: DeckRef-<version>\ 文件夹（含 exe、说明、卸载脚本、MSI 安装包）

$ErrorActionPreference = "Stop"
if ($PSCommandPath) {
    $ProjectRoot = Split-Path -Parent (Split-Path -Parent $PSCommandPath)
} else {
    $ProjectRoot = Split-Path -Parent $PSScriptRoot
}
if (-not $ProjectRoot -or -not (Test-Path (Join-Path $ProjectRoot "Cargo.toml"))) {
    $ProjectRoot = (Get-Location).Path
}
if (-not (Test-Path (Join-Path $ProjectRoot "Cargo.toml"))) {
    Write-Host "[build_windows] ERROR: Run from project root or place script in scripts\ folder." -ForegroundColor Red
    exit 1
}
Set-Location $ProjectRoot

# 从 Cargo.toml 读取版本号（失败时用 0.1.4）
$cargoPath = Join-Path $ProjectRoot "Cargo.toml"
$version = "0.1.4"
try {
    $content = Get-Content -LiteralPath $cargoPath -Raw -ErrorAction Stop
    if ($content -match 'version\s*=\s*"([^"]+)"') {
        $version = $Matches[1].Trim()
    }
} catch {
    Write-Host "[build_windows] Using default version 0.1.4" -ForegroundColor Yellow
}
$OutDirName = "DeckRef-$version"
$OutDir = Join-Path $ProjectRoot $OutDirName
$ExeName = "deckref.exe"
$ExePath = Join-Path (Join-Path $ProjectRoot "target\release") $ExeName

Write-Host "[build_windows] Version: $version" -ForegroundColor Cyan
Write-Host "[build_windows] Building release binary..." -ForegroundColor Cyan
cargo build --release
if (-not (Test-Path $ExePath)) {
    Write-Host "[build_windows] ERROR: $ExePath not found." -ForegroundColor Red
    exit 1
}

$BinSize = (Get-Item $ExePath).Length
$MinSize = 2 * 1024 * 1024
if ($BinSize -lt $MinSize) {
    Write-Host "[build_windows] WARNING: Binary size $([math]::Round($BinSize/1KB)) KB (< 2 MB)." -ForegroundColor Yellow
}
Write-Host "[build_windows] Binary size: $([math]::Round($BinSize/1KB)) KB" -ForegroundColor Green

if (Test-Path $OutDir) {
    Remove-Item -Path $OutDir -Recurse -Force
}
New-Item -ItemType Directory -Path $OutDir | Out-Null
Write-Host "[build_windows] Created folder: $OutDirName" -ForegroundColor Cyan

Copy-Item -Path $ExePath -Destination (Join-Path $OutDir $ExeName) -Force
Write-Host "[build_windows] Copied $ExeName" -ForegroundColor Green

$AssetsSrc = Join-Path $ProjectRoot "assets"
if (Test-Path $AssetsSrc) {
    $AssetsDst = Join-Path $OutDir "assets"
    New-Item -ItemType Directory -Path $AssetsDst -Force | Out-Null
    Copy-Item -Path (Join-Path $AssetsSrc "*") -Destination $AssetsDst -Recurse -Force
    Write-Host "[build_windows] Copied assets" -ForegroundColor Green
}

$ReadmeSrc = Join-Path $ProjectRoot "scripts\install_readme_win.txt"
if (Test-Path $ReadmeSrc) {
    Copy-Item -Path $ReadmeSrc -Destination (Join-Path $OutDir "README.txt") -Force
    Write-Host "[build_windows] Wrote README.txt" -ForegroundColor Green
}

$UninstallSrc = Join-Path $ProjectRoot "scripts\uninstall_win.bat"
if (Test-Path $UninstallSrc) {
    Copy-Item -Path $UninstallSrc -Destination (Join-Path $OutDir "Uninstall.bat") -Force
    Write-Host "[build_windows] Wrote Uninstall.bat" -ForegroundColor Green
}

$ConfigReadmeSrc = Join-Path $ProjectRoot "scripts\config_readme_win.txt"
if (Test-Path $ConfigReadmeSrc) {
    Copy-Item -Path $ConfigReadmeSrc -Destination (Join-Path $OutDir "config_readme.txt") -Force
    Write-Host "[build_windows] Wrote config_readme.txt" -ForegroundColor Green
}

# 生成 MSI（需已安装 WiX Toolset）
$WixPath = $env:WIX
if (-not $WixPath) {
    $WixPath = "${env:ProgramFiles(x86)}\WiX Toolset v3.11\bin"
    if (-not (Test-Path $WixPath)) {
        $WixPath = "${env:ProgramFiles}\WiX Toolset v3.11\bin"
    }
}
$Candle = Join-Path $WixPath "candle.exe"
$Light = Join-Path $WixPath "light.exe"
$WxsPath = Join-Path $ProjectRoot "wix\DeckRef.wxs"

if ((Test-Path $Candle) -and (Test-Path $Light) -and (Test-Path $WxsPath)) {
    $SourceDir = $OutDir -replace '\\', '\\'
    $WixObjDir = Join-Path $ProjectRoot "wix\obj"
    $WixOutDir = Join-Path $ProjectRoot "wix\out"
    if (-not (Test-Path $WixObjDir)) { New-Item -ItemType Directory -Path $WixObjDir -Force | Out-Null }
    if (-not (Test-Path $WixOutDir)) { New-Item -ItemType Directory -Path $WixOutDir -Force | Out-Null }
    Write-Host "[build_windows] Building MSI..." -ForegroundColor Cyan
    & $Candle -nologo -out (Join-Path $WixObjDir "DeckRef.wixobj") -dVersion=$version -dSourceDir=$OutDir $WxsPath
    if ($LASTEXITCODE -eq 0) {
        $MsiPath = Join-Path $OutDir "DeckRef-$version.msi"
        & $Light -nologo -out $MsiPath -b $OutDir (Join-Path $WixObjDir "DeckRef.wixobj") -cultures:2052
        if ($LASTEXITCODE -eq 0) {
            Write-Host "[build_windows] MSI created: $OutDirName\DeckRef-$version.msi" -ForegroundColor Green
        } else {
            Write-Host "[build_windows] WARNING: light failed, MSI not created." -ForegroundColor Yellow
        }
    } else {
        Write-Host "[build_windows] WARNING: candle failed, skip MSI. Install WiX Toolset v3.11 if needed." -ForegroundColor Yellow
    }
} else {
    Write-Host "[build_windows] WiX not found, skipping MSI. Install WiX Toolset v3.11 for one-click MSI." -ForegroundColor Yellow
}

Write-Host "[build_windows] Done. Output: $OutDir" -ForegroundColor Cyan
