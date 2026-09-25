#!/usr/bin/env bash
#
# The end-to-end check: package the example application around a real runner stub, run the
# result, and require it to behave as a command should.
#
# This is the only stage that exercises the whole chain at once — a stub built (or fetched)
# from this repository, an `ETHRCFG` record and an application JAR joined to it by the `xeq`
# builder script, a daemon started over the launcher protocol, and its output carried back. It is
# also the only stage that needs a daemon implementation, which is why it lives here and not in
# the test suite.
#
# Beyond the greeting, the cases exercise what the launcher does *around* an invocation — the
# things a daemon cannot do for itself and a unit test cannot see: end-of-file on a piped stdin,
# argument values reaching the application intact, exit statuses, and signals in each of their
# delivery paths. Each case is one shell interaction, of the kind a user would have.
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

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

failures=0

fail() {
  echo "e2e: FAIL — $1" >&2
  failures=$((failures + 1))
}

pass() {
  echo "e2e: ok — $1"
}

# Runs a command under a deadline, so a regression that hangs — an invocation that never sees
# the end of its input, say — fails rather than blocking the build. `exec` replaces perl with
# the command, so the alarm kills the command itself; a shell's `wait` reports 142 for it.
limit() {
  perl -e 'alarm shift @ARGV; exec @ARGV or exit 127' "$@"
}

# `command` in the background, with stdout captured. A backgrounded command has no terminal
# on stdin, so this is the piped-stdin delivery path for signals: SIGINT arrives as a `Signal`
# document, never as a byte. A shell without job control starts a background command with
# INT and QUIT *ignored*, which the launcher honours (see the HUP case below), so perl puts
# the default dispositions back before the exec.
started() {
  perl -e '$SIG{INT} = "DEFAULT"; $SIG{QUIT} = "DEFAULT"; exec @ARGV or exit 127' "$@" \
    > "$TMP/out" 2> "$TMP/err" &
  BG=$!
  # Handlers are installed once the launcher has connected; allow a warm daemon a moment.
  sleep 1
}

expect() {  # expect <description> <expected> <actual>
  if [[ "$3" == "$2" ]]; then pass "$1"; else fail "$1: expected '$2', got '$3'"; fi
}

expect_status() {  # expect_status <description> <expected> <actual>
  if [[ "$3" == "$2" ]]; then pass "$1"; else fail "$1: expected status $2, got $3"; fi
}

echo "e2e: running $OUT"

# The greeting: the whole chain works at all.
expect "prints the greeting" "Hello world" "$("$OUT")"

# A piped stdin reaches end-of-file (#1): without the half-close, `cat` never returns.
actual=$(printf 'one\ntwo\n' | limit 20 "$OUT" cat) || true
expect "a piped stdin reaches EOF" "$(printf 'one\ntwo')" "$actual"

# An empty stdin does too.
actual=$(limit 20 "$OUT" cat < /dev/null) || true
expect "an empty stdin reaches EOF" "" "$actual"

# `--download` is the launcher's only when it is the sole argument (#6).
expect "--download after another argument reaches the application" \
  "$(printf 'install\n--download')" "$("$OUT" args install --download)"
expect "--download after -- reaches the application" \
  "$(printf -- '--\n--download')" "$("$OUT" args -- --download)"

# The wrapper mode is selected by an environment variable, so its old sentinel is an
# ordinary argument (#6).
expect "{wrap-java} reaches the application" "{wrap-java}" "$("$OUT" args '{wrap-java}')"

# An argument that is not UTF-8 no longer aborts the launcher (#4): it arrives with U+FFFD
# in place of the byte, as the JVM itself would decode it.
actual=$("$OUT" args $'ok\xff' && echo "/$?") || echo "/$?"
expect "a non-UTF-8 argument does not abort the launcher" "$(printf 'ok\xef\xbf\xbd')/0" "$actual"

# Exit statuses and stderr are carried back.
status=0; "$OUT" exit 3 || status=$?
expect_status "the exit status is carried back" 3 "$status"
expect "stderr is carried back" "oops" "$("$OUT" stderr oops 2>&1 >/dev/null)"

# A forwarded signal reaches the application as a `Signal` document.
started "$OUT" signal; kill -USR1 "$BG"; wait "$BG" || true
expect "USR1 is forwarded" "USR1" "$(cat "$TMP/out")"

# QUIT is in the forwarded set (#12).
started "$OUT" signal; kill -QUIT "$BG"; wait "$BG" || true
expect "QUIT is forwarded" "QUIT" "$(cat "$TMP/out")"

# A stop and a continue are survived, and both are reported to the application (#12).
started "$OUT" signal; kill -TSTP "$BG"; sleep 0.5; kill -CONT "$BG" 2>/dev/null || true
status=0; wait "$BG" || status=$?
expect "TSTP is forwarded before stopping" "TSTP" "$(cat "$TMP/out")"
expect_status "a stopped and continued invocation completes" 0 "$status"

# An accepted TERM ends the launcher by that signal, once stderr has drained (#5): the
# shell reports 143, not 1.
started "$OUT" sleep 30; kill -TERM "$BG"; status=0; wait "$BG" || status=$?
expect_status "an accepted TERM reports 128+15" 143 "$status"

# A rejected signal — `sleep` traps only TERM — takes its default action.
started "$OUT" sleep 30; kill -INT "$BG"; status=0; wait "$BG" || status=$?
expect_status "a rejected INT reports 128+2" 130 "$status"

# A signal inherited ignored stays ignored (#3): under `trap '' HUP` the launcher installs no
# handler, the daemon is never told, and the launcher survives to print the timeout.
( trap '' HUP; exec "$OUT" signal > "$TMP/out" 2> "$TMP/err" ) &
BG=$!; sleep 1; kill -HUP "$BG"; status=0; wait "$BG" || status=$?
expect "an ignored HUP is not forwarded" "(timeout)" "$(cat "$TMP/out")"
expect_status "an ignored HUP does not kill the launcher" 0 "$status"

# A background job under job control completes instead of stopping on SIGTTOU (#2). With
# stdin on a terminal the job is in a background process group; elsewhere the case is the
# same invocation without the hazard, and passes trivially.
set -m
"$OUT" > "$TMP/out" 2> "$TMP/err" &
BG=$!
set +m
for _ in $(seq 1 40); do
  [[ "$(cat "$TMP/out" 2>/dev/null)" == "Hello world" ]] && break
  sleep 0.25
done
if [[ "$(cat "$TMP/out")" == "Hello world" ]]; then
  pass "a background job completes"
else
  fail "a background job did not complete: $(jobs -l 2>/dev/null | tr '\n' ' ')"
  kill -KILL "$BG" 2>/dev/null || true
fi
wait "$BG" 2>/dev/null || true

if [[ "$failures" -ne 0 ]]; then
  echo "e2e: $failures failure(s)" >&2
  exit 1
fi

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

echo "e2e: all cases passed"
