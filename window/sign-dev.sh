#!/bin/sh
# Signs the programs under target/{debug,release} (appd, agentcore, bate) with the self-made
# certificate "Bate Dev" (window/dev-identity.sh makes it the first time, #21), so the
# Keychain's access list for Bate's items stays valid across rebuilds: a build signed ad hoc,
# or not at all, is a new program to the Keychain, and macOS asks again after every build.
# Run it after each `cargo build`. The first signed appd to read an item macOS asks about
# once: choose Always Allow.
set -eu
cd "$(dirname "$0")/.."
cert="Bate Dev"
target=${CARGO_TARGET_DIR:-target}

keychain=$(window/dev-identity.sh)

signed=0
for profile in debug release; do
    for program in appd agentcore bate; do
        file=$target/$profile/$program
        [ -f "$file" ] || continue
        # A fixed identifier: the Keychain recognises a program by it and by the certificate.
        codesign --force --keychain "$keychain" --sign "$cert" --identifier "com.oursnever.bate.$program" "$file"
        echo "signed $file"
        signed=$((signed + 1))
    done
done
[ "$signed" -gt 0 ] || { echo "nothing to sign under $target: run cargo build first" >&2; exit 1; }
