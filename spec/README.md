# The XEQ specification

XEQ is a way of shipping a JVM application as a single executable file. An XEQ executable is a
small native *runner stub* with the application's JAR appended to it: running it starts (or
reuses) a background daemon holding a warm JVM, forwards the invocation's arguments,
environment, streams and signals to that daemon, and returns its exit status. The stub is
generic and reusable — it is the same bytes for every application on a given platform — so
building an executable is concatenating the stub, a small configuration record and the JAR,
not compiling.

This directory is the contract between the two halves, which live in different repositories and
release on different cadences:

- **the launcher** — the Rust runner in this repository (`src/runner`), published as reusable
  per-platform stubs, plus the `xeq` builder script published with them (`src/script`) and the
  Scala packaging front ends over it (`src/packager`, `src/toolchain`);
- **the daemon** — the JVM side that the stub launches and talks to. The reference
  implementation is `ethereal` in [Soundness](https://github.com/propensive/soundness).

Neither repository depends on the other. Each implements what is written here, and each pins
the artefacts of the other that it tests against.

## The documents

| File | Contract |
|---|---|
| [`ethereal-launcher.tel`](ethereal-launcher.tel) | The wire protocol: a TEL schema whose BinTEL documents are exchanged over the daemon socket |
| [`ethrcfg.md`](ethrcfg.md) | The `ETHRCFG` configuration record a builder places between a stub and the JAR |
| [`properties.md`](properties.md) | The `-Dethereal.*` and `-Dbuild.id` system properties the launcher passes to the JVM |
| [`layout.md`](layout.md) | The files and directories the launcher and daemon share |
| [`launcher.md`](launcher.md) | What the launcher does around an invocation: reserved arguments, the terminal, end of input, signals and exit status |
| [`COMPATIBILITY.md`](COMPATIBILITY.md) | Which runner release speaks which protocol, and against which daemon |

## Why "ethereal" appears in a specification owned by XEQ

The protocol, its system properties and its on-disk layout are named after `ethereal`, the
daemon implementation they were written for, and those names are on the wire and in the
filesystem: renaming them would break every launcher and daemon already deployed. The names are
therefore frozen, and the specification keeps them. "XEQ" names the executable format and this
project; "ethereal" names the protocol an XEQ executable speaks.

## Changing a contract

Both sides carry the schema's signature (§ `ethereal-launcher.tel`) and refuse a peer that
disagrees, so a mismatched pair fails loudly at the first document instead of misreading
fields. There are two kinds of change, and which kind a change is decides how it ships.

### A compatible change: a layer

An addition that an old peer could safely ignore — a new optional field on an existing message,
a new record — goes in a **layer** of `ethereal-launcher.tel` (TEL §20.3). A layer has its own
hash and leaves the base's signature untouched; each invocation is written under the richest
composition of base and layers that both sides hold, which the launcher learns from the
acceptance the daemon publishes (BinTEL §8.3–8.4; [`layout.md`](layout.md), *Negotiating the
composition*), and the daemon answers under the same. A launcher and a daemon that hold
different layers therefore still talk, using what they share, and the runner and the daemon
may adopt a layer **in either order**, each on its own cadence. What a layer may contain, and
why, is in [`COMPATIBILITY.md`](COMPATIBILITY.md).

### A breaking change: a base revision

A new message kind, a required field, or a change to what an existing field means alters the
base, and so its signature. The rollout order is then strict:

1. **This repository first.** Change the contract here, change the runner, and publish a new
   `xeq-<version>` release. Nothing depends on the daemon, so this can ship alone.
2. **The daemon next.** Update its copy of the schema, its pinned signature, and the runner
   version its tests fetch; release.
3. **The packager last**, if it needs anything from the new daemon release.

Between 1 and 2 the two are incompatible by construction, which is why every entry in
`COMPATIBILITY.md` names both sides.

A change that does not touch a contract — a runner bug fix, a new target platform, a faster
JVM search — needs only step 1, and a daemon built months earlier keeps working. That
independence is the point of the split.
