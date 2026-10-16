#!/usr/bin/env bash
# Builds libvizdaw_ffi and generates the Swift bindings into app/Sources.
# Usage: scripts/build-ffi.sh [--debug]
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
if ! xcodebuild -version >/dev/null 2>&1; then
    export DEVELOPER_DIR="${DEVELOPER_DIR:-/Library/Developer/CommandLineTools}"
fi
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

profile=release
flag=--release
if [[ "${1:-}" == "--debug" ]]; then
    profile=debug
    flag=
fi

cargo build -p core-ffi $flag
lib="target/$profile/libvizdaw_ffi"
gen="target/ffi/gen"
rm -rf "$gen"
mkdir -p "$gen" target/ffi
cargo run -q -p uniffi-bindgen -- generate --library "$lib.dylib" \
    --language swift --out-dir "$gen"
cp "$lib.a" target/ffi/libvizdaw_ffi.a

for f in vizdaw_ffi.swift vizdaw_ffiFFI.h vizdaw_ffiFFI.modulemap; do
    if [[ ! -f "$gen/$f" ]]; then
        echo "error: bindgen did not produce $gen/$f" >&2
        exit 1
    fi
done
mkdir -p app/Sources/VizDAWCore app/Sources/vizdaw_ffiFFI/include
cp "$gen/vizdaw_ffi.swift" app/Sources/VizDAWCore/vizdaw_ffi.swift
cp "$gen/vizdaw_ffiFFI.h" app/Sources/vizdaw_ffiFFI/include/vizdaw_ffiFFI.h
cp "$gen/vizdaw_ffiFFI.modulemap" app/Sources/vizdaw_ffiFFI/include/module.modulemap

echo "FFI ready ($profile). Next: scripts/swift.sh build   (or: scripts/swift.sh run VizDAW)"
