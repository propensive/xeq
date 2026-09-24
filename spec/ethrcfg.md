# The `ETHRCFG` configuration record

An XEQ executable is three files joined end to end:

```
stub ‖ record ‖ jar
```

The *stub* is a bare, generic runner for one platform, published as-is and never modified. The
*record* is a fixed 3764-byte block, written by the builder, that configures the runner for
one application: the build id that orders upgrades, the Java version policy, and the public
key upgrades are verified against. The *jar* is the application, unmodified.

Building an executable is therefore concatenation. Nothing inside the stub is patched, so the
stub's own code signature (macOS) stays valid, and a macOS executable can be built on any host
without a signing tool.

## Finding the record

The runner reads its own executable — the path it was invoked as, which is also the path it
hands the JVM as the JAR — forwards from byte 0, and takes the first occurrence of the 8-byte
magic `ETHRCFG\x03` as the start of the record, provided 3764 bytes remain from there. A
verifier checking an upgrade does exactly the same on the candidate file.

Because the record precedes the JAR, "first occurrence" is the record whatever the JAR
contains. That rests on one invariant: **a stub contains the magic nowhere**. The runner
reassembles the magic at run time from an obfuscated constant so that the compiler cannot
place a literal copy in the binary, and the stub build (`etc/ci/runners-build.sh`) refuses to
publish a stub in which the bytes `ETHRCFG` occur at all.

A runner that finds no record — a bare stub run directly, or a mis-built file — uses the
defaults in the table below, under which self-upgrade is disabled.

## Layout

All integers are little-endian.

| Offset | Length | Field | Meaning |
|---|---|---|---|
| 0 | 8 | magic | `ETHRCFG` followed by the format version byte, currently `3` |
| 8 | 8 | `build_id` | `u64`; orders upgrades — a `.pending` binary is accepted only if its build id is higher (unless downgrades are permitted). Default `0` |
| 16 | 2 | `java_min` | `u16`; the minimum acceptable JVM major version. `0` means "unset", read as 21 |
| 18 | 2 | `java_pref` | `u16`; the preferred JVM major version to download when none is found. `0` means "unset", read as 24 |
| 20 | 1 | `bundle` | `0` = JRE, `1` = JDK — which runtime to fetch if one must be fetched |
| 21 | 1 | `flags` | bit 0 = downgrade permitted; all other bits reserved and zero |
| 22 | 10 | reserved | zero |
| 32 | 1312 | `public_key` | The ML-DSA-44 public key upgrades are verified against. All-zero disables upgrades, and is the safe default |
| 1344 | 2420 | `signature` | The ML-DSA-44 signature over the whole file. Zero in a running binary; set only by the signer, and zeroed again by the verifier before it recomputes |

Total: 3764 bytes.

## Building an executable

1. Obtain the bare stub for the target platform.
2. Write a record: the magic, the fields above, the public key (or 1312 zero bytes), and 2420
   zero bytes of signature.
3. Concatenate stub, record and JAR, in that order, and mark the result executable (not on
   Windows, where the name's `.exe` suffix does that).
4. If the JAR has a ZIP64 end-of-central-directory locator — the 20-byte block starting
   `PK\x06\x07` immediately before the end-of-central-directory record — add
   `size(stub) + 3764` to the `u64` at offset 8 within the locator. That is the one physical
   offset in a ZIP; every other offset is relative and a reader recovers the shift by itself.
   Left stale, the JVM refuses to open the JAR at all.

That is all. In a POSIX shell the whole of step 3 is `cat stub record app.jar > mytool`, and in
`cmd.exe` it is `copy /b stub+record+app.jar mytool.exe`. The reference builder is the `xeq`
script published with every runner release (`src/script`), which does steps 1–4 and generates
the polyglot launcher scripts; nothing in it is more than a few lines of shell.

### Why nothing is signed

A Mach-O's ad-hoc signature covers the pages of the Mach-O image up to the signature blob.
Bytes appended after the image are neither mapped nor hashed, so appending leaves the
signature valid — which is why the stubs are signed once, when published, and never again.
Windows PE loaders likewise ignore trailing data. Authenticode is not used; if it ever were,
it would have to be applied to the finished executable rather than the stub.

The JAR is found at run time by scanning back from the end of the file for the ZIP end-of-
central-directory record, so nothing records where the record ends and the JAR begins.

## Signing an upgrade

`ethereal-sign` (this repository, `src/sign`) produces the `.pending` binary an application's
self-upgrade path consumes:

- `ethereal-sign keygen --out <prefix>` writes a seed and the 1312-byte raw public key. The
  public key is what a builder writes into the record at offset 32; the seed stays secret.
- `ethereal-sign sign --key <seed> --in <binary> --out <signed>` finds the record, zeroes the
  signature region, signs the whole file, and writes the signature back at offset 1344.

The verifier reverses exactly that: zero the region, verify the signature over the rest against
the public key in the *running* binary's record — not the incoming one — and reject on any
mismatch, on a build id that does not advance, or when the running binary's key is all zeros.

## Changing the layout

Any change to the field layout or to the placement rule increments the version byte in the
magic (`ETHRCFG\x04`), which makes every existing builder, runner and verifier fail to find
its magic in a file of the other version. Adding a meaning to a reserved byte, where zero
keeps the old behaviour, does not.

Version 2, used by releases up to `runners-0.5`, held the record as a static inside the stub
that a builder byte-patched in place. A v2 runner never finds a v3 record and a v3 runner
never finds a v2 record, so the two cannot upgrade into each other; see
[`COMPATIBILITY.md`](COMPATIBILITY.md).

## Implementations

- `src/runner/src/config.rs` — the reader, in the stub.
- `src/runner/src/verify.rs` — the verifier.
- `src/sign/src/main.rs` — the signer.
- `src/script` — the builder: the `xeq` script published with each release.
