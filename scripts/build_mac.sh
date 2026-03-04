#!/usr/bin/env bash
# 在 macOS 上运行此脚本：编译 Release 并打包成 DeckRef.app，最后打成 DeckRef-mac.zip。
# 使用：在项目根目录执行 ./scripts/build_mac.sh
# 产物：DeckRef-mac.zip，解压后双击 DeckRef.app 即可运行。

set -e
cd "$(dirname "$0")/.."
ROOT="$PWD"

echo "[build_mac] Building release binary..."
cargo build --release

BIN="$ROOT/target/release/deckref"
if [[ ! -f "$BIN" ]]; then
  echo "[build_mac] ERROR: Binary not found at $BIN"
  echo "  This script must be run on macOS. On Windows you get deckref.exe, not deckref."
  exit 1
fi

BIN_SIZE=$(stat -f%z "$BIN" 2>/dev/null || stat -c%s "$BIN" 2>/dev/null || echo 0)
MIN_SIZE=$((2 * 1024 * 1024))
if [[ "$BIN_SIZE" -lt "$MIN_SIZE" ]]; then
  echo "[build_mac] WARNING: Binary size is ${BIN_SIZE} bytes (< 2 MB). Expected ~5–15 MB for DeckRef."
  echo "  The zip may not run correctly. Check that 'cargo build --release' completed successfully."
fi
echo "[build_mac] Binary size: $(( BIN_SIZE / 1024 )) KB"

APP="$ROOT/DeckRef.app"
CONTENTS="$APP/Contents"
MACOS="$CONTENTS/MacOS"
rm -rf "$APP"
mkdir -p "$MACOS"

echo "[build_mac] Copying binary into .app..."
cp "$BIN" "$MACOS/deckref"
chmod +x "$MACOS/deckref"

if [[ -d "$ROOT/assets" ]]; then
  echo "[build_mac] Copying assets (e.g. icon)..."
  cp -R "$ROOT/assets" "$MACOS/"
fi

echo "[build_mac] Writing Info.plist..."
cat > "$CONTENTS/Info.plist" << 'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>deckref</string>
  <key>CFBundleIdentifier</key>
  <string>com.deckref.app</string>
  <key>CFBundleName</key>
  <string>DeckRef</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.1</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

ZIP="$ROOT/DeckRef-mac.zip"
echo "[build_mac] Creating $ZIP..."
rm -f "$ZIP"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$ZIP"

ZIP_SIZE=$(stat -f%z "$ZIP" 2>/dev/null || stat -c%s "$ZIP" 2>/dev/null || echo 0)
echo "[build_mac] Done. Output: $ZIP ($(( ZIP_SIZE / 1024 / 1024 )) MB)"
if [[ "$ZIP_SIZE" -lt "$MIN_SIZE" ]]; then
  echo "[build_mac] WARNING: Zip is very small ($(( ZIP_SIZE / 1024 )) KB). Expected roughly 5–15 MB. Unzip and check DeckRef.app/Contents/MacOS/deckref exists and is several MB."
fi
echo "  Unzip and double-click DeckRef.app to run."
