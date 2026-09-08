# The `ETHRCFG` configuration block

Every runner stub carries one 3764-byte static record, emitted into a dedicated section
(`__DATA,__ethereal` on macOS, `.ethereal` on Linux, `.rdata$ether` on Windows) so that it
survives linking as a contiguous, findable run of bytes. A builder turns a generic stub into an
application's launcher by finding the 8-byte magic and overwriting what follows; the runner
reads the block back with a volatile read, so the compiler cannot constant-fold the defaults
away.

Finding the block is a plain byte search for the magic. There is no offset table, and none is
needed: the magic is unique in the stub, and the record is fixed-length.

## Layout

All integers are little-endian.

| Offset | Length | Field | Meaning |
|---|---|---|---|
| 0 | 8 | magic | `ETHRCFG` followed by the format version byte, currently `2` |
| 8 | 8 | `build_id` | `u64`; orders upgrades — a `.pending` binary is accepted only if its build id is higher (unless downgrades are permitted) |
| 16 | 2 | `java_min` | `u16`; the minimum acceptable JVM major version. `0` means "unset", read as 21 |
| 18 | 2 | `java_pref` | `u16`; the preferred JVM major version to download when none is found. `0` means "unset", read as 24 |
| 20 | 1 | `bundle` | `0` = JRE, `1` = JDK — which runtime to fetch if one must be fetched |
| 21 | 1 | `flags` | bit 0 = downgrade permitted; all other bits reserved and zero |
| 22 | 10 | reserved | zero |
| 32 | 1312 | `public_key` | The ML-DSA-44 public key upgrades are verified against. All-zero disables upgrades, and is the safe default |
| 1344 | 2420 | `signature` | The ML-DSA-44 signature over the binary. Zero in a running binary; set only by the signer, and zeroed again by the verifier before it recomputes |

Total: 3764 bytes.

## Building an executable

1. Read the bare stub for the target platform.
2. Find `ETHRCFG` + version byte `2`. Absence of the magic is a hard error: the file is not a
   runner stub, or is a stub of another format version.
3. Overwrite the metadata at offsets 8–32 and, if upgrades are to be enabled, the public key at
   32–1344. Leave the signature region zero.
4. Append the application JAR to the end of the file, unmodified.
5. On macOS, re-sign the result (`codesign --sign - --force`). An unsigned or stale-signed
   Mach-O will not execute on Apple silicon. Note that signing *changes the file's length*, so
   anything derived from the length must be measured after this step, not before.
6. Mark the file executable.

The JAR is found at run time by scanning back from the end of the file for the ZIP end-of-
central-directory record, so nothing records where the stub ends and the JAR begins.

### ZIP64 and the appended JAR

A ZIP's central directory holds absolute offsets. Appending it to a stub moves it, so a JAR
whose offsets are already materialised — in particular a ZIP64 JAR, whose end-of-central-
directory locator holds one physical offset — must have those offsets rebased by the amount the
JAR was shifted, or the JVM will refuse to open the executable at all. The macOS signing step
in stage 5 grows the prefix, so the rebase amount must be computed after signing.

## Signing an upgrade

`ethereal-sign` (this repository, `src/sign`) produces the `.pending` binary an application's
self-upgrade path consumes:

- `ethereal-sign keygen --out <prefix>` writes a seed and the 1312-byte raw public key. The
  public key is what a builder patches into the block at offset 32; the seed stays secret.
- `ethereal-sign sign --key <seed> --in <binary> --out <signed>` zeroes the signature region,
  signs the whole file, and writes the signature back at offset 1344.

The verifier reverses exactly that: zero the region, verify the signature over the rest against
the public key baked into the *running* binary — not the incoming one — and reject on any
mismatch, on a build id that does not advance, or when the running binary's key is all zeros.

## Changing the layout

Any change to the field layout increments the version byte in the magic (`ETHRCFG\x03`), which
makes every existing builder fail to find its magic in a new stub and every new builder fail to
find its magic in an old one. Adding a meaning to a reserved byte, where zero keeps the old
behaviour, does not.

## Implementations

- `src/runner/src/config.rs` — the reader, in the stub.
- `src/sign/src/main.rs` — the signer.
- `src/packager/xeq.Assembler.scala` — the builder in this repository's packager.
- `ethereal.Assembler` in Soundness — the builder in the daemon's own self-packaging path
  (`java -Dbuild.executable=… -jar app.jar`), written against this document.
