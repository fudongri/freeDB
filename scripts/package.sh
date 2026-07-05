#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

DESKTOP_DIR="apps/desktop"
ASSETS_DIR="$DESKTOP_DIR/assets"
ICONSET_DIR="$ASSETS_DIR/freedb-icon.iconset"

echo "=== Generating .icns icon ==="
rm -rf "$ICONSET_DIR"
mkdir -p "$ICONSET_DIR"

for size in 16 32 128 256 512; do
  /usr/bin/sips -z "$size" "$size" "$ASSETS_DIR/freedb-icon-1024.png" \
    --out "$ICONSET_DIR/icon_${size}x${size}.png" >/dev/null
done
/usr/bin/sips -z 32 32 "$ASSETS_DIR/freedb-icon-1024.png" \
  --out "$ICONSET_DIR/icon_16x16@2x.png" >/dev/null
/usr/bin/sips -z 64 64 "$ASSETS_DIR/freedb-icon-1024.png" \
  --out "$ICONSET_DIR/icon_32x32@2x.png" >/dev/null
/usr/bin/sips -z 256 256 "$ASSETS_DIR/freedb-icon-1024.png" \
  --out "$ICONSET_DIR/icon_128x128@2x.png" >/dev/null
/usr/bin/sips -z 512 512 "$ASSETS_DIR/freedb-icon-1024.png" \
  --out "$ICONSET_DIR/icon_256x256@2x.png" >/dev/null
cp "$ASSETS_DIR/freedb-icon-1024.png" "$ICONSET_DIR/icon_512x512@2x.png"
/usr/bin/iconutil -c icns "$ICONSET_DIR" -o "$ASSETS_DIR/freedb-icon.icns"
rm -rf "$ICONSET_DIR"
echo "Icon generated: $ASSETS_DIR/freedb-icon.icns"

echo "=== Building macOS DMG ==="
~/.cargo/bin/cargo bundle -p desktop --bin freedb --release --format dmg

# cargo-bundle may not embed resources; rebuild DMG with icon injected
DMG="target/release/bundle/dmg/FreeDB.dmg"
TMP_DMG=$(mktemp -d)

# Mount original DMG and copy contents out
MOUNT_POINT=$(hdiutil attach "$DMG" -nobrowse -readonly | grep "/Volumes/" | sed 's/.*\(\/Volumes\/.*\)/\1/' | head -1)
cp -R "$MOUNT_POINT"/* "$TMP_DMG/"
hdiutil detach "$MOUNT_POINT" -quiet

# cargo-bundle 生成的 Info.plist 缺少 CFBundleIconFile，需要用源文件覆盖
APP_BUNDLE="$TMP_DMG/FreeDB.app"
cp "$DESKTOP_DIR/macos/Info.plist" "$APP_BUNDLE/Contents/Info.plist"

# 注入图标到 Contents/Resources/
RESOURCES_DIR="$APP_BUNDLE/Contents/Resources"
mkdir -p "$RESOURCES_DIR"
cp "$ASSETS_DIR/freedb-icon.icns" "$RESOURCES_DIR/freedb-icon.icns"
echo "Info.plist 替换完成，图标已注入"

# Re-sign after modifying bundle contents
codesign --force --sign - "$TMP_DMG/FreeDB.app" 2>/dev/null
echo "Re-signed after icon injection"

# Rebuild DMG
rm -f "$DMG"
hdiutil create -volname "FreeDB" -srcfolder "$TMP_DMG" -format UDZO -imagekey zlib-level=9 "$DMG" >/dev/null
rm -rf "$TMP_DMG"
echo "DMG rebuilt with icon"

echo ""
echo "Done: ${DMG}"
ls -lh "${DMG}"
