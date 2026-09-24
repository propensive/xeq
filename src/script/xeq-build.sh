# ---------------------------------------------------------------------------
# xeq — the reference XEQ builder (POSIX sh / bash section).
#
# Joins a bare runner stub, a 3764-byte ETHRCFG v3 record and an application
# JAR into a single executable (`stub ‖ record ‖ jar`; see spec/ethrcfg.md),
# and generates the polyglot launcher scripts. Everything here is portable
# across BSD (macOS) and GNU userlands; no python, perl or bashisms beyond
# what busybox provides.
#
# `xeq_msg` (colour progress on stderr) is defined by the shared header this
# file is spliced into; the payload region (templates + manifest) follows the
# `exit` at the end and is read back with `tail`/`sed`.
# ---------------------------------------------------------------------------

xeq_self=$0
case $xeq_self in */*) : ;; *) xeq_self=./$xeq_self ;; esac

xeq_die() { local code=$1; shift; printf 'xeq: %s\n' "$*" >&2; exit "$code"; }

# --- baked-in release metadata (from the payload region) -------------------
xeq_meta() { sed -n "s/^# $1=//p" "$xeq_self" | head -1; }
XEQ_VERSION=$(xeq_meta XEQ_VERSION)
XEQ_RUNNERS_URL=$(xeq_meta RUNNERS_URL)

# The baked-in `runners:` line (label=sha256,...) — the manifest for verified
# downloads when no local --runners directory is given.
xeq_baked_manifest() { sed -n 's/^runners://p' "$xeq_self" | head -1; }
xeq_baked_hash() {
  xeq_baked_manifest | tr ',' '\n' | sed -n "s/^$1=//p" | head -1
}

# --- cache -----------------------------------------------------------------
xeq_cache() {
  if [ -n "${XEQ_CACHE:-}" ]; then printf '%s' "$XEQ_CACHE"
  elif [ -n "${XDG_CACHE_HOME:-}" ]; then printf '%s/xeq' "$XDG_CACHE_HOME"
  else printf '%s/.cache/xeq' "$HOME"
  fi
}

xeq_sha256() { { sha256sum "$1" 2>/dev/null || shasum -a 256 "$1"; } | cut -d' ' -f1; }

xeq_download() { # url dest
  if command -v curl >/dev/null 2>&1; then curl -fsSL "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then wget -qO "$2" "$1"
  else xeq_die 3 "need curl or wget to download $1"
  fi
}

xeq_stub_name() { case $1 in windows*) printf 'runner-%s.exe' "$1" ;; *) printf 'runner-%s' "$1" ;; esac; }

# Resolve a bare stub for a label to a local path, echoing the path on stdout.
# From --runners <dir> (unverified) when given, else the cache, else a verified
# download from the release.
xeq_resolve_stub() { # label
  local label name path base want cachedir tmp got
  label=$1
  name=$(xeq_stub_name "$label")
  if [ -n "$opt_runners_dir" ]; then
    path=$opt_runners_dir/$name
    [ -f "$path" ] || xeq_die 2 "no stub $name in $opt_runners_dir"
    printf '%s' "$path"; return 0
  fi
  base=${opt_runners_url:-$XEQ_RUNNERS_URL}
  [ -n "$base" ] || xeq_die 2 "no runner source: pass --runners <dir> or --runners-url <url>"
  if [ -n "$opt_runners_manifest" ]; then
    want=$(tr -d '\r' < "$opt_runners_manifest" | sed -n "s/^$label	//p; s/^$label=//p" | head -1)
  else
    want=$(xeq_baked_hash "$label")
  fi
  cachedir=$(xeq_cache)/xeq-${XEQ_VERSION:-0}
  mkdir -p "$cachedir"
  path=$cachedir/$name
  if [ -f "$path" ] && [ -n "$want" ] && [ "$(xeq_sha256 "$path")" = "$want" ]; then
    printf '%s' "$path"; return 0
  fi
  xeq_msg 33 ████████ 0 "Fetching $name…"
  tmp=$path.part.$$
  xeq_download "$base/$name" "$tmp" || xeq_die 3 "download failed: $base/$name"
  if [ -n "$want" ]; then
    got=$(xeq_sha256 "$tmp")
    [ "$got" = "$want" ] || { rm -f "$tmp"; xeq_die 2 "SHA-256 mismatch for $name (got $got, want $want)"; }
  fi
  mv "$tmp" "$path"
  xeq_msg 32 ████████ 1 "Fetched $name"
  printf '%s' "$path"
}

# --- record ----------------------------------------------------------------
# Emit a little-endian integer of $2 bytes for value $1, as raw bytes.
xeq_le() {
  local v n i s
  v=$1 n=$2 i=0 s=
  while [ "$i" -lt "$n" ]; do
    s=$s$(printf '\\%03o' $(( (v >> (8 * i)) & 255 )))
    i=$((i + 1))
  done
  printf '%b' "$s"
}

# Write the 3764-byte ETHRCFG v3 record to $1.
xeq_write_record() { # out
  local out sz
  out=$1
  {
    printf 'ETHRCFG\003'
    xeq_le "$opt_build_id" 8
    xeq_le "$opt_java_min" 2
    xeq_le "$opt_java_pref" 2
    xeq_le "$opt_bundle" 1
    xeq_le "$opt_flags" 1
    head -c 10 /dev/zero
    if [ -n "$opt_pubkey" ]; then
      sz=$(wc -c < "$opt_pubkey" | tr -d ' ')
      [ "$sz" -eq 1312 ] || xeq_die 2 "public key must be 1312 bytes, is $sz"
      head -c 1312 "$opt_pubkey"
    else
      head -c 1312 /dev/zero
    fi
    head -c 2420 /dev/zero
  } > "$out"
  sz=$(wc -c < "$out" | tr -d ' ')
  [ "$sz" -eq 3764 ] || xeq_die 2 "record is $sz bytes, expected 3764"
}

# --- ZIP64 locator rebase --------------------------------------------------
# If $1 ends with a ZIP64 EOCD locator, add $2 to its physical offset (the u64
# at locator+8). No-op for a plain (non-ZIP64) JAR.
xeq_rebase_zip64() { # file delta
  local file delta size n hex before eocd sig old i b new
  file=$1 delta=$2
  size=$(wc -c < "$file" | tr -d ' ')
  n=65557; [ "$size" -lt "$n" ] && n=$size
  # Hex of the tail, space-separated so each byte is exactly 3 chars ("XX ").
  hex=" $(tail -c "$n" "$file" | od -An -v -tx1 | tr -s ' \n' ' ')"
  before=${hex% 50 4b 05 06*}
  [ "$before" = "$hex" ] && return 0   # not a zip (or no EOCD): nothing to rebase
  eocd=$(( size - n + (${#before} - 1) / 3 ))
  [ "$eocd" -ge 20 ] || return 0
  sig=$(dd if="$file" bs=1 skip=$((eocd - 20)) count=4 2>/dev/null | od -An -v -tx1 | tr -d ' \n')
  [ "$sig" = "504b0607" ] || return 0
  old=0 i=7
  set -- $(dd if="$file" bs=1 skip=$((eocd - 12)) count=8 2>/dev/null | od -An -v -tu1)
  while [ "$i" -ge 0 ]; do
    eval "b=\${$((i + 1))}"
    old=$(( (old << 8) + b ))
    i=$((i - 1))
  done
  new=$((old + delta))
  xeq_le "$new" 8 | dd of="$file" bs=1 seek=$((eocd - 12)) count=8 conv=notrunc 2>/dev/null
}

# --- native build ----------------------------------------------------------
xeq_build_native() { # stub-path out
  local stub out dir base tmp stubsize
  stub=$1 out=$2
  dir=$(dirname "$out"); base=$(basename "$out")
  tmp=$dir/.$base.tmp.$$
  xeq_write_record "$dir/.$base.rec.$$"
  stubsize=$(wc -c < "$stub" | tr -d ' ')
  cat "$stub" "$dir/.$base.rec.$$" "$opt_jar" > "$tmp"
  rm -f "$dir/.$base.rec.$$"
  xeq_rebase_zip64 "$tmp" $((stubsize + 3764))
  case $opt_target in windows*) : ;; *) chmod +x "$tmp" ;; esac
  mv -f "$tmp" "$out"
}

# --- template extraction (from this script's payload region) ---------------
# The launcher templates are embedded after `exit`, framed exactly like the
# installer's payloads: an `index:` line of 1-based line offsets, then each
# payload between `-----BEGIN CERTIFICATE-----`/`-----END CERTIFICATE-----`,
# base64, uncompressed. `xeq_template <name>` writes the decoded template to
# stdout.
xeq_index_line=
xeq_index_num=
xeq_load_index() {
  [ -n "$xeq_index_num" ] && return 0
  line=$(grep -n '^index:' "$xeq_self" | head -1)
  xeq_index_num=${line%%:*}
  xeq_index_line=${line#*index:}
}
xeq_template() { # name
  local off absline
  xeq_load_index
  off=$(printf '%s\n' "$xeq_index_line" | tr ',' '\n' | sed -n "s/^$1=//p" | head -1)
  [ -n "$off" ] || xeq_die 2 "no embedded template $1"
  absline=$(( xeq_index_num + off + 1 ))
  tail -n +"$absline" "$xeq_self" | sed -n '/^-----END/q; p' | { base64 -d 2>/dev/null || base64 -D; }
}

xeq_b64() { { base64 2>/dev/null < "$1"; } | tr -d '\r\n' | fold -w 8000; }
xeq_b64_gz() { gzip -n -c "$1" | { base64 2>/dev/null; } | tr -d '\r\n' | fold -w 8000; }

# Build the polyglot prefix for a delivery ($1 = installer|onlinelauncher|dispatcher)
# by substituting the three per-delivery templates into xeq.tmpl at the markers.
xeq_prefix() { # delivery
  local d tmpl bat ps1 sh
  d=$1
  tmpl=$(mktemp); bat=$(mktemp); ps1=$(mktemp); sh=$(mktemp)
  xeq_template xeq.tmpl > "$tmpl"
  xeq_template "xeq-$d.bat" > "$bat"
  xeq_template "xeq-$d.ps1" > "$ps1"
  xeq_template "xeq-$d.sh"  > "$sh"
  awk -v batf="$bat" -v ps1f="$ps1" -v shf="$sh" '
    function dump(f,  l){ while ((getline l < f) > 0) print l; close(f) }
    /@@BAT@@/ { dump(batf); next }
    /@@PS1@@/ { dump(ps1f); next }
    /@@SH@@/  { dump(shf);  next }
    { print }
  ' "$tmpl"
  rm -f "$tmpl" "$bat" "$ps1" "$sh"
}

# Emit an `index:` line plus framed payloads. Args: pairs of "label:file:gz"
# where gz is 1 to gzip. Writes to stdout.
xeq_emit_payloads() {
  local tmpdir offset index n spec label rest file gz lines i
  tmpdir=$(mktemp -d)
  offset=1 ; index=
  n=0
  for spec in "$@"; do
    label=${spec%%:*}; rest=${spec#*:}; file=${rest%%:*}; gz=${rest##*:}
    n=$((n + 1))
    if [ "$gz" = 1 ]; then xeq_b64_gz "$file" > "$tmpdir/$n"; else xeq_b64 "$file" > "$tmpdir/$n"; fi
    lines=$(wc -l < "$tmpdir/$n" | tr -d ' ')
    # `fold` leaves no trailing newline on the last slice; count it.
    [ -s "$tmpdir/$n" ] && lines=$((lines + 1))
    [ -n "$index" ] && index="$index,"
    index="$index$label=$offset"
    offset=$((offset + lines + 2))
    eval "file_$n=\$tmpdir/\$n"
  done
  printf 'index:%s\n' "$index"
  i=0
  for spec in "$@"; do
    i=$((i + 1))
    printf -- '-----BEGIN CERTIFICATE-----\n'
    eval "cat \"\$file_$i\""
    printf '\n-----END CERTIFICATE-----\n'
  done
  rm -rf "$tmpdir"
}

# --- delivery: embed-all (offline installer) -------------------------------
xeq_build_embedall() { # out
  local out rec label stub gz
  out=$1
  rec=$(mktemp); xeq_write_record "$rec"
  set --
  for label in $opt_targets; do
    stub=$(xeq_resolve_stub "$label")
    case $label in windows*) gz=0 ;; *) gz=1 ;; esac
    set -- "$@" "$label:$stub:$gz"
  done
  set -- "$@" "record:$rec:0" "data:$opt_jar:0"
  { xeq_prefix installer; xeq_emit_payloads "$@"; printf '#%s\n' '>'; } > "$out"
  chmod +x "$out"
  rm -f "$rec"
}

# --- delivery: download (online launcher) ----------------------------------
xeq_build_download() { # out
  local out base rec assets label name hash
  out=$1
  base=${opt_runners_url:-$XEQ_RUNNERS_URL}
  [ -n "$base" ] || xeq_die 2 "download delivery needs a runner URL (--runners-url or baked-in)"
  rec=$(mktemp); xeq_write_record "$rec"
  assets=
  for label in $opt_targets; do
    name=$(xeq_stub_name "$label")
    if [ -n "$opt_runners_manifest" ]; then
      hash=$(tr -d '\r' < "$opt_runners_manifest" | sed -n "s/^$label	//p; s/^$label=//p" | head -1)
    else hash=$(xeq_baked_hash "$label"); fi
    [ -n "$hash" ] || xeq_die 2 "no hash for $label"
    [ -n "$assets" ] && assets="$assets,"
    assets="$assets$label=$base/$name|$hash"
  done
  {
    xeq_prefix onlinelauncher
    xeq_emit_payloads "record:$rec:0" "data:$opt_jar:0"
    printf 'assets:%s\n' "$assets"
    printf '#%s\n' '>'
  } > "$out"
  chmod +x "$out"
  rm -f "$rec"
}

# --- delivery: dispatch ----------------------------------------------------
xeq_build_dispatch() { # out manifest(label\turl\tsha256)
  local out manifest assets label url hash
  out=$1 manifest=$2
  assets=
  while IFS=$(printf '\t') read -r label url hash; do
    [ -z "$label" ] && continue
    [ -n "$assets" ] && assets="$assets,"
    assets="$assets$label=$url|$hash"
  done < "$manifest"
  { xeq_prefix dispatcher; printf 'assets:%s\n' "$assets"; printf '#%s\n' '>'; } > "$out"
  chmod +x "$out"
}

# --- option defaults & parsing ---------------------------------------------
opt_jar= ; opt_out= ; opt_target= ; opt_targets= ; opt_manifest=
opt_runners_dir= ; opt_runners_url= ; opt_runners_manifest=
opt_java_min=21 ; opt_java_pref=24 ; opt_bundle=0 ; opt_flags=0 ; opt_build_id=0 ; opt_pubkey=

xeq_host_target() {
  os=$(uname -s); arch=$(uname -m)
  case $os in Darwin) os=macos ;; Linux) os=linux ;; *) os=linux ;; esac
  case $arch in arm64|aarch64) arch=arm64 ;; *) arch=x64 ;; esac
  printf '%s-%s' "$os" "$arch"
}

xeq_all_labels() {
  if [ -n "$opt_runners_manifest" ]; then
    tr -d '\r' < "$opt_runners_manifest" | sed 's/[=	].*//' | grep .
  else
    xeq_baked_manifest | tr ',' '\n' | sed 's/=.*//' | grep .
  fi
}

xeq_parse() {
  while [ $# -gt 0 ]; do
    case $1 in
      --jar) opt_jar=$2; shift 2 ;;
      --out) opt_out=$2; shift 2 ;;
      --target) opt_target=$2; opt_targets="$opt_targets $2"; shift 2 ;;
      --manifest) opt_manifest=$2; shift 2 ;;
      --runners) opt_runners_dir=$2; shift 2 ;;
      --runners-url) opt_runners_url=$2; shift 2 ;;
      --runners-manifest) opt_runners_manifest=$2; shift 2 ;;
      --java-min) opt_java_min=$2; shift 2 ;;
      --java-pref) opt_java_pref=$2; shift 2 ;;
      --jdk) opt_bundle=1; shift ;;
      --jre) opt_bundle=0; shift ;;
      --build-id) opt_build_id=$2; shift 2 ;;
      --public-key) opt_pubkey=$2; shift 2 ;;
      --allow-downgrade) opt_flags=1; shift ;;
      --) shift; break ;;
      -*) xeq_die 1 "unknown option: $1" ;;
      *) xeq_die 1 "unexpected argument: $1" ;;
    esac
  done
}

xeq_usage() {
  cat >&2 <<USAGE
xeq ${XEQ_VERSION:-?} — build XEQ executables and launchers

  xeq build     --jar F --out F [--target L] [record opts] [stub opts]
  xeq embed-all --jar F --out F [--target L ...] [record opts] [stub opts]
  xeq download  --jar F --out F [--target L ...] [record opts] [--runners-url U]
  xeq dispatch  --out F --manifest TSV
  xeq record    --out F [record opts]
  xeq fetch     [--target L ...] [--runners DIR]
  xeq version | help

record opts: --java-min N --java-pref N --jdk|--jre --build-id N --public-key F --allow-downgrade
stub opts:   --runners DIR | --runners-url U [--runners-manifest TSV]
USAGE
}

xeq_main() {
  local cmd stub label
  [ $# -eq 0 ] && { xeq_usage; exit 1; }
  cmd=$1; shift
  case $cmd in
    version) printf 'xeq %s\n' "${XEQ_VERSION:-unknown}"; exit 0 ;;
    help|-h|--help) xeq_usage; exit 0 ;;
  esac
  xeq_parse "$@"
  case $cmd in
    build)
      [ -n "$opt_jar" ] && [ -n "$opt_out" ] || xeq_die 1 "build needs --jar and --out"
      [ -n "$opt_target" ] || opt_target=$(xeq_host_target)
      stub=$(xeq_resolve_stub "$opt_target")
      xeq_build_native "$stub" "$opt_out"
      ;;
    embed-all)
      [ -n "$opt_jar" ] && [ -n "$opt_out" ] || xeq_die 1 "embed-all needs --jar and --out"
      [ -n "$(printf '%s' "$opt_targets" | tr -d ' ')" ] || opt_targets=$(xeq_all_labels)
      xeq_build_embedall "$opt_out"
      ;;
    download)
      [ -n "$opt_jar" ] && [ -n "$opt_out" ] || xeq_die 1 "download needs --jar and --out"
      [ -n "$(printf '%s' "$opt_targets" | tr -d ' ')" ] || opt_targets=$(xeq_all_labels)
      xeq_build_download "$opt_out"
      ;;
    dispatch)
      [ -n "$opt_out" ] && [ -n "$opt_manifest" ] || xeq_die 1 "dispatch needs --out and --manifest"
      xeq_build_dispatch "$opt_out" "$opt_manifest"
      ;;
    record)
      [ -n "$opt_out" ] || xeq_die 1 "record needs --out"
      xeq_write_record "$opt_out"
      ;;
    fetch)
      [ -n "$(printf '%s' "$opt_targets" | tr -d ' ')" ] || opt_targets=$(xeq_all_labels)
      for label in $opt_targets; do xeq_resolve_stub "$label" >/dev/null; done
      ;;
    *) xeq_die 1 "unknown command: $cmd" ;;
  esac
  exit 0
}

xeq_main "$@"
