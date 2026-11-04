//! The far-side CLI read operations (`remote-cli-read-ops`): `list` and the
//! descriptor-bound `verify-open` helper run through `storekit-remote` when the
//! far side has a compatible one, and fall back to the existing perl script
//! when it does not.
//!
//! TWO KINDS OF TEST, both in this file:
//!
//! * TRANSPORT-LEVEL tests build an [`SshTransport`] over an `ssh` SHIM on
//!   `PATH` (the harness `tests/remote_cli_probe.rs` established) that logs the
//!   remote command it was handed and runs it locally. They assert the ACTUAL
//!   logged command: the CLI form with the binary present (and no `perl` for
//!   that operation), the script form with it absent, and exactly ONE probe per
//!   transport.
//! * DIFFERENTIAL tests run the EXACT logged script and the CLI on the same
//!   fixture and compare stdout BYTES, stderr, and the exit status. The script
//!   under test is never re-typed here: it is the command the transport itself
//!   logged while the CLI was absent.
//!
//! WHAT THE SHIM DOES NOT PROVE. No network, no `sshd`, no OpenSSH option
//! parsing, no remote filesystem semantics: the far side is the SAME host. A
//! defect that depends on those is out of scope here.
#![cfg(unix)]
// Test-only fixtures drive the same name-mutating primitives the funnel guards.
#![allow(clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use storekit::env::SysEnv;
use storekit::transport::{Layout, Remote, RootedRelativePath, SshTransport};

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
# can span several lines (the verify-open perl helper does), so a newline is
# not an invocation boundary but NUL cannot occur in a command.
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
        .stdin(std::process::Stdio::piped())
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
            .prefix("storekit-remote-read-")
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

    /// The LAST remote command logged — the operation a test just drove.
    fn last_command(&self) -> String {
        self.log()
            .pop()
            .expect("the shim must have logged the remote command")
    }

    /// The filesystem path a logged command addresses: `root.join(rel)`.
    fn remote_path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
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

/// Run `command` under `/bin/sh -c` — the exact form the ssh argv carries.
fn run_sh(command: &str) -> Output {
    Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .output()
        .expect("run the logged remote command")
}

/// Run `cmd` with a DEADLINE: a command that never returns is killed and the
/// test fails, so a blocking FIFO open is a failure rather than a hang.
fn run_bounded(mut cmd: Command, secs: u64) -> Output {
    let mut child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn the command under test");
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

/// Assert one differential case: the logged script and the CLI produce the SAME
/// stdout bytes, the SAME stderr bytes, and the SAME exit status.
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

fn mkfifo(path: &Path) {
    let status = Command::new("mkfifo")
        .arg(path)
        .status()
        .expect("run mkfifo");
    assert!(status.success(), "mkfifo {path:?} must succeed");
}

/// Announce a SKIPPED assertion on the real console (libtest discards the
/// captured output of a PASSING test, which would make a skip look like a
/// pass). Mirrors the crate's own test support, which an integration test
/// cannot reach.
fn announce_skip(reason: &str) {
    let test = std::thread::current()
        .name()
        .unwrap_or("<unknown test>")
        .to_string();
    let line = format!("STOREKIT_SKIP test={test} reason={reason}\n");
    unsafe {
        libc::write(1, line.as_ptr().cast::<libc::c_void>(), line.len());
    }
}

/// Create a file whose name is not valid UTF-8. It returns `false` (after
/// announcing a skip) when the HOST filesystem refuses invalid UTF-8 names —
/// macOS APFS answers EILSEQ — because that is a property of the fixture's
/// filesystem, not of the code under test.
fn write_non_utf8_name(path: &Path) -> bool {
    match std::fs::write(path, b"binary-name") {
        Ok(()) => true,
        Err(error) if error.raw_os_error() == Some(libc::EILSEQ) => {
            announce_skip("the host filesystem refuses a non-UTF-8 name (EILSEQ)");
            false
        }
        Err(error) => panic!("write {path:?}: {error}"),
    }
}

/// The `list` differential: the CLI is BYTE-IDENTICAL to the transport's
/// existing perl listing helper on the same fixture — stdout, stderr and exit.
///
/// `rel` must already exist in the given state. The script comes from the
/// transport's OWN log (the CLI is absent), so it is never re-typed here. The
/// transport call exists only to make the transport LOG that command: its own
/// parse outcome is deliberately not what this comparison asserts (the
/// near-side decoder refuses a non-UTF-8 name while the listing itself still
/// succeeds, so the two are not the same verdict).
fn list_case(h: &Harness, case: &str, rel: &str) {
    let t = h.transport();
    let _ = Remote::list(&t, &rooted(rel));
    let command = h.last_command();
    assert!(
        command.starts_with("bash -c 'perl -e "),
        "{case}: the absent CLI must log the perl listing script, got {command:?}"
    );
    let script = run_sh(&command);
    let path = h.remote_path(rel);
    let cli = run_bounded(cli(&[OsStr::new("list"), path.as_os_str()]), 60);
    assert_parity(case, &script, &cli);
}

/// `storekit-remote list` against every shape the transport's listing helper is
/// specified over: an empty directory, every entry kind (file, directory,
/// symlink, FIFO, dangling symlink, non-UTF-8 name), a missing directory, a
/// `chmod 400` directory, and a non-directory path (`opendir` → ENOTDIR).
#[test]
fn list_cli_is_byte_identical_to_the_script() {
    let h = Harness::new();
    // An empty directory.
    std::fs::create_dir_all(h.remote_path("empty")).expect("create the empty dir");
    // Every entry kind the frame classifies.
    let tree = h.remote_path("tree");
    std::fs::create_dir_all(tree.join("sub")).expect("create the fixture tree");
    std::fs::write(tree.join("file"), b"abc").expect("write the fixture file");
    std::os::unix::fs::symlink("file", tree.join("link")).expect("create the fixture symlink");
    std::os::unix::fs::symlink("no-such-target", tree.join("dangling"))
        .expect("create the dangling symlink");
    mkfifo(&tree.join("fifo"));
    // A name that is NOT valid UTF-8, in its own directory so the case is
    // independent of whether this host's filesystem accepts such a name.
    let raw_dir = h.remote_path("raw");
    std::fs::create_dir_all(&raw_dir).expect("create the raw-name dir");
    let raw_name = OsStr::from_bytes(b"raw-\xff-name");
    let raw_available = write_non_utf8_name(&raw_dir.join(raw_name));
    // A directory that lists (`r`) but cannot be searched (`x`): every entry's
    // `lstat` fails EACCES, so the listing must FAIL.
    let locked = h.remote_path("locked");
    std::fs::create_dir_all(&locked).expect("create the locked dir");
    std::fs::write(locked.join("entry"), b"payload").expect("write the locked entry");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o400))
        .expect("chmod the locked dir");

    list_case(&h, "empty directory", "empty");
    list_case(&h, "every entry kind", "tree");
    list_case(&h, "missing directory", "never-created");
    list_case(&h, "regular file instead of a directory", "tree/file");
    list_case(&h, "chmod 400 directory", "locked");
    if raw_available {
        list_case(&h, "non-UTF-8 name", "raw");
    }

    // The raw-name case must carry the entry name as RAW BYTES: a lossy
    // re-encode shows up as a byte mismatch here, never as a silent skip. (On a
    // filesystem that refuses such a name the case above is skipped instead.)
    let command = {
        let t = h.transport();
        // The command is logged whether or not the near-side decoder accepts
        // the result — it deliberately REFUSES a non-UTF-8 entry name.
        let _ = Remote::list(&t, &rooted(if raw_available { "raw" } else { "tree" }));
        h.last_command()
    };
    let script = run_sh(&command);
    assert!(script.status.success(), "the listing must succeed");
    if raw_available {
        assert!(
            script
                .stdout
                .windows(b"\traw-\xff-name\0".len())
                .any(|w| w == b"\traw-\xff-name\0"),
            "the non-UTF-8 name must be carried as RAW BYTES: {:?}",
            String::from_utf8_lossy(&script.stdout)
        );
    } else {
        announce_skip("the raw-byte name assertion needs a filesystem that accepts such a name");
    }

    let command = {
        let t = h.transport();
        Remote::list(&t, &rooted("tree")).expect("the tree lists");
        h.last_command()
    };
    let script = run_sh(&command);
    assert!(script.status.success(), "the tree listing must succeed");
    assert!(
        script
            .stdout
            .split(|&b| b == 0)
            .any(|record| record.starts_with(b"l\t") && record.ends_with(b"\tdangling")),
        "a dangling symlink must be listed as a symlink: {:?}",
        String::from_utf8_lossy(&script.stdout)
    );
    assert!(
        script
            .stdout
            .split(|&b| b == 0)
            .any(|record| record.starts_with(b"f\t") && record.ends_with(b"\tfifo")),
        "a FIFO must be listed as a non-directory, non-symlink entry: {:?}",
        String::from_utf8_lossy(&script.stdout)
    );
    assert!(
        !script.stdout.windows(2).any(|w| w == b"\0."),
        "`.`/`..` must never be emitted"
    );

    // The failing case must FAIL LOUDLY, naming the operation and the path,
    // and the CLI must reproduce that byte for byte (checked above).
    let cli = cli(&[OsStr::new("list"), locked.as_os_str()])
        .output()
        .expect("run the failing listing");
    assert_ne!(cli.status.code(), Some(0), "a chmod 400 dir must fail");
    assert!(
        cli.stderr.starts_with(b"list: lstat "),
        "the failure must name the operation and the path: {:?}",
        String::from_utf8_lossy(&cli.stderr)
    );

    // Restore so the fixture directory can be removed.
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700))
        .expect("restore the locked dir");
}

/// Drive `try_write_new` against the EXISTING entry at `rel` so the transport
/// logs its descriptor-bound verify-open command for that exact path. The
/// verdict is not the subject here (the differential is); a failure is fine.
fn log_verify_open_script(h: &Harness, rel: &str) -> String {
    let t = h.transport();
    let _ = Remote::try_write_new(&t, &rooted(rel), b"intended-bytes");
    h.log()
        .into_iter()
        .rev()
        .find(|line| line.contains("O_NOFOLLOW"))
        .unwrap_or_else(|| panic!("the verify-open helper for {rel:?} was never logged"))
}

/// The `verify-open` differential: the CLI is BYTE-IDENTICAL to the transport's
/// existing descriptor-bound perl helper. `adjust` runs AFTER the script is
/// logged and before both forms run, so the fixture can be in a state the
/// create-new path cannot reach (an ABSENT path).
fn verify_open_case<F>(h: &Harness, case: &str, rel: &str, adjust: F) -> Output
where
    F: FnOnce(),
{
    let command = log_verify_open_script(h, rel);
    assert!(
        command.starts_with("bash -c 'perl -e "),
        "{case}: the absent CLI must log the perl verify-open script, got {command:?}"
    );
    adjust();
    let script = run_bounded(
        {
            let mut c = Command::new("/bin/sh");
            c.arg("-c").arg(&command);
            c
        },
        30,
    );
    let path = h.remote_path(rel);
    let cli = run_bounded(cli(&[OsStr::new("verify-open"), path.as_os_str()]), 30);
    assert_parity(case, &script, &cli);
    assert_eq!(
        cli.status.code(),
        Some(0),
        "{case}: a verify-open OUTCOME is always exit 0 (the frame carries it)"
    );
    cli
}

/// `storekit-remote verify-open` over every shape the descriptor-bound helper
/// is specified over: a regular file (content and raw mode), a symlink
/// (`ELOOP`, never followed), an absent path, a directory (`N`), and a FIFO
/// (must never block).
#[test]
fn verify_open_cli_is_byte_identical_to_the_script() {
    let h = Harness::new();
    let state = h.remote_path("state");
    std::fs::create_dir_all(&state).expect("create the fixture state dir");

    // A regular file with a NON-UTF-8, NUL-bearing content: the `O` frame is a
    // header line followed by RAW bytes, and nothing may re-encode them.
    let regular = state.join("regular");
    std::fs::write(&regular, b"payload\0with\xffbytes").expect("write the regular file");
    std::fs::set_permissions(&regular, std::fs::Permissions::from_mode(0o640))
        .expect("chmod the regular file");
    let local = std::fs::metadata(&regular).expect("stat the regular file");
    let cli = verify_open_case(&h, "regular file", "state/regular", || {});
    let mut expected = format!("O\t{:x}\n", local.mode() & 0xffff).into_bytes();
    expected.extend_from_slice(b"payload\0with\xffbytes");
    assert_eq!(
        cli.stdout, expected,
        "the O frame must carry the fd-derived raw mode and the exact content bytes"
    );

    // A symlink pointing at a perfectly matching regular file is still ELOOP:
    // O_NOFOLLOW never follows it.
    std::os::unix::fs::symlink("regular", state.join("link")).expect("create the fixture symlink");
    let cli = verify_open_case(&h, "symlink", "state/link", || {});
    assert_eq!(
        cli.stdout,
        format!("E\t{}\n", libc::ELOOP).into_bytes(),
        "a symlink must be ELOOP in the E frame, never followed to its target"
    );

    // An absent path: the entry exists when the script is logged and is removed
    // before the comparison, so both forms must report ENOENT.
    std::fs::write(state.join("absent"), b"here-now").expect("write the soon-absent entry");
    let cli = verify_open_case(&h, "absent path", "state/absent", || {
        std::fs::remove_file(state.join("absent")).expect("remove the absent-path entry");
    });
    assert_eq!(
        cli.stdout,
        format!("E\t{}\n", libc::ENOENT).into_bytes(),
        "an absent path must be ENOENT in the E frame"
    );

    // A directory: opened and fstat'd, but NOT a regular file → `N`.
    std::fs::create_dir_all(state.join("dir")).expect("create the fixture directory");
    let local = std::fs::metadata(state.join("dir")).expect("stat the fixture directory");
    let cli = verify_open_case(&h, "directory", "state/dir", || {});
    assert_eq!(
        cli.stdout,
        format!("N\t{:x}\n", local.mode() & 0xffff).into_bytes(),
        "a directory must be an N frame carrying the fd-derived raw mode"
    );

    // A FIFO: O_NONBLOCK makes the open return immediately and the inode is not
    // a regular file → `N`. A blocking open would trip the bounded runner.
    mkfifo(&state.join("fifo"));
    let local = std::fs::symlink_metadata(state.join("fifo")).expect("stat the fixture fifo");
    let cli = verify_open_case(&h, "fifo", "state/fifo", || {});
    assert_eq!(
        cli.stdout,
        format!("N\t{:x}\n", local.mode() & 0xffff).into_bytes(),
        "a FIFO must be an N frame, and must never block the open"
    );
}

/// With a compatible CLI on the far side, `list` probes ONCE and then runs
/// `storekit-remote list <path>` — no `perl` for the operation.
#[test]
fn list_uses_the_cli_when_present() {
    let h = Harness::new();
    h.expose_real_cli();
    let tree = h.remote_path("tree");
    std::fs::create_dir_all(tree.join("sub")).expect("create the fixture tree");
    std::fs::write(tree.join("file"), b"abc").expect("write the fixture file");
    let path = h.remote_path("tree");
    let abs = path.to_string_lossy().into_owned();

    let t = h.transport();
    let mut names: Vec<String> = Remote::list(&t, &rooted("tree"))
        .expect("the CLI listing must parse")
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    names.sort();
    assert_eq!(names, ["file", "sub"], "the CLI listing's entries");

    let log = h.log();
    assert_eq!(log.len(), 2, "probe + one op, got {log:?}");
    assert_eq!(log[0], "bash -c 'storekit-remote protocol'");
    assert_eq!(
        log[1],
        expected_cli_command(&["storekit-remote", "list", &abs]),
        "the listing must run the CLI form, exactly"
    );
    assert!(
        !log[1].contains("perl"),
        "a compatible CLI must not fall back to perl: {:?}",
        log[1]
    );

    // A second listing reads the cached decision: no second probe.
    Remote::list(&t, &rooted("tree")).expect("the second CLI listing must parse");
    let log = h.log();
    assert_eq!(log.len(), 3, "the second op must not probe: {log:?}");
    assert_eq!(log[2], log[1], "the second op runs the same CLI form");
    assert_eq!(
        log.iter()
            .filter(|line| line.contains("storekit-remote protocol"))
            .count(),
        1,
        "exactly ONE probe per transport: {log:?}"
    );
}

/// With no `storekit-remote` on the far side, `list` falls back to the perl
/// script and never invokes the CLI.
#[test]
fn list_falls_back_to_the_script_when_absent() {
    let h = Harness::new();
    let tree = h.remote_path("tree");
    std::fs::create_dir_all(&tree).expect("create the fixture tree");
    std::fs::write(tree.join("file"), b"abc").expect("write the fixture file");

    let t = h.transport();
    let names: Vec<String> = Remote::list(&t, &rooted("tree"))
        .expect("the script listing must parse")
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert_eq!(names, ["file"], "the script listing's entries");

    let log = h.log();
    assert_eq!(log.len(), 2, "probe + one op, got {log:?}");
    assert_eq!(log[0], "bash -c 'storekit-remote protocol'");
    assert!(
        log[1].starts_with("bash -c 'perl -e "),
        "the fallback must run the existing perl list script: {:?}",
        log[1]
    );
    assert!(
        !log[1].contains("'storekit-remote' 'list'"),
        "the absent CLI must not be invoked: {:?}",
        log[1]
    );
    assert_eq!(
        log.iter()
            .filter(|line| line.contains("storekit-remote protocol"))
            .count(),
        1,
        "exactly ONE probe per transport: {log:?}"
    );
}

/// With a compatible CLI on the far side, the descriptor-bound verification
/// runs `storekit-remote verify-open <path>` — no `perl` for that operation —
/// and the converged verdict proves the CLI's frame parsed.
#[test]
fn verify_open_uses_the_cli_when_present() {
    let h = Harness::new();
    h.expose_real_cli();
    let state = h.remote_path("state");
    std::fs::create_dir_all(&state).expect("create the fixture state dir");
    // The installed record matches the intent exactly, so the verdict must be
    // AlreadyPresent: a wrong mode or a byte-level content drift in the CLI's
    // `O` frame would turn it into a Conflict.
    let record = state.join("record.json");
    std::fs::write(&record, b"intended-bytes").expect("write the fixture record");
    std::fs::set_permissions(&record, std::fs::Permissions::from_mode(0o644))
        .expect("chmod the fixture record");
    let abs = record.to_string_lossy().into_owned();

    let t = h.transport();
    let verdict = Remote::try_write_new(&t, &rooted("state/record.json"), b"intended-bytes")
        .expect("the converged write must return a verdict");
    assert_eq!(
        verdict,
        storekit::transport::CreateNewVerdict::AlreadyPresent,
        "the CLI verify-open frame must reproduce the installed mode and content"
    );

    let log = h.log();
    let verify_command = expected_cli_command(&["storekit-remote", "verify-open", &abs]);
    let verify: Vec<&String> = log
        .iter()
        .filter(|line| line.as_str() == verify_command)
        .collect();
    assert_eq!(
        verify.len(),
        1,
        "exactly ONE CLI verify-open must run, got {log:?}"
    );
    assert!(
        !verify[0].contains("perl"),
        "a compatible CLI must not fall back to perl for verify-open: {:?}",
        verify[0]
    );
    assert!(
        !log.iter().any(|line| line.contains("O_NOFOLLOW")),
        "the perl descriptor-bound helper must not run when the CLI is present: {log:?}"
    );
    // The WRITE step is a future item and must still be the perl script.
    assert!(
        log.iter()
            .any(|line| line.contains("perl") && line != verify[0]),
        "the write step must still run its perl script: {log:?}"
    );
    assert_eq!(
        log.iter()
            .filter(|line| line.contains("storekit-remote protocol"))
            .count(),
        1,
        "exactly ONE probe per transport: {log:?}"
    );
}

/// With no `storekit-remote` on the far side, the verification falls back to
/// the perl helper and never invokes the CLI.
#[test]
fn verify_open_falls_back_to_the_script_when_absent() {
    let h = Harness::new();
    let state = h.remote_path("state");
    std::fs::create_dir_all(&state).expect("create the fixture state dir");
    let record = state.join("record.json");
    std::fs::write(&record, b"intended-bytes").expect("write the fixture record");
    std::fs::set_permissions(&record, std::fs::Permissions::from_mode(0o644))
        .expect("chmod the fixture record");

    let t = h.transport();
    let verdict = Remote::try_write_new(&t, &rooted("state/record.json"), b"intended-bytes")
        .expect("the converged write must return a verdict");
    assert_eq!(
        verdict,
        storekit::transport::CreateNewVerdict::AlreadyPresent,
        "the perl helper must reproduce the installed mode and content"
    );

    let log = h.log();
    let verify: Vec<&String> = log
        .iter()
        .filter(|line| line.contains("O_NOFOLLOW"))
        .collect();
    assert_eq!(
        verify.len(),
        1,
        "exactly ONE descriptor-bound helper must run, got {log:?}"
    );
    assert!(
        verify[0].starts_with("bash -c 'perl -e "),
        "the fallback must run the existing perl verify-open helper: {:?}",
        verify[0]
    );
    assert!(
        !log.iter()
            .any(|line| line.contains("'storekit-remote' 'verify-open'")),
        "the absent CLI must not be invoked: {log:?}"
    );
    assert_eq!(
        log.iter()
            .filter(|line| line.contains("storekit-remote protocol"))
            .count(),
        1,
        "exactly ONE probe per transport: {log:?}"
    );
}

/// The CLI form's raw name reaches the SAME near-side decoder as the script's,
/// so a non-UTF-8 entry name is REFUSED there exactly as it is refused for the
/// script — never lossily decoded into a name that could collide with a
/// different on-disk byte string. The CLI still emits the raw bytes (the
/// differential above compares them); the refusal is the near-side contract.
#[test]
fn cli_listing_refuses_a_non_utf8_name_like_the_script() {
    let h = Harness::new();
    h.expose_real_cli();
    let tree = h.remote_path("tree");
    std::fs::create_dir_all(&tree).expect("create the fixture tree");
    let raw_name = OsStr::from_bytes(b"entry-\xfe\xff");
    if !write_non_utf8_name(&tree.join(raw_name)) {
        return;
    }
    let abs = h.remote_path("tree").to_string_lossy().into_owned();

    let t = h.transport();
    let err = Remote::list(&t, &rooted("tree"))
        .expect_err("a non-UTF-8 entry name must be refused, never decoded lossily");
    assert!(
        err.to_string().contains("not valid UTF-8"),
        "the refusal must name the reason, got: {err}"
    );
    let log = h.log();
    assert_eq!(
        log[1],
        expected_cli_command(&["storekit-remote", "list", &abs]),
        "the refusal must come from the CLI form's raw bytes, not from a fallback"
    );
    assert!(
        !log[1].contains("perl"),
        "a compatible CLI must not fall back to perl: {:?}",
        log[1]
    );
}
