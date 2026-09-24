#!/usr/bin/env bash
#
# Compile the reusable native runner stubs (one per platform) with `cargo zigbuild`.
#
# These are the small (~0.5 MB), generic, version-independent launchers. An application
# binary is built by concatenating a stub, an ETHRCFG record and the app's JAR; the stubs
# themselves are reusable across applications and versions. This build is deliberately
# *separate* from the Mill build, which never compiles Rust: the stubs are data to it, and are
# published independently by `runners-release.sh`.
#
# Two things are checked and done here that the format depends on (spec/ethrcfg.md):
#
# - A stub must contain the magic `ETHRCFG` nowhere, because the runner finds its record by
#   the FIRST occurrence in its own file. The runner reassembles the magic at run time from an
#   obfuscated constant; this script proves that worked.
# - The macOS stubs are ad-hoc signed here, once. Building an executable never touches the
#   stub's bytes, so that signature stays valid in every executable built from it — which is
#   what lets a macOS executable be built on any host with no `codesign` at all.
#
# Usage: ./etc/ci/runners-build.sh [output-dir]      (default: dist/runners)
#
# Produces <output-dir>/runner-<label>[.exe] for each platform.

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

OUT="${1:-dist/runners}"

# triple|label|binary — the platforms the runner is cross-compiled for.
TARGETS=(
  "x86_64-pc-windows-gnu|windows-x64|runner.exe"
  "x86_64-unknown-linux-gnu|linux-x64|runner"
  "aarch64-unknown-linux-gnu|linux-arm64|runner"
  "x86_64-apple-darwin|macos-x64|runner"
  "aarch64-apple-darwin|macos-arm64|runner"
)

if ! command -v cargo >/dev/null 2>&1; then
  echo "runners-build: cargo (with the zigbuild subcommand) is required" >&2; exit 1
fi

target_dir=$(mktemp -d)
trap 'rm -rf "$target_dir"' EXIT

triple_args=()
for entry in "${TARGETS[@]}"; do
  IFS='|' read -r triple _ _ <<< "$entry"
  triple_args+=(--target "$triple")
done

echo "runners-build: cross-compiling ${#TARGETS[@]} runner stubs (release)…"
cargo zigbuild --release \
  --manifest-path Cargo.toml \
  --target-dir "$target_dir" \
  "${triple_args[@]}"

mkdir -p "$OUT"
for entry in "${TARGETS[@]}"; do
  IFS='|' read -r triple label binary <<< "$entry"
  ext=""; [[ "$binary" == *.exe ]] && ext=".exe"
  cp -f "$target_dir/$triple/release/$binary" "$OUT/runner-$label$ext"
  chmod +x "$OUT/runner-$label$ext"
done

for f in "$OUT"/runner-*; do
  if LC_ALL=C grep -q 'ETHRCFG' "$f"; then
    echo "runners-build: $f contains the ETHRCFG magic; a stub must not (see spec/ethrcfg.md)" >&2
    exit 1
  fi
done
echo "runners-build: no stub contains the ETHRCFG magic"

# Ad-hoc sign the macOS stubs. Apple's `codesign` on a Mac, `rcodesign` (apple-codesign)
# elsewhere; both produce a valid ad-hoc signature. Hashes are taken from the signed bytes.
for f in "$OUT"/runner-macos-*; do
  if command -v codesign >/dev/null 2>&1; then
    codesign --sign - --force "$f"
  elif command -v rcodesign >/dev/null 2>&1; then
    rcodesign sign "$f" >/dev/null
  else
    echo "runners-build: neither codesign nor rcodesign found; macOS stubs are unsigned" >&2
    exit 1
  fi
done
echo "runners-build: signed the macOS stubs"

echo "runners-build: built into $OUT:"
ls -la "$OUT"/runner-*
