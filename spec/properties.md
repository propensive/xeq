# System properties

## Launcher to daemon

When the launcher starts a daemon it invokes the JVM with the application JAR (itself, since
the JAR is appended to the stub) and these properties. They are the daemon's only account of
who launched it, and the daemon may rely on every one of them being present.

| Property | Value |
|---|---|
| `build.id` | The `build_id` from the stub's `ETHRCFG` block (see [`ethrcfg.md`](ethrcfg.md)) |
| `ethereal.name` | The command name — the basename the executable was invoked as. Its presence is what tells the JVM it was started by a launcher rather than run directly |
| `ethereal.user.id` | The invoking user's numeric id (`0` where the platform has none) |
| `ethereal.user.name` | The invoking user's name |
| `ethereal.script` | The absolute path of the executable that launched this daemon |
| `ethereal.startTime` | Milliseconds since the epoch at launch, used to report startup latency |
| `ethereal.payloadSize` | Reserved; currently always `0` |
| `ethereal.jarSize` | The length in bytes of the appended JAR |
| `ethereal.command` | The path `PATH` resolution finds for `ethereal.name`, or empty |
| `ethereal.fpath` | zsh's `$fpath`, newline-separated, or empty where zsh is absent — for installing completions |
| `burdock.progress` | A file the JVM may append dependency-download progress lines to, which the launcher tails and renders |

Adding a property is compatible; removing or repurposing one is a protocol change and follows
the rollout in [`README.md`](README.md).

## Builder to builder

These are read by a *builder* — an application packaging itself — not by the launcher. The
reference is Soundness's `java -Dbuild.executable=… -jar app.jar` path, which produces a
single-platform executable without any of this project's packaging machinery.

| Property | Meaning |
|---|---|
| `build.executable` | The output path. Its presence selects "build an executable and exit" over "run" |
| `build.target` | The platform label to build for (see below); defaults to the host |
| `build.id` | The build id to patch in; defaults to `0` |
| `build.java.minimum` | Minimum JVM major version; defaults to 21 |
| `build.java.preferred` | Preferred JVM major version; defaults to 24 |
| `build.java.bundle` | `jre` (default) or `jdk` |
| `ethereal.publicKey` | Path to a 1312-byte raw ML-DSA-44 public key to patch in. Absent leaves the key zero, which disables upgrades |
| `ethereal.runners` | A directory of bare stubs to build from, as `runner-<label>[.exe]` |

## Platform labels

One label per published stub, used in asset names, manifests and `build.target`:

`linux-x64`, `linux-arm64`, `macos-x64`, `macos-arm64`, `windows-x64`.

A stub's published asset name is `runner-<label>`, with `.exe` appended for Windows labels.
