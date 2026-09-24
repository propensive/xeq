# XEQ

**A JVM application as a single executable file.**

An XEQ executable is a small native *runner stub* with the application's JAR appended to it.
Running it starts — or reuses — a background daemon holding a warm JVM, forwards the
invocation's arguments, environment, streams and signals to it, and returns its exit status. So
the JVM's startup cost is paid once rather than once per invocation, and the command behaves
like any other command: it reads a pipe, respects `Ctrl-C`, and reports a status.

The stub is generic and reusable. It is the same bytes for every application on a given
platform, so building an executable is joining three files — the stub, a small configuration
record and the JAR — not compiling. That is what makes cross-platform packaging cheap: every platform's
executable can be built on one machine, in about as long as it takes to copy a file.

```sh
$ ls -la mytool
-rwxr-xr-x  1 you  staff  4014592  mytool
$ ./mytool --version
mytool 1.0.0
```

## What's here

| Path | |
|---|---|
| `src/runner` | The runner stub, in Rust: platform detection, JVM discovery, the daemon handshake, terminal modes, signals, and signed self-upgrade. ~0.5 MB per platform |
| `src/sign` | `ethereal-sign` — keygen and signing for the self-upgrade path |
| `src/script` | The `xeq` builder: a polyglot script (one file valid as `sh`, `.bat` and PowerShell) that joins a stub, a record and a JAR, and generates the polyglot launchers. Published with the runners |
| `src/packager` | `Packager` — a thin front end that turns a `Packaging` into a distributable by invoking the `xeq` script |
| `src/toolchain` | The same packaging as an [Anthology](https://github.com/propensive/soundness) toolchain format, so an application compiles and packages in one pass |
| `spec/` | **The contract** between a launcher and a daemon, and the reason the two can be developed apart |
| `src/example` | The end-to-end fixture: the smallest daemonized application there is |

## Delivery modes

An application's JAR reaches a user in one of four shapes, and the same `Packaging` describes
each:

- **native** — one self-contained binary for one platform. The plain case.
- **embed-all** — a polyglot script carrying *every* platform's stub and the application,
  which unpacks the right one where it runs. One file, works anywhere, offline.
- **download** — a polyglot script carrying the application and a table of
  (url, hash) pairs; on first run it fetches the one stub it needs, verifies it, appends the
  embedded JAR and replaces itself.
- **dispatcher** — a polyglot script carrying *nothing* but a table of (url, hash) pairs
  naming complete per-platform executables. The smallest possible cross-platform artefact.

In every downloading case the bytes are verified against a SHA-256 recorded at publication.

## The two halves

XEQ is the launcher half. The other half is a **daemon** — the JVM-side implementation that
accepts the connection, reconstitutes the invocation's context and runs the application. The
reference daemon is `ethereal`, in [Soundness](https://github.com/propensive/soundness).

Neither repository depends on the other. They meet at [`spec/`](spec/README.md): a TEL schema
for the wire protocol, the configuration block's layout, the system properties, and the files
they share. Both sides carry the schema's signature and refuse a peer that disagrees, so a
mismatched pair fails at the first message rather than misreading fields — which is precisely
what makes it safe for them to release on their own cadences.

XEQ's Scala modules are *built* against Soundness's libraries, as any Scala project might be.
Nothing here depends on `ethereal`, the daemon, except the end-to-end fixture, which needs
something at the other end of the socket to be a test at all.

## Building

Requires a JDK, and — to build stubs rather than download them — a Rust toolchain with
[`cargo-zigbuild`](https://github.com/rust-cross/cargo-zigbuild) and `zig` for cross-compiling.

```sh
make xeq-script      # assemble dist/xeq, the builder
	make build           # the Scala modules
make test            # the test suite, through the `fume` runner
make cargo-test      # the runner's own unit tests

make runners-build   # cross-compile the five stubs into dist/runners
make runners-fetch RUNNERS_VERSION=0.5   # or download them, hash-verified

make e2e             # package the example app around a real stub and run it
```

The Scala side resolves Soundness components from `~/.ivy2/local`; `make sync-releases
VERSION=X.Y.Z` in a Soundness checkout puts them there. The compiler is the
[proscala](https://github.com/propensive/proscala) fork, downloaded and cached automatically.

## Publishing runner stubs

Stubs are published on their own cadence, and only when the Rust source changes:

```sh
make runners-release RUNNERS_VERSION=0.6
```

which cross-compiles the five stubs, assembles the `xeq` builder script, uploads them all to a
`xeq-0.6` release, records their hashes in `etc/runners/0.6.tsv` and `etc/runners/0.6.SHA256SUMS`,
and rewrites `res/packager/xeq/runners.{tsv,version,url}` — the resources the packager reads. Publishing is therefore a data change, not a code change, and
an application picks up a runner fix without anything being rebuilt.

## Status

Extracted from Soundness, where this machinery grew as the `ziggurat` library and the Rust
runner inside `ethereal`. Runner releases up to `runners-0.5` were published from that
repository under the `runners-` tag prefix; releases from here use `xeq-`, and `xeq-0.6` — the
first made from this repository — supersedes them and adds the builder script as a release asset.

## Licence

Apache 2.0. See [LICENSE](LICENSE).
