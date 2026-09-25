#[cfg(unix)]
use std::io::Write;

use std::io::IsTerminal;

#[cfg(unix)]
#[derive(Clone, Copy)]
pub struct TtyState {
    termios: Option<libc::termios>,
    is_tty: bool,
}

#[cfg(windows)]
#[derive(Clone, Copy)]
pub struct TtyState {
    stdin_mode: Option<u32>,
    stdout_mode: Option<u32>,
    is_tty: bool,
}

// The three descriptors are independent — `cmd > file` leaves stdin on the
// terminal while stdout is a file — so each is asked separately. The daemon
// cannot ask for itself: its streams are the socket, not the client's terminal,
// so whatever is learned here is the only answer it will ever have.
//
// `is_terminal` is `isatty` on Unix and `GetConsoleMode` on Windows, plus a
// check for the MSYS/Cygwin pseudo-terminals that a bare `GetConsoleMode` calls
// pipes.

pub fn stdin_is_tty() -> bool { std::io::stdin().is_terminal() }

pub fn stdout_is_tty() -> bool { std::io::stdout().is_terminal() }

pub fn stderr_is_tty() -> bool { std::io::stderr().is_terminal() }

impl TtyState {
    // The state of a terminal this launcher never reconfigured — a pipe, or a terminal it
    // was not entitled to touch — so that restoring it is a no-op.
    #[cfg(unix)]
    pub fn detached() -> TtyState { TtyState { termios: None, is_tty: false } }

    #[cfg(windows)]
    pub fn detached() -> TtyState {
        TtyState { stdin_mode: None, stdout_mode: None, is_tty: false }
    }
}

// The process's file-creation mask, in octal. `umask` can only be read by setting it, so
// this must run before any thread that might create a file — it is called once, at startup.
#[cfg(unix)]
pub fn umask() -> Option<String> {
    unsafe {
        let current = libc::umask(0);
        libc::umask(current);
        Some(format!("{:03o}", current))
    }
}

#[cfg(windows)]
pub fn umask() -> Option<String> { None }

// The console's input and output code pages: how the daemon should interpret the bytes it
// reads and encode the bytes it writes, on a console that is not UTF-8.
#[cfg(windows)]
pub fn codepages() -> Option<(u32, u32)> {
    use windows_sys::Win32::System::Console::{GetConsoleCP, GetConsoleOutputCP};
    let (input, output) = unsafe { (GetConsoleCP(), GetConsoleOutputCP()) };
    if input == 0 && output == 0 { None } else { Some((input, output)) }
}

#[cfg(unix)]
pub fn codepages() -> Option<(u32, u32)> { None }

// Whether this process may read and reconfigure the terminal on stdin: under job control,
// only the foreground process group of the controlling terminal may, and a background job
// that tried would be stopped by SIGTTIN or SIGTTOU. True when stdin is not a terminal —
// there is then no terminal to be in the background of — and when the answer cannot be
// determined, since the pre-existing behaviour is to proceed.
#[cfg(unix)]
pub fn in_foreground() -> bool {
    if !stdin_is_tty() { return true; }
    let terminal_group = unsafe { libc::tcgetpgrp(libc::STDIN_FILENO) };
    terminal_group < 0 || terminal_group == unsafe { libc::getpgrp() }
}

// Windows has no job control.
#[cfg(windows)]
pub fn in_foreground() -> bool { true }


// The launcher must know the real terminal size so it can forward it to the
// daemon (which only sees a socket and cannot query the tty itself). Querying
// from the launcher avoids a fragile ANSI cursor-position handshake across
// the socket pipeline.
#[cfg(unix)]
pub fn terminal_size() -> Option<(u16, u16)> {
    // Whichever standard stream is the terminal answers; stdout first, since the size
    // matters to what is written there. A pseudo-terminal whose size was never set reports
    // zero, which is no answer.
    for fd in [libc::STDOUT_FILENO, libc::STDIN_FILENO, libc::STDERR_FILENO] {
        if unsafe { libc::isatty(fd) } == 0 { continue; }
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        let result = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) };
        if result == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
            return Some((ws.ws_col, ws.ws_row));
        }
    }
    None
}

// OSC 11 query (`\e]11;?\e\\`) asks the terminal to report its background
// colour as `\e]11;rgb:RRRR/GGGG/BBBB\e\\`. Running this from the launcher
// (before forwarders start) keeps the handshake confined to one process with
// direct tty access — much more reliable than letting the daemon attempt it
// across the socket pipeline. Any non-response bytes are returned so they can
// be prepended to the daemon's stdin and not lost.
#[cfg(unix)]
pub fn query_bg_color(timeout: std::time::Duration) -> (Option<String>, Vec<u8>) {
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::time::Instant;

    // The query goes out on stdout and the reply comes back on stdin, so unless
    // both are the terminal the handshake cannot complete — and writing it to a
    // redirected stdout would prepend eight bytes of escape sequence to the
    // invocation's output.
    if !(stdin_is_tty() && stdout_is_tty()) { return (None, Vec::new()); }

    let mut stdout = std::io::stdout();
    if stdout.write_all(b"\x1b]11;?\x1b\\").is_err() { return (None, Vec::new()); }
    let _ = stdout.flush();

    let mut buf: Vec<u8> = Vec::with_capacity(64);
    let deadline = Instant::now() + timeout;
    let fd = std::io::stdin().as_raw_fd();

    loop {
        let now = Instant::now();
        if now >= deadline { break; }
        let remaining = (deadline - now).as_millis() as i32;
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let r = unsafe { libc::poll(&mut pfd, 1, remaining.max(1)) };
        if r <= 0 { break; }

        let mut chunk = [0u8; 64];
        match std::io::stdin().read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                // Stop as soon as we see a terminator that could end an OSC response.
                if find_subseq(&buf, b"\x1b\\").is_some() || buf.contains(&0x07) {
                    break;
                }
            }
        }
    }

    parse_osc11(&buf)
}

// The classic Windows console does not answer an OSC 11 query, and a query it does not
// understand would sit in the input as garbage; Windows Terminal answers it. So ask only
// under Windows Terminal (which marks its sessions with `WT_SESSION`), and read the reply
// through the console input queue rather than a blocking read: the queue may hold focus or
// mouse records that a character read would wait behind.
#[cfg(windows)]
pub fn query_bg_color(timeout: std::time::Duration) -> (Option<String>, Vec<u8>) {
    use std::io::{Read, Write};
    use std::time::Instant;
    use windows_sys::Win32::System::Console::{
        GetNumberOfConsoleInputEvents, GetStdHandle, PeekConsoleInputW, ReadConsoleInputW,
        INPUT_RECORD, KEY_EVENT, STD_INPUT_HANDLE,
    };

    if !stdin_is_tty() || std::env::var_os("WT_SESSION").is_none() { return (None, Vec::new()); }

    let mut stdout = std::io::stdout();
    if stdout.write_all(b"\x1b]11;?\x1b\\").is_err() { return (None, Vec::new()); }
    let _ = stdout.flush();

    let mut buf: Vec<u8> = Vec::with_capacity(64);
    let deadline = Instant::now() + timeout;
    let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };

    while Instant::now() < deadline {
        let mut pending: u32 = 0;
        if unsafe { GetNumberOfConsoleInputEvents(handle, &mut pending) } == 0 { break; }
        if pending == 0 { std::thread::sleep(std::time::Duration::from_millis(5)); continue; }

        let mut records: [INPUT_RECORD; 32] = unsafe { std::mem::zeroed() };
        let mut count: u32 = 0;
        if unsafe { PeekConsoleInputW(handle, records.as_mut_ptr(), 32, &mut count) } == 0 { break; }
        let characters = records[..count as usize].iter().any(|record| unsafe {
            record.EventType as u32 == KEY_EVENT
                && record.Event.KeyEvent.bKeyDown != 0
                && record.Event.KeyEvent.uChar.UnicodeChar != 0
        });

        if characters {
            // A character read returns what is pending without waiting for more.
            let mut chunk = [0u8; 64];
            match std::io::stdin().read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if find_subseq(&buf, b"\x1b\\").is_some() || buf.contains(&0x07) { break; }
                }
            }
        } else {
            // Only non-character records: discard them so they cannot block a later read.
            let mut discarded: u32 = 0;
            unsafe { ReadConsoleInputW(handle, records.as_mut_ptr(), count, &mut discarded); }
        }
    }

    parse_osc11(&buf)
}

fn parse_osc11(buf: &[u8]) -> (Option<String>, Vec<u8>) {
    let prefix = b"\x1b]11;rgb:";
    let Some(start) = find_subseq(buf, prefix) else {
        return (None, buf.to_vec());
    };
    let after_prefix = start + prefix.len();
    let rest = &buf[after_prefix..];

    let term_st = find_subseq(rest, b"\x1b\\");
    let term_bel = rest.iter().position(|&b| b == 0x07);
    let (rgb_end, term_len) = match (term_st, term_bel) {
        (Some(a), Some(b)) if a < b => (a, 2),
        (Some(_), Some(b))           => (b, 1),
        (Some(a), None)              => (a, 2),
        (None, Some(b))              => (b, 1),
        (None, None)                 => return (None, buf.to_vec()),
    };

    let rgb_str = std::str::from_utf8(&rest[..rgb_end]).ok().map(str::to_owned);

    let mut leftover = buf[..start].to_vec();
    leftover.extend_from_slice(&rest[rgb_end + term_len..]);

    (rgb_str, leftover)
}

fn find_subseq(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() { return None; }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(windows)]
pub fn terminal_size() -> Option<(u16, u16)> {
    use windows_sys::Win32::System::Console::{
        CONSOLE_SCREEN_BUFFER_INFO, GetConsoleScreenBufferInfo, GetStdHandle, STD_OUTPUT_HANDLE,
    };
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut info: CONSOLE_SCREEN_BUFFER_INFO = std::mem::zeroed();
        if GetConsoleScreenBufferInfo(handle, &mut info) != 0 {
            let cols = (info.srWindow.Right - info.srWindow.Left + 1).max(0) as u16;
            let rows = (info.srWindow.Bottom - info.srWindow.Top + 1).max(0) as u16;
            if cols > 0 && rows > 0 { Some((cols, rows)) } else { None }
        } else {
            None
        }
    }
}

#[cfg(unix)]
pub fn save_tty_state() -> TtyState {
    let mut termios: libc::termios = unsafe { std::mem::zeroed() };
    let is_tty = stdin_is_tty();
    if !is_tty {
        crate::debug!("tty: save_tty_state — stdin is not a tty, nothing to save");
        return TtyState { termios: None, is_tty: false };
    }
    unsafe {
        if libc::tcgetattr(libc::STDIN_FILENO, &mut termios) == 0 {
            crate::debug!("tty: save_tty_state — saved termios");
            return TtyState { termios: Some(termios), is_tty: true };
        }
    }
    crate::debug!(
        "tty: save_tty_state — tcgetattr failed: {}",
        std::io::Error::last_os_error(),
    );
    TtyState { termios: None, is_tty }
}

#[cfg(windows)]
pub fn save_tty_state() -> TtyState {
    use windows_sys::Win32::System::Console::{GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    let is_tty = stdin_is_tty();
    unsafe {
        let h_in = GetStdHandle(STD_INPUT_HANDLE);
        let h_out = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut in_mode: u32 = 0;
        let mut out_mode: u32 = 0;
        let in_ok = GetConsoleMode(h_in, &mut in_mode) != 0;
        let out_ok = GetConsoleMode(h_out, &mut out_mode) != 0;
        TtyState {
            stdin_mode: if in_ok { Some(in_mode) } else { None },
            stdout_mode: if out_ok { Some(out_mode) } else { None },
            is_tty,
        }
    }
}

#[cfg(unix)]
pub fn set_raw_mode() {
    if !stdin_is_tty() {
        crate::debug!("tty: set_raw_mode skipped — stdin is not a tty");
        return;
    }
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(libc::STDIN_FILENO, &mut t) != 0 {
            crate::debug!(
                "tty: set_raw_mode — tcgetattr failed: {}",
                std::io::Error::last_os_error(),
            );
            return;
        }
        // Equivalent to: intr undef -echo icanon raw opost (keep opost)
        t.c_lflag &= !(libc::ICANON | libc::ECHO | libc::IEXTEN | libc::ISIG);
        t.c_iflag &= !(libc::IXON | libc::ICRNL | libc::BRKINT | libc::INPCK | libc::ISTRIP);
        // OPOST stays on (as in the bash prefix: `opost`)
        t.c_oflag |= libc::OPOST;
        t.c_cc[libc::VMIN] = 1;
        t.c_cc[libc::VTIME] = 0;
        // intr undef
        t.c_cc[libc::VINTR] = 0;
        if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &t) != 0 {
            crate::debug!(
                "tty: set_raw_mode — tcsetattr failed: {}",
                std::io::Error::last_os_error(),
            );
        } else {
            crate::debug!("tty: set_raw_mode — raw mode set");
        }
    }
}

// Put the terminal back into canonical ("cooked") mode, at the daemon's request, so a
// line-based command gets the driver's own echo and line editing. The saved pre-launch
// termios *is* the user's cooked mode, so prefer restoring it verbatim; only when nothing
// was saved (tcgetattr failed) do we synthesise a plausible cooked setting.
#[cfg(unix)]
pub fn set_cooked_mode(state: &TtyState) {
    if !stdin_is_tty() {
        crate::debug!("tty: set_cooked_mode skipped — stdin is not a tty");
        return;
    }
    if state.termios.is_some() {
        restore_tty_state(state);
        crate::debug!("tty: set_cooked_mode — restored saved (cooked) termios");
        return;
    }
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(libc::STDIN_FILENO, &mut t) != 0 {
            crate::debug!(
                "tty: set_cooked_mode — tcgetattr failed: {}",
                std::io::Error::last_os_error(),
            );
            return;
        }
        t.c_lflag |= libc::ICANON | libc::ECHO | libc::ECHOE | libc::ISIG | libc::IEXTEN;
        t.c_iflag |= libc::ICRNL | libc::IXON | libc::BRKINT;
        t.c_oflag |= libc::OPOST | libc::ONLCR;
        t.c_cc[libc::VINTR] = 3;
        if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &t) != 0 {
            crate::debug!(
                "tty: set_cooked_mode — tcsetattr failed: {}",
                std::io::Error::last_os_error(),
            );
        } else {
            crate::debug!("tty: set_cooked_mode — synthesised cooked mode set");
        }
    }
}

#[cfg(windows)]
pub fn set_cooked_mode(state: &TtyState) {
    use windows_sys::Win32::System::Console::{
        GetConsoleMode, GetStdHandle, SetConsoleMode, STD_INPUT_HANDLE,
        ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, ENABLE_PROCESSED_INPUT,
        ENABLE_VIRTUAL_TERMINAL_INPUT,
    };
    if !state.is_tty {
        crate::debug!("tty: set_cooked_mode skipped — stdin is not a console");
        return;
    }
    unsafe {
        let h_in = GetStdHandle(STD_INPUT_HANDLE);
        let mut in_mode: u32 = 0;
        if GetConsoleMode(h_in, &mut in_mode) == 0 {
            crate::debug!(
                "tty: set_cooked_mode — GetConsoleMode(stdin) failed: {}",
                std::io::Error::last_os_error(),
            );
            return;
        }
        in_mode |= ENABLE_ECHO_INPUT | ENABLE_LINE_INPUT | ENABLE_PROCESSED_INPUT;
        in_mode &= !ENABLE_VIRTUAL_TERMINAL_INPUT;
        if SetConsoleMode(h_in, in_mode) == 0 {
            crate::debug!(
                "tty: set_cooked_mode — SetConsoleMode(stdin) failed: {}",
                std::io::Error::last_os_error(),
            );
        } else {
            crate::debug!("tty: set_cooked_mode — stdin cooked mode set");
        }
    }
}

#[cfg(windows)]
pub fn set_raw_mode() {
    use windows_sys::Win32::System::Console::{
        GetStdHandle, SetConsoleMode, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
        ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, ENABLE_PROCESSED_INPUT,
        ENABLE_VIRTUAL_TERMINAL_INPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
    };
    unsafe {
        let h_in = GetStdHandle(STD_INPUT_HANDLE);
        let mut in_mode: u32 = 0;
        if windows_sys::Win32::System::Console::GetConsoleMode(h_in, &mut in_mode) != 0 {
            in_mode &= !(ENABLE_ECHO_INPUT | ENABLE_LINE_INPUT | ENABLE_PROCESSED_INPUT);
            in_mode |= ENABLE_VIRTUAL_TERMINAL_INPUT;
            if SetConsoleMode(h_in, in_mode) == 0 {
                crate::debug!(
                    "tty: set_raw_mode — SetConsoleMode(stdin) failed: {}",
                    std::io::Error::last_os_error(),
                );
            } else {
                crate::debug!("tty: set_raw_mode — stdin raw mode set");
            }
        } else {
            crate::debug!(
                "tty: set_raw_mode — GetConsoleMode(stdin) failed: {}",
                std::io::Error::last_os_error(),
            );
        }
        let h_out = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut out_mode: u32 = 0;
        if windows_sys::Win32::System::Console::GetConsoleMode(h_out, &mut out_mode) != 0 {
            out_mode |= ENABLE_VIRTUAL_TERMINAL_PROCESSING;
            if SetConsoleMode(h_out, out_mode) == 0 {
                crate::debug!(
                    "tty: set_raw_mode — SetConsoleMode(stdout) failed: {}",
                    std::io::Error::last_os_error(),
                );
            } else {
                crate::debug!("tty: set_raw_mode — stdout vt processing enabled");
            }
        } else {
            crate::debug!(
                "tty: set_raw_mode — GetConsoleMode(stdout) failed: {}",
                std::io::Error::last_os_error(),
            );
        }
    }
}

#[cfg(unix)]
pub fn restore_tty_state(state: &TtyState) {
    if !state.is_tty {
        crate::debug!("tty: restore_tty_state — stdin is not a tty, nothing to restore");
        return;
    }
    if let Some(termios) = state.termios {
        unsafe {
            if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &termios) != 0 {
                crate::debug!(
                    "tty: restore_tty_state — tcsetattr failed: {}",
                    std::io::Error::last_os_error(),
                );
            } else {
                crate::debug!("tty: restore_tty_state — termios restored");
            }
        }
    } else {
        crate::debug!("tty: restore_tty_state — no saved termios available");
    }
}

#[cfg(windows)]
pub fn restore_tty_state(state: &TtyState) {
    if !state.is_tty {
        crate::debug!("tty: restore_tty_state — stdin is not a tty, nothing to restore");
        return;
    }
    use windows_sys::Win32::System::Console::{GetStdHandle, SetConsoleMode, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    unsafe {
        if let Some(mode) = state.stdin_mode {
            if SetConsoleMode(GetStdHandle(STD_INPUT_HANDLE), mode) == 0 {
                crate::debug!(
                    "tty: restore_tty_state — SetConsoleMode(stdin) failed: {}",
                    std::io::Error::last_os_error(),
                );
            }
        }
        if let Some(mode) = state.stdout_mode {
            if SetConsoleMode(GetStdHandle(STD_OUTPUT_HANDLE), mode) == 0 {
                crate::debug!(
                    "tty: restore_tty_state — SetConsoleMode(stdout) failed: {}",
                    std::io::Error::last_os_error(),
                );
            }
        }
    }
}

