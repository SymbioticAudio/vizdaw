#!/usr/bin/env bash
# Builds the VST3 SDK's sample plug-ins that the VST3 host tests load
# (host track H1 Task 6) into target/vst3sdk/out, from the pinned SDK.
# Usage: scripts/build-vst3-samples.sh   (no arguments; macOS and Linux)
#
# The tests in crates/core-host/tests/vst3_{host,engine}.rs find the bundles in
# target/vst3sdk/out (or $VIZDAW_VST3_SAMPLES). They skip when the folder
# is missing, unless VIZDAW_REQUIRE_VST3_SAMPLES=1 (set in CI).
#
# Nothing is installed: SMTG_CREATE_PLUGIN_LINK=OFF, so no link reaches the
# user's plug-in folders, and the SDK's validator is not run. The samples
# are test fixtures only; VizDAW does not ship them.
#
# Linux needs VSTGUI's packages (the list its own Linux CI installs; see
# .github/workflows/ci.yml): libx11-dev libx11-xcb-dev libxcb-util-dev
# libxcb-cursor-dev libxcb-keysyms1-dev libxcb-xkb-dev libxkbcommon-dev
# libxkbcommon-x11-dev libfontconfig1-dev libcairo2-dev libfreetype6-dev
# libpango1.0-dev libwayland-dev wayland-protocols, plus ninja-build.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"

# The pin: the SDK's release tag v3.8.0_build_66 (the SDK version the vst3
# crate's bindings are generated from). Its URL, tag and commit live beside
# its licence text (scripts/licence-texts/vst3sdk/SOURCE).
source_file="$root/scripts/licence-texts/vst3sdk/SOURCE"
pin() { sed -n "s/^$1=//p" "$source_file"; }
SDK_URL="$(pin url)"
SDK_TAG="$(pin tag)"
SDK_SHA="$(pin commit)"
[[ -n "$SDK_URL" && -n "$SDK_TAG" && -n "$SDK_SHA" ]] || { echo "vst3 samples: $source_file lacks url, tag or commit" >&2; exit 1; }
SUBMODULES=(base cmake pluginterfaces public.sdk vstgui4)
# The CMake targets; each makes <target>.vst3.
TARGETS=(again again-sample-accurate adelay note-expression-synth host-checker panner)

base="$root/target/vst3sdk"
work="$base/work"
out="$base/out"
src="$work/vst3sdk"
build="$work/build"

pins="SDK_SHA=$SDK_SHA
TARGETS=${TARGETS[*]}
OS=$(uname -s)-$(uname -m)"

if [[ -f "$out/stamp" && "$(cat "$out/stamp")" == "$pins" ]]; then
    echo "vst3 samples: up to date"
    exit 0
fi

for tool in git cmake ninja; do
    command -v "$tool" >/dev/null || { echo "vst3 samples: $tool is missing" >&2; exit 1; }
done

mkdir -p "$work"
if [[ ! -d "$src/.git" ]]; then
    rm -rf "$src"
    git clone --filter=blob:none --no-checkout "$SDK_URL.git" "$src"
fi
if ! git -C "$src" cat-file -e "$SDK_SHA^{commit}" 2>/dev/null; then
    git -C "$src" fetch --filter=blob:none origin "$SDK_SHA"
fi
git -C "$src" checkout -q --detach "$SDK_SHA"
# The submodule commits are the ones the SDK commit pins.
git -C "$src" submodule update --init --depth 1 --filter=blob:none "${SUBMODULES[@]}"

# VSTGUI enables Objective-C++ in a subdirectory, which newer CMake (4.x)
# leaves half set up ("CMAKE_OBJCXX_COMPILE_OBJECT" missing): enable it in
# the SDK's top-level project instead.
top="$work/top-level.cmake"
printf '%s\n' 'if(APPLE)' '    enable_language(OBJC OBJCXX)' 'endif()' >"$top"
configure=(cmake -G Ninja -S "$src" -B "$build"
    -DCMAKE_BUILD_TYPE=Release
    -DCMAKE_PROJECT_vstsdk_INCLUDE="$top"
    -DSMTG_ENABLE_VST3_HOSTING_EXAMPLES=OFF
    -DSMTG_ENABLE_VSTGUI_SUPPORT=ON
    -DSMTG_CREATE_PLUGIN_LINK=OFF
    -DSMTG_RUN_VST_VALIDATOR=OFF
    -DSMTG_BUILD_UNIVERSAL_BINARY=OFF
    -DSMTG_ENABLE_AUV2_BUILDS=OFF
    # The samples need only the VSTGUI library, not its standalone apps and
    # tools (those need gtkmm on Linux).
    -DVSTGUI_STANDALONE=OFF
    -DVSTGUI_STANDALONE_EXAMPLES=OFF
    -DVSTGUI_TOOLS=OFF)
# VSTGUI builds with -Werror, and newer standard libraries deprecate what
# it uses (std::wstring_convert): keep those warnings, not as errors.
lenient="-Wno-error=deprecated-declarations"
configure+=(-DCMAKE_CXX_FLAGS="$lenient")
if [[ "$(uname -s)" == Darwin ]]; then
    # The SDK's default (10.13) is older than current libc++ supports.
    configure+=(-DCMAKE_OSX_DEPLOYMENT_TARGET=14.0 -DCMAKE_OBJCXX_FLAGS="$lenient")
fi
"${configure[@]}"
cmake --build "$build" --target "${TARGETS[@]}"

# The SDK writes every bundle to <build>/VST3/<config>.
built="$build/VST3/Release"
rm -rf "$out"
mkdir -p "$out"
for target in "${TARGETS[@]}"; do
    bundle="$target.vst3"
    [[ -d "$built/$bundle" ]] || { echo "vst3 samples: $built/$bundle was not built" >&2; exit 1; }
    cp -R "$built/$bundle" "$out/$bundle"
done
cp "$src/LICENSE.txt" "$out/LICENSE.txt"
cat >"$out/SOURCE.txt" <<EOF
VST3 SDK sample plug-ins (test fixtures, not shipped), built by VizDAW's
scripts/build-vst3-samples.sh from $SDK_URL
commit $SDK_SHA (tag $SDK_TAG), with the submodules it pins:
${SUBMODULES[*]}. The SDK is MIT licensed (LICENSE.txt).
EOF

# Written last, so a failed build is retried on the next run.
printf '%s\n' "$pins" >"$out/stamp"
echo "vst3 samples: built $out"
