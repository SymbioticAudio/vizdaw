#!/usr/bin/env bash
# Builds Dusk Audio Multi-Q (CLAP and AU) from pinned sources into
# target/multiq/out, with its licence texts and SOURCE.txt (plan 6g).
# Usage: scripts/build-multiq.sh   (no arguments; macOS only)
#
# Nothing is installed: CMake runs with HOME set to target/multiq/work/home
# and DUSK_COPY_AFTER_BUILD=OFF, so no copy-after-build can reach
# ~/Library/Audio/Plug-Ins. The AU is never registered or validated.
set -euo pipefail

# The pins. Multi-Q: release tag multi-q-v0.10.9. JUCE: tag 8.0.4, the version
# the tag's own CI builds with. clap-juce-extensions: the submodule the
# Multi-Q commit pins (it pins CLAP and clap-helpers in turn).
MULTIQ_SHA=00e3dc10f4a26931da823eca425dc9860d5e1737
JUCE_SHA=51d11a2be6d5c97ccf12b4e5e827006e19f0555a
CJE_SHA=e8de9e8571626633b8541a54c2406fccc4272767
# JUCE's LICENSE.md names the AGPLv3 but does not ship its text: take it from
# gnu.org, checked against this digest.
AGPL_URL=https://www.gnu.org/licenses/agpl-3.0.txt
AGPL_SHA256=0d96a4ff68ad6d4b6f1f30f713b18d5184912ba8dd389f86aa7710db079abcb0

MULTIQ_URL=https://github.com/dusk-audio/dusk-audio-plugins
JUCE_URL=https://github.com/juce-framework/JUCE
CJE_URL=https://github.com/free-audio/clap-juce-extensions

if [[ "$(uname -s)" != Darwin ]]; then
    echo "multiq: macOS only"
    exit 0
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
base="$root/target/multiq"
work="$base/work"
out="$base/out"
src="$work/dusk-audio-plugins"
juce="$work/JUCE"
build="$work/build"

# The licence texts of the third-party code JUCE compiles into MultiQ_CLAP
# and MultiQ_AU, as "<name in out/licences/JUCE-deps> <path in JUCE>". The
# modules come from plugins/multi-q/CMakeLists.txt (juce_audio_utils,
# juce_dsp and what they pull in); the list was checked against the built
# binaries' symbols (no VST3, LV2, ARA, oboe, box2d or choc code is in them).
# AudioUnitSDK is in the AU only.
JUCE_DEPS=(
    "AudioUnitSDK-LICENSE.txt modules/juce_audio_plugin_client/AU/AudioUnitSDK/LICENSE.txt"
    "FLAC-LICENSE.txt modules/juce_audio_formats/codecs/flac/Flac Licence.txt"
    "OggVorbis-LICENSE.txt modules/juce_audio_formats/codecs/oggvorbis/Ogg Vorbis Licence.txt"
    "libvorbis-COPYING.txt modules/juce_audio_formats/codecs/oggvorbis/libvorbis-1.3.7/COPYING"
    "zlib-README.txt modules/juce_core/zip/zlib/README"
    "jpeglib-README.txt modules/juce_graphics/image_formats/jpglib/README"
    "pnglib-LICENSE.txt modules/juce_graphics/image_formats/pnglib/LICENSE"
    "HarfBuzz-COPYING.txt modules/juce_graphics/fonts/harfbuzz/COPYING"
    "SheenBidi-LICENSE.txt modules/juce_graphics/unicode/sheenbidi/LICENSE"
)

# LICENCES is bumped whenever the set of files in out/licences changes, so
# an existing build is redone and the new files appear.
pins="MULTIQ_SHA=$MULTIQ_SHA
JUCE_SHA=$JUCE_SHA
CJE_SHA=$CJE_SHA
AGPL_SHA256=$AGPL_SHA256
LICENCES=2"

if [[ -f "$out/stamp" && "$(cat "$out/stamp")" == "$pins" ]]; then
    echo "multiq: up to date"
    exit 0
fi

for tool in git cmake ninja ditto curl shasum; do
    command -v "$tool" >/dev/null || { echo "multiq: $tool is missing" >&2; exit 1; }
done

# Clones `url` into `dir` (once) and checks out commit `sha`.
checkout() {
    local url="$1" dir="$2" sha="$3"
    if [[ ! -d "$dir/.git" ]]; then
        rm -rf "$dir"
        git clone --filter=blob:none --no-checkout "$url.git" "$dir"
    fi
    if ! git -C "$dir" cat-file -e "$sha^{commit}" 2>/dev/null; then
        git -C "$dir" fetch --filter=blob:none origin "$sha"
    fi
    git -C "$dir" checkout -q --detach "$sha"
}

mkdir -p "$work/home"
agpl="$work/agpl-3.0.txt"
if [[ ! -f "$agpl" ]] || ! shasum -a 256 "$agpl" | grep -q "^$AGPL_SHA256 "; then
    curl -fsSL "$AGPL_URL" -o "$agpl.part"
    if ! shasum -a 256 "$agpl.part" | grep -q "^$AGPL_SHA256 "; then
        rm -f "$agpl.part"
        echo "multiq: $AGPL_URL does not match AGPL_SHA256" >&2
        exit 1
    fi
    mv "$agpl.part" "$agpl"
fi
checkout "$MULTIQ_URL" "$src" "$MULTIQ_SHA"
checkout "$JUCE_URL" "$juce" "$JUCE_SHA"
git -C "$src" submodule update --init --recursive external/clap-juce-extensions
status="$(git -C "$src" submodule status external/clap-juce-extensions)"
if [[ "$status" != " $CJE_SHA "* ]]; then
    echo "multiq: external/clap-juce-extensions is not at $CJE_SHA: $status" >&2
    exit 1
fi

configure=(cmake -G Ninja -S "$src" -B "$build"
    -DCMAKE_BUILD_TYPE=Release -DJUCE_PATH="$juce"
    -DBUILD_MULTI_Q=ON -DBUILD_4K_EQ=OFF -DBUILD_MULTI_COMP=OFF
    -DBUILD_TAPE_MACHINE=OFF -DBUILD_GROOVEMIND=OFF
    -DBUILD_CONVOLUTION_REVERB=OFF -DBUILD_TAPE_ECHO=OFF
    -DBUILD_CHORD_ANALYZER=OFF -DBUILD_SPECTRUM_ANALYZER=OFF
    -DBUILD_DUSKVERB=OFF -DBUILD_DUSKAMP=OFF
    -DBUILD_VST3=OFF -DBUILD_LV2=OFF -DBUILD_AU=ON -DBUILD_CLAP=ON
    -DDUSK_COPY_AFTER_BUILD=OFF -DCMAKE_OSX_DEPLOYMENT_TARGET=14.0)
compile=(cmake --build "$build" --target MultiQ_CLAP MultiQ_AU)
HOME="$work/home" "${configure[@]}"
HOME="$work/home" "${compile[@]}"

# plugins/multi-q/CMakeLists.txt sets the output directory to <build>/bin.
clap="$build/bin/CLAP/Multi-Q.clap"
au="$build/bin/AU/Multi-Q.component"
for bundle in "$clap" "$au"; do
    [[ -d "$bundle" ]] || { echo "multiq: $bundle was not built" >&2; exit 1; }
done

cje="$src/external/clap-juce-extensions"
rm -rf "$out"
mkdir -p "$out/licences"
ditto "$clap" "$out/Multi-Q.clap"
ditto "$au" "$out/Multi-Q.component"
ditto "$src/LICENSE" "$out/licences/Multi-Q-LICENSE"
ditto "$juce/LICENSE.md" "$out/licences/JUCE-LICENSE.md"
ditto "$cje/LICENSE.md" "$out/licences/clap-juce-extensions-LICENSE.md"
ditto "$cje/clap-libs/clap/LICENSE" "$out/licences/CLAP-LICENSE"
ditto "$agpl" "$out/licences/AGPL-3.0.txt"
mkdir -p "$out/licences/JUCE-deps"
for dep in "${JUCE_DEPS[@]}"; do
    name="${dep%% *}"
    from="$juce/${dep#* }"
    [[ -f "$from" ]] || { echo "multiq: licence $from is missing" >&2; exit 1; }
    ditto "$from" "$out/licences/JUCE-deps/$name"
done

configure_text="${configure[*]}"
compile_text="${compile[*]}"
clap_sha="$(git -C "$cje/clap-libs/clap" rev-parse HEAD)"
helpers_sha="$(git -C "$cje/clap-libs/clap-helpers" rev-parse HEAD)"
cat >"$out/licences/SOURCE.txt" <<EOF
Multi-Q (CLAP and AU), built by VizDAW's scripts/build-multiq.sh from:

Multi-Q              $MULTIQ_URL
                     commit $MULTIQ_SHA (tag multi-q-v0.10.9), folder plugins/multi-q
JUCE                 $JUCE_URL
                     commit $JUCE_SHA (tag 8.0.4)
clap-juce-extensions $CJE_URL
                     commit $CJE_SHA
                     (with its submodules CLAP https://github.com/free-audio/clap
                     at $clap_sha and clap-helpers
                     https://github.com/free-audio/clap-helpers at $helpers_sha)

JUCE is used under the AGPLv3. Its text, AGPL-3.0.txt, is from
$AGPL_URL (SHA-256 $AGPL_SHA256).
CLAP-LICENSE covers clap-helpers too: its LICENSE is the same text.
JUCE-deps/ holds the notices of the third-party code JUCE compiles into
the plug-ins (AudioUnitSDK, FLAC, Ogg Vorbis, zlib, the IJG JPEG library,
libpng, HarfBuzz and SheenBidi), copied from the JUCE commit above.

Build command:
HOME=<temp> ${configure_text//"$root"/<repo>}
HOME=<temp> ${compile_text//"$root"/<repo>}

The source of these plug-ins is available at the URLs above at the commits named; VizDAW offers it under GPLv3 §6 and AGPLv3 §13.
EOF

# Written last, so a failed build is retried on the next run.
printf '%s\n' "$pins" >"$out/stamp"
echo "multiq: built $out"
