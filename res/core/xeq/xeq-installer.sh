os=$(uname -s)
arch=$(uname -m)

case "$os" in
  Darwin) os=macos ;; Linux) os=linux ;;
  *)      printf "Unsupported OS: %s\n" "$os" >&2; exit 1 ;;
esac

case "$arch" in
  x86_64)         arch=x64 ;;
  aarch64|arm64)  arch=arm64 ;;
  *)              printf "Unsupported architecture: %s\n" "$arch" >&2; exit 1 ;;
esac

# Add $2 to a ZIP64 EOCD locator's physical offset in file $1, if present.
xeq_rebase_zip64() {
  file=$1; delta=$2
  size=$(wc -c < "$file" | tr -d ' ')
  n=65557; [ "$size" -lt "$n" ] && n=$size
  hex=" $(tail -c "$n" "$file" | od -An -v -tx1 | tr -s ' \n' ' ')"
  before=${hex% 50 4b 05 06*}
  [ "$before" = "$hex" ] && return 0
  eocd=$(( size - n + (${#before} - 1) / 3 ))
  [ "$eocd" -ge 20 ] || return 0
  sig=$(dd if="$file" bs=1 skip=$((eocd - 20)) count=4 2>/dev/null | od -An -v -tx1 | tr -d ' \n')
  [ "$sig" = "504b0607" ] || return 0
  old=0; i=7
  set -- $(dd if="$file" bs=1 skip=$((eocd - 12)) count=8 2>/dev/null | od -An -v -tu1)
  while [ "$i" -ge 0 ]; do eval "b=\${$((i + 1))}"; old=$(( (old << 8) + b )); i=$((i - 1)); done
  new=$((old + delta)); s=''; i=0
  while [ "$i" -lt 8 ]; do s=$s$(printf '\\%03o' $(( (new >> (8*i)) & 255 ))); i=$((i+1)); done
  printf '%b' "$s" | dd of="$file" bs=1 seek=$((eocd - 12)) count=8 conv=notrunc 2>/dev/null
}

script="$(realpath "$0")"
output="$script"
tmpout=$(mktemp "${script}.XXXXXX")
indexline=$(grep -n "^index:" "$script" | head -1)
indexnum=${indexline%%:*}
indexcontent=${indexline#*index:}

get_offset() {
  printf '%s\n' "$indexcontent" | tr ',' '\n' | grep "^$1=" | cut -d= -f2
}

decode() {
  base64 -d 2>/dev/null || base64 -D;
}

extract() {
  local off=$1 decompress=$2 absline
  absline=$((indexnum + off + 1))
  if [ "$decompress" = "1" ]
  then
    tail -n +"$absline" "$script" | sed -n '/^-----END/q; p' | decode | gunzip
  else
    tail -n +"$absline" "$script" | sed -n '/^-----END/q; p' | decode
  fi
}

offset=$(get_offset "${os}-${arch}")

if [ -z "$offset" ]
then
  printf "No payload for %s-%s\n" "$os" "$arch" >&2
  exit 1
fi

xeq_msg 33 ████████ 0 "Unpacking…"
case "${os}-${arch}" in windows*) gz=0 ;; *) gz=1 ;; esac
extract "$offset" "$gz" > "$tmpout"
stubsize=$(wc -c < "$tmpout" | tr -d ' ')

# The ETHRCFG v3 record (spec/ethrcfg.md), embedded once, appended after the stub.
record_offset=$(get_offset "record")
if [ -n "$record_offset" ]
then extract "$record_offset" 0 >> "$tmpout"
fi

data_offset=$(get_offset "data")
if [ -n "$data_offset" ]
then extract "$data_offset" 0 >> "$tmpout"
fi

# Rebase the JAR's ZIP64 locator, if any, by the bytes now in front of it.
recsize=0; [ -n "$record_offset" ] && recsize=3764
xeq_rebase_zip64 "$tmpout" $((stubsize + recsize))

size=$(wc -c < "$tmpout" | tr -d ' ')
xeq_msg 32 ████████ 1 "Unpacked ($size bytes)"
chmod +x "$tmpout"
mv "$tmpout" "$output"
exec "$output" "$@"
