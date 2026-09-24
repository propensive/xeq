use std::io::Write;
use std::path::Path;
use std::time::Duration;

use crate::bintel::{self, variant, Record, Reply};
use crate::uds::UnixStream;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignalAck {
    Accept,
    Reject,
    Timeout,
}

// What the launcher tells the daemon about an invocation, in the `init` document. Everything
// is text on the wire; see `spec/launcher.md` for what each value means and how it is found.
pub struct ClientInfo {
    pub pid:        u32,
    // The platform's identifier for the user: numeric on Unix, a SID on Windows.
    pub user_id:    String,
    pub user_name:  String,
    // The canonical path of the executable.
    pub script:     String,
    // argv[0] as the caller supplied it, for a multi-call binary to dispatch on.
    pub invoked_as: Option<String>,
    pub pwd:        String,
    pub args:       Vec<String>,
    pub env:        Vec<String>,
    pub stdin_tty:  bool,
    pub stdout_tty: bool,
    pub stderr_tty: bool,
    // The client's umask, in octal, where the platform has one.
    pub umask:      Option<String>,
    // The terminal's size, when stdout is a terminal: (columns, rows).
    pub size:       Option<(u16, u16)>,
    // The console's input and output code pages, on Windows.
    pub codepages:  Option<(u32, u32)>,
}

// Every connection to the daemon opens with one BinTEL document of the `ethereal-launcher`
// schema (see `bintel.rs`); what follows depends on the message. After `init` the connection
// is the invocation's stdin and stdout; after `stderr` it delivers stderr; after `control` the
// daemon sends `mode` documents on it; `signal`, `verify` and `exit` are answered with one
// document each and closed; `closed` is not answered.

pub fn init_document(info: &ClientInfo) -> Vec<u8> {
    let mut record = Record::new();
    record.scalar(0, &info.pid.to_string());
    record.scalar(1, &info.user_id);
    record.scalar(2, &info.user_name);
    record.scalar(3, &info.script);
    record.scalar(4, &info.pwd);
    if info.stdin_tty { record.flag(5); }
    if info.stdout_tty { record.flag(6); }
    if info.stderr_tty { record.flag(7); }
    for argument in &info.args { record.scalar(8, argument); }
    for variable in &info.env { record.scalar(9, variable); }
    if let Some(invoked_as) = &info.invoked_as { record.scalar(10, invoked_as); }
    if let Some(umask) = &info.umask { record.scalar(11, umask); }
    if let Some((columns, rows)) = info.size {
        record.scalar(12, &columns.to_string());
        record.scalar(13, &rows.to_string());
    }
    if let Some((input, output)) = info.codepages {
        record.scalar(14, &input.to_string());
        record.scalar(15, &output.to_string());
    }
    bintel::document(variant::INIT, record)
}

pub fn send_init(connection: &mut UnixStream, info: &ClientInfo) {
    let _ = connection.write_all(&init_document(info));
    let _ = connection.flush();
}

fn pid_record(pid: u32) -> Record {
    let mut record = Record::new();
    record.scalar(0, &pid.to_string());
    record
}

pub fn send_stderr_request(connection: &mut UnixStream, pid: u32) {
    let _ = connection.write_all(&bintel::document(variant::STDERR, pid_record(pid)));
    let _ = connection.flush();
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    Fresh,
    Stale,
}

// Ask the resident daemon whether the launcher it was started from still has the
// content it remembers — sent when the file's mtime disagrees with the build file's
// record, i.e. after a `touch` or a same-size rebuild. The daemon hashes at most once
// per change and remembers the answer, which a stateless launcher cannot. The verdict
// document says fresh or stale (the daemon then shuts down; await its death and launch
// afresh); anything else — including a daemon too old to speak this protocol, which just
// closes the connection — means proceed as normal.
pub fn verify(socket_path: &Path) -> Verdict {
    let mut connection = match UnixStream::connect(socket_path) {
        Ok(connection) => connection,
        Err(_) => return Verdict::Fresh,
    };
    let _ = connection.set_read_timeout(Some(REPLY_TIMEOUT));
    let _ = connection.write_all(&bintel::document(variant::VERIFY, Record::new()));
    let _ = connection.flush();
    match bintel::read_document(&mut connection).ok().and_then(|doc| bintel::parse_reply(&doc)) {
        Some(Reply::Verdict { fresh: false }) => Verdict::Stale,
        _ => Verdict::Fresh,
    }
}

// The control channel: a side-connection on which the daemon sends `mode` documents asking
// for the client's terminal to be put into canonical (cooked) mode or back into raw mode. The
// launcher is the only process that can change the client's tty mode, and it has already
// raw-moded the terminal by the time the daemon knows which command is running, so the
// request has to be pushed back here.
pub fn send_control_request(connection: &mut UnixStream, pid: u32) {
    let _ = connection.write_all(&bintel::document(variant::CONTROL, pid_record(pid)));
    let _ = connection.flush();
}

// What travels with a signal's name: the terminal's size, for a signal that says it may have
// changed, or the time the system allows before it ends the client, for a Windows control
// event that comes with one.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SignalDetail {
    pub size: Option<(u16, u16)>,
    pub deadline_ms: Option<u64>,
}

pub fn signal_document(pid: u32, name: &str, detail: SignalDetail) -> Vec<u8> {
    let mut record = pid_record(pid);
    record.scalar(1, name);
    if let Some((columns, rows)) = detail.size {
        record.scalar(2, &columns.to_string());
        record.scalar(3, &rows.to_string());
    }
    if let Some(deadline) = detail.deadline_ms { record.scalar(4, &deadline.to_string()); }
    bintel::document(variant::SIGNAL, record)
}

pub fn send_signal(
    socket_path: &Path,
    pid: u32,
    name: &str,
    detail: SignalDetail,
    timeout_ms: u64,
) -> SignalAck {
    let mut connection = match UnixStream::connect(socket_path) {
        Ok(connection) => connection,
        Err(_) => return SignalAck::Timeout,
    };
    if connection.write_all(&signal_document(pid, name, detail)).is_err() {
        return SignalAck::Timeout;
    }
    if connection.flush().is_err() { return SignalAck::Timeout; }
    let _ = connection.set_read_timeout(Some(Duration::from_millis(timeout_ms)));
    match bintel::read_document(&mut connection).ok().and_then(|doc| bintel::parse_reply(&doc)) {
        Some(Reply::SignalAck { accept: true })  => SignalAck::Accept,
        Some(Reply::SignalAck { accept: false }) => SignalAck::Reject,
        _                                        => SignalAck::Timeout,
    }
}

pub fn closed_document(pid: u32, stream: &str) -> Vec<u8> {
    let mut record = pid_record(pid);
    record.scalar(1, stream);
    bintel::document(variant::CLOSED, record)
}

// Tells the daemon that the invocation's `stream` (`stdout` or `stderr`) has lost its
// reader, so that it can fail the invocation's further writes as a broken pipe would. Not
// answered: the launcher has nothing to wait for, and goes on draining the stream so the
// daemon is never blocked writing it.
pub fn send_closed(socket_path: &Path, pid: u32, stream: &str) {
    if let Ok(mut connection) = UnixStream::connect(socket_path) {
        let _ = connection.write_all(&closed_document(pid, stream));
        let _ = connection.flush();
    }
}

// How long a daemon that has accepted a connection is given to answer a question — a
// verdict or an exit status — before the launcher treats it as wedged. An invocation's own
// running time is unbounded by design; only the daemon's replies are bounded.
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

pub fn terminate(socket_path: &Path, pid: u32) -> i32 {
    let mut connection = match UnixStream::connect(socket_path) {
        Ok(connection) => connection,
        Err(_) => return 2,
    };
    let _ = connection.write_all(&bintel::document(variant::EXIT, pid_record(pid)));
    let _ = connection.flush();
    let _ = connection.set_read_timeout(Some(REPLY_TIMEOUT));
    match bintel::read_document(&mut connection) {
        Ok(document) => match bintel::parse_reply(&document) {
            Some(Reply::ExitStatus { code }) => code,
            _ => 1,
        },
        Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
            eprintln!("\nThe daemon did not report the exit status within {}s.", REPLY_TIMEOUT.as_secs());
            2
        }
        Err(_) => 1,
    }
}
