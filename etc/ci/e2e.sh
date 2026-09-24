#!/usr/bin/env bash
#
# The end-to-end check: package the example application around a real runner stub, run the
# result, and require it to say hello.
#
# This is the only stage that exercises the whole chain at once — a stub built (or fetched)
# from this repository, an `ETHRCFG` record and an application JAR joined to it by the `xeq`
# builder script, a daemon started over the launcher protocol, and its output carried back. It is
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

# Package with the published builder script, from the local stubs: `Native` delivery, one
# platform, no download and no hash check. This is the same `xeq build` a shell user runs.
if [[ ! -x dist/xeq ]]; then
  echo "e2e: dist/xeq not found — run \`make xeq-script\`" >&2; exit 1
fi
./dist/xeq build --jar "$PWD/$JAR" --out "$PWD/$OUT" --target "$LABEL" --runners "$PWD/dist/runners"

echo "e2e: running $OUT"
ACTUAL=$("$OUT")

if [[ "$ACTUAL" != "Hello world" ]]; then
  echo "e2e: expected 'Hello world', got '$ACTUAL'" >&2
  exit 1
fi

echo "e2e: ok — $OUT printed '$ACTUAL'"

# A launcher must run its OWN bytes whatever the working directory holds. The shell leaves a
# bare name in argv[0] after a $PATH lookup, and resolving that name against the working
# directory picks up any same-named neighbour instead — which the JVM then rejects as an
# invalid JAR. See `resolve_script` in src/runner/src/main.rs.
echo "e2e: checking a \$PATH invocation shadowed by a same-named directory"

SHADOW=$(mktemp -d)
trap 'rm -rf "$SHADOW"' EXIT

# A distinct name, so this stage gets its own daemon and state directory rather than
# disturbing the one the check above just started.
mkdir -p "$SHADOW/bin" "$SHADOW/cwd/hellopath"
cp "$OUT" "$SHADOW/bin/hellopath"

SHADOWED=$(cd "$SHADOW/cwd" && PATH="$SHADOW/bin:$PATH" hellopath)

pkill -f 'ethereal.name=hellopath' >/dev/null 2>&1 || true
rm -rf "${XDG_STATE_HOME:-$HOME/.local/state}/hellopath" "${XDG_RUNTIME_DIR:-/nonexistent}/hellopath"

if [[ "$SHADOWED" != "Hello world" ]]; then
  echo "e2e: shadowed by \$PWD/hellopath — expected 'Hello world', got '$SHADOWED'" >&2
  exit 1
fi

echo "e2e: ok — a shadowed \$PATH invocation printed '$SHADOWED'"
