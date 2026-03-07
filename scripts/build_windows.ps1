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

# 确保使用最新 PATH（如刚安装 WinLibs 的 windres）
$env:Path = [System.Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path","User")

# 若存在 windres 且已安装 GNU 工具链，则用 GNU 构建（可嵌入 exe 图标）；否则用默认 MSVC
$UseGnu = $false
$windresExe = Get-Command windres -ErrorAction SilentlyContinue
$gnuToolchain = "stable-x86_64-pc-windows-gnu"
if ($windresExe -and (rustup toolchain list 2>$null) -match $gnuToolchain) {
    $UseGnu = $true
    Write-Host "[build_windows] Using GNU toolchain (windres) for exe icon embedding." -ForegroundColor Cyan
}

# 从 Cargo 读取版本号（cargo pkgid 优先，失败时解析 Cargo.toml，再失败用 0.1.4）
$version = "0.1.4"
try {
    $pkgid = cargo pkgid -p deckref 2>$null
    if ($pkgid -match '#deckref@(.+)$') {
        $version = $Matches[1].Trim()
    } else {
        $cargoPath = Join-Path $ProjectRoot "Cargo.toml"
        $content = [System.IO.File]::ReadAllText($cargoPath)
        if ($content -match 'version\s*=\s*"([^"]+)"') {
            $version = $Matches[1].Trim()
        }
    }
} catch {
    Write-Host "[build_windows] Using default version 0.1.4" -ForegroundColor Yellow
}
$OutDirName = "DeckRef-$version"
$OutDir = Join-Path $ProjectRoot $OutDirName
$ExeName = "deckref.exe"
$TargetTriple = if ($UseGnu) { "x86_64-pc-windows-gnu" } else { $null }
$ExePath = if ($TargetTriple) {
    Join-Path (Join-Path $ProjectRoot "target\$TargetTriple\release") $ExeName
} else {
    Join-Path (Join-Path $ProjectRoot "target\release") $ExeName
}

Write-Host "[build_windows] Version: $version" -ForegroundColor Cyan
Write-Host "[build_windows] Building release binary..." -ForegroundColor Cyan
if ($TargetTriple) {
    cargo build --release --target $TargetTriple
} else {
    cargo build --release
}
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

$IcoSrc = Join-Path $ProjectRoot "wix\DeckRef.ico"
if (Test-Path $IcoSrc) {
    Copy-Item -Path $IcoSrc -Destination (Join-Path $OutDir "DeckRef.ico") -Force
    Write-Host "[build_windows] Copied DeckRef.ico for MSI" -ForegroundColor Green
}

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
$WixPath = $null
if ($env:WIX -and (Test-Path (Join-Path $env:WIX "candle.exe"))) {
    $WixPath = $env:WIX
}
if (-not $WixPath) {
    $candidates = @(
        "${env:ProgramFiles(x86)}\WiX Toolset v3.11\bin",
        "${env:ProgramFiles}\WiX Toolset v3.11\bin",
        "${env:ProgramFiles(x86)}\WiX Toolset v3.14\bin",
        "${env:ProgramFiles}\WiX Toolset v3.14\bin"
    )
    foreach ($p in $candidates) {
        if ($p -and (Test-Path (Join-Path $p "candle.exe"))) {
            $WixPath = $p
            break
        }
    }
}
if (-not $WixPath) {
    $candleCmd = Get-Command candle -ErrorAction SilentlyContinue
    if ($candleCmd) { $WixPath = Split-Path -Parent $candleCmd.Source }
}
$Candle = $null
$Light = $null
if ($WixPath) {
    $Candle = Join-Path $WixPath "candle.exe"
    $Light = Join-Path $WixPath "light.exe"
}
$WxsPath = Join-Path $ProjectRoot "wix\DeckRef.wxs"

if ($Candle -and $Light -and (Test-Path $Candle) -and (Test-Path $Light) -and (Test-Path $WxsPath)) {
    $SourceDir = $OutDir -replace '\\', '\\'
    $WixObjDir = Join-Path $ProjectRoot "wix\obj"
    $WixOutDir = Join-Path $ProjectRoot "wix\out"
    if (-not (Test-Path $WixObjDir)) { New-Item -ItemType Directory -Path $WixObjDir -Force | Out-Null }
    if (-not (Test-Path $WixOutDir)) { New-Item -ItemType Directory -Path $WixOutDir -Force | Out-Null }
    $versionWix = $version
    if (($versionWix -split '\.').Count -lt 4) { $versionWix = "$version.0" }
    Write-Host "[build_windows] Building MSI (Version=$versionWix)..." -ForegroundColor Cyan
    & $Candle -nologo -out (Join-Path $WixObjDir "DeckRef.wixobj") "-dVersion=$versionWix" "-dSourceDir=$OutDir" $WxsPath
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
