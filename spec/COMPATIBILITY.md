# Compatibility

A launcher and a daemon interoperate only if they agree on the protocol schema, byte for byte.
Both carry its signature and compare it on every document, so a mismatched pair fails at the
first message rather than misreading fields.

| Runner release | Protocol | Schema signature (BLAKE3-256 + cadence) | `ETHRCFG` | Reference daemon |
|---|---|---|---|---|
| `runners-0.5` | `ethereal-launcher` BinTEL | `4701ec19cd0fd3ecfc0e1b8a6525b4edc3a3b1deda370f681986db9aa39c1da692` | v2 | Soundness ≥ 0.65.0 (`ethereal-core`) |
| `runners-0.4` | `ethereal-launcher` BinTEL | `4701ec19cd0fd3ecfc0e1b8a6525b4edc3a3b1deda370f681986db9aa39c1da692` | v2 | Soundness 0.65.0 (`ethereal-core`) |
| `runners-0.3` and earlier | line-oriented (`i`/`e`/`m`/`s`/`x`/`v` opcodes) | — | v2 | Soundness ≤ 0.64.0 |

Releases up to and including `runners-0.5` were published from the Soundness repository, at
`propensive/soundness`, before this project was extracted. They are byte-identical to what this
repository builds from the same sources, and their hashes are recorded in `etc/runners/`.

The signature is pinned in three places, which the test suites check against each other:

- `src/runner/src/bintel.rs`, as the `SIGNATURE` constant the runner compares on the wire;
- `spec/ethereal-launcher.tel`, from which it is derived;
- the daemon's own tests (`ethereal_test.scala` in Soundness, "the schema signature is pinned").

## Protocol versions

There is no version number on the wire, and deliberately so: the signature *is* the version,
and it is derived from the schema rather than maintained alongside it, so it cannot drift from
what the two sides actually encode. A protocol change is therefore never silent and never
partially compatible — which is what makes it safe for the two repositories to release
independently.
