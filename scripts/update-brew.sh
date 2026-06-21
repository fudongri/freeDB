#!/usr/bin/env bash
set -euo pipefail

echo "=== Update Homebrew Cask ==="

VERSION=$(gh release view --repo fudongri/freeDB --json tagName -q '.tagName')
echo "Latest release: $VERSION"

DMG_URL="https://github.com/fudongri/freeDB/releases/download/${VERSION}/FreeDB.dmg"
echo "Downloading DMG..."
TMP_DMG=$(mktemp)
curl -sL "$DMG_URL" -o "$TMP_DMG"
SHA256=$(shasum -a 256 "$TMP_DMG" | awk '{print $1}')
rm "$TMP_DMG"
echo "SHA256: $SHA256"

TAP_DIR="/tmp/homebrew-tap"
rm -rf "$TAP_DIR"
gh repo clone fudongri/homebrew-tap "$TAP_DIR" -- --depth 1

cat > "$TAP_DIR/Casks/freedb.rb" << EOF
cask "freedb" do
  version "${VERSION}"
  sha256 "${SHA256}"

  url "https://github.com/fudongri/freeDB/releases/download/#{version}/FreeDB.dmg"
  name "FreeDB"
  desc "Lightweight cross-platform database client for MySQL & PostgreSQL"
  homepage "https://github.com/fudongri/freeDB"

  app "FreeDB.app"

  postflight do
    system_command "/usr/bin/xattr", args: ["-cr", "#{staged_path}/FreeDB.app"]
  end

  zap trash: [
    "~/Library/Application Support/freedb",
    "~/Library/Saved Application State/com.freedb.desktop.savedState",
  ]
end
EOF

cd "$TAP_DIR"
git add .
git commit -m "update freedb to ${VERSION}"
git push

echo ""
echo "Done! Cask updated to ${VERSION}"
