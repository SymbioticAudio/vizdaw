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
# The bundled Multi-Q EQ (plan 6g Ruling 2): both bundles in PlugIns, its
# licence texts and SOURCE.txt in Resources/Licences/Multi-Q. It is not a
# brand asset, so VIZDAW_NO_BRAND=1 bundles it too. Every licence file the
# build wrote must arrive; a missing one fails the build.
scripts/build-multiq.sh
multiq="target/multiq/out"
multiq_licences=(Multi-Q-LICENSE JUCE-LICENSE.md clap-juce-extensions-LICENSE.md
    CLAP-LICENSE AGPL-3.0.txt SOURCE.txt
    JUCE-deps/AudioUnitSDK-LICENSE.txt JUCE-deps/FLAC-LICENSE.txt
    JUCE-deps/OggVorbis-LICENSE.txt JUCE-deps/libvorbis-COPYING.txt
    JUCE-deps/zlib-README.txt JUCE-deps/jpeglib-README.txt
    JUCE-deps/pnglib-LICENSE.txt JUCE-deps/HarfBuzz-COPYING.txt
    JUCE-deps/SheenBidi-LICENSE.txt)
for f in Multi-Q.clap Multi-Q.component; do
    [[ -d "$multiq/$f" ]] || { echo "make-app: $multiq/$f is missing" >&2; exit 1; }
done
for f in "${multiq_licences[@]}"; do
    [[ -s "$multiq/licences/$f" ]] || { echo "make-app: licence $multiq/licences/$f is missing" >&2; exit 1; }
done
licences="$app/Contents/Resources/Licences/Multi-Q"
mkdir -p "$app/Contents/PlugIns" "$licences"
ditto "$multiq/Multi-Q.clap" "$app/Contents/PlugIns/Multi-Q.clap"
ditto "$multiq/Multi-Q.component" "$app/Contents/PlugIns/Multi-Q.component"
ditto "$multiq/licences" "$licences"
diff -r "$multiq/licences" "$licences" >/dev/null || {
    echo "make-app: $licences differs from $multiq/licences" >&2
    exit 1
}
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
        <dict>
            <key>CFBundleTypeName</key><string>MIDI File</string>
            <key>CFBundleTypeRole</key><string>Viewer</string>
            <key>LSHandlerRank</key><string>Alternate</string>
            <key>LSItemContentTypes</key><array><string>public.midi-audio</string></array>
        </dict>
        <dict>
            <key>CFBundleTypeName</key><string>MIDI Clip File</string>
            <key>CFBundleTypeRole</key><string>Viewer</string>
            <key>LSHandlerRank</key><string>Alternate</string>
            <key>LSItemContentTypes</key><array><string>org.vizdaw.midi-clip</string></array>
        </dict>
        <dict>
            <key>CFBundleTypeName</key><string>DAWproject</string>
            <key>CFBundleTypeRole</key><string>Viewer</string>
            <key>LSHandlerRank</key><string>Alternate</string>
            <key>LSItemContentTypes</key><array><string>org.vizdaw.dawproject</string></array>
        </dict>
        <dict>
            <key>CFBundleTypeName</key><string>AAF</string>
            <key>CFBundleTypeRole</key><string>Viewer</string>
            <key>LSHandlerRank</key><string>Alternate</string>
            <key>LSItemContentTypes</key><array><string>org.vizdaw.aaf</string></array>
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
        <dict>
            <key>UTTypeIdentifier</key><string>org.vizdaw.midi-clip</string>
            <key>UTTypeDescription</key><string>MIDI Clip File</string>
            <key>UTTypeConformsTo</key><array><string>public.data</string></array>
            <key>UTTypeTagSpecification</key>
            <dict>
                <key>public.filename-extension</key><array><string>midi2</string></array>
            </dict>
        </dict>
    </array>
    <!-- DAWproject (plan 6d): an open format we read and write but do not
         own, so the type is imported, not exported. -->
    <key>UTImportedTypeDeclarations</key>
    <array>
        <dict>
            <key>UTTypeIdentifier</key><string>org.vizdaw.dawproject</string>
            <key>UTTypeDescription</key><string>DAWproject</string>
            <key>UTTypeConformsTo</key><array><string>public.zip-archive</string></array>
            <key>UTTypeTagSpecification</key>
            <dict>
                <key>public.filename-extension</key><array><string>dawproject</string></array>
            </dict>
        </dict>
        <!-- AAF (plan 6f): an interchange format we read and write but do
             not own. -->
        <dict>
            <key>UTTypeIdentifier</key><string>org.vizdaw.aaf</string>
            <key>UTTypeDescription</key><string>AAF</string>
            <key>UTTypeConformsTo</key><array><string>public.data</string></array>
            <key>UTTypeTagSpecification</key>
            <dict>
                <key>public.filename-extension</key><array><string>aaf</string></array>
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
codesign --force -s - "$app/Contents/PlugIns/Multi-Q.clap"
codesign --force -s - "$app/Contents/PlugIns/Multi-Q.component"
codesign --force -s - --deep "$app"
codesign --verify --deep --strict "$app"
echo "built $app. Next: open $app"
