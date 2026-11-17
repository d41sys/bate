#!/bin/sh
# Signs the programs under target/{debug,release} (appd, agentcore, bate) with a self-made
# certificate named "Bate Dev" (#21), so the Keychain's access list for Bate's items stays valid
# across rebuilds: a build signed ad hoc, or not at all, is a new program to the Keychain, and
# macOS asks again after every build. Run it after each `cargo build`.
set -eu
cd "$(dirname "$0")/.."
cert="Bate Dev"
target=${CARGO_TARGET_DIR:-target}

if ! security find-identity -v -p codesigning | grep -q "\"$cert\""; then
    cat >&2 <<EOF
There is no valid code-signing certificate named "$cert". Make it once, in Keychain Access:

  1. Open Keychain Access (Spotlight: "Keychain Access").
  2. Menu bar: Keychain Access > Certificate Assistant > Create a Certificate...
  3. Name: $cert
     Identity Type: Self Signed Root
     Certificate Type: Code Signing
     Leave "Let me override defaults" off. Click Create, then Done.
  4. In the sidebar, select the login keychain, find "$cert" under My Certificates, and
     double-click it. Open Trust, set "Code Signing" to Always Trust, close the window and
     enter your password.
  5. Run this script again. "security find-identity -v -p codesigning" must list "$cert".

The first time a signed appd reads an item, macOS asks once for the login keychain's password:
choose Always Allow. It does not ask again after later rebuilds.
EOF
    exit 1
fi

signed=0
for profile in debug release; do
    for program in appd agentcore bate; do
        file=$target/$profile/$program
        [ -f "$file" ] || continue
        # A fixed identifier: the Keychain recognises a program by it and by the certificate.
        codesign --force --sign "$cert" --identifier "com.oursnever.bate.$program" "$file"
        echo "signed $file"
        signed=$((signed + 1))
    done
done
[ "$signed" -gt 0 ] || { echo "nothing to sign under $target: run cargo build first" >&2; exit 1; }
