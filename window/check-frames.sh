#!/bin/sh
# Shows that a bot's page, in its frame, cannot reach the service (#25): builds a window with
# `frameCheck.ts` in it, runs it against a throwaway home and prints what the page could do.
# Opens a window, so it runs on a Mac with a screen, not in CI.
set -eu
cd "$(dirname "$0")/.."
home=$(mktemp -d)
app= win=
done_() {
    kill $app $win 2>/dev/null || true
    wait 2>/dev/null || true
    (cd window/ui && npm run build >/dev/null)
    rm -rf "$home"
}
trap done_ EXIT
(cd window/ui && VITE_FRAME_CHECK=1 npm run build >/dev/null)
cargo build -q -p appd -p window
bin=${CARGO_TARGET_DIR:-target}/debug
APPD_HOME=$home APPD_HOOK_PORT=0 APPD_ENGINE=/usr/bin/false "$bin"/appd >/dev/null 2>&1 &
app=$!
sleep 1
# What the window would open in the browser is written down instead.
printf '#!/bin/sh\necho "$1" >> %s/opened.txt\n' "$home" > "$home/opener"
chmod +x "$home/opener"
APPD_HOME=$home BATE_OPENER=$home/opener "$bin"/bate >/dev/null 2>&1 &
win=$!
for _ in $(seq 30); do
    found=$(ls "$home"/bots/*/memory/frame-check.md 2>/dev/null || true)
    [ -n "$found" ] && break
    sleep 1
done
[ -n "$found" ] || { echo "FAIL: no result from the window"; exit 1; }
sleep 1
cat "$found"; echo
echo "opened: $(tr '\n' ' ' < "$home/opened.txt" 2>/dev/null)"
[ "$(head -1 "$found")" = PASS ] &&
    grep -q 'from=page' "$home/opened.txt" &&
    ! grep -q leak "$home/opened.txt"
