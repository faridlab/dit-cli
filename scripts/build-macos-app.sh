#!/bin/sh
# Assemble DIT.app (ADR 0029) from a built `dit` and `dit-tray`, and zip it.
#
#   scripts/build-macos-app.sh <dit> <dit-tray> <version> <out-dir>
#
# The binaries may be single-architecture or universal (lipo'd by the
# caller). The bundle is signed ad hoc — Apple Silicon runs nothing unsigned —
# and carries no Developer ID until the project has one; ADR 0029 says what
# that costs and how the Homebrew cask copes.
set -eu

if [ "$#" -ne 4 ]; then
  echo "usage: $0 <dit> <dit-tray> <version> <out-dir>" >&2
  exit 2
fi
DIT_BIN=$1
TRAY_BIN=$2
VERSION=$3
OUT=$4
ROOT=$(cd "$(dirname "$0")/.." && pwd)
LOGO="$ROOT/apps/web/src/assets/dit-logo.png"

mkdir -p "$OUT"
APP="$OUT/DIT.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$DIT_BIN" "$APP/Contents/MacOS/dit"
cp "$TRAY_BIN" "$APP/Contents/MacOS/dit-tray"
chmod 755 "$APP/Contents/MacOS/dit" "$APP/Contents/MacOS/dit-tray"

# LSUIElement: a menu bar app, no Dock icon. The minimum system is the one
# the Rust toolchain targets for aarch64-apple-darwin.
cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>DIT</string>
  <key>CFBundleDisplayName</key>
  <string>DIT</string>
  <key>CFBundleIdentifier</key>
  <string>dev.dit.tray</string>
  <key>CFBundleExecutable</key>
  <string>dit-tray</string>
  <key>CFBundleIconFile</key>
  <string>DIT</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$VERSION</string>
  <key>CFBundleVersion</key>
  <string>$VERSION</string>
  <key>LSMinimumSystemVersion</key>
  <string>11.0</string>
  <key>LSUIElement</key>
  <true/>
  <key>NSHumanReadableCopyright</key>
  <string>Apache-2.0</string>
</dict>
</plist>
EOF
plutil -lint "$APP/Contents/Info.plist" >/dev/null

# The Finder icon, from the same mark the web app uses.
ICONSET=$(mktemp -d)/DIT.iconset
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$LOGO" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  if [ "$double" -le 512 ]; then
    sips -z "$double" "$double" "$LOGO" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
  fi
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/DIT.icns"

# Sign the inner binaries, then the bundle — ad hoc, no identity.
codesign --force --sign - "$APP/Contents/MacOS/dit"
codesign --force --sign - "$APP/Contents/MacOS/dit-tray"
codesign --force --sign - "$APP"
codesign --verify --strict "$APP"

# ditto keeps the bundle's structure and signature intact in the zip.
ZIP="$OUT/DIT-macos.zip"
rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"
shasum -a 256 "$ZIP" | awk '{print $1 "  DIT-macos.zip"}' > "$ZIP.sha256"
echo "$ZIP"
