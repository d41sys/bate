#!/bin/sh
# Wraps bate-helper as "Bate Helper.app": macOS gives Accessibility, Screen Recording and Full Disk Access to an
# app, not to a bare program, and the service looks for it at a fixed path.
#
#   helper/bundle.sh [folder]     folder defaults to ~/Applications
#
# SIGN_IDENTITY names the certificate to sign with; the default, "-", signs ad hoc, which
# macOS treats as a new app after every rebuild (the permissions must be given again).
# BATE_HELPER_BIN names a built bate-helper to wrap instead of building one.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
app="${1:-$HOME/Applications}/Bate Helper.app"

bin=${BATE_HELPER_BIN:-}
if [ -z "$bin" ]; then
    (cd "$root" && cargo build --release --locked -p bate-helper)
    bin="$root/target/release/bate-helper"
fi

rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp "$bin" "$app/Contents/MacOS/bate-helper"
cat > "$app/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>bate-helper</string>
    <key>CFBundleIdentifier</key><string>com.oursnever.bate.helper</string>
    <key>CFBundleName</key><string>Bate Helper</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>LSUIElement</key><true/>
</dict>
</plist>
EOF
codesign --force --sign "${SIGN_IDENTITY:--}" "$app"
echo "$app"
