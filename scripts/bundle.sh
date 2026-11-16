#!/bin/sh
# Build target/release/Dabble.app (release binary, icon, Info.plist), ad-hoc signed.
# Usage: scripts/bundle.sh   then   open target/release/Dabble.app
set -eu
cd "$(dirname "$0")/.."

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
APP=target/release/Dabble.app
WORK=target/bundle

cargo build --release
rm -rf "$APP" "$WORK"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$WORK"

cargo run --release --quiet --example make_icon -- "$WORK/Dabble.iconset"
iconutil -c icns "$WORK/Dabble.iconset" -o "$APP/Contents/Resources/Dabble.icns"

cp target/release/dabble "$APP/Contents/MacOS/dabble"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Dabble</string>
    <key>CFBundleDisplayName</key><string>Dabble</string>
    <key>CFBundleIdentifier</key><string>local.dabble.Dabble</string>
    <key>CFBundleExecutable</key><string>dabble</string>
    <key>CFBundleIconFile</key><string>Dabble</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.graphics-design</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST

codesign --force --deep --sign - "$APP"
echo "Built $APP"
