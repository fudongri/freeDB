#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

echo "=== FreeDB Windows Package Builder ==="

export PATH="$HOME/.cargo/bin:$PATH"

# ---- Pre-flight checks ----

if ! command -v makensis &>/dev/null; then
    echo "makensis not found. Install with: brew install makensis"
    exit 1
fi

echo "makensis: $(makensis -VERSION)"

# ---- Regenerate icon & header BMP ----

echo ""
echo "=== Regenerating icon assets ==="

magick apps/desktop/assets/freedb-icon-256.png \
    -define icon:auto-resize=256,128,64,48,32,16 \
    apps/desktop/assets/freedb-icon.ico

magick apps/desktop/assets/freedb-icon-256.png \
    -resize 150x57! -type TrueColor -depth 24 \
    -define bmp:format=bmp3 \
    apps/desktop/nsis/header.bmp

echo "  freedb-icon.ico regenerated"
echo "  header.bmp regenerated"

# ---- Cross-compile ----

echo ""
echo "=== Building FreeDB for Windows (release) ==="

cargo build \
    --package desktop \
    --bin freedb \
    --release \
    --target x86_64-pc-windows-gnu

EXE="target/x86_64-pc-windows-gnu/release/freedb.exe"
ls -lh "$EXE"

# ---- Build NSIS installer ----

echo ""
echo "=== Building NSIS installer ==="

rm -f target/FreeDB-*.exe

makensis apps/desktop/nsis/build_windows_installer.nsi

SETUP=$(echo target/FreeDB-*-setup.exe)

if [ -f "$SETUP" ]; then
    echo ""
    echo "=== NSIS installer ==="
    ls -lh "$SETUP"
else
    echo "ERROR: NSIS installer not found"
    exit 1
fi

# ---- Build ZIP portable version ----

echo ""
echo "=== Building ZIP portable package ==="

ZIP_DIR="target/FreeDB-portable"
VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
ZIP_FILE="$(pwd)/target/FreeDB-${VERSION}-x86_64-portable.zip"

rm -rf "$ZIP_DIR"
mkdir -p "$ZIP_DIR"

cp "$EXE" "$ZIP_DIR/freedb.exe"
cp apps/desktop/assets/freedb-icon.ico "$ZIP_DIR/"
cp apps/desktop/nsis/LICENSE.txt "$ZIP_DIR/"

# Create a simple README for portable users
cat > "$ZIP_DIR/README.txt" << 'EOF'
FreeDB - Portable Edition
=========================

How to use:
  1. Extract this ZIP to any folder (e.g. C:\Tools\FreeDB)
  2. Double-click freedb.exe to launch
  3. To uninstall, just delete the folder

Your data (connections, history) is stored at:
  %APPDATA%\freedb\

No registry changes. No installer. Just extract and run.
EOF

rm -f "$ZIP_FILE"
(cd "$ZIP_DIR" && zip -r "$ZIP_FILE" .)

ls -lh "$ZIP_FILE"

# ---- Output summary ----

echo ""
echo "========================================="
echo "  Build complete!"
echo "========================================="
echo ""
echo "  NSIS installer:  $SETUP ($(ls -lh "$SETUP" | awk '{print $5}'))"
echo "  Portable ZIP:    $ZIP_FILE ($(ls -lh "$ZIP_FILE" | awk '{print $5}'))"
echo ""
echo "  The ZIP version avoids antivirus false positives."
echo "  If Defender flags the NSIS installer, submit it at:"
echo "    https://www.microsoft.com/en-us/wdsi/filesubmission"
