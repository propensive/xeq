# Shared on-disk layout

The launcher and the daemon rendezvous through the filesystem. If they disagree about any path
here, the launcher polls one socket while the JVM binds another and startup silently times out,
so these are contract, not implementation detail.

## The state directory

For an application whose command name is `<name>`:

- **Unix**: `$XDG_RUNTIME_DIR/<name>`, falling back to `$XDG_STATE_HOME/<name>`, then
  `$HOME/.local/state/<name>`.
- **Windows**: `%LOCALAPPDATA%\Temp\<name>` (with `%USERPROFILE%\AppData\Local` as the fallback
  base).

Within it:

| File | Written by | Meaning |
|---|---|---|
| `socket` | daemon | The Unix-domain socket the daemon listens on (an `AF_UNIX` socket on Windows too) |
| `pid` | daemon | The daemon's process id |
| `build` | launcher | The launcher content the running daemon was started from, used to detect a stale daemon after a rebuild |
| `fail` | daemon | Written when startup fails, so the launcher can report the reason instead of timing out |
| `progress` | daemon | Dependency-download progress, tailed and rendered by the launcher (see `burdock.progress`) |
| `lock` | launcher | Held while starting a daemon, so concurrent invocations start exactly one |
| `daemon.log` | daemon | Diagnostics |

## The data directory

- **Unix**: `$XDG_DATA_HOME`, falling back to `$HOME/.local/share`.
- **Windows**: `%LOCALAPPDATA%`.

Within `<data>/<name>`:

| File | Meaning |
|---|---|
| `.pending` | A complete signed replacement binary. On its next start the launcher verifies this against the public key baked into the *running* binary, and on success swaps it into place and re-execs; on failure it is deleted silently |

## Launcher diagnostics

`$TMPDIR/ethereal-launcher.log` — the launcher's own log, outside the per-application state
directory because it must be writable before any application directory is known.

## Staleness

The launcher records the content it was started from in `build`. When the executable's mtime
disagrees with that record — after a `touch`, or a rebuild of the same size — the launcher asks
the daemon, over the protocol, whether it is still fresh (`verify` → `verdict`). The daemon
hashes at most once per change and remembers the answer, which a stateless launcher cannot do.
A stale daemon shuts down and the launcher waits for its death before starting a new one.
