#!/usr/bin/env bash
#
# Assemble the polyglot `xeq` builder script from its three shell sections and the launcher
# templates, and write `dist/xeq` and a byte-identical `dist/xeq.cmd`.
#
# The result is one file that runs as bash, cmd.exe and PowerShell (via res/core/xeq/xeq.tmpl),
# carrying the ten launcher templates in a base64 payload region so it can generate installer,
# online-launcher and dispatcher scripts with no other files. Publishing is a data change: the
# release version, base URL and stub hashes are baked in here.
#
# Usage: ./etc/ci/xeq-script-build.sh <version> <runners-url> <manifest.tsv> [out]
#          (or `make xeq-script`)

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

VERSION="${1:?usage: xeq-script-build.sh <version> <runners-url> <manifest.tsv> [out]}"
URL="${2:?missing runners base URL}"
MANIFEST="${3:?missing manifest tsv}"
OUT="${4:-dist/xeq}"

RES=res/core/xeq
SRC=src/script

# The launcher templates the builder embeds (uncompressed). xeq.tmpl carries the @@markers@@
# the builder substitutes the per-delivery templates into at generation time.
TEMPLATES=(
  xeq.tmpl
  xeq-installer.sh xeq-installer.bat xeq-installer.ps1
  xeq-onlinelauncher.sh xeq-onlinelauncher.bat xeq-onlinelauncher.ps1
  xeq-dispatcher.sh xeq-dispatcher.bat xeq-dispatcher.ps1
)

mkdir -p "$(dirname "$OUT")"
tmp=$(mktemp)

# 1. The polyglot prefix: xeq.tmpl with the builder's own three sections spliced in, and the
#    header's ${VERSION} filled.
VERSION="$VERSION" envsubst '$VERSION' < "$RES/xeq.tmpl" | awk \
  -v batf="$SRC/xeq-build.bat" -v ps1f="$SRC/xeq-build.ps1" -v shf="$SRC/xeq-build.sh" '
  function dump(f,  l){ while ((getline l < f) > 0) print l; close(f) }
  /@@BAT@@/ { dump(batf); next }
  /@@PS1@@/ { dump(ps1f); next }
  /@@SH@@/  { dump(shf);  next }
  { print }
' > "$tmp"

# 2. The payload region: baked metadata, then the manifest, then the templates.
{
  printf '# XEQ_VERSION=%s\n' "$VERSION"
  printf '# RUNNERS_URL=%s\n' "$URL"

  # runners:label=sha256,...  (from the tab-separated manifest, platform rows only)
  runners=$(awk -F'\t' '$1 ~ /^(linux|macos|windows)-/ {printf "%s%s=%s", sep, $1, $2; sep=","} END{print ""}' "$MANIFEST")
  printf 'runners:%s\n' "$runners"

  # Encode each template, computing 1-based line offsets relative to the index line, matching
  # the extractor in xeq-build.sh (`absline = index_num + off + 1`).
  staging=$(mktemp -d)
  index=""; offset=1
  for t in "${TEMPLATES[@]}"; do
    base64 < "$RES/$t" | tr -d '\r\n' | fold -w 8000 > "$staging/$t.b64"
    lines=$(wc -l < "$staging/$t.b64" | tr -d ' ')
    [ -s "$staging/$t.b64" ] && lines=$((lines + 1))   # fold leaves the last slice unterminated
    [ -n "$index" ] && index="$index,"
    index="$index$t=$offset"
    offset=$((offset + lines + 2))
  done
  printf 'index:%s\n' "$index"
  for t in "${TEMPLATES[@]}"; do
    printf -- '-----BEGIN CERTIFICATE-----\n'
    cat "$staging/$t.b64"
    printf '\n-----END CERTIFICATE-----\n'
  done
  rm -rf "$staging"

  printf '#>\n'
} >> "$tmp"

cp -f "$tmp" "$OUT"; chmod +x "$OUT"
cp -f "$tmp" "${OUT}.cmd"
rm -f "$tmp"
echo "xeq-script-build: wrote $OUT and ${OUT}.cmd ($(wc -c < "$OUT") bytes)"
