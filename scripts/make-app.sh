#!/usr/bin/env bash
# Builds dist/VizDAW.app (release, ad-hoc signed). No Xcode needed.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

scripts/build-ffi.sh
scripts/swift.sh build -c release --product VizDAW
bin="$(scripts/swift.sh build -c release --product VizDAW --show-bin-path 2>/dev/null | tail -1)"

app="dist/VizDAW.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin/VizDAW" "$app/Contents/MacOS/VizDAW"
cp target/ffi/vizdaw-scan "$app/Contents/MacOS/vizdaw-scan"
# The licence, with its GPL section 7 permission for the branded icons
# (plan 7b Ruling 6): the conveyed app carries it.
cp LICENSE "$app/Contents/Resources/LICENSE"
# The app icon: the SymbioticAudio mark (glyph-39, the figure with a cube head), drawn from the
# symbiotic-design checkout. VIZDAW_NO_BRAND=1 (forks) keeps the default icon.
glyph="app/.build/checkouts/symbiotic-design/icons/svg/glyph-39.svg"
if [[ "${VIZDAW_NO_BRAND:-}" != "1" && -f "$glyph" ]]; then
    mkdir -p target/icon
    swiftc -O scripts/make-icon.swift -o target/icon/make-icon
    rm -rf target/icon/AppIcon.iconset
    target/icon/make-icon "$glyph" target/icon/AppIcon.iconset
    iconutil -c icns target/icon/AppIcon.iconset -o "$app/Contents/Resources/AppIcon.icns"
fi
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key><string>org.vizdaw.VizDAW</string>
    <key>CFBundleName</key><string>VizDAW</string>
    <key>CFBundleDisplayName</key><string>VizDAW</string>
    <key>CFBundleExecutable</key><string>VizDAW</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>LSMinimumSystemVersion</key><string>14.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
    <key>NSMicrophoneUsageDescription</key><string>VizDAW records audio from your audio interface or microphone into armed tracks.</string>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key><string>VizDAW Project</string>
            <key>CFBundleTypeRole</key><string>Editor</string>
            <key>LSHandlerRank</key><string>Owner</string>
            <key>LSTypeIsPackage</key><true/>
            <key>LSItemContentTypes</key><array><string>org.vizdaw.project</string></array>
        </dict>
    </array>
    <key>UTExportedTypeDeclarations</key>
    <array>
        <dict>
            <key>UTTypeIdentifier</key><string>org.vizdaw.project</string>
            <key>UTTypeDescription</key><string>VizDAW Project</string>
            <key>UTTypeConformsTo</key><array><string>com.apple.package</string></array>
            <key>UTTypeTagSpecification</key>
            <dict>
                <key>public.filename-extension</key><array><string>vizdaw</string></array>
            </dict>
        </dict>
    </array>
</dict>
</plist>
PLIST
# The build, as About VizDAW shows it ("Version 0.1.0 (abc1234 · date)"),
# so every bug report names the build it came from. "+" marks uncommitted
# changes.
build="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
git diff --quiet HEAD -- 2>/dev/null || build="$build+"
plutil -replace CFBundleVersion -string "$build · $(date '+%Y-%m-%d %H:%M')" "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" >/dev/null
codesign --force -s - --deep "$app"
echo "built $app. Next: open $app"
