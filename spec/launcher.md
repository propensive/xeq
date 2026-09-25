# The launcher's side of an invocation

The protocol schema says what the two halves exchange. This document says what the launcher
does *around* that exchange — which argument values it keeps for itself, what it does to the
client's terminal, and how a signal or an end-of-file on the client's side reaches the daemon —
because an application author has to live with all of it, and a daemon implementation has to
expect it.

## Reserved arguments and environment variables

The launcher's own controls live in the environment, so that an application can receive any
argument vector at all. One argument form is also accepted, for a user at a shell:

| Name | Meaning |
|---|---|
| `XEQ_DOWNLOAD` | Set (to anything but the empty string or `0`): download a JVM if no suitable one is installed, rather than failing with instructions |
| `--download` | The same request, recognised **only when it is the sole argument**. `mytool --download` downloads a JVM if necessary, starts the daemon, and runs the application *with no arguments*. In any other position — `mytool install --download`, `mytool -- --download` — the argument belongs to the application and is delivered unchanged |
| `XEQ_WRAP_JAVA` | Internal. Set by the launcher on the process it starts the JVM through, so the daemon appears under the application's name; never set it yourself |
| `ETHEREAL_DEBUG` | Set: the launcher traces its progress to stderr and to `$TMPDIR/ethereal-launcher.log` |
| `ETHEREAL_SIGNAL_TIMEOUT_MS` | How long the launcher waits for the daemon to acknowledge a forwarded signal; default 250 |

Two argument values are reserved by the **daemon** rather than the launcher, as the first
argument only: `{completions}`, under which the reference daemon computes shell completions,
and `{admin}`, its administrative interface. The launcher recognises them only to run such an
invocation without touching the terminal, reading stdin or installing signal handlers, since
it runs behind the user's shell — for instance under a completion function's `< <(...)`. An
application built on the reference daemon cannot use either as its own first argument.

Nothing else is intercepted. In particular the launcher never removes an argument from the
middle of the vector.

## Text on the wire

Arguments, environment variables, the working directory and the script path are transmitted
as text, and the protocol's scalars must be valid UTF-8 (BinTEL §7.1). A value that is not —
on Linux and macOS, a byte sequence that is not UTF-8; on Windows, an unpaired UTF-16
surrogate — is delivered with U+FFFD in place of each sequence that cannot be represented.
This is a documented loss, not a failure: the launcher does not abort on such a value, and the
substitution is exactly the one the JVM makes when it decodes its own argument vector, so an
application sees what it would have seen if run directly. A re-exec after a self-upgrade
passes the original bytes on, since they are still the launcher's to give.

## The terminal

When stdin is a terminal and the launcher is in the terminal's **foreground process group**,
the launcher:

1. saves the terminal's attributes, to restore them at exit;
2. puts the terminal into raw mode — no canonical line editing, no echo, and no signal
   generation from keys (`ISIG` off, `VINTR` undefined), with output post-processing kept on —
   so that every keystroke reaches the application as bytes;
3. asks the terminal for its background colour (OSC 11), waiting briefly for the reply, and
   delivers it as `TERMINAL_BG` in the invocation's environment; any other bytes the user typed
   meanwhile are pushed back ahead of stdin, not lost;
4. measures the terminal's size and delivers it as `COLUMNS` and `LINES`, replacing any
   inherited values;
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

## End of input

The connection that carries the invocation's stdin also carries its stdout. When the client's
stdin ends — the pipe closes, the file is exhausted, or the launcher decided above not to read
it — the launcher **half-closes** the connection, shutting down its write direction only. The
daemon's read of the invocation's stdin then returns end-of-file while its writes continue to
flow the other way. A daemon must treat this as end of input, not as a dropped client; the
client is gone only when the connection closes entirely.

A terminal in raw mode never delivers end-of-file: Ctrl-D is the byte 0x04, and it is the
application's to interpret.

## Signals

On Linux and macOS the launcher forwards these signals to the daemon as `signal` documents,
named without their `SIG` prefix: `INT`, `QUIT`, `TERM`, `HUP`, `WINCH`, `USR1`, `USR2`, `TSTP`
and `CONT`. The daemon answers each with a `signal-ack` saying whether the invocation accepted
it. What the launcher does next depends on the signal:

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
would stop it again — and forwards `CONT`, so an application can redraw. A terminal in raw
mode does not generate `TSTP` from Ctrl-Z (that byte, 0x1A, goes to the application); the
path is exercised by `kill -TSTP`, and by Ctrl-Z when stdin is a pipe.

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
either terminal state. A rejected or unanswered event ends the launcher with the system's own
status for a process ended by a control event, `STATUS_CONTROL_C_EXIT` (`0xC000013A`); an
accepted close, logoff or shutdown drains the invocation's stderr and then ends with the same
status, since the system is about to end the process regardless.

## Exit status

Once the invocation's stdout and stderr have both ended, the launcher asks the daemon for the
invocation's exit status (`exit`) and exits with it. A launcher that cannot reach the daemon
exits with 2; one that was terminated by a signal dies of that signal, as above, and never
asks.
