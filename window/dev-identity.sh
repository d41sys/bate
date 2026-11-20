#!/bin/sh
# Makes, once, the self-made code-signing certificate "Bate Dev" (#21) in a keychain file of
# its own, unlocks it, and prints the file's path. window/bundle.sh and window/sign-dev.sh
# sign with it: macOS (the Keychain's access lists, the privacy permissions) then knows every
# build made on this Mac as the same program. A build signed ad hoc is a new program after
# every rebuild, and macOS asks again.
#
# Nothing here asks the user anything: `codesign` signs with a certificate nobody has marked
# as trusted, and the keychain is not put in the user's search list.
#
# The keychain has an empty password and protects nothing: a program running as this user can
# sign as "Bate Dev". That is true of every certificate `codesign` may use without asking. It
# is for builds made and run on this Mac; a release is signed with a Developer ID (#37).
set -eu
keychain="$HOME/Library/Keychains/bate-dev.keychain-db"

if [ ! -f "$keychain" ]; then
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    /usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj "/CN=Bate Dev" \
        -addext "keyUsage=critical,digitalSignature" \
        -addext "extendedKeyUsage=critical,codeSigning" \
        -addext "basicConstraints=critical,CA:false" \
        -keyout "$work/key.pem" -out "$work/cert.pem" 2> /dev/null
    # Made beside its place and moved there whole: a half-made keychain is never found.
    security create-keychain -p "" "$work/new.keychain-db"
    # No lock after a time or on sleep.
    security set-keychain-settings "$work/new.keychain-db"
    security import "$work/key.pem" -k "$work/new.keychain-db" -T /usr/bin/codesign > /dev/null
    security import "$work/cert.pem" -k "$work/new.keychain-db" > /dev/null
    security set-key-partition-list -S apple-tool:,apple: -s -k "" "$work/new.keychain-db" \
        > /dev/null
    mv "$work/new.keychain-db" "$keychain"
fi
security unlock-keychain -p "" "$keychain"
echo "$keychain"
