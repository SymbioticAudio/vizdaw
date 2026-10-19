#!/usr/bin/env bash
# Runs SwiftPM on app/ with the flags this toolchain needs.
# Usage: scripts/swift.sh <build|run|test> [swift args...]
set -euo pipefail
if [[ $# -lt 1 ]]; then
    echo "usage: scripts/swift.sh <build|run|test> [args...]" >&2
    exit 2
fi
root="$(cd "$(dirname "$0")/.." && pwd)"
cmd="$1"
shift
# `swift run` has no app bundle: the scanner comes from build-ffi.sh.
export VIZDAW_SCANNER="${VIZDAW_SCANNER:-$root/target/ffi/vizdaw-scan}"

if xcodebuild -version >/dev/null 2>&1; then
    # Full Xcode (CI runners): default build system, XCTest/Testing in place.
    exec swift "$cmd" --package-path "$root/app" "$@"
fi

# Command Line Tools only: the native build system, plus the paths of
# Swift Testing, which the CLT ship outside the default search paths.
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Library/Developer/CommandLineTools}"
clt="$DEVELOPER_DIR"
fw="$clt/Library/Developer/Frameworks"
exec swift "$cmd" --package-path "$root/app" --build-system native \
    -Xswiftc "-F$fw" \
    -Xlinker "-F$fw" \
    -Xlinker -rpath -Xlinker "$fw" \
    -Xlinker -rpath -Xlinker "$clt/Library/Developer/usr/lib" \
    "$@"
