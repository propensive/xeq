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

label="${os}-${arch}"
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

s=$(realpath "$0")
row=$(sed -n 's/^assets://p' "$s" | head -1 | tr ',' '\n' | grep "^${label}=" | head -1)
if [ -z "$row" ]
then printf "No runner for %s\n" "$label" >&2; exit 1
fi
value=${row#*=}
url=${value%%|*}
hash=${value#*|}
t="$s.tmp"
xeq_msg 33 ████████ 0 "Downloading runner…"
if command -v curl >/dev/null 2>&1
then curl -fsSL "$url" -o "$t" || exit 1
elif command -v wget >/dev/null 2>&1
then wget -qO "$t" "$url" || exit 1
else printf 'Need curl or wget\n' >&2; exit 1
fi
size=$(wc -c < "$t" | tr -d ' ')
xeq_msg 32 ████████ 1 "Downloaded $size bytes"
xeq_msg 33 ████████ 0 "Verifying SHA-256…"
g=$( { sha256sum "$t" 2>/dev/null || shasum -a 256 "$t"; } | cut -d' ' -f1)
if [ "$g" != "$hash" ]
then printf 'Hash mismatch\n' >&2; rm -f "$t"; exit 1
fi
xeq_msg 32 ████████ 1 "Verified SHA-256"

# Append the embedded application JAR (the `data` payload) to the downloaded stub, exactly
# as the offline installer does, turning the bare runner into the self-contained executable.
xeq_msg 33 ████████ 0 "Assembling…"
indexline=$(grep -n "^index:" "$s" | head -1)
indexnum=${indexline%%:*}
indexcontent=${indexline#*index:}
data_offset=$(printf '%s\n' "$indexcontent" | tr ',' '\n' | grep "^data=" | cut -d= -f2)
if [ -z "$data_offset" ]
then printf 'No embedded data payload\n' >&2; rm -f "$t"; exit 1
fi
stubsize=$(wc -c < "$t" | tr -d ' ')
record_offset=$(printf '%s\n' "$indexcontent" | tr ',' '\n' | grep "^record=" | cut -d= -f2)
recsize=0
if [ -n "$record_offset" ]
then
  rabs=$((indexnum + record_offset + 1))
  tail -n +"$rabs" "$s" | sed -n '/^-----END/q; p' | { base64 -d 2>/dev/null || base64 -D; } >> "$t"
  recsize=3764
fi
absline=$((indexnum + data_offset + 1))
tail -n +"$absline" "$s" | sed -n '/^-----END/q; p' | { base64 -d 2>/dev/null || base64 -D; } >> "$t"
xeq_rebase_zip64 "$t" $((stubsize + recsize))
xeq_msg 32 ████████ 1 "Assembled"

chmod +x "$t"
mv "$t" "$s"
exec "$s" "$@"
