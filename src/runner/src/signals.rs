use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

use crate::bintel::Composition;
use crate::protocol::{SignalAck, SignalDetail};
use crate::tty::TtyState;

static SOCKET_PATH: OnceLock<PathBuf> = OnceLock::new();
// The composition the invocation's documents are written under; see `acceptance.rs`.
static COMPOSITION: OnceLock<Composition> = OnceLock::new();
static CLIENT_PID: AtomicU32 = AtomicU32::new(0);
static TERMINATION: OnceLock<Arc<AtomicI32>> = OnceLock::new();
static SAVED_TTY: OnceLock<TtyState> = OnceLock::new();
// Whether this launcher put the terminal into raw mode, and so must put it back after a
// stop and re-apply it after a continue. False for a pipe, and for a terminal the launcher
// was not entitled to reconfigure (a background job).
static RAW_MODE_OWNED: AtomicBool = AtomicBool::new(false);
static TIMEOUT_MS: AtomicU32 = AtomicU32::new(250);

const DEFAULT_TIMEOUT_MS: u32 = 250;

fn forward_signal(name: &str, detail: SignalDetail) -> SignalAck {
    if let (Some(path), Some(composition)) = (SOCKET_PATH.get(), COMPOSITION.get()) {
        // UnixStream::connect is not strictly async-signal-safe (allocates),
        // but this matches the pre-existing TcpStream::connect behaviour and
        // has been reliable in practice. Revisit only if signal storms cause
        // trouble.
        crate::protocol::send_signal(
            path.as_path(),
            CLIENT_PID.load(Ordering::SeqCst),
            name,
            detail,
            TIMEOUT_MS.load(Ordering::SeqCst) as u64,
            composition,
        )
    } else {
        SignalAck::Timeout
    }
}

// The terminal's size travels with the signals that say it may have changed: WINCH, and
// CONT, since the window may have been resized while the job was stopped. A POSIX signal
// carries no payload and the daemon holds no terminal to ask, so this is the only way it
// can learn the new size. TIOCGWINSZ is async-signal-safe.
#[cfg(unix)]
fn sized() -> SignalDetail {
    SignalDetail { size: crate::tty::terminal_size(), deadline_ms: None }
}

// Records that the launcher must die once the invocation's streams are drained: by the
// signal numbered `code` on Unix, or with `code` as the exit status on Windows. The main
// path polls for this; see `die`.
fn flag_termination(code: i32) {
    if let Some(flag) = TERMINATION.get() {
        flag.store(code, Ordering::SeqCst);
    }
}

fn install_state(
    socket_path: PathBuf,
    composition: Composition,
    pid: u32,
    termination: Arc<AtomicI32>,
    saved_tty: TtyState,
    raw_mode_owned: bool,
) {
    let _ = SOCKET_PATH.set(socket_path);
    let _ = COMPOSITION.set(composition);
    CLIENT_PID.store(pid, Ordering::SeqCst);
    let _ = TERMINATION.set(termination);
    let _ = SAVED_TTY.set(saved_tty);
    RAW_MODE_OWNED.store(raw_mode_owned, Ordering::SeqCst);
    let timeout = std::env::var("ETHEREAL_SIGNAL_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(DEFAULT_TIMEOUT_MS);
    TIMEOUT_MS.store(timeout, Ordering::SeqCst);
}

// ── Unix ──────────────────────────────────────────────────────────────────────

#[cfg(unix)]
fn signal_name(signal: libc::c_int) -> Option<&'static str> {
    Some(match signal {
        libc::SIGINT   => "INT",
        libc::SIGQUIT  => "QUIT",
        libc::SIGWINCH => "WINCH",
        libc::SIGTERM  => "TERM",
        libc::SIGHUP   => "HUP",
        libc::SIGUSR1  => "USR1",
        libc::SIGUSR2  => "USR2",
        libc::SIGTSTP  => "TSTP",
        libc::SIGCONT  => "CONT",
        _ => return None,
    })
}

// Restores the OS default action for `signal` and raises it on this process. For a
// terminating signal that is the end: the parent sees a genuine signal death, and a shell
// reports the conventional 128+signum status. Falls through only if the signal is blocked
// or otherwise does not terminate, in which case the caller decides what to do.
#[cfg(unix)]
fn raise_default(signal: libc::c_int) {
    unsafe {
        libc::signal(signal, libc::SIG_DFL);
        libc::raise(signal);
    }
}

#[cfg(unix)]
fn fallback(signal: libc::c_int) {
    match signal {
        // The daemon declined, or did not answer: take the signal's default action, which
        // for these is to terminate (QUIT with a core dump), as if it had never been caught.
        libc::SIGINT | libc::SIGTERM | libc::SIGHUP | libc::SIGQUIT => raise_default(signal),
        // WINCH, USR1, USR2: the default action is to ignore, so the signal is dropped.
        _ => {}
    }
}

// Installs `handler` for `signal` unless the signal is inherited ignored. A disposition of
// `SIG_IGN` survives exec by design — it is how `nohup` and `trap '' INT` ask that a command
// never see the signal — and a launcher that overrode it would then, on a rejected forward,
// restore the default action and die of a signal its caller had asked it to ignore.
// `sigaction` rather than `signal`, so restart semantics are explicit rather than
// implementation-defined: the forwarders' blocking reads must resume after a handler runs.
#[cfg(unix)]
pub(crate) fn install_handler(signal: libc::c_int, handler: extern "C" fn(libc::c_int)) {
    unsafe {
        let mut current: libc::sigaction = std::mem::zeroed();
        if libc::sigaction(signal, std::ptr::null(), &mut current) != 0 { return; }
        if current.sa_sigaction == libc::SIG_IGN {
            crate::debug!("signals: {} is inherited ignored; leaving it", signal);
            return;
        }
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handler as *const () as libc::sighandler_t;
        action.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(signal, &action, std::ptr::null_mut());
    }
}

#[cfg(unix)]
pub fn install(
    socket_path: PathBuf,
    composition: Composition,
    pid: u32,
    termination: Arc<AtomicI32>,
    saved_tty: TtyState,
    raw_mode_owned: bool,
) {
    install_state(socket_path, composition, pid, termination, saved_tty, raw_mode_owned);

    let signals = [
        libc::SIGINT, libc::SIGQUIT, libc::SIGWINCH, libc::SIGTERM,
        libc::SIGHUP, libc::SIGUSR1, libc::SIGUSR2, libc::SIGTSTP, libc::SIGCONT,
    ];
    for signal in signals { install_handler(signal, handler); }
}

#[cfg(unix)]
extern "C" fn handler(signal: libc::c_int) {
    match signal {
        libc::SIGTSTP => suspend(),
        libc::SIGCONT => resume(),
        _ => {
            let Some(name) = signal_name(signal) else { return };
            let detail = if signal == libc::SIGWINCH { sized() } else { SignalDetail::default() };
            let ack = forward_signal(name, detail);
            if signal == libc::SIGTERM && ack == SignalAck::Accept { flag_termination(signal); }
            match ack {
                SignalAck::Accept                      => {}
                SignalAck::Reject | SignalAck::Timeout => fallback(signal),
            }
        }
    }
}

// A stop request: tell the daemon, put the terminal back the way the shell expects to
// find it, and then actually stop, by taking the default action for TSTP. The signal is
// blocked while its own handler runs, so it is unblocked explicitly; the stop then happens
// here, and execution resumes here on SIGCONT, when the handler is put back.
#[cfg(unix)]
fn suspend() {
    let _ = forward_signal("TSTP", SignalDetail::default());
    if RAW_MODE_OWNED.load(Ordering::SeqCst) {
        if let Some(saved) = SAVED_TTY.get() { crate::tty::restore_tty_state(saved); }
    }
    unsafe {
        libc::signal(libc::SIGTSTP, libc::SIG_DFL);
        libc::raise(libc::SIGTSTP);
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGTSTP);
        libc::sigprocmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
    }
    // Stopped above; continuing here.
    install_handler(libc::SIGTSTP, handler);
}

// Continued after a stop: re-apply raw mode if this launcher owns the terminal's mode and
// is (still, or again) in the foreground — from the background, tcsetattr would stop the
// job once more — and tell the daemon, so an application can redraw. The window may have
// been resized while the job was stopped; the daemon learns the new size the same way it
// does for WINCH.
#[cfg(unix)]
fn resume() {
    if RAW_MODE_OWNED.load(Ordering::SeqCst) && crate::tty::in_foreground() {
        crate::tty::set_raw_mode();
    }
    let _ = forward_signal("CONT", sized());
}

// The end of a launcher that was told to terminate: the invocation's streams are drained,
// the terminal restored, and the process now dies of the signal it was sent, so that its
// parent sees a signal death rather than an exit status that merely imitates one.
#[cfg(unix)]
pub fn die(signal: i32) -> ! {
    raise_default(signal);
    std::process::exit(128 + signal)
}

// ── Windows ───────────────────────────────────────────────────────────────────

// The exit status of a process that died of a console control event, by Windows's own
// convention (`STATUS_CONTROL_C_EXIT`, the status the system assigns when no handler runs).
#[cfg(windows)]
pub const CONTROL_EXIT_STATUS: i32 = 0xC000_013Au32 as i32;

#[cfg(windows)]
fn fallback(name: &str) {
    match name {
        "CTRL_C" | "CTRL_BREAK" | "CTRL_CLOSE" | "CTRL_LOGOFF" | "CTRL_SHUTDOWN" => {
            std::process::exit(CONTROL_EXIT_STATUS);
        }
        _ => {}
    }
}

#[cfg(windows)]
pub fn install(
    socket_path: PathBuf,
    composition: Composition,
    pid: u32,
    termination: Arc<AtomicI32>,
    saved_tty: TtyState,
    raw_mode_owned: bool,
) {
    use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
    install_state(socket_path, composition, pid, termination, saved_tty, raw_mode_owned);
    unsafe { SetConsoleCtrlHandler(Some(console_handler), 1); }
}

#[cfg(windows)]
unsafe extern "system" fn console_handler(ctrl_type: u32) -> windows_sys::Win32::Foundation::BOOL {
    use windows_sys::Win32::System::Console::{
        CTRL_C_EVENT, CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT,
    };
    let name = match ctrl_type {
        CTRL_C_EVENT        => "CTRL_C",
        CTRL_BREAK_EVENT    => "CTRL_BREAK",
        CTRL_CLOSE_EVENT    => "CTRL_CLOSE",
        CTRL_LOGOFF_EVENT   => "CTRL_LOGOFF",
        CTRL_SHUTDOWN_EVENT => "CTRL_SHUTDOWN",
        _ => return 0,
    };
    let ending = matches!(ctrl_type, CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT);
    // The system ends the process about five seconds after these, whatever it is doing;
    // the daemon is told, so the application can choose what to finish.
    let detail = SignalDetail {
        size: None,
        deadline_ms: if ending { Some(CONTROL_EVENT_DEADLINE_MS) } else { None },
    };
    let ack = forward_signal(name, detail);
    if ending && ack == SignalAck::Accept { flag_termination(CONTROL_EXIT_STATUS); }
    match ack {
        SignalAck::Accept                      => {}
        SignalAck::Reject | SignalAck::Timeout => fallback(name),
    }
    1
}

#[cfg(windows)]
pub fn die(code: i32) -> ! {
    std::process::exit(code)
}

#[cfg(windows)]
const CONTROL_EVENT_DEADLINE_MS: u64 = 5000;

// Windows has no SIGWINCH. A resize is delivered as a console input record, but reading
// those would take keystrokes away from stdin, so the screen-buffer size is polled instead
// and a `WINCH` document sent, with the new size, when it changes.
#[cfg(windows)]
pub fn watch_for_resize() {
    const POLL: std::time::Duration = std::time::Duration::from_millis(200);
    std::thread::spawn(move || {
        let mut last = crate::tty::terminal_size();
        loop {
            std::thread::sleep(POLL);
            let current = crate::tty::terminal_size();
            if current.is_some() && current != last {
                last = current;
                let _ = forward_signal("WINCH", SignalDetail { size: current, deadline_ms: None });
            }
        }
    });
}
