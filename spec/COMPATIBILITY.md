# Compatibility

A launcher and a daemon interoperate only if they agree on the protocol schema's *base*, byte
for byte. Both carry its signature and compare it on every document, so a mismatched pair fails
at the first message rather than misreading fields. Above the base, the schema may grow by
*layers* without breaking either side; see *Layers and acceptances* below.

| Release | Protocol | Base signature (BLAKE3-256 + cadence) | `ETHRCFG` | Reference daemon |
|---|---|---|---|---|
| next | `ethereal-launcher` BinTEL | `e50b7e82c11b06783dafa8a2ecc4e35f7ba31044ecd38fc5d9fe9e47a7c11e59e5` | v3 | Soundness: unreleased; see below |
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

- `src/runner/src/bintel.rs`, as the `BASE` hash from which the runner derives every
  signature it writes and compares on the wire;
- `spec/ethereal-launcher.tel`, from which it is derived;
- the daemon's own tests (`ethereal_test.scala` in Soundness, "the schema signature is pinned").

The daemon derives the signature at runtime from its own copy of the schema text, in
`ethereal.Launcher.scala`. That copy and `spec/ethereal-launcher.tel` must stay identical byte
for byte — descriptions and layers included, since they are part of what is hashed — or this
file records a signature no side actually sends.

`xeq-0.7` changed `record Init`: the single `tty` flag, which reported only stdin, became
`stdin-tty`, `stdout-tty` and `stderr-tty`, and `argument` and `environment` moved to indices
8 and 9. A command can now tell whether *its own output* is a terminal, which is what a tool
emitting binary data needs in order to refuse to run without a redirection.

The next release revises the protocol again: `record Init` gains `invoked-as`, `umask`,
`columns`, `rows`, `input-codepage` and `output-codepage` (indices 10–15, after the existing
fields, which keep theirs); `record Signal` gains `columns`, `rows` and `deadline`; and two
variants are added at the end of `select Message`, `closed` (an output stream has lost its
reader) and `shutdown` (a request that the daemon exit). `uid` becomes the platform's
identifier — a SID on Windows — rather than a number. What each means is in
[`launcher.md`](launcher.md) and [`layout.md`](layout.md).

## Protocol versions

There is no version number on the wire, and deliberately so: the base signature *is* the
version, and it is derived from the schema rather than maintained alongside it, so it cannot
drift from what the two sides actually encode. A change to the base is therefore never silent
and never partially compatible — which is what makes it safe for the two repositories to
release independently. What *can* change compatibly is described next.

## Layers and acceptances

The schema is a base and, from the first release to need one, an ordered list of **layers**
(TEL §20.3), each of which appends members to the base's records. A layer is a separate
component with its own hash; the base's hash is taken over the schema with every `layer`
removed (BinTEL §8.1), so adding a layer to `ethereal-launcher.tel` leaves the base signature
in the table above exactly as it is. A document is written under a **composition** — the base
with the first *n* layers — and carries that composition's signature, a palimpsest of its
components' hashes (BinTEL §8.2): 33 bytes for the base alone, two more per layer.

A launcher and a daemon need not hold the same layers. Each invocation is written under the
richest composition both hold, which the launcher learns from the **acceptance** the daemon
publishes in its state directory (BinTEL §8.4; `layout.md`, *Negotiating the composition*),
and the daemon answers under the composition the launcher wrote under. Consequently, with the
same base:

| Launcher | Daemon | Outcome |
|---|---|---|
| `xeq-0.8` (predates acceptances) | any later daemon | the launcher writes the base, the daemon answers under it |
| any later launcher | a daemon that publishes no acceptance | the launcher writes the base |
| holds layers 1–2 | holds layer 1 | both use the base with layer 1 |
| holds layer 1 | holds layers 1–2 | both use the base with layer 1 |
| any | another base | the launcher reports the mismatch before connecting |

So a layer is a **compatible** change — the runner and the daemon may adopt it in either order,
on their own cadences — and a base revision remains the **breaking** one, released as
`spec/README.md` describes. What may go in a layer is constrained by what makes the launcher's
prefix rule sound and its compiled-in keyword indices stable, and by TEL itself:

- a layer **appends optional members** (fields or select references) to existing records or
  to the document root, and may **add** records, scalars and selects; the members it appends
  take the keyword indices after those of the base and of every earlier layer;
- a layer never `exclude`s a variant, never tightens a member (`required`, `irrepeatable`),
  never changes a default, and never touches `select Message` — TEL forbids a layer to add a
  variant (E213), so a **new message kind is always a base revision**, as is any change to the
  meaning of an existing field;
- layers are declared in the order they were introduced, appended after every existing one,
  and once released are never reordered, edited or removed.

Under that discipline every longer prefix of the chain is a subtype of every shorter one (TEL
§24.4), which is what lets the launcher extend a composition layer by layer without a subtype
check. The layers, their hashes and the signature of each prefix belong in the table above as
they are released, pinned in the same three places as the base.

No layer exists yet: `xeq-0.8`'s base is the whole schema, and the mechanism is exercised by
the runner's tests against fixture hashes until the first feature that needs a layer arrives.

## Record versions

The `ETHRCFG` column is the version of the configuration record ([`ethrcfg.md`](ethrcfg.md)).
It is independent of the protocol: `xeq-0.6` changed the record without touching the wire, so
a daemon built for `runners-0.5` runs unchanged under an `xeq-0.6` launcher.

What it does affect is self-upgrade. A running executable verifies a `.pending` replacement
by finding *its own* magic in it, so an executable built from `runners-0.5` (v2) rejects a
`.pending` built from `xeq-0.6` (v3) as having no record, deletes it, and carries on. Moving an
installed application from v2 to v3 is therefore a fresh install, not an upgrade. No v2
executable was ever published with a signing key, so no working upgrade channel is lost.
