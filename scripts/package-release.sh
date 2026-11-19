#!/usr/bin/env bash
# Builds dist/VizDAW.app and zips it as dist/VizDAW-<version>.zip for a
# GitHub release, then prints the zip's SHA-256. It never publishes: making
# the GitHub release and uploading the zip is the owner's step.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

# The version comes from one place: [workspace.package] in the root
# Cargo.toml (the same read as make-app.sh).
version="$(awk '/^\[/{s=($0=="[workspace.package]")} s&&/^version[[:space:]]*=/{gsub(/.*=[[:space:]]*"|".*/,"");print;exit}' Cargo.toml)"
[ -n "$version" ] || { echo "package-release.sh: no version in Cargo.toml [workspace.package]" >&2; exit 1; }

scripts/make-app.sh

app="dist/VizDAW.app"
zip="dist/VizDAW-$version.zip"
shipped="$(plutil -extract CFBundleShortVersionString raw "$app/Contents/Info.plist")"
[ "$shipped" = "$version" ] || {
    echo "package-release.sh: $app says $shipped, Cargo.toml says $version" >&2
    exit 1
}
rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"
sha="$(shasum -a 256 "$zip" | awk '{print $1}')"
echo "packaged $zip"
echo "SHA-256 $sha"
