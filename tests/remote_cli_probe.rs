//! The far-side CLI probe (`remote-cli-probe`): `storekit-remote` exists, states
//! a protocol, and `lstat` runs through it when it is compatible.
//!
//! THE HARNESS — an `ssh` SHIM ON `PATH` (the pattern `tests/ssh_farside_*`
//! uses), extended to LOG the remote command it was handed and to run it
//! locally. The transport is pointed at the shim through a hermetic [`SysEnv`]
//! snapshot, so every spawned `ssh` is the shim and every far-side command is
//! re-parsed by a real POSIX shell against a real filesystem.
//!
//! WHAT THE SHIM DOES NOT PROVE. No network, no `sshd`, no OpenSSH option
//! parsing, no remote filesystem semantics: the far side is the SAME host. A
//! defect that depends on those is out of scope here.
//!
//! The tests assert the ACTUAL logged command string and the ACTUAL stdout
//! bytes — never just "it returned Ok" — so a probe that runs per operation, a
//! fallback to perl despite a compatible CLI, or an `lstat` frame that drifts
//! from the script's all fail.
#![cfg(unix)]
// Test-only fixtures drive the same name-mutating primitives the funnel guards.
#![allow(clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use storekit::env::SysEnv;
use storekit::transport::{Layout, Remote, RootedRelativePath, SshTransport};

/// The environment variable the shim reads to find its "remote working
/// directory" (the directory a remote login shell would start in).
const SHIM_WORK_VAR: &str = "STOREKIT_SSH_SHIM_WORK";
/// The environment variable the shim appends each remote command to.
const SHIM_LOG_VAR: &str = "STOREKIT_SSH_SHIM_LOG";
/// When set, the shim exits with this status instead of running the command —
/// the ssh-level failure case.
const SHIM_EXIT_VAR: &str = "STOREKIT_SSH_SHIM_EXIT";

const SHIM_SCRIPT: &str = r#"#!/bin/sh
# Test-only `ssh` shim: it never opens a network connection. It records the
# remote command string (the FINAL argument, `bash -c '<script>'`) and then
# runs it in $STOREKIT_SSH_SHIM_WORK with stdin/stdout/stderr connected exactly
# as the operation connects them. $STOREKIT_SSH_SHIM_EXIT simulates the local
# `ssh` client failing (255) without running anything remotely.
set -u
work=${STOREKIT_SSH_SHIM_WORK:?the shim work directory is not configured}
log=${STOREKIT_SSH_SHIM_LOG:?the shim log is not configured}
last=''
for arg in "$@"; do last=$arg; done
printf '%s\n' "$last" >> "$log"
if [ -n "${STOREKIT_SSH_SHIM_EXIT:-}" ]; then exit "$STOREKIT_SSH_SHIM_EXIT"; fi
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
    let mut child = std::process::Command::new("/bin/sh")
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
            .prefix("storekit-remote-")
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

    /// Put a `storekit-remote` on `PATH` that states a DIFFERENT protocol.
    fn install_wrong_token_cli(&self) {
        write_executable(
            &self.bin.join("storekit-remote"),
            b"#!/bin/sh\nprintf 'storekit-remote-protocol 999\\n'\nexit 0\n",
        );
    }

    /// A transport whose every child sees exactly this environment snapshot:
    /// the shim first on `PATH`, the short `TMPDIR`, the shim's work dir, its
    /// log, and (optionally) a forced `ssh` exit status.
    fn transport(&self, shim_exit: Option<&str>) -> SshTransport {
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
        if let Some(code) = shim_exit {
            vars.insert(OsString::from(SHIM_EXIT_VAR), OsString::from(code));
        }
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

    /// Every remote command the shim was handed, in order.
    fn log(&self) -> Vec<String> {
        let text = std::fs::read_to_string(&self.log_path).unwrap_or_default();
        text.lines().map(str::to_string).collect()
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

/// The `lstat` frame of the fixture file, as the script sees it.
fn fixture() -> (Harness, PathBuf) {
    let h = Harness::new();
    std::fs::create_dir_all(h.root.join("state")).expect("create the fixture dir");
    let file = h.root.join("state/f");
    std::fs::write(&file, b"hello").expect("write the fixture file");
    (h, file)
}

/// The protocol token is the ONE value both halves agree on. A compatible CLI
/// makes the first operation probe ONCE, then run the CLI `lstat`; a second
/// operation reads the cached decision and never probes again.
#[test]
fn cli_present_probes_once_then_runs_the_cli_lstat() {
    let (h, file) = fixture();
    h.expose_real_cli();
    let t = h.transport(None);

    let meta = Remote::metadata(&t, &rooted("state/f")).expect("the CLI lstat must parse");
    assert_eq!(meta.size, 5, "the CLI reports the real size: {meta:?}");
    assert!(meta.is_file && !meta.is_dir && !meta.is_symlink, "{meta:?}");

    let log = h.log();
    assert_eq!(log.len(), 2, "probe + one op, got {log:?}");
    assert_eq!(log[0], "bash -c 'storekit-remote protocol'");
    assert_eq!(
        log[1],
        expected_cli_command(&["storekit-remote", "lstat", &file.display().to_string()]),
        "the op must run the CLI form, exactly"
    );
    assert!(
        !log[1].contains("perl"),
        "a compatible CLI must not fall back to perl: {:?}",
        log[1]
    );

    let meta = Remote::metadata(&t, &rooted("state/f")).expect("the second CLI lstat must parse");
    assert_eq!(meta.size, 5, "{meta:?}");
    let log = h.log();
    assert_eq!(log.len(), 3, "the second op must not probe: {log:?}");
    assert_eq!(log[2], log[1], "the second op runs the same CLI form");
    assert_eq!(
        log.iter().filter(|l| l.contains("protocol")).count(),
        1,
        "exactly ONE probe per transport: {log:?}"
    );
}

/// No `storekit-remote` on the far side: the probe's 127 is ABSENCE, the
/// original perl script runs, and the parsed result is unchanged.
#[test]
fn cli_absent_falls_back_to_the_perl_script_unchanged() {
    use std::os::unix::fs::MetadataExt;

    let (h, file) = fixture();
    let t = h.transport(None);

    let meta = Remote::metadata(&t, &rooted("state/f")).expect("the script lstat must parse");
    let local = std::fs::symlink_metadata(&file).expect("stat the fixture file");
    assert_eq!(meta.size, local.len(), "{meta:?}");
    assert_eq!(
        meta.mode & 0o7777,
        local.mode() & 0o7777,
        "the permission bits are unchanged: {meta:?}"
    );
    assert!(meta.is_file && !meta.is_dir && !meta.is_symlink, "{meta:?}");

    let log = h.log();
    assert_eq!(log.len(), 2, "probe + one op, got {log:?}");
    assert_eq!(log[0], "bash -c 'storekit-remote protocol'");
    assert!(
        log[1].starts_with("bash -c 'perl -e "),
        "the fallback must run the existing perl lstat script: {:?}",
        log[1]
    );
    assert!(
        !log[1].contains("'storekit-remote' 'lstat'"),
        "the absent CLI must not be invoked: {:?}",
        log[1]
    );
}

/// A `storekit-remote` that exits 0 with the WRONG token is an incompatible
/// version, so the far side falls back to the scripts exactly like absence.
#[test]
fn wrong_protocol_token_falls_back_to_the_script() {
    let (h, _file) = fixture();
    h.install_wrong_token_cli();
    let t = h.transport(None);

    let meta = Remote::metadata(&t, &rooted("state/f")).expect("the script lstat must parse");
    assert_eq!(meta.size, 5, "{meta:?}");

    let log = h.log();
    assert_eq!(log.len(), 2, "probe + one op, got {log:?}");
    assert_eq!(log[0], "bash -c 'storekit-remote protocol'");
    assert!(
        log[1].starts_with("bash -c 'perl -e "),
        "a wrong token must fall back to the script: {:?}",
        log[1]
    );
    assert!(
        !log[1].contains("'storekit-remote' 'lstat'"),
        "the incompatible CLI must not be invoked: {:?}",
        log[1]
    );
}

/// An ssh-level failure (255) on the probe is a TRANSPORT ERROR: the run fails
/// and the log proves nothing fell back to perl. Reading it as absence would
/// silently downgrade a broken connection to the scripts.
#[test]
fn ssh_255_on_the_probe_fails_and_never_falls_back() {
    let (h, _file) = fixture();
    h.expose_real_cli();
    let t = h.transport(Some("255"));

    let err = Remote::metadata(&t, &rooted("state/f"))
        .expect_err("an ssh-level probe failure must fail the run");
    let msg = err.to_string();
    assert!(
        msg.contains("probe"),
        "the error must name the probe: {msg}"
    );
    assert!(
        msg.contains("255"),
        "the error must carry the status: {msg}"
    );

    let log = h.log();
    assert_eq!(log.len(), 1, "no fallback command may run: {log:?}");
    assert_eq!(log[0], "bash -c 'storekit-remote protocol'");
}

/// The `protocol` subcommand's entire contract: exactly the shared token, one
/// newline, exit 0.
#[test]
fn protocol_subcommand_prints_exactly_the_token() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_storekit-remote"))
        .arg("protocol")
        .output()
        .expect("run storekit-remote protocol");
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(out.stdout, b"storekit-remote-protocol 1\n", "{out:?}");
    assert!(out.stderr.is_empty(), "{out:?}");
}

/// `storekit-remote lstat` is BYTE-IDENTICAL to the transport's existing perl
/// helper for every outcome, with exit 0 in every case: a file, a symlink, an
/// absent path, an absent parent (ENOTDIR), and a permission error (EACCES).
///
/// The script under test is not re-typed here: it is the command the transport
/// itself logged while the CLI was absent, run on the same fixture path. The
/// CLI is then run on that same path and the stdout bytes and exit are
/// compared.
#[test]
fn lstat_cli_frame_is_byte_identical_to_the_script() {
    let h = Harness::new();
    std::fs::create_dir_all(h.root.join("locked")).expect("create the locked dir");
    std::fs::write(h.root.join("file"), b"abc").expect("write the fixture file");
    std::fs::write(h.root.join("locked/entry"), b"x").expect("write the locked entry");
    std::os::unix::fs::symlink("file", h.root.join("link")).expect("create the fixture symlink");
    // Remove the owner's search permission: lstat of a CHILD inside it is
    // EACCES. Only meaningful for a non-root caller (root bypasses it).
    std::fs::set_permissions(
        h.root.join("locked"),
        std::fs::Permissions::from_mode(0o000),
    )
    .expect("chmod the locked dir");

    let non_root = unsafe { libc::geteuid() } != 0;
    if !non_root {
        eprintln!(
            "STOREKIT_SKIP test=lstat_cli_frame_is_byte_identical_to_the_script \
             reason=running as root, EACCES is unreachable (the other four cases still run)"
        );
    }

    let cases: &[(&str, PathBuf, &str)] = &[
        ("file", h.root.join("file"), "P"),
        ("link", h.root.join("link"), "P"),
        ("absent", h.root.join("absent"), "A"),
        ("no/such/entry", h.root.join("no/such/entry"), "A"),
        (
            "locked/entry",
            h.root.join("locked/entry"),
            if non_root { "E" } else { "P" },
        ),
    ];

    // CLI absent: every lstat runs the EXISTING perl script, whose command
    // string the shim logs.
    let t = h.transport(None);
    for (rel, path, tag) in cases {
        // The outcome (Ok/Err) is not what this test compares; the command the
        // transport ran is. It is logged either way.
        let _ = Remote::metadata(&t, &rooted(rel));
        let log = h.log();
        let command = log
            .last()
            .unwrap_or_else(|| panic!("no command logged for {rel}"));
        assert!(
            command.contains("perl -e"),
            "the script form must be logged for {rel}: {command:?}"
        );

        let script = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(command)
            .output()
            .expect("run the logged perl lstat script");
        let cli = std::process::Command::new(env!("CARGO_BIN_EXE_storekit-remote"))
            .arg("lstat")
            .arg(path)
            .output()
            .expect("run storekit-remote lstat");

        assert_eq!(script.status.code(), Some(0), "script exit for {rel}");
        assert_eq!(cli.status.code(), Some(0), "CLI exit for {rel}");
        assert_eq!(
            cli.status.code(),
            script.status.code(),
            "the exit status for {rel} must match"
        );
        assert_eq!(
            cli.stdout, script.stdout,
            "the {tag} frame for {rel} must be byte-identical"
        );
        assert_eq!(
            cli.stdout.first().copied(),
            Some(tag.as_bytes()[0]),
            "the expected frame kind for {rel}: {:?}",
            String::from_utf8_lossy(&cli.stdout)
        );
        assert!(
            cli.stderr.is_empty(),
            "an lstat outcome must not explain itself on stderr: {:?}",
            String::from_utf8_lossy(&cli.stderr)
        );
    }

    // Restore so the fixture directory can be removed.
    std::fs::set_permissions(
        h.root.join("locked"),
        std::fs::Permissions::from_mode(0o755),
    )
    .expect("restore the locked dir");
}
