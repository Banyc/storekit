//! The far-side CLI fsync/rename/compare-and-delete operations
//! (`remote-cli-fsync-ops`): `fsync-file`, `fsync-dir`, `fsync-tree`, `rename`,
//! `remove-file-if` and `remove-file-if-sidecar` run through `storekit-remote`
//! when the far side has a compatible one, and fall back to the existing perl
//! scripts (and the `find` walk) when it does not.
//!
//! TWO KINDS OF TEST, both in this file:
//!
//! * DIFFERENTIAL tests run the EXACT logged script and the CLI on the same
//!   fixture and compare stdout bytes, stderr bytes, the exit status, and the
//!   final tree (kinds, raw modes, symlink targets, file bytes). The script is
//!   never re-typed here: it is the command the transport itself logged while
//!   the CLI was absent — with ONE stated exception, [`fsync_file_script`].
//! * TRANSPORT-LEVEL tests build an [`SshTransport`] over an `ssh` SHIM on
//!   `PATH` (the harness `tests/remote_cli_probe.rs` established) that logs
//!   each remote command and runs it locally. They assert the ACTUAL logged
//!   command: the CLI form with the binary present, the script form with it
//!   absent, and exactly ONE probe per transport.
//!
//! WHAT THE SHIM DOES NOT PROVE. No network, no `sshd`, no OpenSSH option
//! parsing, no remote filesystem semantics: the far side is the SAME host. A
//! defect that depends on those is out of scope here.
//!
//! ONE KNOWN, DELIBERATE DIFFERENCE, asserted rather than hidden: where the
//! script's diagnostic comes from a SHELL COREUTIL (`find`'s own walk errors,
//! `mkdir`'s own failures) the CLI states its own message — those coreutils
//! word their diagnostics differently on GNU and BSD, so no single byte string
//! can be both — while keeping the STATUS the coreutil exits with. Everywhere
//! the script's diagnostic is perl's `die` (the fsync primitive, `rename`, the
//! sidecar's open/flock/unlink) the bytes and the status are identical.
#![cfg(unix)]
// Test-only fixtures drive the same name-mutating primitives the funnel guards.
#![allow(clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use storekit::env::SysEnv;
use storekit::transport::{
    Layout, Remote, RemoveIfVerdict, RootedRelativePath, SSH_TWRITE_PREINSTALL_EXIT, SshTransport,
};

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

/// Create a FIFO at `path` (the fixture's "special entry").
fn mkfifo(path: &Path) {
    use std::ffi::CString;
    let c = CString::new(path.as_os_str().as_bytes()).expect("a path without NUL");
    // SAFETY: `c` is a valid NUL-terminated path for the duration of the call.
    let rc = unsafe { libc::mkfifo(c.as_ptr(), 0o644) };
    assert_eq!(
        rc,
        0,
        "mkfifo {}: {}",
        path.display(),
        std::io::Error::last_os_error()
    );
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
            .prefix("storekit-remote-fsync-")
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

    /// The path token the CLI argv carries for `rel`.
    fn cli_path(&self, rel: &str) -> PathBuf {
        self.remote_path(rel)
    }
}

fn rooted(rel: &str) -> RootedRelativePath {
    RootedRelativePath::parse(Path::new(rel)).expect("valid rooted relative path")
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

/// A command that runs `command` exactly as the ssh argv does.
fn sh(command: &str) -> Command {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg(command);
    cmd
}

/// Run `cmd` with no stdin under a DEADLINE, so a blocking run is a failed test
/// rather than a hang.
fn run_bounded(mut cmd: Command, secs: u64) -> Output {
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the command under test");
    wait_child(child, secs)
}

/// Wait for `child` under a DEADLINE, then collect its output.
fn wait_child(mut child: std::process::Child, secs: u64) -> Output {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    loop {
        match child.try_wait().expect("poll the command under test") {
            Some(_) => {
                return child
                    .wait_with_output()
                    .expect("collect the command's output");
            }
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let out = child.wait_with_output().expect("collect the killed output");
                panic!(
                    "the command did not finish within {secs}s (it blocked): stdout {:?} stderr {:?}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            None => std::thread::sleep(std::time::Duration::from_millis(10)),
        }
    }
}

/// Assert the logged script and the CLI produce the SAME stdout bytes, the SAME
/// stderr bytes and the SAME exit status.
fn assert_parity(case: &str, script: &Output, cli: &Output) {
    assert_eq!(
        cli.status.code(),
        script.status.code(),
        "{case}: exit status must match (script stderr {:?}, CLI stderr {:?})",
        String::from_utf8_lossy(&script.stderr),
        String::from_utf8_lossy(&cli.stderr)
    );
    assert_eq!(
        cli.stdout, script.stdout,
        "{case}: stdout bytes must be identical"
    );
    assert_eq!(
        cli.stderr, script.stderr,
        "{case}: stderr bytes must be identical"
    );
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

/// No writer may leave a temp behind, on success OR failure.
fn assert_no_temp_residue(case: &str, state: &BTreeMap<String, Node>) {
    for rel in state.keys() {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        assert!(
            !storekit::atomic::is_crate_temp_name(name),
            "{case}: a writer left a temp behind: {rel}"
        );
    }
}

/// One differential case: `setup` places the fixture, the script and the CLI
/// each run against the SAME freshly reset state, and the outputs plus the
/// final tree are compared.
fn run_parity<F: Fn()>(
    h: &Harness,
    case: &str,
    logged: &str,
    setup: F,
    cli_args: &[&OsStr],
) -> (Output, Output) {
    setup();
    let script = run_bounded(sh(logged), 60);
    let script_state = snapshot(&h.root);
    setup();
    let cli_out = run_bounded(cli(cli_args), 60);
    let cli_state = snapshot(&h.root);
    assert_parity(case, &script, &cli_out);
    assert_eq!(
        cli_state, script_state,
        "{case}: the final tree (kinds, modes, targets and bytes) must match the script's"
    );
    assert_no_temp_residue(case, &cli_state);
    (script, cli_out)
}

/// The exact prefix of a CLI `argv` command: `argv_cmd` single-quotes every
/// word, and `ssh_command_argv` quotes the whole command again for `bash -c`, so
/// the CLI form begins `bash -c ''\''storekit-remote'\'' '\''<subcommand>…` while
/// a script form begins `bash -c 'find'`, `bash -c 'perl'` or
/// `bash -c 'mkdir` (whose first word is quoted one level shallower). Derived
/// from the quoting rule and cut before the SUBCOMMAND word rather than
/// hand-escaped.
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

// -- logging the exact script each op builds (CLI absent) --------------------

fn log_fsync_tree(h: &Harness, rel: &str) -> String {
    let t = h.transport();
    Remote::fsync_tree(&t, &rooted(rel)).expect("the script walk must succeed on this fixture");
    let command = h
        .log()
        .pop()
        .expect("the shim logged the fsync-tree command");
    assert_script_form("fsync-tree", &command);
    assert!(
        command.contains("find") && command.contains("perl -e"),
        "the fsync-tree script is the `find … -exec sh -c … perl -e …` walk: {command:?}"
    );
    command
}

/// The transport's fsync script for EXACTLY the path `rel`.
///
/// `fsync_parent(rel)` fsyncs the PARENT of `root.join(rel)`, so asking it about
/// a CHILD of `rel` logs the transport's own `perl -e '<primitive>' --
/// '<root/rel>'` command for `rel` itself. Nothing is re-typed: the command is
/// the one the transport built and logged, and only the op's own outcome is
/// ignored (a FIFO is refused, a missing path fails — the differential re-runs
/// the LOGGED command against a fresh fixture). Swapping the primitive's trailing
/// hook token ([`fsync_file_script`]) yields the `fsync-file` form, which has no
/// standalone call site in the transport.
fn log_fsync_script(h: &Harness, rel: &str) -> String {
    let t = h.transport();
    let child = format!("{rel}/child");
    let _ = Remote::fsync_parent(&t, &rooted(&child));
    let command = h.log().pop().expect("the shim logged the fsync command");
    assert_script_form("fsync-dir", &command);
    assert!(
        command.contains("perl -e"),
        "the fsync-dir script is the perl fsync primitive: {command:?}"
    );
    command
}

fn log_rename(h: &Harness, from: &str, to: &str) -> String {
    let t = h.transport();
    // The op's own outcome is irrelevant here: the differential re-runs the
    // LOGGED command against a fresh fixture, so a case whose operands make the
    // rename fail (a missing source) still logs the exact script it needs.
    let _ = Remote::rename(&t, &rooted(from), &rooted(to));
    let command = h.log().pop().expect("the shim logged the rename command");
    assert_script_form("rename", &command);
    assert!(
        command.contains("mkdir -p") && command.contains("rename("),
        "the rename script creates the parent then renames: {command:?}"
    );
    command
}

fn log_remove_file_if(h: &Harness, rel: &str, expected: &str) -> String {
    let t = h.transport();
    Remote::remove_file_if(&t, &rooted(rel), expected.as_bytes())
        .expect("the script compare-and-delete must answer a verdict");
    let command = h
        .log()
        .pop()
        .expect("the shim logged the remove-file-if command");
    assert_script_form("remove-file-if", &command);
    assert!(
        command.contains("mktemp") && command.contains("cmp") && command.contains("ln "),
        "the remove-file-if script claims with mktemp+mv and compares with cmp: {command:?}"
    );
    command
}

fn log_remove_file_if_sidecar(h: &Harness, expected: &str) -> String {
    let t = h.transport();
    Remote::remove_file_if(&t, &rooted("state/operation.lock"), expected.as_bytes())
        .expect("the script sidecar compare-and-delete must answer a verdict");
    let command = h.log().pop().expect("the shim logged the sidecar command");
    assert_script_form("remove-file-if-sidecar", &command);
    assert!(
        command.contains("flock") && command.contains("unlink"),
        "the sidecar helper takes the flock and unlinks on a match: {command:?}"
    );
    command
}

/// The `fsync-file` primitive's script.
///
/// The transport carries the SAME program for `PERL_FSYNC_FILE` and
/// `PERL_FSYNC_DIR` — they differ only in the trailing test-hook comment, which
/// the transport's own unit test pins in both directions — and only the DIR form
/// has a standalone call site (`fsync_parent`); the file form is embedded in the
/// `publish`/`write-new` scripts, which their own differentials cover. Swapping
/// the one hook token therefore yields the exact file script the transport
/// carries, instead of a re-typed copy.
fn fsync_file_script(dir_script: &str) -> String {
    assert_eq!(
        dir_script.matches("STOREKIT_TEST_FSYNC_DIR").count(),
        1,
        "the logged fsync-dir script must carry its hook exactly once: {dir_script}"
    );
    dir_script.replace("STOREKIT_TEST_FSYNC_DIR", "STOREKIT_TEST_FSYNC_FILE")
}

/// The script's refusal bytes for `path`, which both forms must print.
fn fsync_refuse_message(path: &Path) -> Vec<u8> {
    format!(
        "fsync-refuse {}: not a regular file or directory at -e line 1.\n",
        path.display()
    )
    .into_bytes()
}

/// The script's open-failure bytes for `path` (ENOENT's C-library text).
fn fsync_open_enoent_message(path: &Path) -> Vec<u8> {
    format!(
        "fsync-open {}: No such file or directory at -e line 1.\n",
        path.display()
    )
    .into_bytes()
}

// -- fsync-file / fsync-dir ---------------------------------------------------

/// The single-path fsync primitive: a regular file, a directory, a FIFO (opened
/// `O_NONBLOCK`, refused without blocking), a missing path, and a symlink to a
/// regular file (which BOTH forms follow: the primitive's open is not
/// `O_NOFOLLOW`, unlike `verify-open`).
#[test]
fn fsync_primitives_cli_are_byte_identical_to_the_script() {
    let h = Harness::new();
    h.reset_root();
    std::fs::create_dir_all(h.remote_path("plain")).unwrap();
    std::fs::write(h.remote_path("plain/file.txt"), b"payload").unwrap();
    let dir_script = log_fsync_script(&h, "plain");
    let file_script = fsync_file_script(&log_fsync_script(&h, "plain/file.txt"));
    let link_script = fsync_file_script(&log_fsync_script(&h, "plain/link.txt"));
    let fifo_script = fsync_file_script(&log_fsync_script(&h, "plain/pipe"));
    let missing_script = fsync_file_script(&log_fsync_script(&h, "plain/absent"));

    // A regular FILE: accepted, fsynced, silent, exit 0.
    let file = h.cli_path("plain/file.txt");
    let (script, cli_out) = run_parity(
        &h,
        "fsync-file regular file",
        &file_script,
        || {
            h.reset_root();
            std::fs::create_dir_all(h.remote_path("plain")).unwrap();
            std::fs::write(h.remote_path("plain/file.txt"), b"payload").unwrap();
        },
        &[OsStr::new("fsync-file"), file.as_os_str()],
    );
    assert_eq!(cli_out.status.code(), Some(0));
    assert!(cli_out.stdout.is_empty() && cli_out.stderr.is_empty());
    assert!(script.stdout.is_empty());

    // A DIRECTORY: accepted too (the same primitive serves both hooks).
    let dir = h.cli_path("plain");
    let (_, cli_out) = run_parity(
        &h,
        "fsync-dir directory",
        &dir_script,
        || {
            h.reset_root();
            std::fs::create_dir_all(h.remote_path("plain")).unwrap();
            std::fs::write(h.remote_path("plain/file.txt"), b"payload").unwrap();
        },
        &[OsStr::new("fsync-dir"), dir.as_os_str()],
    );
    assert_eq!(cli_out.status.code(), Some(0));
    assert!(cli_out.stdout.is_empty() && cli_out.stderr.is_empty());

    // A SYMLINK to a regular file: FOLLOWED by both forms (a plain read open),
    // so it is accepted rather than refused.
    let link = h.cli_path("plain/link.txt");
    let (_, cli_out) = run_parity(
        &h,
        "fsync-file symlink to a file",
        &link_script,
        || {
            h.reset_root();
            std::fs::create_dir_all(h.remote_path("plain")).unwrap();
            std::fs::write(h.remote_path("plain/file.txt"), b"payload").unwrap();
            std::os::unix::fs::symlink("file.txt", h.remote_path("plain/link.txt")).unwrap();
        },
        &[OsStr::new("fsync-file"), link.as_os_str()],
    );
    assert_eq!(cli_out.status.code(), Some(0));

    // A FIFO: the `O_NONBLOCK` read open returns at once, the inode is
    // classified, and it is REFUSED. The exit status is perl's `die` status,
    // which at the refusal holds the errno perl's own handle setup left behind
    // (ESPIPE: 29 on both Linux and macOS) — NOT the `die` rule's 255, and NOT
    // an arbitrary choice: the CLI reproduces the two probes that set it.
    let fifo = h.cli_path("plain/pipe");
    let setup_fifo = || {
        h.reset_root();
        std::fs::create_dir_all(h.remote_path("plain")).unwrap();
        mkfifo(&h.remote_path("plain/pipe"));
    };
    let (script, cli_out) = run_parity(
        &h,
        "fsync-file FIFO",
        &fifo_script,
        setup_fifo,
        &[OsStr::new("fsync-file"), fifo.as_os_str()],
    );
    assert_eq!(
        script.stderr,
        fsync_refuse_message(&fifo),
        "the script's refusal bytes"
    );
    assert_eq!(
        cli_out.stderr,
        fsync_refuse_message(&fifo),
        "the FIFO must be refused with the primitive's exact diagnostic"
    );
    assert_eq!(
        cli_out.status.code(),
        Some(29),
        "the refusal inherits perl's leftover ESPIPE, so the status is 29 (script: {:?})",
        script.status.code()
    );
    assert!(cli_out.stdout.is_empty(), "a refusal prints no frame");

    // A MISSING path: `sysopen` fails, so the die carries a REAL errno (ENOENT).
    let missing = h.cli_path("plain/absent");
    let (script, cli_out) = run_parity(
        &h,
        "fsync-file missing",
        &missing_script,
        || {
            h.reset_root();
            std::fs::create_dir_all(h.remote_path("plain")).unwrap();
        },
        &[OsStr::new("fsync-file"), missing.as_os_str()],
    );
    assert_eq!(script.stderr, fsync_open_enoent_message(&missing));
    assert_eq!(cli_out.stderr, fsync_open_enoent_message(&missing));
    assert_eq!(
        cli_out.status.code(),
        Some(2),
        "the missing path exits with `$!` (ENOENT)"
    );
}

// -- fsync-tree ---------------------------------------------------------------

/// A depth-3 tree, a tree holding a symlink and a FIFO, and a read-only
/// directory: the walk visits children before parents, SKIPS the symlink and the
/// FIFO (their directory entry is the parent's fsync's job), and succeeds on a
/// directory it may not write.
#[test]
fn fsync_tree_cli_is_byte_identical_to_the_script() {
    let h = Harness::new();
    let tree = h.cli_path("bundle");
    h.reset_root();
    std::fs::create_dir_all(h.remote_path("bundle")).unwrap();
    let logged = log_fsync_tree(&h, "bundle");

    // A depth-3 tree: files at every level, an empty directory, and a symlink.
    let (_, cli_out) = run_parity(
        &h,
        "fsync-tree depth-3",
        &logged,
        || {
            h.reset_root();
            let t = h.remote_path("bundle");
            std::fs::create_dir_all(t.join("a/b/c")).unwrap();
            std::fs::create_dir_all(t.join("empty")).unwrap();
            std::fs::write(t.join("top"), b"top").unwrap();
            std::fs::write(t.join("a/mid"), b"mid").unwrap();
            std::fs::write(t.join("a/b/c/deep"), b"deep").unwrap();
            std::os::unix::fs::symlink("top", t.join("a/link")).unwrap();
        },
        &[OsStr::new("fsync-tree"), tree.as_os_str()],
    );
    assert_eq!(cli_out.status.code(), Some(0));
    assert!(cli_out.stdout.is_empty() && cli_out.stderr.is_empty());

    // A tree holding a SYMLINK (live and dangling) and a FIFO: neither is
    // opened, so the walk terminates and succeeds.
    let (_, cli_out) = run_parity(
        &h,
        "fsync-tree symlink and FIFO",
        &logged,
        || {
            h.reset_root();
            let t = h.remote_path("bundle");
            std::fs::create_dir_all(t.join("sub")).unwrap();
            std::fs::write(t.join("sub/file"), b"x").unwrap();
            mkfifo(&t.join("sub/pipe"));
            std::os::unix::fs::symlink("sub/file", t.join("live")).unwrap();
            std::os::unix::fs::symlink("nowhere", t.join("dangling")).unwrap();
        },
        &[OsStr::new("fsync-tree"), tree.as_os_str()],
    );
    assert_eq!(
        cli_out.status.code(),
        Some(0),
        "a skipped FIFO must not fail the walk (stderr {:?})",
        String::from_utf8_lossy(&cli_out.stderr)
    );
    assert!(cli_out.stdout.is_empty() && cli_out.stderr.is_empty());

    // A READ-ONLY directory (0o500): opening it `O_RDONLY` and fsyncing it needs
    // no write permission, so the walk succeeds.
    let (_, cli_out) = run_parity(
        &h,
        "fsync-tree read-only directory",
        &logged,
        || {
            h.reset_root();
            let t = h.remote_path("bundle");
            std::fs::create_dir_all(t.join("locked")).unwrap();
            std::fs::write(t.join("locked/file"), b"x").unwrap();
            std::fs::set_permissions(t.join("locked"), std::fs::Permissions::from_mode(0o500))
                .unwrap();
        },
        &[OsStr::new("fsync-tree"), tree.as_os_str()],
    );
    assert_eq!(
        cli_out.status.code(),
        Some(0),
        "a read-only directory is still fsyncable (stderr {:?})",
        String::from_utf8_lossy(&cli_out.stderr)
    );
    // Restore the mode so the harness's tempdir cleanup can remove the tree.
    std::fs::set_permissions(
        h.remote_path("bundle/locked"),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
}

/// A directory the walk cannot READ is a walk failure: the script's `find`
/// reports it and exits 1, and the CLI stops at the FIRST such entry, names it,
/// and exits 1 too. `find`'s own wording is userland-specific (GNU quotes the
/// path, BSD does not) and `find` keeps walking, so the CLI states its own
/// diagnostic and STOPS — the status is the script's, and the single diagnostic
/// is the observable evidence that the walk stopped.
#[test]
fn fsync_tree_stops_at_the_first_unreadable_entry() {
    let h = Harness::new();
    h.reset_root();
    std::fs::create_dir_all(h.remote_path("bundle")).unwrap();
    let logged = log_fsync_tree(&h, "bundle");
    // TWO unreadable siblings: `find` would report both, the walk reports the
    // first one in `readdir` order and stops.
    let setup = || {
        h.reset_root();
        let t = h.remote_path("bundle");
        std::fs::create_dir_all(t.join("blocked-a")).unwrap();
        std::fs::create_dir_all(t.join("blocked-b")).unwrap();
        std::fs::create_dir_all(t.join("readable")).unwrap();
        std::fs::write(t.join("readable/file"), b"x").unwrap();
        for name in ["blocked-a", "blocked-b"] {
            std::fs::set_permissions(t.join(name), std::fs::Permissions::from_mode(0o300)).unwrap();
        }
    };

    setup();
    let script = run_bounded(sh(&logged), 60);
    assert!(
        !script.status.success(),
        "the script's `find` must fail on an unreadable directory"
    );

    setup();
    // The FIRST unreadable sibling in the order the walk itself sees them.
    let blocked = std::fs::read_dir(h.remote_path("bundle"))
        .expect("read the bundle")
        .map(|entry| entry.expect("read an entry").file_name())
        .find(|name| name == "blocked-a" || name == "blocked-b")
        .expect("one blocked sibling comes first");
    let tree = h.cli_path("bundle");
    let cli_out = run_bounded(cli(&[OsStr::new("fsync-tree"), tree.as_os_str()]), 60);
    let expected = format!(
        "storekit-remote: fsync-tree: {}: Permission denied\n",
        h.remote_path("bundle").join(&blocked).display()
    );
    assert_eq!(
        cli_out.status.code(),
        Some(SSH_TWRITE_PREINSTALL_EXIT),
        "the walk must fail with the status the script's `find` exits with"
    );
    assert_eq!(
        String::from_utf8_lossy(&cli_out.stderr),
        expected,
        "the walk must name the FIRST unreadable entry and stop there"
    );
    assert!(cli_out.stdout.is_empty());

    for name in ["blocked-a", "blocked-b"] {
        std::fs::set_permissions(
            h.remote_path(&format!("bundle/{name}")),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
    }
}

// -- rename -------------------------------------------------------------------

/// The `rename` chain: an atomic REPLACE of a regular file, an atomic REPLACE of
/// a SYMLINK (the link's own entry, never its target), a DIRECTORY target
/// refusal, a missing parent (created), and a missing source.
#[test]
fn rename_cli_is_byte_identical_to_the_script() {
    let h = Harness::new();
    h.reset_root();
    std::fs::create_dir_all(h.remote_path("src")).unwrap();
    std::fs::write(h.remote_path("src/from"), b"NEW").unwrap();
    let logged = log_rename(&h, "src/from", "dst/to");

    // Replace a regular FILE.
    let (_, cli_out) = run_parity(
        &h,
        "rename over a regular file",
        &logged,
        || {
            h.reset_root();
            std::fs::create_dir_all(h.remote_path("src")).unwrap();
            std::fs::create_dir_all(h.remote_path("dst")).unwrap();
            std::fs::write(h.remote_path("src/from"), b"NEW").unwrap();
            std::fs::write(h.remote_path("dst/to"), b"OLD").unwrap();
        },
        &[
            OsStr::new("rename"),
            h.cli_path("src/from").as_os_str(),
            h.cli_path("dst/to").as_os_str(),
        ],
    );
    assert_eq!(cli_out.status.code(), Some(0));
    assert_eq!(std::fs::read(h.remote_path("dst/to")).unwrap(), b"NEW");
    assert!(
        !h.remote_path("src/from").exists(),
        "the source must be gone after a rename"
    );

    // Replace a SYMLINK: the LINK ENTRY is replaced, its target is untouched.
    let (_, cli_out) = run_parity(
        &h,
        "rename over a symlink",
        &logged,
        || {
            h.reset_root();
            std::fs::create_dir_all(h.remote_path("src")).unwrap();
            std::fs::create_dir_all(h.remote_path("dst")).unwrap();
            std::fs::write(h.remote_path("src/from"), b"NEW").unwrap();
            std::fs::write(h.remote_path("dst/pointed-at"), b"TARGET").unwrap();
            std::os::unix::fs::symlink("pointed-at", h.remote_path("dst/to")).unwrap();
        },
        &[
            OsStr::new("rename"),
            h.cli_path("src/from").as_os_str(),
            h.cli_path("dst/to").as_os_str(),
        ],
    );
    assert_eq!(cli_out.status.code(), Some(0));
    assert!(
        !std::fs::symlink_metadata(h.remote_path("dst/to"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "the symlink ENTRY must be replaced by the source"
    );
    assert_eq!(std::fs::read(h.remote_path("dst/to")).unwrap(), b"NEW");
    assert_eq!(
        std::fs::read(h.remote_path("dst/pointed-at")).unwrap(),
        b"TARGET",
        "the symlink's TARGET is never followed or modified"
    );

    // A DIRECTORY target: refused (never moved into), with perl's `die` bytes
    // and the errno it exits with.
    let (script, cli_out) = run_parity(
        &h,
        "rename onto a directory",
        &logged,
        || {
            h.reset_root();
            std::fs::create_dir_all(h.remote_path("src")).unwrap();
            std::fs::create_dir_all(h.remote_path("dst/to")).unwrap();
            std::fs::write(h.remote_path("src/from"), b"NEW").unwrap();
        },
        &[
            OsStr::new("rename"),
            h.cli_path("src/from").as_os_str(),
            h.cli_path("dst/to").as_os_str(),
        ],
    );
    let expected = format!(
        "rename {} -> {}: Is a directory at -e line 1.\n",
        h.remote_path("src/from").display(),
        h.remote_path("dst/to").display(),
    );
    assert_eq!(script.stderr, expected.as_bytes());
    assert!(script.stdout.is_empty(), "a refused rename prints nothing");
    assert_eq!(
        cli_out.stderr, script.stderr,
        "the refusal is perl's OWN `die` text"
    );
    assert_eq!(
        cli_out.status.code(),
        Some(libc::EISDIR),
        "perl's `die` exits with `$!` (EISDIR)"
    );
    assert!(
        h.remote_path("src/from").exists(),
        "a refused rename must leave the source in place"
    );

    // A MISSING parent: created by both forms.
    let (_, cli_out) = run_parity(
        &h,
        "rename with a missing parent",
        &logged,
        || {
            h.reset_root();
            std::fs::create_dir_all(h.remote_path("src")).unwrap();
            std::fs::write(h.remote_path("src/from"), b"NEW").unwrap();
        },
        &[
            OsStr::new("rename"),
            h.cli_path("src/from").as_os_str(),
            h.cli_path("dst/to").as_os_str(),
        ],
    );
    assert_eq!(cli_out.status.code(), Some(0));
    assert_eq!(
        std::fs::read(h.remote_path("dst/to")).unwrap(),
        b"NEW",
        "the parent must have been created and the entry renamed into it"
    );

    // A MISSING source: perl's `die` with ENOENT.
    let missing_logged = log_rename(&h, "src/absent", "dst/to");
    let (_, cli_out) = run_parity(
        &h,
        "rename a missing source",
        &missing_logged,
        || {
            h.reset_root();
            std::fs::create_dir_all(h.remote_path("src")).unwrap();
            std::fs::create_dir_all(h.remote_path("dst")).unwrap();
        },
        &[
            OsStr::new("rename"),
            h.cli_path("src/absent").as_os_str(),
            h.cli_path("dst/to").as_os_str(),
        ],
    );
    assert_eq!(cli_out.status.code(), Some(2));
    assert_eq!(
        cli_out.stderr,
        format!(
            "rename {} -> {}: No such file or directory at -e line 1.\n",
            h.remote_path("src/absent").display(),
            h.remote_path("dst/to").display()
        )
        .into_bytes()
    );
}

// -- remove-file-if -----------------------------------------------------------

/// The compare-and-delete verdicts: match (`R`), mismatch (`M`, content
/// RESTORED), absent (`A`), and a symlink whose TARGET content matches (`R`:
/// the script's `cmp` follows the claimed link and `rm` removes the link entry).
#[test]
fn remove_file_if_cli_is_byte_identical_to_the_script() {
    let h = Harness::new();
    let expected = "generation-1";
    h.reset_root();
    std::fs::create_dir_all(h.remote_path("staged")).unwrap();
    std::fs::write(h.remote_path("staged/record"), expected).unwrap();
    let logged = log_remove_file_if(&h, "staged/record", expected);

    let record = h.cli_path("staged/record");
    let cli_args = [
        OsStr::new("remove-file-if"),
        record.as_os_str(),
        OsStr::new(expected),
    ];

    // MATCH: frame `R`, the entry is gone.
    let (script, cli_out) = run_parity(
        &h,
        "remove-file-if match",
        &logged,
        || {
            h.reset_root();
            std::fs::create_dir_all(h.remote_path("staged")).unwrap();
            std::fs::write(h.remote_path("staged/record"), expected).unwrap();
        },
        &cli_args,
    );
    assert_eq!(script.stdout, b"R".to_vec(), "the script's frame");
    assert_eq!(cli_out.stdout, b"R".to_vec());
    assert!(cli_out.stderr.is_empty());
    assert_eq!(cli_out.status.code(), Some(0));
    assert!(
        !h.remote_path("staged/record").exists(),
        "a match must remove the entry"
    );

    // MISMATCH: frame `M`, the original content RESTORED, and nothing else in
    // the directory.
    let (script, cli_out) = run_parity(
        &h,
        "remove-file-if mismatch",
        &logged,
        || {
            h.reset_root();
            std::fs::create_dir_all(h.remote_path("staged")).unwrap();
            std::fs::write(h.remote_path("staged/record"), b"generation-0").unwrap();
        },
        &cli_args,
    );
    assert_eq!(script.stdout, b"M".to_vec(), "the script's frame");
    assert_eq!(cli_out.stdout, b"M".to_vec());
    assert_eq!(
        std::fs::read(h.remote_path("staged/record")).unwrap(),
        b"generation-0",
        "a mismatch must RESTORE the claimed content"
    );

    // ABSENT: frame `A`.
    let (script, cli_out) = run_parity(
        &h,
        "remove-file-if absent",
        &logged,
        || {
            h.reset_root();
            std::fs::create_dir_all(h.remote_path("staged")).unwrap();
        },
        &cli_args,
    );
    assert_eq!(script.stdout, b"A".to_vec(), "the script's frame");
    assert_eq!(cli_out.stdout, b"A".to_vec());
    assert_eq!(cli_out.status.code(), Some(0));

    // A SYMLINK whose TARGET holds the expected bytes: `cmp` and the CLI's read
    // both FOLLOW the claimed link, so the verdict is `R` and the LINK ENTRY is
    // removed while its target survives.
    let (_, cli_out) = run_parity(
        &h,
        "remove-file-if through a symlink",
        &logged,
        || {
            h.reset_root();
            std::fs::create_dir_all(h.remote_path("staged")).unwrap();
            std::fs::write(h.remote_path("staged/target"), expected).unwrap();
            std::os::unix::fs::symlink("target", h.remote_path("staged/record")).unwrap();
        },
        &cli_args,
    );
    assert_eq!(cli_out.stdout, b"R".to_vec());
    assert!(
        std::fs::symlink_metadata(h.remote_path("staged/record")).is_err(),
        "the claimed symlink ENTRY must be removed"
    );
    assert_eq!(
        std::fs::read(h.remote_path("staged/target")).unwrap(),
        expected.as_bytes(),
        "the symlink's target must be untouched"
    );
}

/// Drive one `remove-file-if` run whose CLAIMED entry is a FIFO, so the compare
/// blocks until this thread releases it. While it is blocked a CONCURRENT
/// INSTALL appears at the record path — the exact situation the no-replace
/// restore exists for.
///
/// The ordering is deterministic, not a sleep race: this thread holds a writer
/// on the FIFO before the run starts, so BOTH compare forms must block (the
/// script's `cmp` on an empty pipe with a writer waits for data; the CLI's
/// `read_to_end` waits for EOF), and releasing the writer happens only AFTER the
/// concurrent install has landed. The claim itself is observed as the record
/// path's disappearance — the `mv`/`rename(2)` that takes it.
fn run_claim_race(mut cmd: Command, record: &Path, stale: &[u8], next: &[u8]) -> Output {
    use std::io::Write;

    let mut writer = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(record)
        .expect("open the FIFO claim for writing before the run");
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the compare-and-delete under test");

    // Wait for the CLAIM: the record path disappears exactly when it is renamed
    // to the claim temp (nothing else in the run unlinks it).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::fs::symlink_metadata(record).is_ok() {
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let out = child.wait_with_output().expect("collect the killed output");
            panic!(
                "the run never claimed the record within 30s: stdout {:?} stderr {:?}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        std::thread::sleep(std::time::Duration::from_micros(200));
    }

    // CONCURRENT INSTALL: a successor publishes at the now-free record path.
    std::fs::write(record, next).expect("the concurrent install must land");

    // Release the blocked compare with bytes that do NOT match, so the verdict
    // is `M` and the restore is the no-replace path under contention. The writer
    // stays open for a GRACE period because only a CLOSE delivers the EOF the
    // CLI's read-to-EOF needs: a compare that had not yet opened the FIFO would
    // block forever in `open(2)` if the last writer vanished first. The grace is
    // orders of magnitude longer than the claim-to-open path, and a stall longer
    // than it surfaces as the bounded deadline's diagnostic rather than a hang.
    let _ = writer.write_all(stale);
    std::thread::sleep(std::time::Duration::from_secs(1));
    drop(writer);

    wait_child(child, 60)
}

/// A concurrent install is NEVER replaced: the claimed entry's stale content
/// cannot be restored over the successor's record, so the verdict is `M`, the
/// successor's bytes survive, and the claim temp (the FIFO) is discarded.
#[test]
fn remove_file_if_cli_never_replaces_a_concurrent_install() {
    let h = Harness::new();
    let expected = "generation-1";
    let stale = b"generation-0";
    let next = b"generation-2";
    let record = h.remote_path("staged/record");
    let logged = log_remove_file_if(&h, "staged/record", expected);

    let setup = || {
        h.reset_root();
        std::fs::create_dir_all(h.remote_path("staged")).unwrap();
        mkfifo(&h.remote_path("staged/record"));
    };

    setup();
    let script = run_claim_race(sh(&logged), &record, stale, next);
    assert_eq!(
        script.stdout,
        b"M".to_vec(),
        "the script's frame under a concurrent install (stderr {:?})",
        String::from_utf8_lossy(&script.stderr)
    );
    assert_eq!(std::fs::read(&record).unwrap(), next);

    setup();
    let cli_out = run_claim_race(
        cli(&[
            OsStr::new("remove-file-if"),
            record.as_os_str(),
            OsStr::new(expected),
        ]),
        &record,
        stale,
        next,
    );
    assert_parity("remove-file-if concurrent install", &script, &cli_out);
    assert_eq!(cli_out.stdout, b"M".to_vec());
    assert_eq!(
        std::fs::read(&record).unwrap(),
        next,
        "the concurrent install's bytes must survive untouched"
    );
    assert_no_temp_residue("remove-file-if concurrent install", &snapshot(&h.root));
}

// -- remove-file-if-sidecar ---------------------------------------------------

/// The sidecar helper's verdicts under its own flock: match (`R`, the record is
/// unlinked), mismatch (`M`, the record is left alone), absent (`A`).
#[test]
fn remove_file_if_sidecar_cli_is_byte_identical_to_the_script() {
    let h = Harness::new();
    let expected = "lock-record";
    h.reset_root();
    std::fs::create_dir_all(h.remote_path("state")).unwrap();
    std::fs::write(h.remote_path("state/operation.lock"), expected).unwrap();
    let logged = log_remove_file_if_sidecar(&h, expected);

    let sidecar = h.cli_path("state/operation.lock.mutex");
    let lock = h.cli_path("state/operation.lock");
    let cli_args = [
        OsStr::new("remove-file-if-sidecar"),
        sidecar.as_os_str(),
        lock.as_os_str(),
        OsStr::new(expected),
    ];

    // MATCH: `R`, the lock record is gone (and the sidecar remains).
    let (script, cli_out) = run_parity(
        &h,
        "sidecar remove match",
        &logged,
        || {
            h.reset_root();
            std::fs::create_dir_all(h.remote_path("state")).unwrap();
            std::fs::write(h.remote_path("state/operation.lock"), expected).unwrap();
        },
        &cli_args,
    );
    assert_eq!(script.stdout, b"R".to_vec(), "the script's frame");
    assert_eq!(cli_out.stdout, b"R".to_vec());
    assert!(cli_out.stderr.is_empty());
    assert_eq!(cli_out.status.code(), Some(0));
    assert!(
        !h.remote_path("state/operation.lock").exists(),
        "a match must unlink the record"
    );
    assert!(
        h.remote_path("state/operation.lock.mutex").is_file(),
        "the sidecar file itself is never removed"
    );

    // MISMATCH: `M`, the record keeps its own content.
    let (script, cli_out) = run_parity(
        &h,
        "sidecar remove mismatch",
        &logged,
        || {
            h.reset_root();
            std::fs::create_dir_all(h.remote_path("state")).unwrap();
            std::fs::write(h.remote_path("state/operation.lock"), b"someone-else").unwrap();
        },
        &cli_args,
    );
    assert_eq!(script.stdout, b"M".to_vec(), "the script's frame");
    assert_eq!(cli_out.stdout, b"M".to_vec());
    assert_eq!(
        std::fs::read(h.remote_path("state/operation.lock")).unwrap(),
        b"someone-else",
        "a mismatch must never touch the record"
    );

    // ABSENT: `A`.
    let (script, cli_out) = run_parity(
        &h,
        "sidecar remove absent",
        &logged,
        || {
            h.reset_root();
            std::fs::create_dir_all(h.remote_path("state")).unwrap();
        },
        &cli_args,
    );
    assert_eq!(script.stdout, b"A".to_vec(), "the script's frame");
    assert_eq!(cli_out.stdout, b"A".to_vec());
    assert_eq!(cli_out.status.code(), Some(0));
}

/// A sidecar held by another process: both forms exhaust the bounded deadline
/// and take the script's `die "sidecar contended"` path. The helper's program is
/// MULTILINE, so perl's location suffix is `-e line 19` — NOT the one-line
/// program's `line 1` and not the sidecar PUBLISH script's `line 16` — and the
/// status is the errno the failed `flock` left in `$!`.
#[test]
fn sidecar_removal_contention_cli_is_byte_identical_to_the_script() {
    let h = Harness::new();
    let expected = "lock-record";
    h.reset_root();
    std::fs::create_dir_all(h.remote_path("state")).unwrap();
    std::fs::write(h.remote_path("state/operation.lock"), expected).unwrap();
    let logged = log_remove_file_if_sidecar(&h, expected);

    h.reset_root();
    std::fs::create_dir_all(h.remote_path("state")).unwrap();
    std::fs::write(h.remote_path("state/operation.lock"), expected).unwrap();
    std::fs::write(h.remote_path("state/operation.lock.mutex"), b"").unwrap();
    let held = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(h.remote_path("state/operation.lock.mutex"))
        .expect("open the sidecar to hold its lock");
    // SAFETY: `held` is a descriptor this test owns for the whole call, and
    // `flock` neither retains nor closes it.
    let locked = unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    assert_eq!(
        locked,
        0,
        "the test must hold the sidecar lock: {}",
        std::io::Error::last_os_error()
    );

    let script = run_bounded(sh(&logged), 60);
    let cli_out = run_bounded(
        cli(&[
            OsStr::new("remove-file-if-sidecar"),
            h.cli_path("state/operation.lock.mutex").as_os_str(),
            h.cli_path("state/operation.lock").as_os_str(),
            OsStr::new(expected),
        ]),
        60,
    );
    drop(held);

    assert_parity("sidecar removal contended", &script, &cli_out);
    assert_eq!(
        script.stderr,
        b"sidecar contended at -e line 19.\n".to_vec(),
        "the script's own words: the message plus perl's location suffix"
    );
    assert_eq!(
        script.status.code(),
        Some(libc::EAGAIN),
        "perl's `die` exits with `$!` (the failed `flock`'s errno)"
    );
    assert!(
        script.stdout.is_empty(),
        "a contended helper prints no verdict frame"
    );
    assert_eq!(
        std::fs::read(h.remote_path("state/operation.lock")).unwrap(),
        expected.as_bytes(),
        "a contended helper must leave the record alone"
    );
}

// -- transport level: CLI first, script fallback, ONE probe -------------------

/// The fixture the transport-level tests drive: a tree to fsync, a pair of
/// rename endpoints, a claimable record, and the layout's own lock record.
fn transport_fixture(h: &Harness) {
    h.reset_root();
    std::fs::create_dir_all(h.remote_path("bundle/sub")).unwrap();
    std::fs::write(h.remote_path("bundle/sub/file"), b"x").unwrap();
    std::fs::write(h.remote_path("bundle/from"), b"NEW").unwrap();
    std::fs::create_dir_all(h.remote_path("staged")).unwrap();
    std::fs::write(h.remote_path("staged/record"), b"generation-1").unwrap();
    std::fs::create_dir_all(h.remote_path("state")).unwrap();
    std::fs::write(h.remote_path("state/operation.lock"), b"lock-record").unwrap();
}

/// With the CLI present every op runs the CLI form — no `perl`, no `find` — and
/// the transport probes exactly ONCE.
#[test]
fn fsync_ops_use_the_cli_when_present() {
    let h = Harness::new();
    h.expose_real_cli();
    transport_fixture(&h);
    let t = h.transport();

    Remote::fsync_tree(&t, &rooted("bundle")).expect("the CLI walk must succeed");
    Remote::fsync_parent(&t, &rooted("bundle/sub/file"))
        .expect("the CLI parent fsync must succeed");
    Remote::rename(&t, &rooted("bundle/from"), &rooted("bundle/to"))
        .expect("the CLI rename must succeed");
    Remote::rename_aside(&t, &rooted("bundle/to"), &rooted("bundle/aside"))
        .expect("the CLI aside rename must succeed");
    let verdict = Remote::remove_file_if(&t, &rooted("staged/record"), b"generation-1")
        .expect("the CLI compare-and-delete must answer");
    assert_eq!(verdict, RemoveIfVerdict::Removed);
    let verdict = Remote::remove_file_if(&t, &rooted("state/operation.lock"), b"lock-record")
        .expect("the CLI sidecar compare-and-delete must answer");
    assert_eq!(verdict, RemoveIfVerdict::Removed);

    assert_eq!(
        std::fs::read(h.remote_path("bundle/aside")).unwrap(),
        b"NEW"
    );
    assert!(!h.remote_path("staged/record").exists());
    assert!(!h.remote_path("state/operation.lock").exists());

    let log = h.log();
    assert_eq!(log.len(), 7, "probe + the six CLI ops, got {log:?}");
    assert_eq!(log[0], "bash -c 'storekit-remote protocol'");

    let bundle = h.remote_path("bundle").to_string_lossy().into_owned();
    assert_eq!(
        log[1],
        expected_cli_command(&["storekit-remote", "fsync-tree", &bundle])
    );
    let parent = h.remote_path("bundle/sub").to_string_lossy().into_owned();
    assert_eq!(
        log[2],
        expected_cli_command(&["storekit-remote", "fsync-dir", &parent])
    );
    let from = h.remote_path("bundle/from").to_string_lossy().into_owned();
    let to = h.remote_path("bundle/to").to_string_lossy().into_owned();
    assert_eq!(
        log[3],
        expected_cli_command(&["storekit-remote", "rename", &from, &to])
    );
    let aside = h.remote_path("bundle/aside").to_string_lossy().into_owned();
    assert_eq!(
        log[4],
        expected_cli_command(&["storekit-remote", "rename", &to, &aside])
    );
    let record = h
        .remote_path("staged/record")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        log[5],
        expected_cli_command(&["storekit-remote", "remove-file-if", &record, "generation-1"])
    );
    let sidecar = h
        .remote_path("state/operation.lock.mutex")
        .to_string_lossy()
        .into_owned();
    let lock = h
        .remote_path("state/operation.lock")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        log[6],
        expected_cli_command(&[
            "storekit-remote",
            "remove-file-if-sidecar",
            &sidecar,
            &lock,
            "lock-record"
        ])
    );

    assert!(
        !log.iter()
            .any(|line| line.contains("perl") || line.contains("find")),
        "a compatible CLI must not fall back to perl or find: {log:?}"
    );
    for (index, command) in log[1..].iter().enumerate() {
        assert!(
            is_cli_form(command),
            "op {index} must invoke the CLI: {command:?}"
        );
    }
    assert_eq!(
        log.iter()
            .filter(|line| line.contains("storekit-remote protocol"))
            .count(),
        1,
        "exactly ONE probe per transport: {log:?}"
    );

    // A second op reads the CACHED decision: no second probe.
    std::fs::write(h.remote_path("bundle/sub/file2"), b"y").unwrap();
    Remote::fsync_parent(&t, &rooted("bundle/sub/file2")).expect("the second CLI fsync must work");
    let log = h.log();
    assert_eq!(log.len(), 8, "the second op must not probe: {log:?}");
    assert_eq!(
        log.iter()
            .filter(|line| line.contains("storekit-remote protocol"))
            .count(),
        1,
        "exactly ONE probe per transport: {log:?}"
    );
}

/// With no `storekit-remote` on the far side every op falls back to its existing
/// script form, and the transport still probes exactly ONCE.
#[test]
fn fsync_ops_fall_back_to_the_script_when_absent() {
    let h = Harness::new();
    transport_fixture(&h);
    let t = h.transport();

    Remote::fsync_tree(&t, &rooted("bundle")).expect("the script walk must succeed");
    Remote::fsync_parent(&t, &rooted("bundle/sub/file"))
        .expect("the script parent fsync must succeed");
    Remote::rename(&t, &rooted("bundle/from"), &rooted("bundle/to"))
        .expect("the script rename must succeed");
    Remote::rename_aside(&t, &rooted("bundle/to"), &rooted("bundle/aside"))
        .expect("the script aside rename must succeed");
    let verdict = Remote::remove_file_if(&t, &rooted("staged/record"), b"generation-1")
        .expect("the script compare-and-delete must answer");
    assert_eq!(verdict, RemoveIfVerdict::Removed);
    let verdict = Remote::remove_file_if(&t, &rooted("state/operation.lock"), b"lock-record")
        .expect("the script sidecar compare-and-delete must answer");
    assert_eq!(verdict, RemoveIfVerdict::Removed);

    let log = h.log();
    assert_eq!(log.len(), 7, "probe + the six script ops, got {log:?}");
    assert_eq!(log[0], "bash -c 'storekit-remote protocol'");
    for (index, command) in log[1..].iter().enumerate() {
        assert!(
            command.starts_with("bash -c '"),
            "op {index} must be a `bash -c` command: {command:?}"
        );
        assert!(
            !is_cli_form(command),
            "the absent CLI must not be invoked: {command:?}"
        );
    }
    // The two fsync forms and the rename/claim/sidecar chains, by their own
    // first step: `find` for the walk, `perl -e` for a single fsync, and
    // `mkdir -p` for the three mutating chains.
    assert!(log[1].contains("find"), "fsync-tree: {:?}", log[1]);
    assert!(log[2].contains("perl -e"), "fsync-dir: {:?}", log[2]);
    for command in &log[3..] {
        assert!(
            command.contains("mkdir -p"),
            "the mutating chains create the parent first: {command:?}"
        );
    }
    assert!(
        log[5].contains("mktemp") && log[5].contains("cmp"),
        "remove-file-if claims and compares: {:?}",
        log[5]
    );
    assert!(
        log[6].contains("flock") && log[6].contains("unlink"),
        "remove-file-if-sidecar locks and unlinks: {:?}",
        log[6]
    );
    assert_eq!(
        log.iter()
            .filter(|line| line.contains("storekit-remote protocol"))
            .count(),
        1,
        "exactly ONE probe per transport: {log:?}"
    );
}
