# Shared on-disk layout

The launcher and the daemon rendezvous through the filesystem. If they disagree about any path
here, the launcher polls one socket while the JVM binds another and startup silently times out,
so these are contract, not implementation detail.

## The state directory

For an application whose command name is `<name>` — the basename of the executable's
canonical path, so that every symbolic link to one executable shares one daemon:

- **Unix**: `$XDG_RUNTIME_DIR/<name>`, falling back to `$XDG_STATE_HOME/<name>`, then
  `$HOME/.local/state/<name>`.
- **Windows**: `%LOCALAPPDATA%\Temp\<name>` (with `%USERPROFILE%\AppData\Local` as the fallback
  base).

Within it:

| File | Written by | Meaning |
|---|---|---|
| `socket` | daemon | The Unix-domain socket the daemon listens on (an `AF_UNIX` socket on Windows too) |
| `pid` | daemon | The daemon's process id |
| `build` | daemon | The launcher content the daemon was started from, which the launcher reads to detect a stale daemon after a rebuild; see *Staleness* |
| `fail` | daemon | Written when startup fails, so the launcher can report the reason instead of timing out |
| `progress` | daemon | Dependency-download progress, tailed and rendered by the launcher (see `burdock.progress`) |
| `lock` | launcher | Held while starting a daemon, so concurrent invocations start exactly one |
| `daemon.log` | daemon | Diagnostics |

### Permissions

The socket is a boundary between users. The daemon believes what a connecting client says
about itself (`init`'s `uid` and `username` are ordinary fields), and runs whatever it is
asked to as the daemon's owner, so whoever can open the socket can act as that user. The
directory and the socket must therefore be private, by construction rather than by luck:

- The launcher creates the state directory **mode 0700**, explicitly and not under the umask,
  and on every start verifies an existing one: it must be owned by the invoking (effective)
  user, and one that is readable, writable or searchable by anyone else is tightened to 0700.
  A directory owned by another user is refused, with a message, and the launcher exits with
  status 2. `$XDG_RUNTIME_DIR` is normally already 0700; the fallbacks under `$HOME` are not
  guaranteed to be, which is why the check exists.
- The daemon creates the socket **mode 0600**: its umask is inherited from whichever
  invocation first started it, so it must set the mode explicitly (`fchmod`, or bind under a
  temporary umask of 077). Before connecting, the launcher verifies that the socket is owned
  by the invoking user and admits no one else, and refuses otherwise.
- The daemon **should check peer credentials** on every connection — `SO_PEERCRED` on Linux,
  `LOCAL_PEERCRED` or `getpeereid` on macOS — and refuse a connection whose peer user is not
  the daemon's own user or does not match the `uid` the client claims. That closes the gap
  the file modes leave: a mode is a snapshot, credentials are the fact.
- On **Windows** the directory lies under the user's own profile, whose ACL grants no other
  user access, and `AF_UNIX` sockets carry no peer credentials. The directory's ACL is the
  boundary; a daemon that needs more can place a secret in the directory and require it in
  the first document.

## The data directory

- **Unix**: `$XDG_DATA_HOME`, falling back to `$HOME/.local/share`.
- **Windows**: `%LOCALAPPDATA%`.

Within `<data>/<name>`:

| File | Meaning |
|---|---|
| `.pending` | A complete signed replacement binary. On its next start the launcher verifies this against the public key baked into the *running* binary, and on success swaps it into place and re-execs; on failure it is deleted silently |

## Launcher diagnostics

`$TMPDIR/ethereal-launcher.log` — the launcher's own trace, written **only when
`ETHEREAL_DEBUG` is set** (see `launcher.md`), and kept outside the per-application state
directory because it must be writable before any application directory is known.

## Staleness

The daemon records the launcher it was started from in `build`, as one line of
whitespace-separated fields:

    <build-id> <size> <mtime-ms>

`build-id` is the `ETHRCFG` build id; `size` is the executable's length in bytes; `mtime-ms`
its modification time in milliseconds since the epoch. The last two are optional, for daemons
that predate the content check, and a launcher that finds them absent — or a record it cannot
parse — treats the daemon as fresh. The launcher never writes this file.

On each invocation the launcher compares the executable against the record. A different size
proves a rebuild: the daemon is displaced. A different mtime alone — after a `touch`, or a
rebuild of the same size — is a question only content can settle, so the launcher asks the
daemon, over the protocol, whether it is still fresh (`verify` → `verdict`). The daemon hashes
at most once per change and remembers the answer, which a stateless launcher cannot do. A
stale daemon shuts down and the launcher waits for its death before starting a new one.

## Lifecycle

The daemon's lifetime is part of the contract, because a launcher has to know what to expect
of a daemon it did not start.

**Idle exit.** A daemon may exit after a period without invocations; the reference daemon
does so after six hours idle. A launcher must not assume a daemon it once found is still
there: a missing or unresponsive socket means *start one*, never *fail*. Nothing is lost by an
idle exit but the warm JVM, which the next invocation pays for again.

**Shutdown.** The `shutdown` document asks a daemon to exit cleanly: it must accept no further
invocations, let those in flight finish, and then end, removing its state files. It is not
answered; the connection is simply closed. This is how tooling stops a daemon without finding
its pid, and how a user reclaims a warm JVM held for a command run rarely. The launcher itself
never sends it.

**A stale verdict.** A daemon that answers `verify` with a stale verdict is about to be
displaced, and must not serve a new invocation from its old content — but an invocation
already running has the code it started with, and rebuilding an executable while a long job
runs against it is ordinary. The daemon therefore stops accepting and lets in-flight
invocations finish, as for `shutdown`; the launcher that asked waits for its pid to
disappear before starting the successor, and after a bounded wait proceeds regardless, since
a successor bound to a fresh socket path is not disturbed by a predecessor draining on the old
one. The reference daemon currently exits at once on a stale verdict, ending anything in
flight; that is the behaviour to change.

**A daemon that accepts and then hangs.** The launcher bounds every wait for a *reply*: a
`signal-ack` by `ETHEREAL_SIGNAL_TIMEOUT_MS` (default 250 ms), and a `verdict` or an
`exit-status` by 10 seconds, after which it reports that the daemon did not answer and exits
with status 2 (for a missing verdict it proceeds as if fresh). It does *not* bound the
invocation itself: the streams stay open for as long as the application runs, which may be
forever by design. A daemon that accepted `init` and then wedges is therefore indistinguishable
from a long-running command until the client ends its input or sends a signal, and the signal
path is what surfaces it — a rejected or unanswered `INT` ends the launcher.
