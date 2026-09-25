# System properties

## Launcher to daemon

When the launcher starts a daemon it invokes the JVM with the application JAR (itself, since
the JAR is appended to the stub) and these properties. They are the daemon's only account of
who launched it, and the daemon may rely on every one of them being present.

| Property | Value |
|---|---|
| `build.id` | The `build_id` from the executable's `ETHRCFG` record (see [`ethrcfg.md`](ethrcfg.md)) |
| `ethereal.name` | The command name — the basename the executable was invoked as. Its presence is what tells the JVM it was started by a launcher rather than run directly |
| `ethereal.user.id` | The *effective* user the daemon runs as: the numeric id on Unix (`geteuid`), the SID (`S-1-5-…`) on Windows, or empty if it cannot be determined. Each invocation's own user arrives in its `init` document |
| `ethereal.user.name` | The invoking user's name |
| `ethereal.script` | The absolute path of the executable that launched this daemon |
| `ethereal.startTime` | Milliseconds since the epoch at launch, used to report startup latency |
| `ethereal.payloadSize` | Reserved; currently always `0` |
| `ethereal.jarSize` | The length in bytes of the whole executable file (stub, record and JAR) |
| `ethereal.command` | The path `PATH` resolution finds for `ethereal.name`, or empty |
| `ethereal.fpath` | zsh's `$fpath`, newline-separated, or empty where zsh is absent — for installing completions |
| `burdock.progress` | A file the JVM may append dependency-download progress lines to, which the launcher tails and renders |

Adding a property is compatible; removing or repurposing one is a protocol change and follows
the rollout in [`README.md`](README.md).

## Platform labels

One label per published stub, used in asset names, manifests and the `xeq` script's `--target`:

`linux-x64`, `linux-arm64`, `macos-x64`, `macos-arm64`, `windows-x64`.

A stub's published asset name is `runner-<label>`, with `.exe` appended for Windows labels.
