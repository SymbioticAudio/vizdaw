#!/usr/bin/env bash
# Writes THIRD-PARTY.txt: the licence notices of the third-party code the
# VizDAW.app bundle carries, apart from Multi-Q's (those are in
# Licences/Multi-Q, written by build-multiq.sh). make-app.sh copies it to
# Contents/Resources/Licences/THIRD-PARTY.txt, and VizDAW › Licences… shows it.
#
# Usage: scripts/third-party-notices.sh [out-file]
#   (default target/third-party/THIRD-PARTY.txt)
#
# The output names copyright holders, as the notices must, so it is build
# output only: it is written under target/ or dist/ (or outside the repo),
# never into repo text the name scans read.
#
# It covers:
#   - the Rust crates in the normal (non-dev, non-build) dependency graph of
#     the binaries the bundle ships: core-ffi (the library the app links),
#     vizdaw-scan and vizdaw-mcp (vizctl is not bundled). Proc-macro crates
#     run in the compiler only, and are left out with their own dependencies;
#   - the vendored DAWproject XSDs and the AAF MetaDictionary tables;
#   - the Swift packages in app/Package.resolved.
# It reads the registry sources only (cargo metadata --offline) and fails if
# a crate has no licence text and is not in EXCEPTIONS below.
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

out="${1:-target/third-party/THIRD-PARTY.txt}"
mkdir -p "$(dirname "$out")"
out_abs="$(cd "$(dirname "$out")" && pwd)/$(basename "$out")"
case "$out_abs" in
    "$root"/target/* | "$root"/dist/*) ;;
    "$root"/*)
        echo "third-party-notices: write under target/ or dist/, not $out" >&2
        exit 2
        ;;
esac

host="$(rustc -vV | sed -n 's/^host: //p')"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/vizdaw-notices.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT
cargo metadata --format-version 1 --locked --offline --filter-platform "$host" >"$tmp/metadata.json"

checkouts="${VIZDAW_SWIFT_CHECKOUTS:-$root/app/.build/checkouts}"

python3 -I - "$tmp/metadata.json" "$root" "$checkouts" "${VIZDAW_NO_BRAND:-}" "$tmp/out.txt" <<'PY'
import hashlib, json, os, re, sys

meta_path, root, checkouts, no_brand, out_path = sys.argv[1:6]

# The binaries the bundle ships (make-app.sh copies these).
ROOTS = ["core-ffi", "vizdaw-scan", "vizdaw-mcp"]

# Crates whose published package has no licence file. Their project's own
# texts are kept in scripts/licence-texts/<project>/ (copied from the
# project's repository, 2026-11-18) and printed in their place. A new entry
# needs its texts there; keep the list short.
OBJC2 = "objc2"
OBJC2_MIT = (OBJC2, ["LICENSE-MIT.txt"])  # objc2, block2, objc2-foundation, objc2-encode: MIT only
OBJC2_TRIO = (OBJC2, ["LICENSE-ZLIB.txt", "LICENSE-APACHE.txt", "LICENSE-MIT.txt"])  # the rest: Zlib OR Apache-2.0 OR MIT
UNIFFI = ("uniffi-rs", ["LICENSE"])  # MPL-2.0; source: https://github.com/mozilla/uniffi-rs
EXCEPTIONS = {
    "block2": OBJC2_MIT,
    "objc2": OBJC2_MIT,
    "objc2-encode": OBJC2_MIT,
    "objc2-foundation": OBJC2_MIT,
    "dispatch2": OBJC2_TRIO,
    "objc2-audio-toolbox": OBJC2_TRIO,
    "objc2-core-audio": OBJC2_TRIO,
    "objc2-core-audio-types": OBJC2_TRIO,
    "objc2-core-foundation": OBJC2_TRIO,
    "objc2-core-midi": OBJC2_TRIO,
    "uniffi": UNIFFI,
    "uniffi_bindgen": UNIFFI,
    "uniffi_core": UNIFFI,
    "uniffi_meta": UNIFFI,
    "uniffi_pipeline": UNIFFI,
    "uniffi_udl": UNIFFI,
}
VENDORED = os.path.join(root, "scripts/licence-texts")

# The Swift package that is the owner's own work, not third-party.
OWNER_PACKAGES = {"symbiotic-design"}

LICENCE_FILE = re.compile(r"^(licen[cs]e|copying|notice|unlicense|copyright)([-_.].*)?$", re.I)
LICENCE_DIR = re.compile(r"^licen[cs]es?$", re.I)


def licence_files(directory, extra=None):
    found = []
    try:
        names = sorted(os.listdir(directory))
    except OSError:
        return found
    for name in names:
        path = os.path.join(directory, name)
        if os.path.isfile(path) and LICENCE_FILE.match(name):
            found.append((name, path))
        elif os.path.isdir(path) and LICENCE_DIR.match(name):
            for sub in sorted(os.listdir(path)):
                p = os.path.join(path, sub)
                if os.path.isfile(p):
                    found.append((name + "/" + sub, p))
    if extra:
        p = os.path.normpath(os.path.join(directory, extra))
        if os.path.isfile(p) and all(p != q for _, q in found):
            found.append((os.path.relpath(p, directory), p))
    return found


def read(path):
    with open(path, "rb") as f:
        return f.read().decode("utf-8", errors="replace").replace("\r\n", "\n")


lines = []
seen = {}  # text hash -> the section that printed it first


def rule(ch="-"):
    lines.append(ch * 72)


def texts(section, files):
    for label, path in files:
        text = read(path).strip("\n")
        digest = hashlib.sha256(text.encode()).hexdigest()
        if digest in seen:
            lines.append(f"[{label}: the same text as in {seen[digest]}, above.]")
            lines.append("")
            continue
        seen[digest] = f"{section} ({label})"
        lines.append(f"--- {label} ---")
        lines.append(text)
        lines.append("")


with open(meta_path) as f:
    meta = json.load(f)
packages = {p["id"]: p for p in meta["packages"]}
nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
workspace = set(meta["workspace_members"])
version = next(p["version"] for p in meta["packages"] if p["name"] == "core-ffi" and p["id"] in workspace)


def is_proc_macro(pkg):
    return any("proc-macro" in t["kind"] for t in pkg["targets"])


roots = [p["id"] for p in meta["packages"] if p["name"] in ROOTS and p["id"] in workspace]
missing_roots = set(ROOTS) - {packages[i]["name"] for i in roots}
if missing_roots:
    sys.exit(f"third-party-notices: no workspace package {sorted(missing_roots)}")
reached, stack = set(), list(roots)
while stack:
    pid = stack.pop()
    if pid in reached:
        continue
    reached.add(pid)
    for dep in nodes[pid]["deps"]:
        if not any(k["kind"] is None for k in dep["dep_kinds"]):
            continue  # dev- or build-only
        if is_proc_macro(packages[dep["pkg"]]):
            continue  # runs in the compiler, not in the binary
        stack.append(dep["pkg"])

crates = sorted((packages[i] for i in reached if i not in workspace), key=lambda p: (p["name"], p["version"]))

lines += [
    f"VizDAW {version}: third-party notices",
    "",
    "VizDAW is free software under the GNU General Public License, version 3",
    "only (see LICENSE). VizDAW bundles Dusk Audio Multi-Q, which includes",
    "parts under the GNU AGPLv3 (JUCE); its notices and source offer are in",
    "the Multi-Q folder beside this file.",
    "",
    "This file holds the notices of the other third-party code in VizDAW.app:",
    "the Rust crates compiled into it, data files it carries, and its Swift",
    "packages. A licence text that repeats one printed earlier is named, not",
    "printed again.",
    "",
]

rule("=")
lines.append(f"Rust crates ({len(crates)})")
rule("=")
lines.append("")
failures = []
for pkg in crates:
    section = f"{pkg['name']} {pkg['version']}"
    rule()
    lines.append(section)
    lines.append(f"Licence: {pkg.get('license') or 'see the text below'}")
    if pkg.get("repository"):
        lines.append(f"Source: {pkg['repository']}")
    rule()
    files = licence_files(os.path.dirname(pkg["manifest_path"]), pkg.get("license_file"))
    if not files:
        entry = EXCEPTIONS.get(pkg["name"])
        if entry is None:
            failures.append(section)
            continue
        project, names = entry
        files = [(f"{project} {n}", os.path.join(VENDORED, project, n)) for n in names]
        lost = [p for _, p in files if not os.path.isfile(p)]
        if lost:
            sys.exit("third-party-notices: missing " + ", ".join(lost))
        if project == "uniffi-rs":
            lines.append("Source Code Form: https://github.com/mozilla/uniffi-rs (MPL-2.0 section 3.2).")
    texts(section, files)
if failures:
    sys.exit("third-party-notices: no licence text in " + ", ".join(failures)
             + " (add the text's source to EXCEPTIONS in scripts/third-party-notices.sh)")

rule("=")
lines.append("Data files")
rule("=")
lines.append("")
xsd = os.path.join(root, "crates/core-io/tests/data/dawproject/LICENSE")
rule()
lines.append("DAWproject XML schemas (used by the DAWproject reader and writer)")
rule()
texts("the DAWproject schemas", [("LICENSE", xsd)])

aaf = os.path.join(root, "crates/core-io/src/aaf/metadict.rs")
header = []
with open(aaf) as f:
    for line in f:
        if not line.startswith("//") or line.startswith("//!"):
            break
        header.append(line[2:].rstrip("\n").removeprefix(" "))
if not any("Copyright" in h for h in header):
    sys.exit(f"third-party-notices: no licence header in {aaf}")
rule()
lines.append("AAF MetaDictionary tables (crates/core-io/src/aaf/metadict.rs)")
rule()
lines += header
lines.append("")

rule("=")
lines.append("Swift packages")
rule("=")
lines.append("")
with open(os.path.join(root, "app/Package.resolved")) as f:
    pins = json.load(f)["pins"]
for pin in sorted(pins, key=lambda p: p["identity"]):
    ident = pin["identity"]
    state = pin.get("state", {})
    section = f"{ident} {state.get('version') or state.get('revision', '')[:12]}".strip()
    rule()
    lines.append(section)
    rule()
    if ident in OWNER_PACKAGES:
        lines.append("The SymbioticAudio design package, the VizDAW owner's own work; not")
        lines.append("third-party code.")
        if no_brand == "1":
            lines.append("Its branded artwork (SymbioticBrand) is not bundled in this build.")
        lines.append("")
        continue
    name = pin["location"].rstrip("/").split("/")[-1].removesuffix(".git")
    files = licence_files(os.path.join(checkouts, name)) or licence_files(os.path.join(checkouts, ident))
    if not files:
        sys.exit(f"third-party-notices: no licence text for the Swift package {ident} under {checkouts}")
    texts(section, files)

with open(out_path, "w") as f:
    f.write("\n".join(lines).rstrip("\n") + "\n")
print(f"{len(crates)} crates, {len(pins)} Swift packages")
PY
mv "$tmp/out.txt" "$out"
echo "wrote $out"
