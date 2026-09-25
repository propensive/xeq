# The launcher's side of an invocation

The protocol schema says what the two halves exchange. This document says what the launcher
does *around* that exchange — which argument values it keeps for itself, what it does to the
client's terminal, what it learns about the client and how, and how a signal or an end-of-file
on the client's side reaches the daemon — because an application author has to live with all
of it, and a daemon implementation has to expect it.

## Reserved arguments and environment variables

The launcher's own controls live in the environment, so that an application can receive any
argument vector at all. One argument form is also accepted, for a user at a shell:

| Name | Meaning |
|---|---|
| `XEQ_DOWNLOAD` | Set (to anything but the empty string or `0`): download a JVM if no suitable one is installed, rather than failing with instructions |
| `--download` | The same request, recognised **only when it is the sole argument**. `mytool --download` downloads a JVM if necessary, starts the daemon, and runs the application *with no arguments*. In any other position — `mytool install --download`, `mytool -- --download` — the argument belongs to the application and is delivered unchanged |
| `XEQ_WRAP_JAVA` | Internal. Set by the launcher on the process it starts the JVM through, so the daemon appears under the application's name; never set it yourself |
| `ETHEREAL_DEBUG` | Set: the launcher traces its progress to stderr and to `$TMPDIR/ethereal-launcher.log`, which is otherwise never written |
| `ETHEREAL_SIGNAL_TIMEOUT_MS` | How long the launcher waits for the daemon to acknowledge a forwarded signal before taking the signal's fallback action (below); default 250 |

Two argument values are reserved by the **daemon** rather than the launcher, as the first
argument only: `{completions}`, under which the reference daemon computes shell completions,
and `{admin}`, its administrative interface. The launcher recognises them only to run such an
invocation without touching the terminal, reading stdin or installing signal handlers, since
it runs behind the user's shell — for instance under a completion function's `< <(...)`. An
application built on the reference daemon cannot use either as its own first argument.

Nothing else is intercepted. In particular the launcher never removes an argument from the
middle of the vector.

## What the daemon is told

The `init` document carries what follows. The daemon holds a socket, not the client's
process, so for each of these the document is its only source.

| Field | Source |
|---|---|
| `pid` | The launcher's process id |
| `uid` | The invoking user: the *real* user id on Unix (`getuid`), the user's SID (`S-1-5-…`) on Windows, read from the process token; empty if that fails |
| `username` | `$USER`, then `$LOGNAME`, on Unix; `%USERNAME%` on Windows. Whatever the environment says, in other words, and not authenticated |
| `script` | The canonical path of the running executable, asked of the operating system — never derived from `argv[0]` |
| `invoked-as` | `argv[0]` exactly as the caller supplied it, so a multi-call binary — one executable installed under several names by symbolic links — can dispatch on the name it was invoked by. It is the caller's to choose and may be anything, including a path that does not exist; never use it to locate a file. The daemon and its state directory are keyed on the executable, not on this name, so every alias shares one warm daemon, and a daemon must therefore serve concurrent invocations under different names |
| `pwd` | The working directory |
| `argument` | The arguments, in order, after the launcher's own (above) |
| `environment` | The environment, as `NAME=value`, with the injections described below |
| `stdin-tty`, `stdout-tty`, `stderr-tty` | Whether each standard stream is a terminal (`isatty`; on Windows a console, or one of the pipes through which MSYS2, Cygwin and mintty present a pseudo-terminal). `stdin-tty` is *unset* for a terminal the launcher decided not to read: see *The terminal* |
| `umask` | The client's file-creation mask, in octal (`022`); absent on Windows. The daemon should apply it to files the invocation creates; it is process-wide state in a daemon serving several invocations, so that takes care on its side |
| `columns`, `rows` | The terminal's size, when stdout is a terminal: `TIOCGWINSZ` on the first of stdout, stdin and stderr that is a terminal, or the console screen buffer's window on Windows. Absent when no stream is a terminal or the size is unknown (a pseudo-terminal whose size was never set reports zero). The same values are sent again with `WINCH` and `CONT`, so a daemon never needs to probe the terminal by escape sequence |
| `input-codepage`, `output-codepage` | On Windows, the console's input and output code pages (`GetConsoleCP`, `GetConsoleOutputCP`), so the daemon can decode what it reads and encode what it writes for a console that is not UTF-8 |

**Environment injection.** The launcher measures the terminal itself and delivers the result
in the environment, replacing what was inherited, since inherited values are unreliable across
a shared daemon: any inherited `COLUMNS` and `LINES` are removed and the measured size
substituted, and any inherited `TERMINAL_BG` is removed and the terminal's background colour
(from an OSC 11 query, as `rgb:RRRR/GGGG/BBBB`) substituted. When the launcher measured nothing
the inherited values are passed through unchanged, so their presence alone does not mean a
terminal was detected; the `columns` and `rows` fields and the tty flags do. New code should
read the fields; the variables remain for applications that read the environment.

**Text on the wire.** Every value above is text, and the protocol's scalars must be valid
UTF-8 (BinTEL §7.1). A value that is not — on Linux and macOS, a byte sequence that is not
UTF-8; on Windows, an unpaired UTF-16 surrogate — is delivered with U+FFFD in place of each
sequence that cannot be represented. This is a documented loss, not a failure: the launcher
does not abort on such a value, and the substitution is exactly the one the JVM makes when it
decodes its own argument vector, so an application sees what it would have seen if run
directly. A re-exec after a self-upgrade passes the original bytes on, since they are still
the launcher's to give.

**Context that does not cross the socket.** The invocation runs in the daemon's process, and
inherits that process's context, not the client's, for everything not listed above:

- resource limits (`ulimit`), so a limit set for one command has no effect on it;
- nice level and scheduling class;
- cgroup or container membership on Linux, and any memory or CPU limit that comes with it;
- the macOS sandbox profile and any SELinux or AppArmor context;
- the *effective* user under setuid: `uid` is the real user of the invocation, while the daemon
  runs — and was launched — as the effective user (`ethereal.user.id`, `properties.md`), and
  the two can differ. Neither is authenticated by the daemon on the strength of the document
  alone; see `layout.md` on the socket's permissions and peer credentials.

An author of a security- or resource-sensitive tool should not rely on any of these
following the client.

## The composition an invocation is written under

Every document the launcher writes carries the signature of the schema composition it was
written under (BinTEL §6.1, §8.2): the base `ethereal-launcher` schema, or the base with the
first *n* of its layers. The launcher settles on one composition per invocation, before its
first connection, and uses it on every connection the invocation opens — `init`, `stderr`,
`control`, each `signal`, `closed` and `exit` — and expects every reply under it.

It chooses by reading the daemon's `acceptance` file from the state directory, and the rule is
in `layout.md` under *Negotiating the composition*: the first alternative whose requirement is
a prefix of the launcher's own chain of layers, extended by every further layer the daemon
names, in order. In short, the richest composition both sides hold. Under
`ETHEREAL_DEBUG` the depth and signature chosen are traced.

The launcher compiles in, for each layer it knows, the layer's hash and the members it appends
to each record, so a layer's fields take the keyword indices after the base's (and after any
earlier layer's) and the base's indices never move. Nothing is parsed at run time but the
acceptance itself, whose form is fixed.

Three outcomes:

- **No file, or one the launcher cannot parse**: the base alone. That is what a daemon which
  predates acceptances reads, and the wire bytes are then exactly those of a launcher without
  this mechanism.
- **A servable alternative**: the invocation proceeds under the chosen composition.
- **No servable alternative**: the daemon speaks another base, or requires a layer this
  launcher lacks. The launcher prints, to stderr, that the daemon speaks another launcher
  protocol, giving both base hashes, and exits with status 2 without connecting — a daemon
  would only close the connection, which is less to go on.

## The terminal

When stdin is a terminal and the launcher is in the terminal's **foreground process group**,
the launcher:

1. saves the terminal's attributes, to restore them at exit;
2. puts the terminal into raw mode — no canonical line editing, no echo, and no signal
   generation from keys (`ISIG` off, `VINTR` undefined), with output post-processing kept on —
   so that every keystroke reaches the application as bytes;
3. asks the terminal for its background colour (OSC 11), waiting briefly for the reply, and
   delivers it as `TERMINAL_BG`; any other bytes the user typed meanwhile are pushed back
   ahead of stdin, not lost. On Windows the query is made only under Windows Terminal
   (`WT_SESSION` set), since the classic console leaves an unanswered query in the input;
4. measures the terminal's size and delivers it as `COLUMNS` and `LINES`, and as the `columns`
   and `rows` fields;
5. opens the control channel (`control`), on which the daemon may ask for the terminal to be
   put into canonical (cooked) mode and back (`mode`), for a command that wants the driver's
   own line editing.

When stdin is a terminal but the launcher is **not in the foreground** — a background job under
job control, `mytool > log &` — none of the above happens, since any of it would stop the job
with SIGTTOU or SIGTTIN. The invocation runs with its stdin at end-of-file, `stdin-tty` unset,
and stdout and stderr forwarded as usual. A job that is later foregrounded and continued
(`fg`) is *not* retroactively given the terminal: its stdin stays closed.

When stdin is not a terminal — a pipe or a file — nothing is reconfigured, and stdin is
forwarded until it ends.

An MSYS2, Cygwin or mintty pseudo-terminal on Windows is reported as a terminal but cannot be
reconfigured through the console API, so it stays in whatever mode the pseudo-terminal is in.

## End of input

The connection that carries the invocation's stdin also carries its stdout. When the client's
stdin ends — the pipe closes, the file is exhausted, or the launcher decided above not to read
it — the launcher **half-closes** the connection, shutting down its write direction only. The
daemon's read of the invocation's stdin then returns end-of-file while its writes continue to
flow the other way. A daemon must treat this as end of input, not as a dropped client; the
client is gone only when the connection closes entirely.

A terminal in raw mode never delivers end-of-file: Ctrl-D is the byte 0x04, and it is the
application's to interpret.

## End of output

When the client's side of an output stream can no longer be written — `mytool | head -1`, once
`head` has gone — a process writing a pipe would get SIGPIPE or `EPIPE`. The invocation is
writing a socket the launcher reads, so nothing of the kind happens by itself. Instead the
launcher sends a `closed` document naming the stream (`stdout` or `stderr`) on a fresh
connection, once, and expects no answer; the daemon should then fail the invocation's further
writes to that stream as a broken pipe would, so that a program which writes until it cannot
ends. The launcher goes on reading and discarding the stream regardless, so a daemon that does
not act on the document is never blocked writing it, and the invocation still ends when it
chooses to.

## Signals

On Linux and macOS the launcher forwards these signals to the daemon as `signal` documents,
named without their `SIG` prefix: `INT`, `QUIT`, `TERM`, `HUP`, `WINCH`, `USR1`, `USR2`, `TSTP`
and `CONT`. `WINCH` and `CONT` carry the terminal's current size in `columns` and `rows`, since
a signal has no payload of its own and the daemon holds no terminal to ask. The daemon answers
each with a `signal-ack` saying whether the invocation accepted it. What the launcher does next
depends on the signal:

| Signal | Accepted | Rejected, or no answer within the timeout |
|---|---|---|
| `INT`, `QUIT`, `HUP` | Nothing further; the application is handling it | The launcher restores the signal's default action and re-raises it on itself, so it dies as it would have without a handler — and the shell reports 128 + the signal number |
| `TERM` | The launcher stops reading the invocation's stdout at once, drains its stderr (for at most a short grace period), restores the terminal, and then **dies of SIGTERM** itself, so its parent sees a genuine signal death and a shell reports 143 | As for `INT` |
| `WINCH`, `USR1`, `USR2` | Nothing further | The signal is dropped, as its default action would have |
| `TSTP` | The launcher has already told the daemon; see below | Likewise |
| `CONT` | Likewise | Likewise |

**Stopping and continuing.** On `TSTP` the launcher forwards the signal, restores the
terminal's saved attributes so the shell finds it as it left it, and then stops, by taking the
signal's default action. On `CONT` it re-applies raw mode — only if it owned the terminal's
mode before, and only if it is once more in the foreground, since from the background that
would stop it again — and forwards `CONT` with the terminal's size, since the window may have
been resized while the job was stopped, so an application can redraw. A terminal in raw mode
does not generate `TSTP` from Ctrl-Z (that byte, 0x1A, goes to the application); the path is
exercised by `kill -TSTP`, and by Ctrl-Z when stdin is a pipe.

**Inherited dispositions.** A signal the launcher inherits as *ignored* stays ignored: no
handler is installed, nothing is forwarded, and the launcher can never die of it. That is how
`nohup mytool` keeps `HUP` away and how a caller's `trap '' INT` is honoured, as POSIX
requires of a signal ignored across `exec`. The handlers the launcher does install restart
interrupted system calls, so the stream forwarders are not disturbed by a signal's arrival.

**Ctrl-C reaches the daemon by one of two routes**, and a daemon implementation must handle
both:

- *Stdin is a terminal in the foreground.* Raw mode has turned signal generation off, so the
  terminal does not send SIGINT at all. Ctrl-C arrives as the **byte 0x03** in the ordinary
  stdin stream, and it is the daemon's — ultimately the application's — job to notice it. An
  unnoticed 0x03 does nothing. This is deliberate: a full-screen application may treat Ctrl-C
  as input.
- *Stdin is a pipe, a file, or a background terminal.* The terminal still sends SIGINT to the
  foreground process group, the launcher's handler catches it, and it arrives as a **`signal`
  document naming `INT`**. If the invocation rejects it, or does not answer, the launcher
  dies of SIGINT as described above.

The reference daemon handles both: the byte through its keyboard-event decoding, and the
document through the application's signal traps. The two are not unified on purpose; a
launcher that recognised the interrupt character itself would take it away from applications
that want it.

**Windows** has no signals. The console control events `CTRL_C_EVENT`, `CTRL_BREAK_EVENT`,
`CTRL_CLOSE_EVENT`, `CTRL_LOGOFF_EVENT` and `CTRL_SHUTDOWN_EVENT` are forwarded as `signal`
documents named `CTRL_C`, `CTRL_BREAK`, `CTRL_CLOSE`, `CTRL_LOGOFF` and `CTRL_SHUTDOWN`, in
either terminal state. The last three carry a `deadline`, in milliseconds (5000): Windows ends
the process about that long after the event whatever it is doing, so the application knows how
long it has to finish. A rejected or unanswered event ends the launcher with the system's own
status for a process ended by a control event, `STATUS_CONTROL_C_EXIT` (`0xC000013A`); an
accepted close, logoff or shutdown drains the invocation's stderr and then ends with the same
status, since the system is about to end the process regardless.

Windows has no `SIGWINCH` either, and a resize arrives as a console input record, which the
launcher cannot read without taking keystrokes from stdin. It polls the console's window size
instead, a few times a second while stdout is a console, and sends `WINCH` with the new
`columns` and `rows` when it changes.

## Exit status

Once the invocation's stdout and stderr have both ended, the launcher asks the daemon for the
invocation's exit status (`exit`) and exits with it. A launcher that cannot reach the daemon
exits with 2; so does one whose daemon accepts the connection but does not answer within the
reply timeout (`layout.md`, *Lifecycle*), after saying so on stderr. One that was terminated
by a signal dies of that signal, as above, and never asks.
