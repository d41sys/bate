#!/bin/sh
# Builds and signs Bate.app (#37): the window (bate), the service (appd) and the engine
# (agentcore) in Contents/MacOS, the service's login item in Contents/Library/LaunchAgents,
# and Bate Helper.app (helper/bundle.sh) in Contents/Helpers.
#
#   window/bundle.sh [folder]     folder defaults to target/release; Bate.app is made in it
#
# SIGN_IDENTITY names the certificate to sign with. Unset: "Bate Dev" if window/dev-identity.sh
# has made it on this Mac (#21; run it once), else "-", an ad-hoc signature. A Developer ID
# Application identity, once there is one, goes here too. An ad-hoc app is a new app to macOS
# after every rebuild: the Keychain and the privacy permissions ask again.
#
# Notarisation needs an Apple Developer ID, so it runs only when BATE_NOTARIZE_PROFILE names a
# `notarytool` keychain profile (`xcrun notarytool store-credentials`) and SIGN_IDENTITY is a
# Developer ID Application certificate: the app is then signed with the hardened runtime,
# submitted, and the ticket stapled to it.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
target=${CARGO_TARGET_DIR:-target}
app="${1:-$target/release}/Bate.app"

identity=${SIGN_IDENTITY:-}
# The keychain file "Bate Dev" is in; a named identity is looked for in the user's own.
keychain=
if [ -z "$identity" ]; then
    if [ -f "$HOME/Library/Keychains/bate-dev.keychain-db" ]; then
        identity="Bate Dev"
        keychain=$(window/dev-identity.sh)
    else
        identity="-"
    fi
fi
# The hardened runtime always: without it a program ignores DYLD_INSERT_LIBRARIES, and code
# injected into the window would pass the service's signature check. A secure timestamp is
# for notarisation only (it needs the network).
sign_options="--options runtime"
[ -z "${BATE_NOTARIZE_PROFILE:-}" ] || sign_options="--options runtime --timestamp"

# The page is compiled into the window.
(cd window/ui && { [ -d node_modules ] || npm ci; } && npm run build)
cargo build --release --locked -p appd -p agentcore -p window
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$app/Contents/Library/LaunchAgents"
for program in bate appd agentcore; do
    cp "$target/release/$program" "$app/Contents/MacOS/$program"
done
sed "s/__VERSION__/$version/g" window/bundle/Info.plist > "$app/Contents/Info.plist"
cp window/bundle/com.oursnever.bate.appd.plist "$app/Contents/Library/LaunchAgents/"
# The page's icon is 64 px: the smallest size an app icon may have is made from it.
iconset=$(mktemp -d)/Bate.iconset
mkdir "$iconset"
sips -z 128 128 window/icons/icon.png --out "$iconset/icon_128x128.png" > /dev/null
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Bate.icns"
SIGN_IDENTITY=$identity SIGN_KEYCHAIN=$keychain helper/bundle.sh "$app/Contents/Helpers" > /dev/null

# Nested code first, the app last: its signature seals what is inside. The identifiers are the
# ones window/sign-dev.sh gives, so the Keychain still knows the service by them.
for program in appd agentcore; do
    # shellcheck disable=SC2086
    codesign --force --sign "$identity" ${keychain:+--keychain "$keychain"} $sign_options --identifier "com.oursnever.bate.$program" "$app/Contents/MacOS/$program"
done
# shellcheck disable=SC2086
codesign --force --sign "$identity" ${keychain:+--keychain "$keychain"} $sign_options "$app/Contents/Helpers/Bate Helper.app"
# shellcheck disable=SC2086
codesign --force --sign "$identity" ${keychain:+--keychain "$keychain"} $sign_options "$app"
codesign --verify --strict "$app"

if [ -n "${BATE_NOTARIZE_PROFILE:-}" ]; then
    zip=$(mktemp -d)/Bate.zip
    ditto -c -k --keepParent "$app" "$zip"
    xcrun notarytool submit "$zip" --keychain-profile "$BATE_NOTARIZE_PROFILE" --wait
    xcrun stapler staple "$app"
    rm -f "$zip"
fi
echo "$app (signed with ${identity})"
