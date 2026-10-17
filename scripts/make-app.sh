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
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key><string>org.vizdaw.VizDAW</string>
    <key>CFBundleName</key><string>VizDAW</string>
    <key>CFBundleDisplayName</key><string>VizDAW</string>
    <key>CFBundleExecutable</key><string>VizDAW</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>LSMinimumSystemVersion</key><string>14.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
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
plutil -lint "$app/Contents/Info.plist" >/dev/null
codesign --force -s - --deep "$app"
echo "built $app. Next: open $app"
