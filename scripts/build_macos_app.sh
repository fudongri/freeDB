#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
DESKTOP_DIR="$ROOT_DIR/apps/desktop"
ASSETS_DIR="$DESKTOP_DIR/assets"
ICONSET_DIR="$ASSETS_DIR/freedb-icon.iconset"
APP_DIR="$ROOT_DIR/target/debug/freedb.app"
MACOS_DIR="$APP_DIR/Contents/MacOS"
RESOURCES_DIR="$APP_DIR/Contents/Resources"

cd "$ROOT_DIR"

/Users/fdr/.cargo/bin/cargo run -p desktop --bin export-icon

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

/Users/fdr/.cargo/bin/cargo build -p desktop --bin freedb

rm -rf "$APP_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"
cp "$ROOT_DIR/target/debug/freedb" "$MACOS_DIR/freedb"
cp "$DESKTOP_DIR/macos/Info.plist" "$APP_DIR/Contents/Info.plist"
cp "$ASSETS_DIR/freedb-icon.icns" "$RESOURCES_DIR/freedb-icon.icns"
chmod +x "$MACOS_DIR/freedb"

echo "App bundle created at: $APP_DIR"
