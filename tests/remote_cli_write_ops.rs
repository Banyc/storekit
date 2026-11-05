//! The far-side CLI write operations (`remote-cli-write-ops`): `publish`
//! (atomic upload), `write-new` (no-clobber install) and `sidecar-publish`
//! (the lock-record install under the sidecar's `flock`) run through
//! `storekit-remote` when the far side has a compatible one, and fall back to
//! the existing perl scripts when it does not.
//!
//! TWO KINDS OF TEST, both in this file:
//!
//! * DIFFERENTIAL tests run the EXACT logged script and the CLI on the same
//!   fixture and compare stdout, stderr and the exit status, plus the FINAL
//!   tree (bytes and raw modes). The script is never re-typed here: it is the
//!   command the transport itself logged while the CLI was absent.
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
//! ONE NORMALIZATION, stated rather than hidden. The truncation diagnostic
//! names the writer's TEMP, and the temp is a fresh random/unique name on every
//! run (`mktemp` in the script, a process-scoped unique name in the CLI), so
//! the same script run TWICE does not produce the same bytes either. The
//! differential comparison therefore replaces the temp path token between
//! `storekit: remote payload truncated: ` and ` holds ` with `<tmp>` on BOTH
//! sides, and separately asserts the CLI's exact message format and path.
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

use storekit::TransportKind;
use storekit::env::SysEnv;
use storekit::transport::{CreateNewVerdict, Layout, Remote, RootedRelativePath, SshTransport};

/// The environment variable the shim reads to find its "remote working
/// directory" (the directory a remote login shell would start in).
const SHIM_WORK_VAR: &str = "STOREKIT_SSH_SHIM_WORK";
/// The environment variable the shim appends each remote command to.
const SHIM_LOG_VAR: &str = "STOREKIT_SSH_SHIM_LOG";

/// The transport's immutable-record mode (`IMMUTABLE_RECORD_MODE`), which both
/// the `write-new` and `sidecar-publish` call sites use. Re-spelled here
/// because the constant is `pub(crate)`; the logged command is the authority
/// the tests compare against.
const RECORD_MODE: u32 = 0o644;

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
            .prefix("storekit-remote-wrt-")
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

/// Run `command` under `/bin/sh -c` (the exact form the ssh argv carries) with
/// `payload` on stdin, under a DEADLINE so a blocking writer is a failed test
/// rather than a hang.
fn run_sh_stdin(command: &str, payload: &[u8], secs: u64) -> Output {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg(command);
    run_bounded_stdin(cmd, payload, secs)
}

/// Run `cmd` with `payload` on stdin and a DEADLINE.
fn run_bounded_stdin(mut cmd: Command, payload: &[u8], secs: u64) -> Output {
    use std::io::Write;

    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the command under test");
    {
        let mut stdin = child.stdin.take().expect("the command's piped stdin");
        if let Err(error) = stdin.write_all(payload) {
            // A writer that refuses stdin early is a legitimate outcome; its
            // exit status and stderr carry the verdict.
            assert_eq!(
                error.kind(),
                std::io::ErrorKind::BrokenPipe,
                "writing the payload to {}: {error}",
                std::thread::current().name().unwrap_or("<unknown test>")
            );
        }
    }
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

/// Replace the (inherently unique) temp-path token of the truncation
/// diagnostic with `<tmp>` on both sides. Nothing else is touched.
fn normalize_temp_token(mut stderr: Vec<u8>) -> Vec<u8> {
    const PREFIX: &[u8] = b"storekit: remote payload truncated: ";
    const INFIX: &[u8] = b" holds ";
    let Some(start) = find(&stderr, PREFIX) else {
        return stderr;
    };
    let from = start + PREFIX.len();
    let Some(offset) = find(&stderr[from..], INFIX) else {
        return stderr;
    };
    let to = from + offset;
    stderr.splice(from..to, b"<tmp>".iter().copied());
    stderr
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Assert the logged script and the CLI produce the SAME stdout bytes, the SAME
/// stderr bytes (temp token normalized, see the module doc), and the SAME exit
/// status.
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
        normalize_temp_token(cli.stderr.clone()),
        normalize_temp_token(script.stderr.clone()),
        "{case}: stderr bytes must be identical (temp token normalized)"
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

/// The tranport's public entry point for the publish differential: drive
/// `Remote::write` (which LOGS and runs the upload command) and return the
/// logged command. The run's own outcome is not the subject; the fixture is
/// reset before the comparison.
fn log_publish_script(h: &Harness, rel: &str, payload: &[u8], mode: u32) -> String {
    let t = h.transport();
    let _ = Remote::write(&t, &rooted(rel), payload, mode);
    let command = h
        .log()
        .pop()
        .expect("the shim must have logged the publish command");
    assert!(
        command.starts_with("bash -c 'mkdir -p "),
        "the absent CLI must log the perl upload script, got {command:?}"
    );
    command
}

/// One `publish` differential case: `setup` places the fixture state, the
/// script and the CLI each run against the SAME freshly reset state, and the
/// outputs plus the final tree are compared.
fn publish_case<F: Fn()>(
    h: &Harness,
    case: &str,
    rel: &str,
    payload: &[u8],
    mode: u32,
    setup: F,
) -> (Output, Output) {
    setup();
    let command = log_publish_script(h, rel, payload, mode);
    setup();
    let script = run_sh_stdin(&command, payload, 60);
    let script_state = snapshot(&h.root);
    setup();
    let path = h.remote_path(rel);
    let len = payload.len().to_string();
    let mode_token = format!("{:o}", mode & 0o7777);
    let cli_out = run_bounded_stdin(
        cli(&[
            OsStr::new("publish"),
            path.as_os_str(),
            OsStr::new(&mode_token),
            OsStr::new(&len),
        ]),
        payload,
        60,
    );
    let cli_state = snapshot(&h.root);
    assert_parity(case, &script, &cli_out);
    assert_eq!(
        cli_state, script_state,
        "{case}: the final tree (bytes and raw modes) must match the script's"
    );
    assert_no_temp_residue(case, &cli_state);
    (script, cli_out)
}

/// The `publish` differential over the cases the contract names: a new file,
/// replacing a regular file, replacing a symlink, a directory destination, a
/// truncated stdin, a missing parent (created), and the mode permission bits
/// (including mode 0 = keep the temp's private mode).
#[test]
fn publish_cli_is_byte_identical_to_the_script() {
    let h = Harness::new();
    let long = b"NEW-CONTENT";

    // A NEW file, with an explicit mode.
    let (_, cli_out) = publish_case(&h, "new file", "state/new", long, 0o644, || {
        h.reset_root();
        std::fs::create_dir_all(h.remote_path("state")).unwrap();
    });
    assert_eq!(cli_out.status.code(), Some(0));
    assert!(cli_out.stdout.is_empty() && cli_out.stderr.is_empty());
    assert_eq!(std::fs::read(h.remote_path("state/new")).unwrap(), long);
    assert_eq!(
        std::fs::metadata(h.remote_path("state/new"))
            .unwrap()
            .mode()
            & 0o7777,
        0o644
    );

    // REPLACE a regular file: the old bytes must be gone, the new ones visible.
    let (_, cli_out) = publish_case(&h, "replace a regular file", "state/f", long, 0o640, || {
        h.reset_root();
        std::fs::create_dir_all(h.remote_path("state")).unwrap();
        std::fs::write(h.remote_path("state/f"), b"OLD-CONTENT").unwrap();
        std::fs::set_permissions(
            h.remote_path("state/f"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
    });
    assert_eq!(cli_out.status.code(), Some(0));
    assert_eq!(std::fs::read(h.remote_path("state/f")).unwrap(), long);
    assert_eq!(
        std::fs::metadata(h.remote_path("state/f")).unwrap().mode() & 0o7777,
        0o640,
        "the published inode must carry the caller's mode"
    );

    // REPLACE a SYMLINK: the rename replaces the LINK entry (never follows it),
    // and the link's target is untouched.
    let (_, cli_out) = publish_case(&h, "replace a symlink", "state/link", long, 0o644, || {
        h.reset_root();
        std::fs::create_dir_all(h.remote_path("state")).unwrap();
        std::fs::write(h.remote_path("state/target"), b"TARGET").unwrap();
        std::os::unix::fs::symlink("target", h.remote_path("state/link")).unwrap();
    });
    assert_eq!(cli_out.status.code(), Some(0));
    assert!(
        std::fs::symlink_metadata(h.remote_path("state/link"))
            .unwrap()
            .file_type()
            .is_file(),
        "the published entry must be a regular file, not the old symlink"
    );
    assert_eq!(std::fs::read(h.remote_path("state/link")).unwrap(), long);
    assert_eq!(
        std::fs::read(h.remote_path("state/target")).unwrap(),
        b"TARGET",
        "the symlink's target must never be followed or modified"
    );

    // A DIRECTORY destination REFUSES: exit 1, SILENT (the script's rename
    // one-liner prints nothing), the directory and its child intact.
    let (_, cli_out) = publish_case(
        &h,
        "directory destination",
        "state/dir",
        long,
        0o644,
        || {
            h.reset_root();
            std::fs::create_dir_all(h.remote_path("state/dir")).unwrap();
            std::fs::write(h.remote_path("state/dir/child"), b"CHILD").unwrap();
        },
    );
    assert_eq!(
        cli_out.status.code(),
        Some(1),
        "a directory destination must refuse"
    );
    assert!(
        cli_out.stderr.is_empty(),
        "the rename refusal must be silent like the script, got {:?}",
        String::from_utf8_lossy(&cli_out.stderr)
    );
    assert!(
        std::fs::metadata(h.remote_path("state/dir"))
            .unwrap()
            .is_dir()
    );
    assert_eq!(
        std::fs::read(h.remote_path("state/dir/child")).unwrap(),
        b"CHILD"
    );

    // A TRUNCATED stdin is refused BETWEEN STDIN EOF and the publish: exit 1,
    // the script's exact diagnostic, and the previous content intact.
    let short = b"NEW";
    h.reset_root();
    std::fs::create_dir_all(h.remote_path("state")).unwrap();
    std::fs::write(h.remote_path("state/f"), b"OLD-CONTENT").unwrap();
    let command = log_publish_script(&h, "state/f", b"NEW-CONTENT", 0o644);
    h.reset_root();
    std::fs::create_dir_all(h.remote_path("state")).unwrap();
    std::fs::write(h.remote_path("state/f"), b"OLD-CONTENT").unwrap();
    let script_short = run_sh_stdin(&command, short, 60);
    let script_state = snapshot(&h.root);
    h.reset_root();
    std::fs::create_dir_all(h.remote_path("state")).unwrap();
    std::fs::write(h.remote_path("state/f"), b"OLD-CONTENT").unwrap();
    let path = h.remote_path("state/f");
    let cli_short = run_bounded_stdin(
        cli(&[
            OsStr::new("publish"),
            path.as_os_str(),
            OsStr::new("644"),
            OsStr::new("11"),
        ]),
        short,
        60,
    );
    let cli_state = snapshot(&h.root);
    assert_parity("truncated stdin", &script_short, &cli_short);
    assert_eq!(cli_state, script_state);
    assert_no_temp_residue("truncated stdin", &cli_state);
    assert_eq!(
        cli_short.status.code(),
        Some(1),
        "a truncated payload must not be published"
    );
    assert_eq!(
        normalize_temp_token(cli_short.stderr.clone()),
        b"storekit: remote payload truncated: <tmp> holds 3 bytes, expected 11; refusing to publish\n"
            .to_vec(),
        "the CLI must print the script's exact truncation diagnostic: {:?}",
        String::from_utf8_lossy(&cli_short.stderr)
    );
    // ... and the named path is the temp, in the destination's OWN directory.
    let prefix = b"storekit: remote payload truncated: ";
    let rest = &cli_short.stderr[prefix.len()..];
    let end = find(rest, b" holds ").expect("the diagnostic names the held size");
    let temp_path = Path::new(OsStr::from_bytes(&rest[..end]));
    assert_eq!(
        temp_path.parent(),
        Some(h.remote_path("state").as_path()),
        "the temp must be in the destination's own directory"
    );
    assert_eq!(
        std::fs::read(h.remote_path("state/f")).unwrap(),
        b"OLD-CONTENT",
        "a refused truncated publish must leave the previous content intact"
    );

    // A MISSING PARENT is created.
    let (_, cli_out) = publish_case(&h, "missing parent", "deep/sub/file", long, 0o600, || {
        h.reset_root();
    });
    assert_eq!(cli_out.status.code(), Some(0));
    assert_eq!(std::fs::read(h.remote_path("deep/sub/file")).unwrap(), long);
    assert_eq!(
        std::fs::metadata(h.remote_path("deep/sub/file"))
            .unwrap()
            .mode()
            & 0o7777,
        0o600
    );

    // MODE 0 means NO chmod: the temp's private mode survives.
    let (_, cli_out) = publish_case(
        &h,
        "mode 0 keeps the private mode",
        "state/m",
        long,
        0,
        || {
            h.reset_root();
            std::fs::create_dir_all(h.remote_path("state")).unwrap();
        },
    );
    assert_eq!(cli_out.status.code(), Some(0));
    assert_eq!(
        std::fs::metadata(h.remote_path("state/m")).unwrap().mode() & 0o7777,
        0o600,
        "mode 0 must leave the temp's private 0o600 mode"
    );
}

/// The transport's `write-new` script for `rel`, logged with the CLI absent.
/// `Remote::try_write_new` may run the descriptor-bound verify after a conflict,
/// so the script is found by its leading `mkdir -p` (the operation that ran
/// before the verify).
fn log_write_new_script(h: &Harness, rel: &str, payload: &[u8]) -> String {
    let t = h.transport();
    let _ = Remote::try_write_new(&t, &rooted(rel), payload);
    h.log()
        .into_iter()
        .rev()
        .find(|line| line.starts_with("bash -c 'mkdir -p "))
        .unwrap_or_else(|| panic!("the write-new script was never logged"))
}

/// One `write-new` differential case (the temp token is normalized only for the
/// truncation diagnostic).
fn write_new_case<F: Fn()>(h: &Harness, case: &str, rel: &str, payload: &[u8], setup: F) -> Output {
    setup();
    let command = log_write_new_script(h, rel, payload);
    setup();
    let script = run_sh_stdin(&command, payload, 60);
    let script_state = snapshot(&h.root);
    setup();
    let path = h.remote_path(rel);
    let len = payload.len().to_string();
    let mode = format!("{:o}", RECORD_MODE);
    let cli_out = run_bounded_stdin(
        cli(&[
            OsStr::new("write-new"),
            path.as_os_str(),
            OsStr::new(&mode),
            OsStr::new(&len),
        ]),
        payload,
        60,
    );
    let cli_state = snapshot(&h.root);
    assert_parity(case, &script, &cli_out);
    assert_eq!(
        cli_state, script_state,
        "{case}: the final tree (bytes and raw modes) must match the script's"
    );
    assert_no_temp_residue(case, &cli_state);
    cli_out
}

/// The `write-new` differential: a new record, a conflict with a regular file,
/// a conflict with a symlink, a directory destination, a truncated stdin, and a
/// missing parent (created). The install is `link(2)`, so an existing entry is
/// NEVER replaced.
#[test]
fn write_new_cli_is_byte_identical_to_the_script() {
    let h = Harness::new();
    let payload = b"gen-0001";

    // NEW record: installed with the record mode.
    let cli_out = write_new_case(&h, "new record", "state/new.json", payload, || {
        h.reset_root();
    });
    assert_eq!(cli_out.status.code(), Some(0));
    assert!(cli_out.stdout.is_empty() && cli_out.stderr.is_empty());
    assert_eq!(
        std::fs::read(h.remote_path("state/new.json")).unwrap(),
        payload
    );
    assert_eq!(
        std::fs::metadata(h.remote_path("state/new.json"))
            .unwrap()
            .mode()
            & 0o7777,
        RECORD_MODE
    );

    // CONFLICT with a regular file: exit 17, the winner's bytes intact.
    let cli_out = write_new_case(
        &h,
        "conflict: regular file",
        "state/r.json",
        payload,
        || {
            h.reset_root();
            std::fs::create_dir_all(h.remote_path("state")).unwrap();
            std::fs::write(h.remote_path("state/r.json"), b"WINNER").unwrap();
        },
    );
    assert_eq!(cli_out.status.code(), Some(17));
    assert_eq!(
        std::fs::read(h.remote_path("state/r.json")).unwrap(),
        b"WINNER"
    );

    // CONFLICT with a symlink: exit 17, the LINK stays a link (never followed,
    // never replaced).
    let cli_out = write_new_case(&h, "conflict: symlink", "state/s.json", payload, || {
        h.reset_root();
        std::fs::create_dir_all(h.remote_path("state")).unwrap();
        std::fs::write(h.remote_path("state/target"), b"TARGET").unwrap();
        std::os::unix::fs::symlink("target", h.remote_path("state/s.json")).unwrap();
    });
    assert_eq!(cli_out.status.code(), Some(17));
    assert!(
        std::fs::symlink_metadata(h.remote_path("state/s.json"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read(h.remote_path("state/target")).unwrap(),
        b"TARGET"
    );

    // CONFLICT with a DIRECTORY destination: exit 17, never descended into.
    let cli_out = write_new_case(&h, "conflict: directory", "state/d.json", payload, || {
        h.reset_root();
        std::fs::create_dir_all(h.remote_path("state/d.json")).unwrap();
        std::fs::write(h.remote_path("state/d.json/child"), b"CHILD").unwrap();
    });
    assert_eq!(cli_out.status.code(), Some(17));
    assert!(
        std::fs::metadata(h.remote_path("state/d.json"))
            .unwrap()
            .is_dir()
    );
    assert_eq!(
        std::fs::read(h.remote_path("state/d.json/child")).unwrap(),
        b"CHILD"
    );

    // TRUNCATED stdin: exit 1, nothing installed, the exact diagnostic.
    let short = b"gen";
    h.reset_root();
    let command = log_write_new_script(&h, "state/t.json", payload);
    h.reset_root();
    let script_short = run_sh_stdin(&command, short, 60);
    let script_state = snapshot(&h.root);
    h.reset_root();
    let path = h.remote_path("state/t.json");
    let cli_short = run_bounded_stdin(
        cli(&[
            OsStr::new("write-new"),
            path.as_os_str(),
            OsStr::new("644"),
            OsStr::new(&payload.len().to_string()),
        ]),
        short,
        60,
    );
    let cli_state = snapshot(&h.root);
    assert_parity("truncated stdin", &script_short, &cli_short);
    assert_eq!(cli_state, script_state);
    assert_no_temp_residue("truncated stdin", &cli_state);
    assert_eq!(cli_short.status.code(), Some(1));
    let truncated = normalize_temp_token(cli_short.stderr.clone());
    assert!(
        truncated.starts_with(b"storekit: remote payload truncated: <tmp> holds 3 bytes, expected 8; refusing to publish\n"),
        "the CLI must print the script's exact truncation diagnostic, got {:?}",
        String::from_utf8_lossy(&cli_short.stderr)
    );
    assert!(
        !h.remote_path("state/t.json").exists(),
        "a truncated record must not be installed"
    );

    // MISSING PARENT is created.
    let cli_out = write_new_case(&h, "missing parent", "a/b/c.json", payload, || {
        h.reset_root();
    });
    assert_eq!(cli_out.status.code(), Some(0));
    assert_eq!(std::fs::read(h.remote_path("a/b/c.json")).unwrap(), payload);
}

/// The `sidecar-publish` script for the owned lock record, logged with the CLI
/// absent. The sidecar command is the one that opens the `<lock>.mutex`
/// sidecar; a conflict may add a descriptor-bound verify afterward.
fn log_sidecar_script(h: &Harness, payload: &[u8]) -> String {
    let t = h.transport();
    let _ = Remote::try_write_new(&t, &rooted("state/operation.lock"), payload);
    let log = h.log();
    log.iter()
        .rev()
        .find(|line| line.contains("operation.lock.mutex"))
        .cloned()
        .unwrap_or_else(|| panic!("the sidecar script was never logged: {log:?}"))
}

/// One `sidecar-publish` differential case. The destination is the owned lock
/// record (the only path the transport routes through the sidecar).
fn sidecar_case<F: Fn()>(h: &Harness, case: &str, payload: &[u8], setup: F) -> Output {
    setup();
    let command = log_sidecar_script(h, payload);
    setup();
    let script = run_sh_stdin(&command, payload, 60);
    let script_state = snapshot(&h.root);
    setup();
    let sidecar = h.remote_path("state/operation.lock.mutex");
    let lock = h.remote_path("state/operation.lock");
    let mode = format!("{:o}", RECORD_MODE);
    let len = payload.len().to_string();
    let cli_out = run_bounded_stdin(
        cli(&[
            OsStr::new("sidecar-publish"),
            sidecar.as_os_str(),
            lock.as_os_str(),
            OsStr::new(&mode),
            OsStr::new(&len),
        ]),
        payload,
        60,
    );
    let cli_state = snapshot(&h.root);
    assert_parity(case, &script, &cli_out);
    assert_eq!(
        cli_state, script_state,
        "{case}: the final tree (bytes and raw modes) must match the script's"
    );
    assert_no_temp_residue(case, &cli_state);
    cli_out
}

/// The `sidecar-publish` differential: a new record, conflicts with a regular
/// file / symlink / directory, a truncated stdin, and a missing parent. The
/// sidecar file EXISTS FIRST (the script touches it before reading stdin).
#[test]
fn sidecar_publish_cli_is_byte_identical_to_the_script() {
    let h = Harness::new();
    let payload = b"record-payload";

    let cli_out = sidecar_case(&h, "new record", payload, || {
        h.reset_root();
    });
    assert_eq!(cli_out.status.code(), Some(0));
    assert!(cli_out.stdout.is_empty() && cli_out.stderr.is_empty());
    assert_eq!(
        std::fs::read(h.remote_path("state/operation.lock")).unwrap(),
        payload
    );
    assert_eq!(
        std::fs::metadata(h.remote_path("state/operation.lock"))
            .unwrap()
            .mode()
            & 0o7777,
        RECORD_MODE
    );
    assert_eq!(
        std::fs::metadata(h.remote_path("state/operation.lock.mutex"))
            .unwrap()
            .mode()
            & 0o7777,
        0o644,
        "the sidecar file must exist with the script's 0o644 mode"
    );

    let cli_out = sidecar_case(&h, "conflict: regular file", payload, || {
        h.reset_root();
        std::fs::create_dir_all(h.remote_path("state")).unwrap();
        std::fs::write(h.remote_path("state/operation.lock"), b"WINNER").unwrap();
    });
    assert_eq!(cli_out.status.code(), Some(17));
    assert_eq!(
        std::fs::read(h.remote_path("state/operation.lock")).unwrap(),
        b"WINNER"
    );

    let cli_out = sidecar_case(&h, "conflict: symlink", payload, || {
        h.reset_root();
        std::fs::create_dir_all(h.remote_path("state")).unwrap();
        std::fs::write(h.remote_path("state/target"), b"TARGET").unwrap();
        std::os::unix::fs::symlink("target", h.remote_path("state/operation.lock")).unwrap();
    });
    assert_eq!(cli_out.status.code(), Some(17));
    assert!(
        std::fs::symlink_metadata(h.remote_path("state/operation.lock"))
            .unwrap()
            .file_type()
            .is_symlink()
    );

    let cli_out = sidecar_case(&h, "conflict: directory", payload, || {
        h.reset_root();
        std::fs::create_dir_all(h.remote_path("state/operation.lock")).unwrap();
        std::fs::write(h.remote_path("state/operation.lock/child"), b"CHILD").unwrap();
    });
    assert_eq!(cli_out.status.code(), Some(17));
    assert_eq!(
        std::fs::read(h.remote_path("state/operation.lock/child")).unwrap(),
        b"CHILD"
    );

    // TRUNCATED stdin: exit 1, the sidecar script's (path-free) diagnostic, the
    // sidecar still created, and nothing installed.
    let short = b"rec";
    h.reset_root();
    let command = log_sidecar_script(&h, payload);
    h.reset_root();
    let script_short = run_sh_stdin(&command, short, 60);
    let script_state = snapshot(&h.root);
    h.reset_root();
    let sidecar = h.remote_path("state/operation.lock.mutex");
    let lock = h.remote_path("state/operation.lock");
    let cli_short = run_bounded_stdin(
        cli(&[
            OsStr::new("sidecar-publish"),
            sidecar.as_os_str(),
            lock.as_os_str(),
            OsStr::new("644"),
            OsStr::new(&payload.len().to_string()),
        ]),
        short,
        60,
    );
    let cli_state = snapshot(&h.root);
    assert_parity("truncated stdin", &script_short, &cli_short);
    assert_eq!(cli_state, script_state);
    assert_no_temp_residue("truncated stdin", &cli_state);
    assert_eq!(cli_short.status.code(), Some(1));
    assert_eq!(
        cli_short.stderr,
        b"storekit: sidecar payload truncated: received 3 bytes, expected 14; refusing to publish\n"
            .to_vec(),
        "the CLI must print the sidecar script's exact (path-free) diagnostic"
    );
    assert!(
        h.remote_path("state/operation.lock.mutex").is_file(),
        "the sidecar file must exist first, even when the payload is refused"
    );
    assert!(
        !h.remote_path("state/operation.lock").exists(),
        "a truncated record must not be installed"
    );

    let cli_out = sidecar_case(&h, "missing parent", payload, || {
        h.reset_root();
    });
    assert_eq!(cli_out.status.code(), Some(0));
    assert_eq!(
        std::fs::read(h.remote_path("state/operation.lock")).unwrap(),
        payload
    );
}

/// The CONTENDED sidecar: THIS process holds the sidecar's exclusive `flock`
/// for the whole comparison, so both writers exhaust the deadline and take the
/// perl script's `die "sidecar contended"` path. Perl prints the message with
/// its ` at -e line 16.` location suffix — the prelude is multi-line, so this
/// is NOT the one-line program's `line 1` — and exits with the errno the failed
/// `flock` left in `$!` (EAGAIN/EWOULDBLOCK: 11 on Linux, 35 on macOS). The CLI
/// must reproduce both, byte for byte.
#[test]
fn sidecar_contention_cli_is_byte_identical_to_the_script() {
    let h = Harness::new();
    let payload = b"record-payload";

    // Log the script FIRST: the transport RUNS it, and it must not contend with
    // the lock this test is about to take.
    let command = log_sidecar_script(&h, payload);

    // The fixture both writers start from: the sidecar file exists and this
    // process holds its exclusive lock until the end of the test. The script's
    // `touch` and the CLI's create-open both leave an EXISTING file's inode (and
    // so its lock) alone, so the lock outlives both runs.
    h.reset_root();
    std::fs::create_dir_all(h.remote_path("state")).unwrap();
    let sidecar_path = h.remote_path("state/operation.lock.mutex");
    std::fs::write(&sidecar_path, b"").unwrap();
    let held = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&sidecar_path)
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

    let script = run_sh_stdin(&command, payload, 60);
    let lock_path = h.remote_path("state/operation.lock");
    let cli_out = run_bounded_stdin(
        cli(&[
            OsStr::new("sidecar-publish"),
            sidecar_path.as_os_str(),
            lock_path.as_os_str(),
            OsStr::new(&format!("{:o}", RECORD_MODE)),
            OsStr::new(&payload.len().to_string()),
        ]),
        payload,
        60,
    );
    assert_parity("contended sidecar", &script, &cli_out);
    assert_eq!(
        script.stderr,
        b"sidecar contended at -e line 16.\n".to_vec(),
        "the script's own words: the message plus perl's location suffix"
    );
    assert_eq!(
        script.status.code(),
        Some(libc::EAGAIN),
        "perl's `die` exits with `$!` (the failed `flock`'s errno)"
    );
    assert!(
        !lock_path.exists(),
        "a contended sidecar must install nothing"
    );
    assert_no_temp_residue("contended sidecar", &snapshot(&h.root));
}

// The two ADJACENT sidecar `die` diagnostics have no differential fixture, so
// their bytes are covered only by the CLI's construction (and by the script's
// own text, which the contention case pins):
//
// * `sidecar flock failed: <err>` needs a `flock` errno other than
//   `EINTR`/`EAGAIN`/`EWOULDBLOCK`. `flock(2)` can otherwise only report
//   `EBADF` (impossible on the descriptor just opened), `EINVAL`, or `ENOLCK`
//   (kernel lock-table exhaustion) — none of them constructible from a
//   fixture.
// * `open sidecar <path>: <err>` needs the read+write open to fail AFTER the
//   CLI's `mkdir -p`/create-open/`chmod 644` steps, all of which the script
//   performs with the same shell commands. Every path whose read+write open
//   fails (a directory, an unreadable file) already fails the CLI's preceding
//   create-OPEN-FOR-WRITE (a directory is `EISDIR` there, a `0o444` file
//   `EACCES`), and every path that passes it is, after the `0o644` `chmod`,
//   readable and writable by its owner, so the open cannot then fail. A
//   directory sidecar is not a substitute: on macOS perl's `open "+<"` on a
//   directory SUCCEEDS (the run reaches the payload check instead).

/// The exit-74 path: ONLY the parent-directory fsync fails, so the rename has
/// already committed and the new content IS visible.
///
/// The fault is a REAL filesystem property, not a test seam: the parent
/// directory is chmodded to `0o300` (write + search, NO read) AFTER the fixture
/// is created and BEFORE the writer runs. `mktemp`/`O_EXCL` creation, `chmod`,
/// the file fsync and `rename(2)` all need only write+search on the directory,
/// so they succeed; opening the directory `O_RDONLY` for the parent fsync then
/// fails EACCES. The perl primitive dies with `fsync-open <dir>: Permission
/// denied` (status = errno) and the upload script maps that to 74. The CLI
/// reproduces the same message and the same 74.
#[test]
fn publish_exits_74_when_only_the_parent_fsync_fails() {
    let h = Harness::new();
    let state = h.remote_path("state");

    // Log the script on a NORMAL directory, then fault the parent for both runs.
    h.reset_root();
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("f"), b"OLD-CONTENT").unwrap();
    let command = log_publish_script(&h, "state/f", b"NEW-CONTENT", 0o644);

    h.reset_root();
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("f"), b"OLD-CONTENT").unwrap();
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o300)).unwrap();
    let script = run_sh_stdin(&command, b"NEW-CONTENT", 60);
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        script.status.code(),
        Some(74),
        "the script must map the post-rename fsync failure to 74: {:?}",
        String::from_utf8_lossy(&script.stderr)
    );
    assert_eq!(
        std::fs::read(state.join("f")).unwrap(),
        b"NEW-CONTENT",
        "the rename committed before the failing parent fsync"
    );

    h.reset_root();
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("f"), b"OLD-CONTENT").unwrap();
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o300)).unwrap();
    let path = state.join("f");
    let cli_out = run_bounded_stdin(
        cli(&[
            OsStr::new("publish"),
            path.as_os_str(),
            OsStr::new("644"),
            OsStr::new("11"),
        ]),
        b"NEW-CONTENT",
        60,
    );
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_parity("post-rename fsync failure", &script, &cli_out);
    assert_eq!(
        cli_out.status.code(),
        Some(74),
        "the CLI must exit SSH_UPLOAD_POST_RENAME_EXIT: {:?}",
        String::from_utf8_lossy(&cli_out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&cli_out.stderr).contains("fsync-open"),
        "the CLI must name the failing fsync primitive: {:?}",
        String::from_utf8_lossy(&cli_out.stderr)
    );
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"NEW-CONTENT",
        "the new content must be visible after the exit-74 path"
    );
    let state_after = snapshot(&state);
    assert_no_temp_residue("post-rename fsync failure", &state_after);
}

/// The transport-level tests below prove the CLI is USED. This one proves the
/// two contracts the argv form cannot show by itself: the payload travels on
/// STDIN only (arbitrary bytes, including NUL and non-UTF-8, round-trip
/// exactly), and a destination at `NAME_MAX` still gets a BOUNDED, usable temp
/// name.
#[test]
fn cli_writers_preserve_arbitrary_bytes_and_bounded_temp_names() {
    let h = Harness::new();
    h.reset_root();
    std::fs::create_dir_all(h.remote_path("state")).unwrap();
    let payload: &[u8] = b"\0raw\xffbytes'\n\"$`; payload";
    let len = payload.len().to_string();

    let dest = h.remote_path("state/arbitrary");
    let publish = run_bounded_stdin(
        cli(&[
            OsStr::new("publish"),
            dest.as_os_str(),
            OsStr::new("600"),
            OsStr::new(&len),
        ]),
        payload,
        60,
    );
    assert_eq!(publish.status.code(), Some(0));
    assert!(publish.stdout.is_empty() && publish.stderr.is_empty());
    assert_eq!(
        std::fs::read(&dest).unwrap(),
        payload,
        "the published bytes must be EXACTLY the stdin bytes"
    );
    assert_eq!(std::fs::metadata(&dest).unwrap().mode() & 0o7777, 0o600);

    // A 255-byte (NAME_MAX) destination: the temp name must be bounded, so the
    // writer still works. `temp_name_for` is the crate's ONE bounding
    // authority and the CLI uses it.
    let long_name = "a".repeat(255);
    let long_dest = h.remote_path(&format!("state/{long_name}"));
    let write_new = run_bounded_stdin(
        cli(&[
            OsStr::new("write-new"),
            long_dest.as_os_str(),
            OsStr::new("644"),
            OsStr::new(&len),
        ]),
        payload,
        60,
    );
    assert_eq!(write_new.status.code(), Some(0));
    assert!(write_new.stdout.is_empty() && write_new.stderr.is_empty());
    assert_eq!(std::fs::read(&long_dest).unwrap(), payload);
    assert_eq!(
        std::fs::metadata(&long_dest).unwrap().mode() & 0o7777,
        0o644
    );

    let state = snapshot(&h.remote_path("state"));
    assert_no_temp_residue("arbitrary bytes + NAME_MAX", &state);
    assert_eq!(state.len(), 2, "only the two destinations may remain");
}

/// With a compatible CLI on the far side, ALL THREE writers run their native
/// subcommands (no `perl`), and the transport probes exactly ONCE.
#[test]
fn write_ops_use_the_cli_when_present() {
    let h = Harness::new();
    h.expose_real_cli();
    let t = h.transport();

    // publish
    Remote::write(&t, &rooted("files/blob"), b"payload", 0o640)
        .expect("the CLI publish must succeed");
    assert_eq!(
        std::fs::read(h.remote_path("files/blob")).unwrap(),
        b"payload"
    );
    assert_eq!(
        std::fs::metadata(h.remote_path("files/blob"))
            .unwrap()
            .mode()
            & 0o7777,
        0o640
    );

    // write-new
    let verdict = Remote::try_write_new(&t, &rooted("records/new.json"), b"gen-1")
        .expect("the CLI write-new must install");
    assert_eq!(verdict, CreateNewVerdict::Created);
    assert_eq!(
        std::fs::read(h.remote_path("records/new.json")).unwrap(),
        b"gen-1"
    );

    // sidecar-publish (the owned lock record is the sidecar route)
    let verdict = Remote::try_write_new(&t, &rooted("state/operation.lock"), b"lock-record")
        .expect("the CLI sidecar publish must install");
    assert_eq!(verdict, CreateNewVerdict::Created);
    assert_eq!(
        std::fs::read(h.remote_path("state/operation.lock")).unwrap(),
        b"lock-record"
    );
    assert!(
        h.remote_path("state/operation.lock.mutex").is_file(),
        "the CLI must create the sidecar file first"
    );

    let log = h.log();
    assert_eq!(log.len(), 4, "probe + the three CLI writers, got {log:?}");
    assert_eq!(log[0], "bash -c 'storekit-remote protocol'");

    let publish_path = h.remote_path("files/blob").to_string_lossy().into_owned();
    let publish = expected_cli_command(&["storekit-remote", "publish", &publish_path, "640", "7"]);
    assert_eq!(log[1], publish, "the publish must run the CLI form");

    let write_new_path = h
        .remote_path("records/new.json")
        .to_string_lossy()
        .into_owned();
    let record_mode = format!("{:o}", RECORD_MODE);
    let write_new = expected_cli_command(&[
        "storekit-remote",
        "write-new",
        &write_new_path,
        &record_mode,
        "5",
    ]);
    assert_eq!(log[2], write_new, "the write-new must run the CLI form");

    let sidecar_path = h
        .remote_path("state/operation.lock.mutex")
        .to_string_lossy()
        .into_owned();
    let lock_path = h
        .remote_path("state/operation.lock")
        .to_string_lossy()
        .into_owned();
    let sidecar = expected_cli_command(&[
        "storekit-remote",
        "sidecar-publish",
        &sidecar_path,
        &lock_path,
        &record_mode,
        "11",
    ]);
    assert_eq!(log[3], sidecar, "the sidecar must run the CLI form");

    assert!(
        !log.iter().any(|line| line.contains("perl")),
        "a compatible CLI must not fall back to perl for any writer: {log:?}"
    );
    assert_eq!(
        log.iter()
            .filter(|line| line.contains("storekit-remote protocol"))
            .count(),
        1,
        "exactly ONE probe per transport: {log:?}"
    );

    // A second publish reads the cached decision: no second probe.
    Remote::write(&t, &rooted("files/blob2"), b"payload2", 0o600)
        .expect("the second CLI publish must succeed");
    let log = h.log();
    assert_eq!(log.len(), 5, "the second publish must not probe: {log:?}");
    assert_eq!(
        log.iter()
            .filter(|line| line.contains("storekit-remote protocol"))
            .count(),
        1,
        "exactly ONE probe per transport: {log:?}"
    );
}

/// With no `storekit-remote` on the far side, all three writers fall back to
/// their existing perl scripts, and the transport still probes exactly ONCE.
#[test]
fn write_ops_fall_back_to_the_script_when_absent() {
    let h = Harness::new();
    let t = h.transport();

    Remote::write(&t, &rooted("files/blob"), b"payload", 0o640)
        .expect("the script publish must succeed");
    assert_eq!(
        std::fs::read(h.remote_path("files/blob")).unwrap(),
        b"payload"
    );

    let verdict = Remote::try_write_new(&t, &rooted("records/new.json"), b"gen-1")
        .expect("the script write-new must install");
    assert_eq!(verdict, CreateNewVerdict::Created);
    assert_eq!(
        std::fs::read(h.remote_path("records/new.json")).unwrap(),
        b"gen-1"
    );

    let verdict = Remote::try_write_new(&t, &rooted("state/operation.lock"), b"lock-record")
        .expect("the script sidecar publish must install");
    assert_eq!(verdict, CreateNewVerdict::Created);
    assert_eq!(
        std::fs::read(h.remote_path("state/operation.lock")).unwrap(),
        b"lock-record"
    );

    let log = h.log();
    assert_eq!(
        log.len(),
        4,
        "probe + the three script writers, got {log:?}"
    );
    assert_eq!(log[0], "bash -c 'storekit-remote protocol'");
    for command in &log[1..] {
        assert!(
            command.starts_with("bash -c 'mkdir -p "),
            "the absent CLI must run the existing perl script: {command:?}"
        );
        assert!(
            command.contains("perl"),
            "the fallback script is perl-native: {command:?}"
        );
        assert!(
            !command.contains("'storekit-remote' 'publish'")
                && !command.contains("'storekit-remote' 'write-new'")
                && !command.contains("'storekit-remote' 'sidecar-publish'"),
            "the absent CLI must not be invoked: {command:?}"
        );
    }
    assert_eq!(
        log.iter()
            .filter(|line| line.contains("storekit-remote protocol"))
            .count(),
        1,
        "exactly ONE probe per transport: {log:?}"
    );
}

/// The CLI's exit-74 status is mapped by the TRANSPORT to the typed
/// "visible but durability unconfirmed" condition, exactly as the script's 74
/// is — the same fault as the differential (a `0o300` parent directory).
#[test]
fn cli_publish_parent_fsync_failure_reports_durability_unconfirmed() {
    let h = Harness::new();
    h.expose_real_cli();
    let state = h.remote_path("state");
    std::fs::create_dir_all(&state).expect("create the fixture state dir");
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o300))
        .expect("chmod the fixture state dir");

    let t = h.transport();
    let error = Remote::write(&t, &rooted("state/f"), b"payload", 0o644)
        .expect_err("the CLI's 74 must be a propagated error");
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
        .expect("restore the fixture state dir");

    assert_eq!(
        error.transport_reason(),
        Some(TransportKind::DurabilityUnconfirmed),
        "a post-rename fsync failure carries the typed durability kind: {error:?}"
    );
    assert!(
        error
            .to_string()
            .contains("the entry is visible but its durability is unconfirmed"),
        "the error must report visible-but-unconfirmed durability, got: {error}"
    );
    assert_eq!(
        std::fs::read(state.join("f")).unwrap(),
        b"payload",
        "the content must be visible despite the unconfirmed durability"
    );
}
