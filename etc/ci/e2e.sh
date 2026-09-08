#!/usr/bin/env bash
#
# The end-to-end check: package the example application around a real runner stub, run the
# result, and require it to say hello.
#
# This is the only stage that exercises the whole chain at once — a stub built (or fetched)
# from this repository, an `ETHRCFG` block patched by `xeq.Assembler`, an application JAR
# appended, a daemon started over the launcher protocol, and its output carried back. It is
# also the only stage that needs a daemon implementation, which is why it lives here and not in
# the test suite.
#
# Usage: ./etc/ci/e2e.sh          (or `make e2e`)
#
# Requires dist/runners to be populated — `make runners-build`, or `make runners-fetch
# RUNNERS_VERSION=X` where the Rust toolchain is unavailable.

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

if [[ ! -d dist/runners ]]; then
  echo "e2e: dist/runners not found — run \`make runners-build\` or \`make runners-fetch RUNNERS_VERSION=X\`" >&2
  exit 1
fi

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64)  LABEL=macos-arm64 ;;
  Darwin-x86_64) LABEL=macos-x64 ;;
  Linux-aarch64) LABEL=linux-arm64 ;;
  Linux-x86_64)  LABEL=linux-x64 ;;
  *) echo "e2e: unsupported host $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac

if [[ ! -f "dist/runners/runner-$LABEL" ]]; then
  echo "e2e: dist/runners/runner-$LABEL not found (host platform $LABEL)" >&2
  exit 1
fi

echo "e2e: assembling the example application for $LABEL"
./mill xeq.example.assembly

JAR=out/xeq/example/assembly.dest/out.jar
OUT=dist/hello
mkdir -p dist
rm -f "$OUT"

# Package with the toolchain's own packager, from the local stubs: `Native` delivery, one
# platform, no download and no hash check.
./mill -i xeq.packager.runMain xeq.Package "$PWD/$JAR" "$PWD/$OUT" "$LABEL" "$PWD/dist/runners"

echo "e2e: running $OUT"
ACTUAL=$("$OUT")

if [[ "$ACTUAL" != "Hello world" ]]; then
  echo "e2e: expected 'Hello world', got '$ACTUAL'" >&2
  exit 1
fi

echo "e2e: ok — $OUT printed '$ACTUAL'"
