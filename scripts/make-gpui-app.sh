#!/usr/bin/env bash
# Builds target/VizDAW-GPUI.app: a minimal dev wrapper around the GPUI
# app's binary. A shell-launched macOS binary cannot take focus; one
# inside an .app can. Not signed (but for the bundled Multi-Q, ad hoc),
# not for release. `vizctl launch --app target/VizDAW-GPUI.app` finds the
# binary through CFBundleExecutable.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

profile="${1:-debug}"
# `screenshot`: ui.screenshot draws the window offscreen (GPUI's
# render_to_image, behind its test-support feature at this pin).
case "$profile" in
    debug) cargo build -p vizdaw-gpui --features screenshot && cargo build -p vizdaw-scan ;;
    release) cargo build -p vizdaw-gpui --release --features screenshot && cargo build -p vizdaw-scan --release ;;
    *) echo "usage: make-gpui-app.sh [debug|release]" >&2; exit 2 ;;
esac

app="target/VizDAW-GPUI.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp "target/$profile/vizdaw-gpui" "$app/Contents/MacOS/vizdaw-gpui"
# The plug-in scanner the catalog runs, next to the app binary
# (vizdaw-app session.rs `live_catalog`).
cp "target/$profile/vizdaw-scan" "$app/Contents/MacOS/vizdaw-scan"
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
    <key>NSMicrophoneUsageDescription</key>
    <string>VizDAW records audio from your audio interface or microphone into armed tracks.</string>
</dict>
</plist>
PLIST

# The bundled Multi-Q EQ, as scripts/make-app.sh bundles it: both builds in
# PlugIns, its licence texts and SOURCE.txt in Resources/Licences/Multi-Q,
# the bundles signed ad hoc. A missing build is made with
# scripts/build-multiq.sh; if that fails, so does this.
multiq="target/multiq/out"
if [[ ! -d "$multiq/Multi-Q.clap" || ! -d "$multiq/Multi-Q.component" || ! -d "$multiq/licences" ]]; then
    echo "make-gpui-app: $multiq is missing; running scripts/build-multiq.sh" >&2
    scripts/build-multiq.sh || { echo "make-gpui-app: scripts/build-multiq.sh failed" >&2; exit 1; }
fi
for f in Multi-Q.clap Multi-Q.component licences; do
    [[ -d "$multiq/$f" ]] || { echo "make-gpui-app: $multiq/$f is missing" >&2; exit 1; }
done
licences="$app/Contents/Resources/Licences/Multi-Q"
mkdir -p "$app/Contents/PlugIns" "$licences"
ditto "$multiq/Multi-Q.clap" "$app/Contents/PlugIns/Multi-Q.clap"
ditto "$multiq/Multi-Q.component" "$app/Contents/PlugIns/Multi-Q.component"
ditto "$multiq/licences" "$licences"
codesign --force -s - "$app/Contents/PlugIns/Multi-Q.clap"
codesign --force -s - "$app/Contents/PlugIns/Multi-Q.component"
# Seal the bundle last, after everything inside it is in place: an
# unsealed bundle opened from Finder reads as "damaged".
codesign --force -s - "$app"
echo "$app"
