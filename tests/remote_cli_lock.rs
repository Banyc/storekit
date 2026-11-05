//! The far-side CLI lock HOLDER (`remote-cli-lock`): `storekit-remote lock
//! <record> <op_id>` is the native form of the transport's
//! `PERL_HOLD_LOCK_TEMPLATE` — the long-lived `flock` holder an
//! [`SshTransport`]'s [`Remote::lock_far_side`] session runs on the remote.
//!
//! TWO KINDS OF TEST, both in this file:
//!
//! * DIFFERENTIAL tests run the EXACT logged holder script and the CLI on the
//!   same freshly set-up fixture and compare the readiness line's SHAPE, the
//!   record's bytes and mode WHILE the lock is held, the stderr bytes, the exit
//!   status, and the final tree. The script is never re-typed here: it is the
//!   command the transport itself logged while the CLI was absent.
//! * TRANSPORT-LEVEL tests build an [`SshTransport`] over an `ssh` SHIM on
//!   `PATH` (the harness `tests/remote_cli_probe.rs` established) that logs
//!   each remote command and runs it locally. They assert the ACTUAL logged
//!   command: the CLI form with the binary present, the script form with it
//!   absent, exactly ONE probe per transport, and that a CLI holder keeps a
//!   second run out exactly as the perl holder does.
//!
//! WHAT THE SHIM DOES NOT PROVE. No network, no `sshd`, no OpenSSH option
//! parsing, no remote filesystem semantics: the far side is the SAME host. A
//! defect that depends on those is out of scope here.
//!
//! ONE ARM THAT CANNOT BE COMPARED DIFFERENTIALLY, stated rather than hidden:
//! the holder's `make_path` failure arm (`LOCKERR mkdir <dir>: <strerror>`,
//! exit 4). `File::Path::make_path` in the script CROAKS on a failed mkdir
//! before its `or do { … }` arm can run, so the script's observable bytes there
//! are perl's own die (a different message and status) and the CLI's `LOCKERR
//! mkdir` arm is unreachable in the script. This file therefore compares the
//! arm the script CAN reach on that path — an unwritable PARENT, which fails
//! the open with `LOCKERR open`, exit 4 — and the CLI implements the documented
//! `LOCKERR mkdir` arm for the mkdir failure itself.
#![cfg(unix)]
// Test-only fixtures drive the same name-mutating primitives the funnel guards.
#![allow(clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use storekit::env::SysEnv;
use storekit::lock::RECORD_HEADER;
use storekit::transport::{Layout, Remote, SshTransport};

/// The environment variable the shim reads to find its "remote working
/// directory" (the directory a remote login shell would start in).
const SHIM_WORK_VAR: &str = "STOREKIT_SSH_SHIM_WORK";
/// The environment variable the shim appends each remote command to.
const SHIM_LOG_VAR: &str = "STOREKIT_SSH_SHIM_LOG";

const SHIM_SCRIPT: &str = r#"#!/bin/sh
# Test-only `ssh` shim: it never opens a network connection. It records the
# remote command string (the FINAL argument, `bash -c '<script>'`) and then
# runs it in $STOREKIT_SSH_SHIM_WORK with stdin/stdout/stderr connected exactly
# as the operation connects them. The log is NUL-TERMINATED: a remote command
# can span several lines (the sidecar perl helper does), so a newline is not an
# invocation boundary but NUL cannot occur in a command.
set -u
work=${STOREKIT_SSH_SHIM_WORK:?the shim work directory is not configured}
log=${STOREKIT_SSH_SHIM_LOG:?the shim log is not configured}
last=''
for arg in "$@"; do last=$arg; done
printf '%s\0' "$last" >> "$log"
cd "$work" || exit 125
exec /bin/sh -c "$last"
"#;

/// Install `body` as an executable at `path` from a SHORT-LIVED HELPER PROCESS,
/// never from the test process itself: libtest runs this binary's tests on many
/// threads of ONE process, and a write fd held by the test process can be
/// inherited by a sibling test's fork, making a later `execve` of the file fail
/// with `ETXTBSY`.
fn write_executable(path: &Path, body: &[u8]) {
    use std::io::Write;
    std::fs::create_dir_all(path.parent().expect("executable path has a parent"))
        .expect("create the executable's directory");
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg("cat > \"$1.tmp.$$\" && chmod 755 \"$1.tmp.$$\" && mv -f \"$1.tmp.$$\" \"$1\"")
        .arg("sh")
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn the executable-writing helper");
    child
        .stdin
        .take()
        .expect("the helper's piped stdin")
        .write_all(body)
        .expect("write the executable body to the helper");
    let status = child
        .wait()
        .expect("wait for the executable-writing helper");
    assert!(status.success(), "installing {path:?} failed: {status:?}");
}

/// One hermetic fixture: a short `TMPDIR` (the mux socket path is length
/// bounded), a shim bin dir, a logged "remote" working directory, and the
/// destination root inside it.
struct Harness {
    tmp: tempfile::TempDir,
    work: PathBuf,
    root: PathBuf,
    bin: PathBuf,
    log_path: PathBuf,
}

impl Harness {
    fn new() -> Harness {
        // SHORT: the transport derives `<TMPDIR>/dmux/mux-<hash>` and fails
        // closed when `sockaddr_un.sun_path` leaves too little room.
        let tmp = tempfile::Builder::new()
            .prefix("storekit-remote-lock-")
            .tempdir_in("/tmp")
            .expect("create harness tempdir");
        let work = tmp.path().join("work");
        std::fs::create_dir_all(&work).expect("create the shim work dir");
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&bin).expect("create the shim bin dir");
        write_executable(&bin.join("ssh"), SHIM_SCRIPT.as_bytes());
        let root = work.join("remote");
        Harness {
            log_path: tmp.path().join("ssh.log"),
            tmp,
            work,
            root,
            bin,
        }
    }

    /// Put the REAL `storekit-remote` on the shim's `PATH` (a symlink, so no
    /// bytes are written from the test process and the binary is never stale).
    fn expose_real_cli(&self) {
        std::os::unix::fs::symlink(
            env!("CARGO_BIN_EXE_storekit-remote"),
            self.bin.join("storekit-remote"),
        )
        .expect("link the real storekit-remote onto the shim PATH");
    }

    /// A transport whose every child sees exactly this environment snapshot:
    /// the shim first on `PATH`, the short `TMPDIR`, the shim's work dir, and
    /// its log. With the CLI absent the probe's 127 selects the scripts; a test
    /// that called [`Harness::expose_real_cli`] first selects the CLI.
    fn transport(&self) -> SshTransport {
        let mut vars: BTreeMap<OsString, OsString> = BTreeMap::new();
        vars.insert(
            OsString::from("PATH"),
            OsString::from(format!(
                "{}:{}",
                self.bin.display(),
                std::env::var("PATH").unwrap_or_default()
            )),
        );
        vars.insert(
            OsString::from("TMPDIR"),
            self.tmp.path().as_os_str().to_os_string(),
        );
        vars.insert(
            OsString::from(SHIM_WORK_VAR),
            self.work.as_os_str().to_os_string(),
        );
        vars.insert(
            OsString::from(SHIM_LOG_VAR),
            self.log_path.as_os_str().to_os_string(),
        );
        let env = SysEnv::from_map(vars);
        SshTransport::new(
            "deploy",
            "shim.invalid",
            2222,
            &self.root,
            Layout::empty(),
            Some(Path::new("/dev/null")),
            None,
            &self.tmp.path().join("knownhosts"),
            &env,
            false,
        )
        .expect("construct the shimmed ssh transport")
    }

    /// Every remote command the shim was handed, in order (the log is
    /// NUL-terminated, so a multi-line command stays ONE entry).
    fn log(&self) -> Vec<String> {
        let bytes = std::fs::read(&self.log_path).unwrap_or_default();
        bytes
            .split(|&b| b == 0)
            .filter(|chunk| !chunk.is_empty())
            .map(|chunk| {
                String::from_utf8(chunk.to_vec()).expect("fixture commands are ASCII paths")
            })
            .collect()
    }

    /// The filesystem path a logged command addresses: `root.join(rel)`.
    fn remote_path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// Reset the remote root to an EMPTY directory, so the next run of the
    /// same case starts from the identical fixture state.
    fn reset_root(&self) {
        let _ = std::fs::remove_dir_all(&self.root);
        std::fs::create_dir_all(&self.root).expect("recreate the remote root");
    }
}

/// The transport's ONE quoting rule (`shell_quote`): a single-quoted word with
/// an embedded `'` written as `'\''`. Re-derived here so the expected logged
/// command is checked against an INDEPENDENT encoding, not the actual one.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The exact remote command string the transport builds for a CLI `argv`:
/// every element quoted and joined, then the whole command quoted again by
/// `ssh_command_argv`'s `bash -c` wrapper.
fn expected_cli_command(argv: &[&str]) -> String {
    let words: Vec<String> = argv.iter().map(|a| shell_quote(a)).collect();
    format!("bash -c {}", shell_quote(&words.join(" ")))
}

/// The `storekit-remote` binary under test.
fn cli(argv: &[&OsStr]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_storekit-remote"));
    cmd.args(argv);
    cmd
}

/// A command that runs `command` exactly as the ssh argv does (`/bin/sh -c`).
fn sh(command: &str) -> Command {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg(command);
    cmd
}

/// The exact prefix of a CLI `argv` command: `argv_cmd` single-quotes every
/// word, and `ssh_command_argv` quotes the whole command again for `bash -c`, so
/// the CLI form begins `bash -c ''\''storekit-remote'…` while a script form
/// begins `bash -c 'perl'`. Derived from the quoting rule and cut before the
/// SUBCOMMAND word rather than hand-escaped.
fn cli_invocation_prefix() -> String {
    let sample = expected_cli_command(&["storekit-remote", "protocol"]);
    let cut = sample
        .find("protocol")
        .expect("the sample carries its subcommand word");
    sample[..cut].to_string()
}

/// Whether `command` invokes the far-side CLI. A plain substring search for the
/// program name would also match the harness's OWN temporary paths, so the check
/// is on the INVOKED word.
fn is_cli_form(command: &str) -> bool {
    command.starts_with(&cli_invocation_prefix())
}

/// Assert the logged command is the SCRIPT form (the CLI was absent).
fn assert_script_form(case: &str, command: &str) {
    assert!(
        !is_cli_form(command),
        "{case}: the absent CLI must log the script form, got {command:?}"
    );
    assert!(
        command.starts_with("bash -c '"),
        "{case}: every remote command is the `bash -c` form, got {command:?}"
    );
}

/// Wait for `child` under a DEADLINE, then collect its output; a blocking run
/// is a failed test rather than a hang.
fn wait_child(mut child: std::process::Child, secs: u64) -> Output {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        match child.try_wait().expect("poll the command under test") {
            Some(_) => {
                return child
                    .wait_with_output()
                    .expect("collect the command's output");
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let out = child.wait_with_output().expect("collect the killed output");
                panic!(
                    "the command did not finish within {secs}s (it blocked): stdout {:?} stderr {:?}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

/// One RUNNING holder: its process (with the piped stdin that IS the release
/// signal) and the first line it printed, or the empty string when it exited
/// without printing one.
struct Holder {
    child: std::process::Child,
    ready: String,
}

impl Holder {
    /// Release: close stdin (the protocol's ONLY release signal), then wait
    /// (bounded) for the holder to exit.
    fn release(mut self) -> Output {
        drop(self.child.stdin.take());
        wait_child(self.child, 30)
    }
}

/// Start a holder with a piped stdin and read its FIRST stdout line under a
/// deadline. The holder keeps stdout open for its whole session, so the read
/// must not wait for EOF; the reader thread therefore reads exactly one line
/// and returns. A holder that refuses exits without one, so the thread sees EOF
/// and reports the empty line.
fn start_holder(mut cmd: Command) -> Holder {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the holder under test");
    let stdout = child.stdout.take().expect("the holder's piped stdout");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        let result = reader.read_line(&mut line);
        let _ = tx.send((result, line));
    });
    let ready = match rx.recv_timeout(Duration::from_secs(30)) {
        Ok((Ok(_), line)) => line,
        Ok((Err(error), line)) => {
            panic!("reading the holder's readiness line failed: {error} (got {line:?})")
        }
        Err(_) => panic!("the holder printed no readiness line within 30s"),
    };
    Holder { child, ready }
}

/// The pid named by an exact `LOCKOK <pid>\n` readiness line. A malformed line,
/// a zero pid, or the TEST's own pid (the holder must print ITS OWN) panics.
fn ready_pid(line: &str) -> u32 {
    let rest = line
        .strip_prefix("LOCKOK ")
        .unwrap_or_else(|| panic!("the readiness line must begin `LOCKOK `, got {line:?}"));
    let digits = rest
        .strip_suffix('\n')
        .unwrap_or_else(|| panic!("the readiness line must end with one LF, got {line:?}"));
    let pid: u32 = digits.parse().unwrap_or_else(|_| {
        panic!("the readiness line must carry a decimal pid, got {line:?}");
    });
    assert_ne!(
        pid, 0,
        "the holder must print its own nonzero pid: {line:?}"
    );
    assert_ne!(
        pid,
        std::process::id(),
        "the holder must print ITS OWN pid, not the test process's: {line:?}"
    );
    pid
}

/// The C library's text for `errno` — the string perl's `$!` renders and the
/// one both forms must print.
fn strerror(errno: i32) -> String {
    // SAFETY: `strerror` returns a process-global NUL-terminated string for any
    // code; it is copied immediately.
    let ptr = unsafe { libc::strerror(errno) };
    assert!(!ptr.is_null(), "strerror({errno}) must not be NULL");
    unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

/// `LOCKERR <what> <path>: <strerror>\n` — the holder's `print STDERR`, which
/// (unlike every `die`-based script) carries NO perl location suffix.
fn lockerr_message(what: &str, path: &Path, errno: i32) -> Vec<u8> {
    format!("LOCKERR {what} {}: {}\n", path.display(), strerror(errno)).into_bytes()
}

/// The contention diagnostic: same shape, no `what` word.
fn contended_message(path: &Path) -> Vec<u8> {
    format!(
        "LOCKCONTENDED {}: {}\n",
        path.display(),
        strerror(libc::EWOULDBLOCK)
    )
    .into_bytes()
}

/// The "not a record this crate wrote" refusal.
fn not_recognized_message(path: &Path) -> Vec<u8> {
    format!(
        "LOCKRECORDNOTRECOGNIZED {}: the entry is not a lock record this crate wrote; refusing to \
         truncate it\n",
        path.display()
    )
    .into_bytes()
}

/// `path`'s permission bits (not the file type).
fn mode_of(path: &Path) -> u32 {
    std::fs::symlink_metadata(path)
        .expect("lstat the fixture entry")
        .permissions()
        .mode()
        & 0o777
}

fn set_mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
}

/// The fixture tree's observable state: relative path -> kind, raw mode and
/// (for a regular file) the exact bytes.
#[derive(Debug, PartialEq, Eq)]
enum Node {
    File { mode: u32, bytes: Vec<u8> },
    Dir { mode: u32 },
    Symlink { target: PathBuf },
    Other { mode: u32 },
}

fn snapshot(dir: &Path) -> BTreeMap<String, Node> {
    let mut out = BTreeMap::new();
    collect_snapshot(dir, "", &mut out);
    out
}

fn collect_snapshot(dir: &Path, prefix: &str, out: &mut BTreeMap<String, Node>) {
    for entry in std::fs::read_dir(dir).expect("read the fixture directory") {
        let entry = entry.expect("read a fixture entry");
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let meta = std::fs::symlink_metadata(entry.path()).expect("lstat a fixture entry");
        let mode = meta.mode() & 0o7777;
        let node = if meta.file_type().is_symlink() {
            Node::Symlink {
                target: std::fs::read_link(entry.path()).expect("read the symlink target"),
            }
        } else if meta.file_type().is_dir() {
            collect_snapshot(&entry.path(), &rel, out);
            Node::Dir { mode }
        } else if meta.file_type().is_file() {
            Node::File {
                mode,
                bytes: std::fs::read(entry.path()).expect("read the fixture file"),
            }
        } else {
            Node::Other { mode }
        };
        out.insert(rel, node);
    }
}

/// One holder run's observable outcome: what it printed before the release, the
/// fixture state WHILE it held (or refused) the record, and what it printed and
/// exited with after stdin closed.
struct HolderRun {
    ready: String,
    held: BTreeMap<String, Node>,
    output: Output,
}

/// Run one holder: start it, read the readiness line, snapshot the fixture while
/// it is still running (the record is written and flocked at that point, or the
/// refusal has already happened), then release it by closing stdin.
fn run_holder(cmd: Command, root: &Path) -> HolderRun {
    let holder = start_holder(cmd);
    let ready = holder.ready.clone();
    if !ready.is_empty() {
        // The line names a LIVE process: the holder is still blocked on stdin.
        let pid = ready_pid(&ready);
        // SAFETY: `kill(pid, 0)` only probes for the process's existence.
        assert_eq!(
            unsafe { libc::kill(pid as i32, 0) },
            0,
            "the pid the readiness line names must be the live holder process ({pid}), line {ready:?}"
        );
    }
    let held = snapshot(root);
    let output = holder.release();
    HolderRun {
        ready,
        held,
        output,
    }
}

/// One differential case: `setup` places the fixture, the script and the CLI
/// each run against the SAME freshly reset state, and the readiness line shape,
/// the state while held, stderr, exit status and the final tree are compared.
fn holder_parity(
    h: &Harness,
    case: &str,
    logged: &str,
    setup: impl Fn(),
    cli_args: &[&OsStr],
) -> (HolderRun, HolderRun) {
    setup();
    let script = run_holder(sh(logged), &h.root);
    let script_final = snapshot(&h.root);
    setup();
    let cli_run = run_holder(cli(cli_args), &h.root);
    let cli_final = snapshot(&h.root);

    assert_eq!(
        cli_run.output.status.code(),
        script.output.status.code(),
        "{case}: exit status must match (script stderr {:?}, CLI stderr {:?})",
        String::from_utf8_lossy(&script.output.stderr),
        String::from_utf8_lossy(&cli_run.output.stderr)
    );
    assert_eq!(
        cli_run.output.stderr, script.output.stderr,
        "{case}: stderr bytes must be identical"
    );
    assert_eq!(
        cli_run.ready.is_empty(),
        script.ready.is_empty(),
        "{case}: both forms must print (or omit) the readiness line together: script {:?}, CLI {:?}",
        script.ready,
        cli_run.ready
    );
    assert_eq!(
        cli_run.held, script.held,
        "{case}: the state WHILE HELD (record bytes/mode, created parents, symlinks) must match"
    );
    assert_eq!(
        cli_final, script_final,
        "{case}: the final tree (kinds, modes, targets and bytes) must match the script's"
    );
    (script, cli_run)
}

// -- logging the exact holder script (CLI absent) -----------------------------

/// The EXACT holder script the transport builds for `record`/`op_id`: the
/// transport's own command, logged while the CLI is absent, then released.
fn log_holder_script(h: &Harness, record: &Path, op_id: &str) -> String {
    let t = h.transport();
    let mut session = Remote::lock_far_side(&t, record, op_id)
        .expect("the script holder must acquire a fresh record");
    let log = h.log();
    session.release();
    let command = log
        .last()
        .expect("the shim logged the holder command")
        .clone();
    assert_script_form("lock", &command);
    assert!(
        command.contains("perl") && command.contains("LOCKOK"),
        "the holder script is the perl flock program: {command:?}"
    );
    command
}

// -- differential: the CLI holder against the logged script -------------------

/// A FREE record with a MISSING parent chain: both forms create the chain, take
/// the flock, chmod `0600`, write the header followed by the op id, print
/// `LOCKOK <pid>`, and release on stdin close (exit 0).
#[test]
fn lock_cli_acquires_a_free_record_like_the_script() {
    let h = Harness::new();
    h.reset_root();
    let record_rel = "state/deep/thing.operation.lock";
    let record = h.remote_path(record_rel);
    let logged = log_holder_script(&h, &record, "op-free");
    let cli_args = [
        OsStr::new("lock"),
        record.as_os_str(),
        OsStr::new("op-free"),
    ];
    let setup = || {
        h.reset_root();
    };
    let (script, cli_run) = holder_parity(&h, "free record", &logged, setup, &cli_args);

    // The readiness line, in BOTH forms, is exactly `LOCKOK <pid>` plus one LF.
    let script_pid = ready_pid(&script.ready);
    let cli_pid = ready_pid(&cli_run.ready);
    assert!(script_pid > 0 && cli_pid > 0, "both forms print a live pid");

    // The record, while held: the header followed by the op id, mode 0600.
    let mut expected = RECORD_HEADER.as_bytes().to_vec();
    expected.extend_from_slice(b"op-free");
    assert_eq!(
        std::fs::read(&record).expect("the CLI-created record without parents must exist"),
        expected,
        "the CLI must write header+op_id into the record"
    );
    assert_eq!(mode_of(&record), 0o600, "the record must be private");
    // The parent chain the script's `make_path` creates.
    for dir in ["state", "state/deep"] {
        assert!(
            h.remote_path(dir).is_dir(),
            "the record's parent chain must be created ({dir})"
        );
    }

    // Release on stdin close: exit 0, silent stderr, no ready line left over.
    assert_eq!(cli_run.output.status.code(), Some(0), "release must exit 0");
    assert!(cli_run.output.stderr.is_empty(), "a free record is silent");

    // The flock is gone after the release: a SECOND holder acquires at once.
    let again = run_holder(cli(&cli_args), &h.root);
    assert_eq!(
        again.output.status.code(),
        Some(0),
        "the record must be re-acquirable once the holder's stdin closed: {:?}",
        String::from_utf8_lossy(&again.output.stderr)
    );
    ready_pid(&again.ready);
}

/// A HELD record: the contender's single non-blocking `flock` reports
/// `EWOULDBLOCK`, so it prints `LOCKCONTENDED <path>: <strerror>` and exits 3
/// WITHOUT waiting and WITHOUT rewriting the record. Asserted in BOTH
/// directions (script holder / CLI contender and CLI holder / script
/// contender), because the CLI holder must exclude a script contender and vice
/// versa.
#[test]
fn lock_cli_refuses_a_held_record_like_the_script() {
    let h = Harness::new();
    h.reset_root();
    let record = h.remote_path("state/held.operation.lock");
    let logged = log_holder_script(&h, &record, "op-holder");

    // Direction 1: the SCRIPT holds; the CLI contends.
    std::fs::create_dir_all(record.parent().unwrap()).unwrap();
    let holder = start_holder(sh(&logged));
    ready_pid(&holder.ready);
    let cli_args = [OsStr::new("lock"), record.as_os_str(), OsStr::new("op-CLI")];
    let contender = run_closed(cli(&cli_args));
    assert_eq!(
        contender.status.code(),
        Some(3),
        "a contended CLI holder must exit 3, stderr {:?}",
        String::from_utf8_lossy(&contender.stderr)
    );
    assert_eq!(
        contender.stderr,
        contended_message(&record),
        "the contention diagnostic is the script's exact bytes"
    );
    let mut expected = RECORD_HEADER.as_bytes().to_vec();
    expected.extend_from_slice(b"op-holder");
    assert_eq!(
        std::fs::read(&record).unwrap(),
        expected,
        "a contended CLI holder must not rewrite the live holder's record"
    );
    let released = holder.release();
    assert_eq!(released.status.code(), Some(0));

    // Direction 2: the CLI holds; the SCRIPT contends.
    h.reset_root();
    std::fs::create_dir_all(record.parent().unwrap()).unwrap();
    let holder = start_holder(cli(&cli_args));
    ready_pid(&holder.ready);
    let contender = run_closed(sh(&logged));
    assert_eq!(
        contender.status.code(),
        Some(3),
        "a contended script holder must exit 3, stderr {:?}",
        String::from_utf8_lossy(&contender.stderr)
    );
    assert_eq!(contender.stderr, contended_message(&record));
    expected.truncate(RECORD_HEADER.len());
    expected.extend_from_slice(b"op-CLI");
    assert_eq!(
        std::fs::read(&record).unwrap(),
        expected,
        "the script contender must not rewrite the live CLI holder's record"
    );
    let released = holder.release();
    assert_eq!(released.status.code(), Some(0));
}

/// Run a NON-HOLDER command with no stdin (it is not supposed to block) under a
/// deadline.
fn run_closed(mut cmd: Command) -> Output {
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the command under test");
    wait_child(child, 30)
}

/// A NON-EMPTY record that is not a record this crate wrote is REFUSED with
/// `LOCKRECORDNOTRECOGNIZED`, exit 6, leaving the entry byte-for-byte and
/// mode-for-mode untouched (the script's arm, which the CLI must reproduce).
#[test]
fn lock_cli_refuses_a_foreign_record_like_the_script() {
    let h = Harness::new();
    h.reset_root();
    let record = h.remote_path("state/foreign.operation.lock");
    let logged = log_holder_script(&h, &record, "op-foreign");
    let cli_args = [
        OsStr::new("lock"),
        record.as_os_str(),
        OsStr::new("op-foreign"),
    ];
    let mut foreign = b"a caller's data (NOT a storekit lock record)".to_vec();
    foreign.resize(51, b'.');
    let setup = || {
        h.reset_root();
        std::fs::create_dir_all(record.parent().unwrap()).unwrap();
        std::fs::write(&record, &foreign).unwrap();
        set_mode(&record, 0o644);
    };
    let (script, cli_run) = holder_parity(&h, "foreign record", &logged, setup, &cli_args);

    assert!(
        script.ready.is_empty(),
        "a refusal prints no readiness line"
    );
    assert!(
        cli_run.ready.is_empty(),
        "the CLI must print no readiness line on a refusal"
    );
    assert_eq!(cli_run.output.status.code(), Some(6), "the refusal exits 6");
    assert_eq!(
        cli_run.output.stderr,
        not_recognized_message(&record),
        "the refusal diagnostic is the script's exact bytes"
    );
    assert_eq!(
        std::fs::read(&record).unwrap(),
        foreign,
        "a refused record must not be truncated or rewritten"
    );
    assert_eq!(
        mode_of(&record),
        0o644,
        "a refused record must not be chmodded (the chmod is AFTER the check)"
    );
}

/// A SYMLINK planted at the record: the `O_NOFOLLOW` open fails `ELOOP`, so both
/// forms print `LOCKERR open <path>: <strerror>` and exit 4, and the link's
/// TARGET is never opened, truncated or chmodded.
#[test]
fn lock_cli_refuses_a_symlink_record_like_the_script() {
    let h = Harness::new();
    h.reset_root();
    let record = h.remote_path("state/link.operation.lock");
    let victim = h.remote_path("state/victim");
    let logged = log_holder_script(&h, &record, "op-link");
    let cli_args = [
        OsStr::new("lock"),
        record.as_os_str(),
        OsStr::new("op-link"),
    ];
    let setup = || {
        h.reset_root();
        std::fs::create_dir_all(record.parent().unwrap()).unwrap();
        std::fs::write(&victim, b"victim bytes").unwrap();
        set_mode(&victim, 0o644);
        std::os::unix::fs::symlink("victim", &record).unwrap();
    };
    let (script, cli_run) = holder_parity(&h, "symlink record", &logged, setup, &cli_args);

    assert!(
        script.ready.is_empty(),
        "a refusal prints no readiness line"
    );
    assert!(cli_run.ready.is_empty());
    assert_eq!(
        cli_run.output.status.code(),
        Some(4),
        "ELOOP is the unusable-record arm (4)"
    );
    assert_eq!(
        cli_run.output.stderr,
        lockerr_message("open", &record, libc::ELOOP),
        "the CLI must report the open's ELOOP, never follow the link"
    );
    assert_eq!(
        std::fs::read(&victim).unwrap(),
        b"victim bytes",
        "the symlink's target must be untouched"
    );
    assert_eq!(
        mode_of(&victim),
        0o644,
        "the target's mode must be untouched"
    );
    assert!(
        std::fs::symlink_metadata(&record)
            .expect("the record is still a symlink")
            .file_type()
            .is_symlink(),
        "the record must still be the symlink that was planted"
    );
}

/// An UNWRITABLE parent directory: the `O_CREAT` open fails `EACCES`, so both
/// forms print `LOCKERR open <path>: <strerror>` and exit 4 (the
/// `RECORD_UNUSABLE` arm). Unreachable for a root caller, which bypasses the
/// mode bits — skipped there with a stated reason.
#[test]
fn lock_cli_refuses_an_unwritable_parent_like_the_script() {
    if unsafe { libc::geteuid() } == 0 {
        eprintln!(
            "STOREKIT_SKIP test=lock_cli_refuses_an_unwritable_parent_like_the_script \
             reason=running as root, EACCES is unreachable"
        );
        return;
    }
    let h = Harness::new();
    h.reset_root();
    let locked = h.remote_path("state/locked");
    let record = locked.join("thing.operation.lock");
    let logged = log_holder_script(&h, &record, "op-locked");
    let cli_args = [
        OsStr::new("lock"),
        record.as_os_str(),
        OsStr::new("op-locked"),
    ];
    let setup = || {
        let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755));
        h.reset_root();
        std::fs::create_dir_all(&locked).unwrap();
        set_mode(&locked, 0o555);
    };
    let (script, cli_run) = holder_parity(&h, "unwritable parent", &logged, setup, &cli_args);
    // The fixture is read-only, so the snapshot cannot differ; assert the
    // record really is still absent.
    assert!(!record.exists(), "the record must not be created");
    assert!(
        script.ready.is_empty(),
        "a refusal prints no readiness line"
    );
    assert!(cli_run.ready.is_empty());
    assert_eq!(
        cli_run.output.status.code(),
        Some(4),
        "EACCES is the unusable-record arm (4)"
    );
    assert_eq!(
        cli_run.output.stderr,
        lockerr_message("open", &record, libc::EACCES),
        "the CLI must report the open's EACCES"
    );
    // Restore the fixture so `TempDir` can remove it.
    set_mode(&locked, 0o755);
}

/// A SECOND run is held out for as long as the first holder lives, and the
/// record becomes acquirable the moment that holder's stdin closes.
#[test]
fn lock_cli_second_run_is_held_out_while_the_first_holder_lives() {
    let h = Harness::new();
    h.reset_root();
    let record = h.remote_path("state/serial.operation.lock");
    let first_args = [OsStr::new("lock"), record.as_os_str(), OsStr::new("op-1")];
    let holder = start_holder(cli(&first_args));
    ready_pid(&holder.ready);
    let held_mode = snapshot(&h.root);

    // A second run is REFUSED while the first lives, repeatedly.
    for _ in 0..2 {
        let contender = run_closed(cli(&first_args));
        assert_eq!(contender.status.code(), Some(3));
        assert_eq!(contender.stderr, contended_message(&record));
        assert!(
            contender.stdout.is_empty(),
            "a contended run must print NO readiness line"
        );
    }
    assert_eq!(
        snapshot(&h.root),
        held_mode,
        "contending runs must leave the record alone"
    );

    let released = holder.release();
    assert_eq!(released.status.code(), Some(0));

    // Released: the SAME command now acquires and rewrites the record.
    let after = run_holder(
        cli(&[OsStr::new("lock"), record.as_os_str(), OsStr::new("op-2")]),
        &h.root,
    );
    ready_pid(&after.ready);
    assert_eq!(after.output.status.code(), Some(0));
    let mut expected = RECORD_HEADER.as_bytes().to_vec();
    expected.extend_from_slice(b"op-2");
    assert_eq!(
        std::fs::read(&record).unwrap(),
        expected,
        "the next holder records its own op id"
    );
}

// -- transport level: CLI first, script fallback, ONE probe -------------------

/// The record a transport-level test locks: an absolute far-side path whose
/// PARENT CHAIN does not exist yet, so the holder's mkdir arm runs too.
fn lock_record(h: &Harness) -> PathBuf {
    h.remote_path("state/deep/thing.operation.lock")
}

/// The transport's `lock_far_side` takes the far-side record NON-BLOCKING:
/// `Ok` with the record written, `Err(LockContended)` while another holder
/// lives. Asserted identically for both forms by the two tests below. Returns
/// the log AFTER the first acquisition, the commands the contended acquisition
/// appended, and the commands the post-release acquisition appended.
fn exercise_far_side_lock(h: &Harness) -> (Vec<String>, Vec<String>, Vec<String>) {
    let record = lock_record(h);
    let t = h.transport();
    let mut first =
        Remote::lock_far_side(&t, &record, "op-1").expect("the first acquisition must succeed");
    let after_first = h.log();
    assert_eq!(
        std::fs::read(&record).expect("the holder writes the record"),
        record_bytes("op-1"),
        "the record is the crate header followed by the op id"
    );
    assert_eq!(mode_of(&record), 0o600, "the record is private");

    // A SECOND acquisition is REFUSED with the typed contention, not a wait,
    // and it leaves the live holder's record untouched.
    let err = match Remote::lock_far_side(&t, &record, "op-2") {
        Ok(_) => panic!("a contended acquisition must be refused, not queued"),
        Err(err) => err,
    };
    assert!(
        matches!(err, storekit::Error::LockContended(_)),
        "contention must be the typed LockContended, got {err:?}"
    );
    assert_eq!(
        std::fs::read(&record).unwrap(),
        record_bytes("op-1"),
        "a refused acquisition must not rewrite the record"
    );
    let after_second = h.log();

    first.release();
    let mut third = Remote::lock_far_side(&t, &record, "op-3")
        .expect("the released record must be acquirable again");
    let after_third = h.log();
    third.release();
    (
        after_first.clone(),
        after_second[after_first.len()..].to_vec(),
        after_third[after_second.len()..].to_vec(),
    )
}

/// The record bytes both forms must write.
fn record_bytes(op_id: &str) -> Vec<u8> {
    let mut bytes = RECORD_HEADER.as_bytes().to_vec();
    bytes.extend_from_slice(op_id.as_bytes());
    bytes
}

/// Assert the transport's probe ran exactly once across `log`.
fn assert_one_probe(log: &[String]) {
    assert_eq!(
        log.iter()
            .filter(|line| line.contains("storekit-remote protocol"))
            .count(),
        1,
        "exactly ONE probe per transport: {log:?}"
    );
}

/// With the CLI present the lock holder runs `storekit-remote lock <record>
/// <op_id>` — no `perl` — and the transport still probes exactly ONCE.
#[test]
fn lock_uses_the_cli_holder_when_present() {
    let h = Harness::new();
    h.expose_real_cli();
    h.reset_root();
    let record = lock_record(&h);
    let (first, second, third) = exercise_far_side_lock(&h);

    assert_eq!(first.len(), 2, "probe + the first holder, got {first:?}");
    assert_eq!(first[0], "bash -c 'storekit-remote protocol'");
    let record_token = record.to_string_lossy().into_owned();
    assert_eq!(
        first[1],
        expected_cli_command(&["storekit-remote", "lock", &record_token, "op-1"]),
        "the lock must run the CLI `lock` subcommand"
    );
    assert!(
        is_cli_form(&first[1]),
        "the lock must invoke the CLI: {:?}",
        first[1]
    );
    assert!(
        !first[1].contains("perl"),
        "a compatible CLI must not fall back to perl: {:?}",
        first[1]
    );

    // The contended and post-release acquisitions are CLI commands too, with
    // their own op ids, and neither re-probes.
    assert_eq!(second.len(), 1, "the contended acquisition: {second:?}");
    assert_eq!(third.len(), 1, "the post-release acquisition: {third:?}");
    assert_eq!(
        second[0],
        expected_cli_command(&["storekit-remote", "lock", &record_token, "op-2"])
    );
    assert_eq!(
        third[0],
        expected_cli_command(&["storekit-remote", "lock", &record_token, "op-3"])
    );
    let mut all = first;
    all.extend(second);
    all.extend(third);
    assert_one_probe(&all);
}

/// With no `storekit-remote` on the far side the lock holder falls back to the
/// SAME perl script, and the transport still probes exactly ONCE. The contention
/// behaviour is identical — that is the parity the probe must not change.
#[test]
fn lock_falls_back_to_the_perl_holder_when_the_cli_is_absent() {
    let h = Harness::new();
    h.reset_root();
    let record = lock_record(&h);
    let (first, second, third) = exercise_far_side_lock(&h);

    assert_eq!(first.len(), 2, "probe + the first holder, got {first:?}");
    assert_eq!(first[0], "bash -c 'storekit-remote protocol'");
    assert_script_form("lock", &first[1]);
    assert!(
        first[1].contains("perl"),
        "the fallback is the perl holder: {:?}",
        first[1]
    );
    assert!(
        first[1].contains("flock") && first[1].contains("STOREKIT_TEST_HOLD_LOCK"),
        "the fallback is the long-lived holder program: {:?}",
        first[1]
    );
    for command in second.iter().chain(&third) {
        assert_script_form("lock", command);
        assert!(
            command.contains("perl"),
            "the fallback is the perl holder: {command:?}"
        );
    }
    let mut all = first;
    all.extend(second);
    all.extend(third);
    assert_one_probe(&all);
    // The record is still the crate's record, whatever form wrote it.
    assert_eq!(std::fs::read(&record).unwrap(), record_bytes("op-3"));
}
