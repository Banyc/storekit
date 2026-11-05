//! The far-side CLI's recursive sync manifest walk (`remote-cli-manifest`):
//! `storekit-remote manifest <root>` runs through `SshTransport` when the far
//! side has a compatible binary, and the existing `perl -e
//! <remote_tree_verify_script> <root>` command runs when it does not.
//!
//! TWO KINDS OF TEST, both in this file:
//!
//! * DIFFERENTIAL tests run the EXACT logged script and the CLI on the same
//!   fixture and compare stdout BYTES, stderr BYTES and the exit status. The
//!   script is never re-typed here: it is the command the transport itself
//!   logged while the CLI was absent, so a divergence in the record format, the
//!   hashes, the ordering, or one of the `die` diagnostics and statuses fails.
//!   Every refusal is driven through the REAL transport so the near-side
//!   producer's command is the one under test.
//! * TRANSPORT-LEVEL tests assert the ACTUAL logged command: the CLI form with
//!   the binary present (and no `perl` for that operation), the script form with
//!   it absent, exactly ONE probe per transport across repeated manifest reads,
//!   and that the manifest the sync assembles is the SAME in both modes.
//!
//! WHAT THE SHIM DOES NOT PROVE. No network, no `sshd`, no OpenSSH option
//! parsing, no remote filesystem semantics: the far side is the SAME host. The
//! manifest walk's own platform bound (a path deeper than the platform's limit)
//! is not exercised here.
#![cfg(unix)]
// Test-only fixtures drive the same name-mutating primitives the funnel guards.
#![allow(clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use storekit::env::SysEnv;
use storekit::transport::{Layout, SshTransport};

/// The environment variable the shim reads to find its "remote working
/// directory" (the directory a remote login shell would start in).
const SHIM_WORK_VAR: &str = "STOREKIT_SSH_SHIM_WORK";
/// The environment variable the shim appends each remote command to.
const SHIM_LOG_VAR: &str = "STOREKIT_SSH_SHIM_LOG";

/// The log is NUL-TERMINATED: the manifest script spans several lines, so a
/// newline is not an invocation boundary but NUL cannot occur in a command.
const SHIM_SCRIPT: &str = r#"#!/bin/sh
# Test-only `ssh` shim: it never opens a network connection. It records the
# remote command string (the FINAL argument, `bash -c '<script>'`) and then
# runs it in $STOREKIT_SSH_SHIM_WORK with stdin/stdout/stderr connected exactly
# as the operation connects them.
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
/// remote root inside it.
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
            .prefix("storekit-remote-manifest-")
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

    /// A transport over `root` whose every child sees exactly this environment
    /// snapshot: the shim first on `PATH`, the short `TMPDIR`, the shim's work
    /// dir, and its log. With the CLI absent the probe's 127 selects the
    /// scripts; a test that called [`Harness::expose_real_cli`] first selects
    /// the CLI.
    fn transport_rooted(&self, root: &Path) -> SshTransport {
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
            root,
            Layout::empty(),
            Some(Path::new("/dev/null")),
            None,
            &self.tmp.path().join("knownhosts"),
            &env,
            false,
        )
        .expect("construct the shimmed ssh transport")
    }

    /// A transport rooted at the harness's `remote` directory.
    fn transport(&self) -> SshTransport {
        self.transport_rooted(&self.root)
    }

    /// Every remote command the shim was handed, in order (the log is
    /// NUL-terminated, so a multi-line command stays ONE entry).
    fn log(&self) -> Vec<String> {
        let bytes = std::fs::read(&self.log_path).unwrap_or_default();
        bytes
            .split(|&byte| byte == 0)
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
}

/// The transport's ONE quoting rule (`shell_quote`): a single-quoted word with
/// an embedded `'` written as `'\''`. Re-derived here so the expected logged
/// command is checked against an INDEPENDENT encoding, not the actual one.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The exact remote command string `SshTransport::exec` builds for `argv`: every
/// element quoted and joined behind `exec -- `, then the whole command quoted
/// again by `ssh_command_argv`'s `bash -c` wrapper. The manifest producers run
/// through `exec`, so a test that asserts the CLI/script argv asserts this
/// shape.
fn expected_exec_command(argv: &[String]) -> String {
    let words: Vec<String> = argv.iter().map(|arg| shell_quote(arg)).collect();
    let command = format!("exec -- {}", words.join(" "));
    format!("bash -c {}", shell_quote(&command))
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

/// Run `cmd` with a DEADLINE: a walk that never returns is killed and the test
/// fails rather than hanging.
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

/// The EXACT remote command the transport runs for a manifest of `root` with
/// the CLI ABSENT: the script form, taken from the transport's OWN log. The
/// transport call's outcome is deliberately not asserted — a refusal case's
/// walk exits nonzero and the near-side producer reports it, but the command is
/// logged either way.
fn logged_script_command(h: &Harness, root: &Path) -> String {
    let transport = h.transport_rooted(root);
    let _ = storekit::sync::remote_manifest(&transport);
    let command = h.last_command();
    assert!(
        command.contains("perl") && command.contains("manifest"),
        "the absent CLI must log the perl manifest script, got {command:?}"
    );
    command
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

/// The differential over `root`: the script command from the transport's log
/// versus `storekit-remote manifest <root>`.
fn manifest_case(h: &Harness, case: &str, root: &Path) {
    let command = logged_script_command(h, root);
    let script = run_sh(&command);
    let cli_run = run_bounded(cli(&[OsStr::new("manifest"), root.as_os_str()]), 120);
    assert_parity(case, &script, &cli_run);
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

/// Whether this process is root, which bypasses the permission checks the
/// EACCES fixtures need.
fn running_as_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// Create a directory entry with arbitrary RAW bytes as its name. Returns
/// `false` (after announcing a skip) when the HOST filesystem refuses the name
/// (macOS APFS answers EILSEQ for a name that is not valid Unicode), because
/// that is a property of the fixture's filesystem, not of the code under test.
fn make_raw_name(path: &Path) -> bool {
    match std::os::unix::fs::symlink("target", path) {
        Ok(()) => true,
        Err(error) if error.raw_os_error() == Some(libc::EILSEQ) => {
            announce_skip("the host filesystem refuses the fixture's raw name (EILSEQ)");
            false
        }
        Err(error) => panic!("symlink {path:?}: {error}"),
    }
}

/// Build the representative tree the walk is specified over: regular files
/// (empty, non-empty), directories, an EMPTY directory, symlinks (a file, a
/// directory, a dangling target), several permission modes, non-ASCII NFC
/// names, and a hard link.
fn build_representative(root: &Path) {
    std::fs::create_dir_all(root.join("sub/deeper")).expect("create the nested dir");
    std::fs::create_dir_all(root.join("empty")).expect("create the empty dir");
    std::fs::write(root.join("plain"), b"hello").expect("write a file");
    std::fs::write(root.join("zero"), b"").expect("write an empty file");
    std::fs::write(root.join("sub/nested"), b"nested bytes").expect("write a nested file");
    std::fs::write(root.join("sub/deeper/leaf"), b"leaf").expect("write a deep file");
    std::fs::write(root.join("café"), b"nfc name").expect("write an NFC-named file");
    std::fs::write(root.join("日本語"), b"more").expect("write a CJK-named file");
    std::fs::write(root.join("sub/\u{00e9}clair"), b"nested nfc").expect("write a nested NFC name");
    std::fs::set_permissions(root.join("plain"), std::fs::Permissions::from_mode(0o600))
        .expect("chmod a file");
    std::fs::set_permissions(root.join("sub"), std::fs::Permissions::from_mode(0o750))
        .expect("chmod a directory");
    std::fs::set_permissions(root.join("zero"), std::fs::Permissions::from_mode(0o4755))
        .expect("chmod a setuid file");
    std::os::unix::fs::symlink("plain", root.join("link")).expect("create a file symlink");
    std::os::unix::fs::symlink("sub", root.join("dirlink")).expect("create a dir symlink");
    std::os::unix::fs::symlink("../plain", root.join("sub/up")).expect("create a nested symlink");
    std::os::unix::fs::symlink("no-such-target", root.join("dangling"))
        .expect("create a dangling symlink");
    std::fs::hard_link(root.join("plain"), root.join("plain-again")).expect("create a hard link");
}

/// Build a tree the NEAR-SIDE assembler accepts (relative in-root symlink
/// targets, no hard link, no special file, NFC names), so the transport-level
/// tests can compare the manifest the sync actually assembles.
fn build_assemblable(root: &Path) {
    std::fs::create_dir_all(root.join("nested")).expect("create the nested dir");
    std::fs::write(root.join("file"), b"contents").expect("write a file");
    std::fs::write(root.join("empty"), b"").expect("write an empty file");
    std::fs::write(root.join("nested/leaf"), b"leaf").expect("write a nested file");
    std::fs::write(root.join("café"), b"nfc").expect("write an NFC-named file");
    std::fs::set_permissions(root.join("nested"), std::fs::Permissions::from_mode(0o750))
        .expect("chmod the nested dir");
    std::os::unix::fs::symlink("file", root.join("link")).expect("create a symlink");
    std::os::unix::fs::symlink("../file", root.join("nested/up")).expect("create a nested symlink");
}

/// The `manifest` differential over a representative tree: every record's
/// BYTES (path, kind, mode, nlink, hash, target), the order, stdout, stderr and
/// the exit status all match the logged script.
#[test]
fn manifest_cli_is_byte_identical_to_the_script_for_a_representative_tree() {
    let h = Harness::new();
    std::fs::create_dir_all(&h.root).expect("create the remote root");
    build_representative(&h.root);

    let command = logged_script_command(&h, &h.root);
    let script = run_sh(&command);
    let cli_run = run_bounded(cli(&[OsStr::new("manifest"), h.root.as_os_str()]), 120);
    assert_parity("representative tree", &script, &cli_run);

    // NON-VACUITY: two empty streams would also be "identical". The fixture has
    // fifteen entries, and the CLI's stream must carry every one of them with
    // the real hashes.
    assert_eq!(
        cli_run.status.code(),
        Some(0),
        "the walk succeeds: {cli_run:?}"
    );
    let records: Vec<&str> = std::str::from_utf8(&cli_run.stdout)
        .expect("the fixture names are UTF-8")
        .lines()
        .collect();
    assert_eq!(
        records.len(),
        15,
        "one record per fixture entry, got {records:#?}"
    );
    // sha256("hello"), the content of `plain`.
    assert!(
        cli_run
            .stdout
            .windows(64)
            .any(|window| window
                == b"2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"),
        "the content hash of `plain` must be in the stream: {records:#?}"
    );
    assert!(
        records.iter().any(|record| {
            let fields: Vec<&str> = record.split('\t').collect();
            fields[0] == "dangling" && fields[1] == "l" && fields[5] == "no-such-target"
        }),
        "a dangling symlink's record carries its target: {records:#?}"
    );
}

/// A FIFO is classified `o` and its content is NEVER opened: the script prints
/// an empty hash and target without blocking, and the CLI must do the same. The
/// bounded run turns a blocking open into a test failure rather than a hang.
#[test]
fn manifest_cli_classifies_a_fifo_without_opening_it() {
    let h = Harness::new();
    std::fs::create_dir_all(&h.root).expect("create the remote root");
    std::fs::write(h.root.join("plain"), b"x").expect("write a file");
    let status = Command::new("mkfifo")
        .arg(h.root.join("pipe"))
        .status()
        .expect("run mkfifo");
    assert!(status.success(), "mkfifo must succeed");

    let command = logged_script_command(&h, &h.root);
    let script = run_sh(&command);
    let cli_run = run_bounded(cli(&[OsStr::new("manifest"), h.root.as_os_str()]), 60);
    assert_parity("fifo", &script, &cli_run);

    let text = String::from_utf8(cli_run.stdout).expect("the fixture names are UTF-8");
    let fifo = text
        .lines()
        .find(|line| line.split('\t').next() == Some("pipe"))
        .expect("a record for the fifo");
    let fields: Vec<&str> = fifo.split('\t').collect();
    assert_eq!(fields.len(), 6, "six fields: {fifo:?}");
    assert_eq!(fields[1], "o", "a FIFO is the `o` kind: {fifo:?}");
    assert_eq!(fields[4], "", "an `o` entry has no content hash: {fifo:?}");
    assert_eq!(fields[5], "", "an `o` entry has no target: {fifo:?}");
}

/// The hard-link rule is the NEAR SIDE's, not the walk's: the script prints the
/// entry with its `nlink` and exits 0, and the CLI must do exactly that. This
/// pins the absence of a hard-link `die` here, so nobody "helpfully" adds one
/// and makes the CLI observably different from the script it replaces.
#[test]
fn manifest_cli_does_not_refuse_a_hard_link_like_the_script() {
    let h = Harness::new();
    std::fs::create_dir_all(&h.root).expect("create the remote root");
    std::fs::write(h.root.join("original"), b"same inode").expect("write the file");
    std::fs::hard_link(h.root.join("original"), h.root.join("sibling")).expect("link the file");

    let command = logged_script_command(&h, &h.root);
    let script = run_sh(&command);
    let cli_run = run_bounded(cli(&[OsStr::new("manifest"), h.root.as_os_str()]), 120);
    assert_parity("hard link", &script, &cli_run);

    assert_eq!(
        script.status.code(),
        Some(0),
        "the script does not die: {script:?}"
    );
    let text = String::from_utf8(cli_run.stdout).expect("the fixture names are UTF-8");
    for name in ["original", "sibling"] {
        let line = text
            .lines()
            .find(|line| line.split('\t').next() == Some(name))
            .unwrap_or_else(|| panic!("no record for {name} in {text:?}"));
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields.len(), 6, "six fields: {line:?}");
        assert_eq!(fields[1], "f", "a regular file: {line:?}");
        assert_eq!(fields[3], "2", "nlink is reported, not refused: {line:?}");
    }
}

/// A name that Perl's `utf8::decode` accepts but `str::from_utf8` rejects: an
/// EXTENDED-UTF-8 spelling of a surrogate, a code point above U+10FFFF, and the
/// 5-, 6- and 7-byte forms. Perl's `Unicode::Normalize` returns such a string
/// UNCHANGED, so the script's `NFC($c) ne $c` does NOT fire: it PRINTS the
/// record and exits 0 (measured on perl 5.34/5.42). The CLI must print the same
/// RAW name bytes rather than refuse the name as not NFC.
///
/// Only a byte-preserving filesystem can host these names: APFS refuses them
/// with EILSEQ, so that host announces a skip instead of pretending to pass.
#[test]
fn manifest_cli_prints_extended_utf8_names_like_the_script() {
    let h = Harness::new();
    let root = h.root.join("extended-utf8-names");
    std::fs::create_dir_all(&root).expect("create the extended-UTF-8 dir");
    // Each is a complete, non-overlong sequence that Perl's decoder accepts.
    let forms: [(&str, &[u8]); 5] = [
        ("surrogate U+D800", &[0xED, 0xA0, 0x80]),
        ("above U+10FFFF", &[0xF4, 0x90, 0x80, 0x80]),
        ("five-byte form", &[0xF8, 0x88, 0x80, 0x80, 0x80]),
        ("six-byte form", &[0xFC, 0x84, 0x80, 0x80, 0x80, 0x80]),
        (
            "seven-byte form",
            &[0xFE, 0x82, 0x80, 0x80, 0x80, 0x80, 0x80],
        ),
    ];
    for (tag, raw) in forms {
        let mut path = root.as_os_str().as_bytes().to_vec();
        path.push(b'/');
        path.extend_from_slice(raw);
        let path = PathBuf::from(OsStr::from_bytes(&path));
        match std::os::unix::fs::symlink("target", &path) {
            Ok(()) => {}
            Err(error) if error.raw_os_error() == Some(libc::EILSEQ) => {
                announce_skip(&format!(
                    "the host filesystem refuses the extended-UTF-8 name ({tag}, EILSEQ)"
                ));
                return;
            }
            Err(error) => panic!("symlink {path:?}: {error}"),
        }
    }

    let command = logged_script_command(&h, &root);
    let script = run_sh(&command);
    let cli_run = run_bounded(cli(&[OsStr::new("manifest"), root.as_os_str()]), 120);
    assert_parity("extended-UTF-8 names", &script, &cli_run);

    // NON-VACUITY: the script ACCEPTS every name (no `die`), and both streams
    // carry a RAW record for each. Without this, two equal empty streams would
    // also satisfy the parity check.
    assert_eq!(
        script.status.code(),
        Some(0),
        "the script prints these names and exits 0: {script:?}"
    );
    assert!(
        script.stderr.is_empty(),
        "the script does not die on a Perl-valid extended name: {script:?}"
    );
    for (tag, raw) in forms {
        let mut symlink_record = raw.to_vec();
        symlink_record.extend_from_slice(b"\tl\t");
        assert!(
            script
                .stdout
                .windows(symlink_record.len())
                .any(|window| window == symlink_record),
            "the script prints {tag} as raw record bytes: {:?}",
            String::from_utf8_lossy(&script.stdout)
        );
        assert!(
            cli_run
                .stdout
                .windows(symlink_record.len())
                .any(|window| window == symlink_record),
            "the CLI prints {tag} as raw record bytes: {:?}",
            String::from_utf8_lossy(&cli_run.stdout)
        );
    }
    assert_eq!(
        script.stdout.iter().filter(|&&byte| byte == b'\n').count(),
        forms.len(),
        "one record per extended-UTF-8 name: {:?}",
        String::from_utf8_lossy(&script.stdout)
    );
}

/// A name that MIXES an extended code point with a decomposable valid run:
/// Perl's normalizer passes the extended code point through and normalizes
/// each valid run on either side of it, so `NFC($c) ne $c` FIRES. The script
/// prints the NFC diagnostic and exits 255 with EMPTY stdout; a blanket accept
/// of every byte string `str::from_utf8` rejects would print the record and
/// exit 0 here. The third name mixes the same extended code point with a
/// valid run that is already NFC, so both sides still accept it: the boundary
/// is the decomposable run, not the extended code point.
///
/// Measured on perl 5.34/5.42. Only a byte-preserving filesystem can host
/// these names: APFS refuses them with EILSEQ, so that host announces a skip.
#[test]
fn manifest_cli_judges_mixed_extended_and_decomposed_names_like_the_script() {
    let h = Harness::new();
    let base = h.root.join("mixed-extended-names");
    std::fs::create_dir_all(&base).expect("create the mixed-name root");
    // Each in its own directory: the walk dies on the first offending name, so
    // one directory per name keeps the refusal attributable.
    let cases: [(&str, &[u8], bool); 3] = [
        (
            "decomposed run before a surrogate",
            &[0x65, 0xCC, 0x81, 0xED, 0xA0, 0x80],
            false,
        ),
        (
            "surrogate before a decomposed run",
            &[0xED, 0xA0, 0x80, 0x65, 0xCC, 0x81],
            false,
        ),
        (
            "surrogate before an NFC-stable run",
            &[0xED, 0xA0, 0x80, 0x65],
            true,
        ),
    ];
    let mut roots: Vec<(&str, &[u8], bool, PathBuf)> = Vec::new();
    for (index, (tag, raw, nfc_stable)) in cases.into_iter().enumerate() {
        let dir = base.join(format!("case-{index}"));
        std::fs::create_dir_all(&dir).expect("create a mixed-name case dir");
        let mut path = dir.as_os_str().as_bytes().to_vec();
        path.push(b'/');
        path.extend_from_slice(raw);
        if !make_raw_name(&PathBuf::from(OsStr::from_bytes(&path))) {
            return;
        }
        roots.push((tag, raw, nfc_stable, dir));
    }

    for (tag, raw, nfc_stable, dir) in &roots {
        let command = logged_script_command(&h, dir);
        let script = run_sh(&command);
        let cli_run = run_bounded(cli(&[OsStr::new("manifest"), dir.as_os_str()]), 120);
        assert_parity(tag, &script, &cli_run);

        if *nfc_stable {
            // NON-VACUITY for the acceptance side: the script prints the RAW
            // name and exits 0.
            assert_eq!(
                script.status.code(),
                Some(0),
                "{tag}: the script prints an NFC-stable name: {script:?}"
            );
            assert!(script.stderr.is_empty(), "{tag}: no diagnostic: {script:?}");
            let mut record = raw.to_vec();
            record.extend_from_slice(b"\tl\t");
            assert!(
                script
                    .stdout
                    .windows(record.len())
                    .any(|window| window == record),
                "{tag}: the script prints {raw:02x?} as raw record bytes: {:?}",
                String::from_utf8_lossy(&script.stdout)
            );
            assert_eq!(
                script.stdout.iter().filter(|&&byte| byte == b'\n').count(),
                1,
                "{tag}: one record: {:?}",
                String::from_utf8_lossy(&script.stdout)
            );
        } else {
            // NON-VACUITY for the refusal side: EMPTY stdout, the exact NFC
            // diagnostic carrying the RAW name bytes, and exit 255.
            assert_eq!(
                script.status.code(),
                Some(255),
                "{tag}: the script dies on a decomposable run beside a Perl-valid extended code point: {script:?}"
            );
            assert!(
                script.stdout.is_empty(),
                "{tag}: the script refuses before printing: {script:?}"
            );
            let mut expected = b"entry name under ".to_vec();
            expected.extend_from_slice(dir.as_os_str().as_bytes());
            expected.extend_from_slice(b" is not NFC-normalized: ");
            expected.extend_from_slice(raw);
            expected.push(b'\n');
            assert_eq!(
                script.stderr, expected,
                "{tag}: the script's diagnostic carries the raw name bytes"
            );
        }
    }
}

/// Each refusal: the SAME stdout bytes already emitted, the SAME stderr
/// diagnostic and the SAME exit status as the script.
#[test]
fn manifest_cli_refuses_each_case_like_the_script() {
    let h = Harness::new();
    std::fs::create_dir_all(&h.root).expect("create the remote root");

    // An ABSENT root: the script's `absent:` arm, exiting with the stat errno.
    let absent = h.root.join("absent");
    manifest_case(&h, "absent root", &absent);

    // A NON-DIRECTORY root: `not a directory:`, exiting 2 (perl's module load
    // leaves `$!` = ENOENT, which is the script's `die` status).
    std::fs::write(h.root.join("a-file"), b"x").expect("write the fixture file");
    manifest_case(&h, "non-directory root", &h.root.join("a-file"));

    // A non-NFC name (decomposed `é`): valid UTF-8, refused by the NFC check.
    let non_nfc = h.root.join("non-nfc");
    std::fs::create_dir_all(&non_nfc).expect("create the non-NFC dir");
    std::os::unix::fs::symlink("target", non_nfc.join("e\u{0301}"))
        .expect("create the non-NFC name");
    manifest_case(&h, "non-NFC name", &non_nfc);

    // A name containing each WIRE-UNREPRESENTABLE character. NUL is impossible
    // in a POSIX name, so the other three are the reachable cases.
    for (tag, byte) in [
        ("tab", b'\t'),
        ("newline", b'\n'),
        ("carriage return", b'\r'),
    ] {
        let dir = h.root.join(format!("name-{tag}"));
        std::fs::create_dir_all(&dir).expect("create the bad-name dir");
        // A good entry FIRST, so the refusal happens after records were printed.
        std::fs::write(dir.join("good"), b"ok").expect("write a good entry");
        let mut raw = b"bad".to_vec();
        raw.push(byte);
        raw.extend_from_slice(b"tail");
        let mut path = dir.as_os_str().as_bytes().to_vec();
        path.push(b'/');
        path.extend_from_slice(&raw);
        std::fs::write(OsStr::from_bytes(&path), b"x").expect("create the bad name");
        manifest_case(&h, &format!("{tag} in a name"), &dir);
    }

    // A non-UTF-8 name: only creatable where the filesystem allows arbitrary
    // bytes (APFS refuses it with EILSEQ, so that host skips this case).
    let bad_utf8 = h.root.join("bad-utf8-name");
    std::fs::create_dir_all(&bad_utf8).expect("create the bad-UTF-8 dir");
    let mut raw = b"bad".to_vec();
    raw.push(0xFF);
    let mut path = bad_utf8.as_os_str().as_bytes().to_vec();
    path.push(b'/');
    path.extend_from_slice(&raw);
    if make_raw_name(&PathBuf::from(OsStr::from_bytes(&path))) {
        manifest_case(&h, "non-UTF-8 name", &bad_utf8);
    }

    // A SURROGATE-encoded name: Perl's `utf8::decode` ACCEPTS it (its extended
    // UTF-8) and Perl's NFC leaves it unchanged, so the script PRINTS the
    // record; the CLI must print the same bytes. Only a filesystem that allows
    // arbitrary bytes can host the fixture.
    let surrogate = h.root.join("surrogate-name");
    std::fs::create_dir_all(&surrogate).expect("create the surrogate-name dir");
    let mut path = surrogate.as_os_str().as_bytes().to_vec();
    path.push(b'/');
    path.extend_from_slice(&[0xED, 0xA0, 0x80]);
    if make_raw_name(&PathBuf::from(OsStr::from_bytes(&path))) {
        manifest_case(&h, "surrogate-encoded name", &surrogate);
    }

    // Symlink TARGETS with each wire character, and one with invalid UTF-8.
    for (tag, target) in [
        ("tab", b"tar\tget".to_vec()),
        ("newline", b"tar\nget".to_vec()),
        ("carriage return", b"tar\rget".to_vec()),
        ("invalid UTF-8", b"tar\xFFget".to_vec()),
    ] {
        let dir = h.root.join(format!("target-{tag}"));
        std::fs::create_dir_all(&dir).expect("create the target dir");
        std::fs::write(dir.join("good"), b"ok").expect("write a good entry");
        std::os::unix::fs::symlink(OsStr::from_bytes(&target), dir.join("link"))
            .expect("create the symlink");
        manifest_case(&h, &format!("{tag} in a target"), &dir);
    }

    // A SURROGATE-encoded target is ACCEPTED (Perl-valid): the record carries
    // the RAW target bytes and their hash, so acceptance is part of parity too.
    let surrogate_target = h.root.join("surrogate-target");
    std::fs::create_dir_all(&surrogate_target).expect("create the surrogate-target dir");
    std::os::unix::fs::symlink(
        OsStr::from_bytes(&[b's', 0xED, 0xA0, 0x80]),
        surrogate_target.join("link"),
    )
    .expect("create the surrogate-target symlink");
    manifest_case(&h, "surrogate-encoded target", &surrogate_target);

    // A NON-UTF-8 target: refused by the target check.
    let bad_target = h.root.join("bad-utf8-target");
    std::fs::create_dir_all(&bad_target).expect("create the bad-target dir");
    std::os::unix::fs::symlink(OsStr::from_bytes(b"x\xFF"), bad_target.join("link"))
        .expect("create the bad-target symlink");
    manifest_case(&h, "non-UTF-8 target", &bad_target);

    // An EMPTY directory exits 0 with empty stdout (the empty manifest).
    let empty = h.root.join("empty-dir");
    std::fs::create_dir_all(&empty).expect("create the empty dir");
    manifest_case(&h, "empty directory", &empty);

    if running_as_root() {
        announce_skip("running as root, the EACCES refusals below are unreachable");
        return;
    }

    // An UNREADABLE directory: `opendir <dir>: Permission denied`, after the
    // records the walk already emitted.
    let unreadable_dir = h.root.join("unreadable-dir");
    std::fs::create_dir_all(unreadable_dir.join("locked")).expect("create the locked dir");
    std::fs::write(unreadable_dir.join("first"), b"x").expect("write a first entry");
    std::fs::set_permissions(
        unreadable_dir.join("locked"),
        std::fs::Permissions::from_mode(0o000),
    )
    .expect("chmod the locked dir");
    manifest_case(&h, "unreadable directory", &unreadable_dir);
    std::fs::set_permissions(
        unreadable_dir.join("locked"),
        std::fs::Permissions::from_mode(0o755),
    )
    .expect("restore the locked dir");

    // An UNREADABLE FILE whose `lstat` succeeds: the script's one `die` without
    // a trailing newline, so perl appends ` at -e line 20.`.
    let unreadable_file = h.root.join("unreadable-file");
    std::fs::create_dir_all(&unreadable_file).expect("create the file dir");
    std::fs::write(unreadable_file.join("secret"), b"x").expect("write the secret");
    std::fs::set_permissions(
        unreadable_file.join("secret"),
        std::fs::Permissions::from_mode(0o000),
    )
    .expect("chmod the secret");
    manifest_case(&h, "unreadable file", &unreadable_file);
    std::fs::set_permissions(
        unreadable_file.join("secret"),
        std::fs::Permissions::from_mode(0o644),
    )
    .expect("restore the secret");
}

/// With a compatible CLI on the far side, the sync's manifest read runs
/// `storekit-remote manifest <root>` (no `perl` for the operation) after ONE
/// probe, and a SECOND read neither probes again nor changes the argv.
#[test]
fn manifest_uses_the_cli_when_present() {
    let h = Harness::new();
    std::fs::create_dir_all(&h.root).expect("create the remote root");
    build_assemblable(&h.root);
    h.expose_real_cli();
    let transport = h.transport();

    let via_cli = storekit::sync::remote_manifest(&transport).expect("the CLI manifest must parse");
    assert_eq!(
        storekit::manifest::canonicalize_tree(&h.root).expect("local walk"),
        via_cli,
        "the CLI manifest must be the real tree"
    );

    let log = h.log();
    assert_eq!(log.len(), 2, "probe + one op, got {log:?}");
    assert_eq!(log[0], "bash -c 'storekit-remote protocol'");
    assert_eq!(
        log[1],
        expected_exec_command(&[
            "storekit-remote".to_string(),
            "manifest".to_string(),
            h.root.display().to_string(),
        ]),
        "the op must run the CLI form, exactly"
    );
    assert!(
        !log[1].contains("perl"),
        "a compatible CLI must not fall back to perl: {:?}",
        log[1]
    );
    assert!(
        log[1].contains(&format!("'{}'", h.root.display())),
        "the CLI is handed the SAME far-side root the script would get: {:?}",
        log[1]
    );

    let again = storekit::sync::remote_manifest(&transport).expect("a second CLI manifest");
    assert_eq!(again, via_cli, "a second read describes the same tree");
    let log = h.log();
    assert_eq!(log.len(), 3, "the second op must not probe: {log:?}");
    assert_eq!(log[2], log[1], "the second op runs the same CLI form");
    assert_eq!(
        log.iter().filter(|line| line.contains("protocol")).count(),
        1,
        "exactly ONE probe per transport: {log:?}"
    );
}

/// Without a CLI (the probe's 127), the sync's manifest read runs EXACTLY the
/// original perl script and assembles the same manifest.
#[test]
fn manifest_falls_back_to_the_script_when_absent() {
    let h = Harness::new();
    std::fs::create_dir_all(&h.root).expect("create the remote root");
    build_assemblable(&h.root);
    let transport = h.transport();

    let via_script =
        storekit::sync::remote_manifest(&transport).expect("the script manifest must parse");
    assert_eq!(
        storekit::manifest::canonicalize_tree(&h.root).expect("local walk"),
        via_script,
        "the script manifest must be the real tree"
    );

    let log = h.log();
    assert_eq!(log.len(), 2, "probe + one op, got {log:?}");
    assert_eq!(log[0], "bash -c 'storekit-remote protocol'");
    assert_eq!(
        log[1],
        expected_exec_command(&[
            "perl".to_string(),
            "-e".to_string(),
            storekit::manifest::remote_tree_verify_script().to_string(),
            h.root.display().to_string(),
        ]),
        "the fallback must run the existing perl manifest script, exactly"
    );
}

/// The sync sees the SAME manifest in both modes: the wire stream and the
/// near-side assembler agree, so the producer switch is invisible above the
/// transport.
#[test]
fn the_sync_manifest_is_identical_in_both_modes() {
    let h = Harness::new();
    std::fs::create_dir_all(&h.root).expect("create the remote root");
    build_assemblable(&h.root);

    let script_transport = h.transport();
    let via_script =
        storekit::sync::remote_manifest(&script_transport).expect("the script manifest must parse");

    h.expose_real_cli();
    let cli_transport = h.transport();
    let via_cli =
        storekit::sync::remote_manifest(&cli_transport).expect("the CLI manifest must parse");

    assert!(
        !via_script.entries.is_empty(),
        "the fixture must produce a non-empty manifest: {via_script:?}"
    );
    assert_eq!(
        via_script, via_cli,
        "the CLI and the script must assemble the SAME manifest"
    );
    let local = storekit::manifest::canonicalize_tree(&h.root).expect("local walk");
    assert_eq!(
        local, via_cli,
        "and both must be the tree the local canonicalizer describes"
    );
    assert!(
        via_cli
            .entries
            .iter()
            .any(|entry| entry.path == "nested/leaf"),
        "the manifest carries the nested file: {via_cli:?}"
    );
}
