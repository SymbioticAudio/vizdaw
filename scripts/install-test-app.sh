#!/usr/bin/env bash
# Copies dist/VizDAW.app to ~/Applications/VizDAW Test.app for manual
# testing. Agents and e2e rebuild dist/ all the time; this copy changes only
# when the owner (or Claude, when asked) runs this script.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
src="$root/dist/VizDAW.app"
dst="$HOME/Applications/VizDAW Test.app"
[[ -d "$src" ]] || { echo "no $src: run scripts/make-app.sh first" >&2; exit 1; }
if pgrep -f "$dst/Contents/MacOS/VizDAW" >/dev/null; then
    echo "VizDAW Test is running: quit it first" >&2
    exit 1
fi
mkdir -p "$HOME/Applications"
rm -rf "$dst"
ditto "$src" "$dst"
echo "installed $dst ($(plutil -extract CFBundleVersion raw "$dst/Contents/Info.plist"))"
