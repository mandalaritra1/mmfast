#!/bin/bash
# Build mmfast.app (release) into dist/. Drag it to /Applications to install.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release
APP=dist/mmfast.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
BIN="${CARGO_TARGET_DIR:-target}/release/mmfast"
cp "$BIN" "$APP/Contents/MacOS/mmfast"
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
# The app icon, drawn by the app itself (src/icon.rs).
ICONSET=$(mktemp -d)/mmfast.iconset
"$BIN" --export-icon "$ICONSET"
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/mmfast.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>mmfast</string>
  <key>CFBundleDisplayName</key><string>mmfast</string>
  <key>CFBundleIdentifier</key><string>dev.mmfast.mmfast</string>
  <key>CFBundleExecutable</key><string>mmfast</string>
  <key>CFBundleIconFile</key><string>mmfast</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.social-networking</string>
  <key>CFBundleURLTypes</key>
  <array>
    <dict>
      <key>CFBundleURLName</key><string>Mattermost sign-in</string>
      <key>CFBundleURLSchemes</key><array><string>mmauth</string></array>
    </dict>
  </array>
</dict>
</plist>
PLIST
# A stable identity (dev/make-signing-identity.sh) keeps the keychain's
# approval across rebuilds; otherwise sign ad hoc.
if security find-identity -v -p codesigning 2>/dev/null | grep -q '"mmfast-dev"'; then
  codesign --force --sign mmfast-dev "$APP"
else
  codesign --force --sign - "$APP" >/dev/null 2>&1 || true
fi
# Tell Launch Services this bundle answers mmauth:// (single sign-on).
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$APP" || true
du -sh "$APP"
echo "built $APP"
