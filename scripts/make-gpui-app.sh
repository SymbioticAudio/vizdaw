#!/usr/bin/env bash
# Builds target/VizDAW-GPUI.app: a minimal dev wrapper around the GPUI
# app's binary. A shell-launched macOS binary cannot take focus; one
# inside an .app can. Not signed, not for release. `vizctl launch --app
# target/VizDAW-GPUI.app` finds the binary through CFBundleExecutable.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

profile="${1:-debug}"
case "$profile" in
    debug) cargo build -p vizdaw-gpui ;;
    release) cargo build -p vizdaw-gpui --release ;;
    *) echo "usage: make-gpui-app.sh [debug|release]" >&2; exit 2 ;;
esac

app="target/VizDAW-GPUI.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp "target/$profile/vizdaw-gpui" "$app/Contents/MacOS/vizdaw-gpui"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>vizdaw-gpui</string>
    <key>CFBundleIdentifier</key>
    <string>org.vizdaw.gpui.dev</string>
    <key>CFBundleName</key>
    <string>VizDAW GPUI</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST
echo "$app"
