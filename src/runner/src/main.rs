use std::env;
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod bintel;
mod config;
mod state;
mod java;
mod launch;
mod progress;
mod protocol;
mod signals;
mod tty;
mod uds;
mod update;
mod verify;
mod wrapper;
mod xeq;

// Debug-trace helper: when `ETHEREAL_DEBUG=1` is set, writes a timestamped
// line to stderr from the launcher. Intentionally lazy — avoids any cost
// when the env var isn't set.
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {{
        if std::env::var_os("ETHEREAL_DEBUG").is_some_and(|v| !v.is_empty() && v != "0") {
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            eprintln!("[eth +{ms}] {}", format!($($arg)*));
        }
    }};
}

use protocol::ClientInfo;
use uds::UnixStream;

const FORWARD_BUFFER_SIZE: usize = 4096;
const TERMINATION_POLL: Duration = Duration::from_millis(50);
// After a termination signal, how long the invocation's stderr is drained before the
// launcher gives up on it and dies. The daemon closes the stderr connection when the
// invocation ends, so this bounds only an invocation that accepted the signal and then
// failed to act on it.
const TERMINATION_GRACE: Duration = Duration::from_secs(2);
const STARTUP_FAILURE_EXIT_CODE: i32 = 2;

// The launcher re-invokes itself with this variable set when starting the daemon, so the
// JVM runs as a child of a process whose name matches the client (this binary *is* the
// renamed launcher). A variable rather than an argument sentinel, so that no argument value
// is reserved: the application may receive any argv at all.
pub const WRAP_VARIABLE: &str = "XEQ_WRAP_JAVA";

// Asks the launcher to download a JVM when none suitable is found. Recognised as an
// environment variable, or as `--download` when it is the *sole* argument; in any other
// position `--download` belongs to the application. See `spec/launcher.md`.
pub const DOWNLOAD_VARIABLE: &str = "XEQ_DOWNLOAD";
const DOWNLOAD_FLAG: &str = "--download";

// Argument values the daemon side reserves for its own internal invocations (shell
// completion and administration). They are the daemon's contract, not the launcher's; the
// launcher only recognises them so as not to touch the terminal for an invocation that runs
// behind the user's shell. See `spec/launcher.md`.
const INTERNAL_SENTINELS: [&str; 2] = ["{completions}", "{admin}"];

fn main() {
    let raw: Vec<OsString> = env::args_os().collect();
    debug!("main: argv={:?}", raw);
    if env::var_os(WRAP_VARIABLE).is_some() {
        debug!("main: dispatching to wrapper");
        wrapper::run(&raw[1..]);
    }

    let (script, args, download) = parse_arguments(raw);
    let name = script.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    debug!("main: script={} name={} args={:?}", script.display(), name, args);
    // This path is handed to the JVM as the JAR, and `update` renames it. A resolution that
    // landed on a directory or a missing file must stop here, legibly, rather than surface
    // downstream as `Invalid or corrupt jarfile` — or as a rename of whatever it hit.
    if !script.is_file() {
        eprintln!("{name}: could not locate its own executable — {} is not a file", script.display());
        std::process::exit(1);
    }
    // The configuration record follows the stub in this very file (spec/ethrcfg.md); read it
    // once, before anything consults the build id or the public key.
    let build_config = config::load(&script);
    debug!("main: build_id={} java_min={} java_pref={}", build_config.build_id, build_config.java_min, build_config.java_pref);

    update::check_updates(&script, &args, &name);
    debug!("main: post-update-check");

    let base_dir = state::base_dir(&name);
    let _ = std::fs::create_dir_all(&base_dir);
    let build_file  = base_dir.join("build");
    let pid_file    = base_dir.join("pid");
    let socket_file = base_dir.join("socket");
    let fail_file   = base_dir.join("fail");
    let progress_file = base_dir.join("progress");
    debug!("main: base_dir={}", base_dir.display());

    // Internal invocations (completions, admin) must not touch the TTY, fork a
    // stdin-forwarding thread, or install signal handlers — doing so can steal input from
    // the parent shell or trigger SIGTTOU when the process runs in a background process
    // group (e.g. under `< <(...)`).
    let internal = args.first().is_some_and(|arg| INTERNAL_SENTINELS.iter().any(|s| arg == s));
    debug!("main: internal={}", internal);

    state::backout(&fail_file, &pid_file, &name);
    state::check_state(&pid_file, &build_file, &socket_file, &script);
    debug!("main: post check_state, pid_file_has_content={}", state::file_has_content(&pid_file));

    if !state::file_has_content(&pid_file) {
        let lock_path = base_dir.join("lock");
        match state::try_exclusive_lock(&lock_path) {
            Some(_lock) => {
                debug!("main: acquired lock, launching daemon");
                launch::launch(
                    &script, &name, &base_dir,
                    &build_file, &pid_file, &socket_file, &fail_file, &progress_file,
                    &build_config, download,
                );
                debug!("main: launch::launch returned");
            }
            None => {
                // Wait by the same rule as the launcher doing the spawning: a cold
                // Burdock cache being fetched by that daemon is progress here too.
                debug!("main: another launcher holds the lock; awaiting socket");
                let (outcome, shown) =
                    launch::await_startup(&socket_file, &fail_file, &progress_file, &name, None);
                if shown && matches!(outcome, launch::Outcome::Bound) { xeq::done(&name, "Started"); }
                if !matches!(outcome, launch::Outcome::Bound) {
                    debug!("main: socket did not appear");
                    state::abort(&fail_file);
                    let reason = match outcome {
                        launch::Outcome::Idle(Some(_)) => launch::idle_reason(&outcome),
                        _ => "another launcher held the startup lock but bound no socket".to_string(),
                    };
                    state::report_failure(&base_dir, &name, &reason);
                    state::backout(&fail_file, &pid_file, &name);
                    std::process::exit(1);
                }
            }
        }
    }
    state::backout(&fail_file, &pid_file, &name);

    if !state::socket_alive(&socket_file) {
        debug!("main: socket not alive, exiting STARTUP_FAILURE");
        state::report_failure(&base_dir, &name, "its socket does not accept connections");
        std::process::exit(STARTUP_FAILURE_EXIT_CODE);
    }
    debug!("main: socket is alive");

    if internal {
        std::process::exit(run_non_interactive(&socket_file, &script, &args));
    }

    // The terminal is ours to reconfigure only when stdin is a terminal *and* this process is
    // in its foreground process group. A background job (`mytool > log &` under job control)
    // that called tcsetattr or read the terminal would be stopped by SIGTTOU or SIGTTIN; it is
    // treated instead as though its stdin were empty, and still forwards stdout and stderr.
    let stdin_tty = tty::stdin_is_tty();
    let foreground = tty::in_foreground();
    let attached = stdin_tty && foreground;
    debug!("main: stdin_tty={} foreground={} attached={}", stdin_tty, foreground, attached);

    let saved_tty = if attached { tty::save_tty_state() } else { tty::TtyState::detached() };
    if attached { tty::set_raw_mode(); }

    // Query the terminal's background colour while we still own stdin/stdout
    // directly. Anything the user happens to type during the handshake is
    // returned as `leftover` and chained ahead of stdin into the forwarder so
    // no bytes are lost.
    let (bg_color, leftover) = if attached {
        tty::query_bg_color(Duration::from_millis(150))
    } else {
        (None, Vec::new())
    };
    debug!("main: bg_color={:?} leftover={}bytes", bg_color, leftover.len());

    let info = ClientInfo::collect(&script, &args, attached, bg_color.as_deref());
    debug!("main: connecting to daemon (pid={})", info.pid);
    let (main_socket, stderr_socket) = match connect_to_daemon(&socket_file, &info) {
        Ok(connections) => { debug!("main: connected to daemon"); connections },
        Err(e) => {
            debug!("main: connect failed: {}", e);
            tty::restore_tty_state(&saved_tty);
            std::process::exit(STARTUP_FAILURE_EXIT_CODE);
        }
    };

    // The control channel lets the running command ask for a cooked (canonical) terminal —
    // ordinary echo and line editing — instead of the raw mode set above, and ask for raw
    // mode back afterwards. Only opened for a terminal we are entitled to reconfigure: with
    // a pipe there is nothing to switch, and the extra connection would be pure cost.
    if attached {
        match UnixStream::connect(&socket_file) {
            Ok(mut control) => {
                protocol::send_control_request(&mut control, info.pid);
                debug!("main: control channel open");
                spawn_control(control, saved_tty);
            }

            Err(error) => debug!("main: control channel unavailable: {}", error),
        }
    }

    // Stdin is forwarded from a foreground terminal or from anything that is not a terminal
    // (a pipe, a file). A terminal we may not read — a background job — is presented to the
    // daemon as already at end-of-file. Either way the write half of the connection is shut
    // down once stdin is exhausted, which is how the invocation's stdin reaches EOF: dropping
    // the forwarder's clone of the socket is not enough while other clones stay open.
    let stdin_socket = main_socket.try_clone().expect("clone main socket");
    if attached || !stdin_tty {
        let stdin_reader = std::io::Cursor::new(leftover).chain(std::io::stdin());
        spawn_stdin_forwarder(stdin_reader, stdin_socket);
    } else {
        let _ = stdin_socket.shutdown(Shutdown::Write);
    }
    let stdout_thread = spawn_forwarder(
        main_socket.try_clone().expect("clone main socket"),
        std::io::stdout(),
        true,
    );
    let stderr_thread = spawn_forwarder(
        stderr_socket.try_clone().expect("clone stderr socket"),
        std::io::stderr(),
        true,
    );

    // Set to the terminating signal's number (or, on Windows, the exit code for the console
    // event) once the daemon has accepted a termination the launcher must follow.
    let termination = Arc::new(AtomicI32::new(0));
    signals::install(socket_file.clone(), info.pid, termination.clone(), saved_tty, attached);

    // When termination is flagged, shut down the main socket so the stdout forwarder
    // unblocks at once, then bound the stderr drain: the daemon closes that connection when
    // the invocation ends, but an invocation that accepted the signal and then ignored it
    // must not keep the launcher alive.
    let socket_for_shutdown = main_socket.try_clone().expect("clone main socket");
    let monitor_flag = termination.clone();
    std::thread::spawn(move || {
        while monitor_flag.load(Ordering::SeqCst) == 0 { std::thread::sleep(TERMINATION_POLL); }
        let _ = socket_for_shutdown.shutdown(Shutdown::Both);
        std::thread::sleep(TERMINATION_GRACE);
        let _ = stderr_socket.shutdown(Shutdown::Both);
    });

    let _ = stdout_thread.join();
    tty::restore_tty_state(&saved_tty);
    let _ = stderr_thread.join();

    let signal = termination.load(Ordering::SeqCst);
    if signal != 0 {
        debug!("main: terminated by signal {}; dying by it", signal);
        signals::die(signal);
    }
    std::process::exit(protocol::terminate(&socket_file, info.pid));
}

fn parse_arguments(raw: Vec<OsString>) -> (PathBuf, Vec<OsString>, bool) {
    let executable = raw.first().cloned().unwrap_or_default();
    let (args, download) = intercept(&raw[raw.len().min(1)..]);
    let script = resolve_script(&executable.to_string_lossy(), std::env::current_exe().ok());
    (strip_extended_prefix(script), args, download)
}

// The launcher's own arguments, separated from the application's. `--download` is a
// launcher concern — it matters only when no JVM is present — so it is recognised only as
// the sole argument, where it cannot be an application's option value or follow a `--`
// separator. `XEQ_DOWNLOAD` in the environment asks for the same thing from a script.
fn intercept(args: &[OsString]) -> (Vec<OsString>, bool) {
    let requested = env::var_os(DOWNLOAD_VARIABLE).is_some_and(|v| !v.is_empty() && v != "0");
    if args.len() == 1 && args[0] == DOWNLOAD_FLAG { (Vec::new(), true) }
    else { (args.to_vec(), requested) }
}

// The file the runner hands the JVM is its OWN executable — stub, record and JAR are one file
// (spec/ethrcfg.md) — so ask the operating system for it. argv[0] cannot answer: a $PATH
// lookup leaves a bare name there, and canonicalizing a bare name resolves it against the
// CURRENT DIRECTORY, so a launcher run beside anything of the same name launched that instead
// (`Invalid or corrupt jarfile …`), and `update::check_updates` would have renamed it.
//
// `current_exe` is taken as an argument rather than read here so that the fallback arm below,
// which is unreachable in practice, is still testable.
fn resolve_script(executable: &str, current_exe: Option<PathBuf>) -> PathBuf {
    if let Some(path) = current_exe { return path; }

    // `current_exe` fails only in exotic cases — the binary unlinked mid-run, or /proc not
    // mounted. Fall back to argv[0], read the way `execvp` reads it: a name with no separator
    // came from $PATH and is not a relative path.
    if argv0_is_path(executable) {
        if let Ok(path) = std::fs::canonicalize(executable) { return path; }
    } else if let Some(path) = java::which(executable) {
        return path;
    }

    PathBuf::from(executable)
}

// True when argv[0] names a path rather than a command found on $PATH.
fn argv0_is_path(executable: &str) -> bool {
    Path::new(executable).parent().is_some_and(|parent| !parent.as_os_str().is_empty())
}

// On Windows, `std::fs::canonicalize` returns the extended-length form
// (`\\?\C:\…`). Java's JAR loader can read the manifest of a JAR opened via
// `\\?\…` but cannot load class entries from it, so the daemon launches with
// `Could not find or load main class …` even though the class is in the JAR.
// Strip the prefix back to a conventional drive-letter path on Windows; on
// Unix this is a no-op.
fn strip_extended_prefix(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let s = path.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{}", rest));
        }
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path
}

fn connect_to_daemon(socket_path: &Path, info: &ClientInfo)
-> io::Result<(UnixStream, UnixStream)> {
    let mut main_socket = UnixStream::connect(socket_path)?;
    protocol::send_init(&mut main_socket, info);
    let mut stderr_socket = UnixStream::connect(socket_path)?;
    protocol::send_stderr_request(&mut stderr_socket, info.pid);
    Ok((main_socket, stderr_socket))
}

fn spawn_forwarder(
    reader: impl Read + Send + 'static,
    writer: impl Write + Send + 'static,
    flush_each_chunk: bool,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || forward(reader, writer, flush_each_chunk))
}

// The stdin direction: copy until stdin is exhausted, then half-close the connection so
// the daemon's read of the invocation's stdin returns end-of-file while the other
// direction — the invocation's stdout — stays open.
fn spawn_stdin_forwarder(
    reader: impl Read + Send + 'static,
    socket: UnixStream,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        forward(reader, &socket, false);
        debug!("main: stdin exhausted; half-closing");
        let _ = socket.shutdown(Shutdown::Write);
    })
}

// Apply terminal-mode commands from the daemon as they arrive. The thread ends when the
// daemon closes the connection at client exit; the main path's `restore_tty_state` remains
// the backstop for whatever mode we were left in.
fn spawn_control(mut reader: UnixStream, saved: tty::TtyState) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        loop {
            match bintel::read_document(&mut reader) {
                Err(_) => break,
                Ok(document) => match bintel::parse_reply(&document) {
                    Some(bintel::Reply::Mode { canonical: true })  => tty::set_cooked_mode(&saved),
                    Some(bintel::Reply::Mode { canonical: false }) => tty::set_raw_mode(),
                    other => debug!("main: unrecognised control document {:?}", other),
                },
            }
        }
    })
}

fn forward(mut reader: impl Read, mut writer: impl Write, flush_each_chunk: bool) {
    let mut buffer = [0u8; FORWARD_BUFFER_SIZE];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                if writer.write_all(&buffer[..count]).is_err() { break; }
                if flush_each_chunk { let _ = writer.flush(); }
            }
        }
    }
    let _ = writer.flush();
}

fn run_non_interactive(socket_file: &Path, script: &Path, args: &[OsString]) -> i32 {
    debug_log(format!(
        "run_non_interactive socket={} args={:?}", socket_file.display(), args,
    ));

    let info = ClientInfo::collect(script, args, false, None);
    let (main_socket, stderr_socket) = match connect_to_daemon(socket_file, &info) {
        Ok(connections) => { debug_log("connected"); connections }
        Err(error) => {
            debug_log(format!("connect failed: {}", error));
            return STARTUP_FAILURE_EXIT_CODE;
        }
    };

    // Nothing is forwarded from stdin, so the invocation sees it at end-of-file at once.
    let _ = main_socket.shutdown(Shutdown::Write);
    let stdout_thread = spawn_forwarder(main_socket, std::io::stdout(), true);
    let stderr_thread = spawn_forwarder(stderr_socket, std::io::stderr(), true);

    let _ = stdout_thread.join();
    debug_log("stdout joined");
    let _ = stderr_thread.join();
    debug_log("stderr joined; calling terminate");
    let exit_code = protocol::terminate(socket_file, info.pid);
    debug_log(format!("terminate returned {}", exit_code));
    exit_code
}

impl ClientInfo {
    // Arguments, environment, working directory and script path cross the wire as UTF-8
    // text (the protocol's scalars must be valid UTF-8), so a value that is not — a byte
    // sequence that is not UTF-8 on Unix, an unpaired surrogate on Windows — is carried with
    // U+FFFD in place of what could not be represented, exactly as the JVM itself would
    // decode it. That is a documented loss, not a crash: see `spec/launcher.md`.
    pub fn collect(
        script: &Path,
        args: &[OsString],
        stdin_tty: bool,
        bg_color: Option<&str>,
    ) -> Self {
        let size = tty::terminal_size();
        let mut env: Vec<String> = env::vars_os()
            .filter(|(name, _)| {
                // Strip any inherited COLUMNS/LINES/TERMINAL_BG; if we detected real
                // values from the tty, ours are authoritative for this client, and if
                // we didn't, inherited values are unreliable across a shared-daemon
                // pipeline.
                let n = name.to_string_lossy();
                let strip_size = size.is_some() && (n == "COLUMNS" || n == "LINES");
                let strip_bg = bg_color.is_some() && n == "TERMINAL_BG";
                !(strip_size || strip_bg)
            })
            .map(|(name, value)| {
                let mut entry = OsString::new();
                entry.push(&name);
                entry.push("=");
                entry.push(&value);
                entry.to_string_lossy().into_owned()
            })
            .collect();
        if let Some((cols, rows)) = size {
            env.push(format!("COLUMNS={}", cols));
            env.push(format!("LINES={}", rows));
        }
        if let Some(bg) = bg_color {
            env.push(format!("TERMINAL_BG={}", bg));
        }
        ClientInfo {
            pid: std::process::id(),
            user_id: user_info::uid(),
            user_name: user_info::username(),
            script: script.to_string_lossy().into_owned(),
            pwd: env::current_dir()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            args: args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect(),
            env,
            // Whether stdin is a terminal is passed in rather than probed here: the
            // non-interactive path deliberately reports `false` even from a terminal, so
            // that `cooked` blocks do not wait on a control channel it never opens. Nothing
            // makes the output streams worth lying about, so those are asked directly.
            stdin_tty,
            stdout_tty: tty::stdout_is_tty(),
            stderr_tty: tty::stderr_is_tty(),
        }
    }
}

mod user_info {
    #[cfg(unix)]
    pub fn uid() -> u32 { unsafe { libc::getuid() as u32 } }

    #[cfg(unix)]
    pub fn username() -> String {
        std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).unwrap_or_default()
    }

    #[cfg(windows)]
    pub fn uid() -> u32 { 0 }

    #[cfg(windows)]
    pub fn username() -> String { std::env::var("USERNAME").unwrap_or_default() }
}

pub fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_millis()).unwrap_or(0)
}

fn debug_log(message: impl AsRef<str>) {
    if std::env::var_os("ETHEREAL_DEBUG").is_none() { return; }
    let log_path = std::env::temp_dir().join("ethereal-launcher.log");
    if let Ok(mut log_file) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
        let _ = writeln!(log_file, "[{}] {}", std::process::id(), message.as_ref());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(values: &[&str]) -> Vec<OsString> { values.iter().map(OsString::from).collect() }

    #[test]
    fn download_is_recognised_only_as_the_sole_argument() {
        // The variable is process-wide state; every case here leaves it unset.
        unsafe { env::remove_var(DOWNLOAD_VARIABLE); }
        assert_eq!(intercept(&os(&["--download"])), (Vec::new(), true));
        assert_eq!(intercept(&os(&[])), (Vec::new(), false));
        assert_eq!(intercept(&os(&["install", "--download"])), (os(&["install", "--download"]), false));
        assert_eq!(intercept(&os(&["--download", "install"])), (os(&["--download", "install"]), false));
        assert_eq!(intercept(&os(&["--", "--download"])), (os(&["--", "--download"]), false));
    }

    #[test]
    fn internal_sentinels_are_only_the_first_argument() {
        let internal = |args: &[&str]| {
            os(args).first().is_some_and(|arg| INTERNAL_SENTINELS.iter().any(|s| arg == s))
        };
        assert!(internal(&["{completions}", "zsh"]));
        assert!(internal(&["{admin}"]));
        assert!(!internal(&["run", "{admin}"]));
        assert!(!internal(&[]));
    }

    #[test]
    fn a_bare_name_is_not_a_path_but_anything_with_a_separator_is() {
        assert!(!argv0_is_path("flame"));
        assert!(argv0_is_path("./flame"));
        assert!(argv0_is_path("bin/flame"));
        assert!(argv0_is_path("/usr/local/bin/flame"));
    }

    // The regression: argv[0] is a bare `flame` after a $PATH lookup, and the working
    // directory holds something of that name. `Cargo.toml` stands in for it — cargo runs
    // tests with the crate root as the working directory, so it is a real neighbour, and no
    // test needs to write a file or move the (process-wide) working directory to prove it.
    #[test]
    fn a_bare_name_never_resolves_against_the_working_directory() {
        let exe = PathBuf::from("/opt/xeq/bin/flame");
        let neighbour = std::env::current_dir().unwrap().join("Cargo.toml");
        assert!(neighbour.is_file(), "expected the crate root as the test working directory");

        let resolved = resolve_script("Cargo.toml", Some(exe.clone()));
        assert_eq!(resolved, exe);
        assert_ne!(resolved, neighbour);

        // …and with no `current_exe` to fall back on, it is still not the neighbour.
        assert_ne!(resolve_script("Cargo.toml", None), neighbour);
    }

    #[test]
    fn the_running_executable_wins_over_a_path_shaped_argv0() {
        let exe = PathBuf::from("/opt/xeq/bin/flame");
        assert_eq!(resolve_script("./Cargo.toml", Some(exe.clone())), exe);
    }

    #[test]
    fn without_current_exe_a_path_shaped_argv0_is_canonicalized() {
        let expected = std::fs::canonicalize("Cargo.toml").unwrap();
        assert_eq!(resolve_script("./Cargo.toml", None), expected);
    }

    #[cfg(unix)]
    #[test]
    fn without_current_exe_a_bare_name_is_looked_up_on_the_path() {
        let resolved = resolve_script("sh", None);
        assert!(resolved.is_absolute(), "expected a $PATH hit, got {}", resolved.display());
        assert_eq!(resolved.file_name().unwrap(), "sh");
    }

    #[test]
    fn an_unresolvable_argv0_is_returned_unchanged() {
        let missing = "xeq-no-such-command-9f3a1c";
        assert_eq!(resolve_script(missing, None), PathBuf::from(missing));
    }

    #[cfg(unix)]
    #[test]
    fn arguments_that_are_not_utf8_are_carried_lossily_rather_than_panicking() {
        use std::os::unix::ffi::OsStringExt;
        let args = vec![OsString::from_vec(vec![b'a', 0xff, b'b'])];
        let info = ClientInfo::collect(Path::new("/x"), &args, false, None);
        assert_eq!(info.args, vec!["a\u{fffd}b".to_string()]);
    }
}
