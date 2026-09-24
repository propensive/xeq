# Compatibility

A launcher and a daemon interoperate only if they agree on the protocol schema, byte for byte.
Both carry its signature and compare it on every document, so a mismatched pair fails at the
first message rather than misreading fields.

| Release | Protocol | Schema signature (BLAKE3-256 + cadence) | `ETHRCFG` | Reference daemon |
|---|---|---|---|---|
| `xeq-0.7` | `ethereal-launcher` BinTEL | `eeced165c15f73119cf7710812671924aa558722927d29f37538e7b3953296c2ce` | v3 | Soundness ≥ 0.69.0 (`ethereal-core`) |
| `xeq-0.6` | `ethereal-launcher` BinTEL | `4701ec19cd0fd3ecfc0e1b8a6525b4edc3a3b1deda370f681986db9aa39c1da692` | v3 | Soundness ≥ 0.65.0 (`ethereal-core`) |
| `runners-0.5` | `ethereal-launcher` BinTEL | `4701ec19cd0fd3ecfc0e1b8a6525b4edc3a3b1deda370f681986db9aa39c1da692` | v2 | Soundness ≥ 0.65.0 (`ethereal-core`) |
| `runners-0.4` | `ethereal-launcher` BinTEL | `4701ec19cd0fd3ecfc0e1b8a6525b4edc3a3b1deda370f681986db9aa39c1da692` | v2 | Soundness 0.65.0 (`ethereal-core`) |
| `runners-0.3` and earlier | line-oriented (`i`/`e`/`m`/`s`/`x`/`v` opcodes) | — | v2 | Soundness ≤ 0.64.0 |

Releases up to and including `runners-0.5` were published from the Soundness repository, at
`propensive/soundness`, before this project was extracted, under the tag prefix `runners-`.
Releases from this repository use the prefix `xeq-`, and each carries the five stubs and the
`xeq` builder script (as `xeq` and `xeq.cmd`, the same bytes). Their hashes are recorded in
`etc/runners/`.

The signature is pinned in three places, which the test suites check against each other:

- `src/runner/src/bintel.rs`, as the `SIGNATURE` constant the runner compares on the wire;
- `spec/ethereal-launcher.tel`, from which it is derived;
- the daemon's own tests (`ethereal_test.scala` in Soundness, "the schema signature is pinned").

The daemon derives the signature at runtime from its own copy of the schema text, in
`ethereal.Launcher.scala`. That copy and `spec/ethereal-launcher.tel` must stay identical byte
for byte — descriptions included, since they are part of what is hashed — or this file records
a signature no side actually sends.

`xeq-0.7` changed `record Init`: the single `tty` flag, which reported only stdin, became
`stdin-tty`, `stdout-tty` and `stderr-tty`, and `argument` and `environment` moved to indices
8 and 9. A command can now tell whether *its own output* is a terminal, which is what a tool
emitting binary data needs in order to refuse to run without a redirection.

## Protocol versions

There is no version number on the wire, and deliberately so: the signature *is* the version,
and it is derived from the schema rather than maintained alongside it, so it cannot drift from
what the two sides actually encode. A protocol change is therefore never silent and never
partially compatible — which is what makes it safe for the two repositories to release
independently.

## Record versions

The `ETHRCFG` column is the version of the configuration record ([`ethrcfg.md`](ethrcfg.md)).
It is independent of the protocol: `xeq-0.6` changed the record without touching the wire, so
a daemon built for `runners-0.5` runs unchanged under an `xeq-0.6` launcher.

What it does affect is self-upgrade. A running executable verifies a `.pending` replacement
by finding *its own* magic in it, so an executable built from `runners-0.5` (v2) rejects a
`.pending` built from `xeq-0.6` (v3) as having no record, deletes it, and carries on. Moving an
installed application from v2 to v3 is therefore a fresh install, not an upgrade. No v2
executable was ever published with a signing key, so no working upgrade channel is lost.
