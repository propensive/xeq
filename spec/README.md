# The XEQ specification

XEQ is a way of shipping a JVM application as a single executable file. An XEQ executable is a
small native *runner stub* with the application's JAR appended to it: running it starts (or
reuses) a background daemon holding a warm JVM, forwards the invocation's arguments,
environment, streams and signals to that daemon, and returns its exit status. The stub is
generic and reusable — it is the same bytes for every application on a given platform — so
building an executable is byte-patching a configuration block and concatenating a JAR, not
compiling.

This directory is the contract between the two halves, which live in different repositories and
release on different cadences:

- **the launcher** — the Rust runner in this repository (`src/runner`), published as reusable
  per-platform stubs, plus the packaging machinery that patches and wraps them (`src/core`,
  `src/packager`, `src/toolchain`);
- **the daemon** — the JVM side that the stub launches and talks to. The reference
  implementation is `ethereal` in [Soundness](https://github.com/propensive/soundness).

Neither repository depends on the other. Each implements what is written here, and each pins
the artefacts of the other that it tests against.

## The documents

| File | Contract |
|---|---|
| [`ethereal-launcher.tel`](ethereal-launcher.tel) | The wire protocol: a TEL schema whose BinTEL documents are exchanged over the daemon socket |
| [`ethrcfg.md`](ethrcfg.md) | The `ETHRCFG` configuration block a builder patches into a stub |
| [`properties.md`](properties.md) | The `-Dethereal.*` and `-Dbuild.*` system properties the launcher passes to the JVM |
| [`layout.md`](layout.md) | The files and directories the launcher and daemon share |
| [`COMPATIBILITY.md`](COMPATIBILITY.md) | Which runner release speaks which protocol, and against which daemon |

## Why "ethereal" appears in a specification owned by XEQ

The protocol, its system properties and its on-disk layout are named after `ethereal`, the
daemon implementation they were written for, and those names are on the wire and in the
filesystem: renaming them would break every launcher and daemon already deployed. The names are
therefore frozen, and the specification keeps them. "XEQ" names the executable format and this
project; "ethereal" names the protocol an XEQ executable speaks.

## Changing a contract

Both sides carry the schema signature (§ `ethereal-launcher.tel`) and refuse a peer that
disagrees, so a mismatched pair fails loudly at the first document instead of misreading
fields. That makes the rollout order safe but strict:

1. **This repository first.** Change the contract here, change the runner, and publish a new
   `runners-<version>` release. Nothing depends on the daemon, so this can ship alone.
2. **The daemon next.** Update its copy of the schema, its pinned signature, and the runner
   version its tests fetch; release.
3. **The packager last**, if it needs anything from the new daemon release.

Between 1 and 2 the two are incompatible by construction, which is why every entry in
`COMPATIBILITY.md` names both sides.

A change that does not touch a contract — a runner bug fix, a new target platform, a faster
JVM search — needs only step 1, and a daemon built months earlier keeps working. That
independence is the point of the split.
