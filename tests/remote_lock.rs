//! REAL-SSHD evidence for the far-side operation lock
//! (`DestinationOwnership::lock_remote` / [`storekit::transport::FarSideLockSession`]).
//!
//! # Why a real `sshd`, and not the `PATH`-shim harness
//!
//! `tests/ssh_farside_quoting.rs` and `tests/ssh_farside_portability.rs` point
//! the transport at an `ssh` SHIM on `PATH` that runs the constructed command
//! string locally. That shim removes exactly the mechanism this feature
//! depends on: there is no SSH connection, no long-lived remote process, and no
//! connection whose death could release a far-side `flock`. A shim-based test
//! therefore cannot say anything about what this feature does. This file stands
//! up a REAL, self-contained `sshd` instead:
//!
//! * its OWN host key and its OWN client key (`ssh-keygen`, ed25519);
//! * its OWN `AuthorizedKeysFile` and its OWN `known_hosts` (via `ssh-keyscan`);
//! * a NON-DEFAULT, dynamically chosen port on `127.0.0.1`, run as the current
//!   (non-root) user under the test's `tempfile::TempDir`;
//! * nothing in `/etc/ssh`, nothing in the real `~/.ssh`, never port 22.
//!
//! The harness's `Drop` shuts the listener down and asks every OpenSSH
//! `ControlMaster` under the test's own `TMPDIR` to exit, so the test leaves no
//! processes behind. The temp dir (host key, client key, keys, config, log) is
//! removed with the `TempDir`.
//!
//! These tests are `#[cfg(unix)]`, like the crate's other ssh tests. They are
//! gated behind `STOREKIT_FULL_TESTS` ONLY in the sense that they need `sshd`,
//! `ssh`, `ssh-keygen`, `ssh-keyscan`, and `perl` on `PATH`; if `sshd` is
//! genuinely absent the harness reports that clearly rather than silently
//! passing.
#![cfg(unix)]
// Test-only fixtures drive the same name-mutating primitives the funnel guards;
// the production name-mutation rule does not apply to this test crate.
#![allow(clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use storekit::env::SysEnv;
use storekit::error::{Error, PreflightKind};
use storekit::lock::FileLock;
use storekit::sync::{
    DestinationOwnership, Direction, Extraneous, ReplaceAll, SyncResult, destination_lock_path,
    sync,
};
use storekit::transport::{Layout, SshTransport};

/// Locate the real `sshd`. Both supported platforms install it at
/// `/usr/sbin/sshd`; fall back to a PATH lookup.
fn find_sshd() -> PathBuf {
    let candidate = PathBuf::from("/usr/sbin/sshd");
    if candidate.is_file() {
        return candidate;
    }
    for dir in std::env::var("PATH").unwrap_or_default().split(':') {
        let p = Path::new(dir).join("sshd");
        if p.is_file() {
            return p;
        }
    }
    panic!(
        "these tests require a real `sshd` (looked in /usr/sbin/sshd and on PATH); \
         install openssh-server or run them on a host that has it"
    );
}

/// A self-contained, non-privileged `sshd` plus the client-side identity the
/// crate's `SshTransport` uses to reach it.
struct TestSshd {
    dir: tempfile::TempDir,
    child: Child,
    port: u16,
    user: String,
    client_key: PathBuf,
    known_hosts: PathBuf,
    /// The per-test `TMPDIR` handed to every child the transport spawns, so the
    /// `ControlMaster` mux sockets live under this harness and can be shut down
    /// on drop.
    tmpdir: PathBuf,
}

impl TestSshd {
    fn start() -> TestSshd {
        // A SHORT scratch root under /tmp: the crate's `ControlMaster` socket
        // path is bounded by `sockaddr_un.sun_path`, and the macOS default
        // `$TMPDIR` (`/var/folders/...`) leaves too little room. `/tmp` is the
        // scratch prefix this file is allowed to use.
        let dir = tempfile::Builder::new()
            .prefix("sk-sshd-")
            .tempdir_in("/tmp")
            .expect("create the sshd harness tempdir under /tmp");
        let root = dir.path();
        let host_key = root.join("host_ed25519");
        let client_key = root.join("client_ed25519");
        for (key, comment) in [
            (&host_key, "storekit-host"),
            (&client_key, "storekit-client"),
        ] {
            let status = Command::new("ssh-keygen")
                .args(["-q", "-t", "ed25519", "-N", "", "-C", comment, "-f"])
                .arg(key)
                .status()
                .expect("run ssh-keygen");
            assert!(status.success(), "ssh-keygen failed for {key:?}");
        }
        let authorized = root.join("authorized_keys");
        std::fs::copy(root.join("client_ed25519.pub"), &authorized)
            .expect("install authorized_keys");
        set_mode(&authorized, 0o600);
        set_mode(&host_key, 0o600);

        let tmpdir = root.join("tmp");
        std::fs::create_dir_all(&tmpdir).expect("create the harness TMPDIR");

        let user = whoami();
        // The ed25519 blob our OWN sshd must present. The harness verifies the
        // listener against it, so a port stolen between `free_port` and the
        // `sshd` bind can never be mistaken for our sshd.
        let host_key_blob = host_key_blob(&host_key);
        let (child, port) = start_listener(root, &host_key, &authorized, &host_key_blob);
        let known_hosts = scan_known_hosts(root, port, &host_key_blob);
        TestSshd {
            dir,
            child,
            port,
            user,
            client_key,
            known_hosts,
            tmpdir,
        }
    }

    /// The hermetic child environment: the process env with `TMPDIR` redirected
    /// under the harness, so the crate's mux directory is per-test.
    fn env(&self) -> SysEnv {
        let mut vars: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
        vars.insert("TMPDIR".into(), self.tmpdir.as_os_str().to_os_string());
        SysEnv::from_map(vars)
    }

    /// A transport rooted at `root` (a far-side path) that reaches this sshd.
    fn transport(&self, root: &Path) -> SshTransport {
        SshTransport::new(
            &self.user,
            "127.0.0.1",
            self.port,
            root,
            Layout::empty(),
            Some(&self.known_hosts),
            None,
            &self.dir.path().join("knownhosts-cache"),
            &self.env(),
            false,
        )
        .expect("construct the real-sshd transport")
        .with_identity_file(&self.client_key)
    }
}

impl Drop for TestSshd {
    fn drop(&mut self) {
        // Ask every ControlMaster under THIS test's mux dir to exit, so the
        // harness leaves no `ssh ... [mux]` process behind. The lock sessions
        // are already dropped by the test (their `Drop` kills the client); this
        // covers the persistent masters the run's other operations created.
        let mux_dir = self.tmpdir.join("dmux");
        if let Ok(entries) = std::fs::read_dir(&mux_dir) {
            for entry in entries.flatten() {
                let socket = entry.path();
                if socket
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("mux-"))
                {
                    let _ = Command::new("ssh")
                        .arg("-o")
                        .arg(format!("ControlPath={}", socket.display()))
                        .args(["-p", &self.port.to_string()])
                        .args(["-O", "exit"])
                        .arg(format!("{}@127.0.0.1", self.user))
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                }
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
}

/// The permission bits (not the file type) of `path`, so a test can assert the
/// record's mode is untouched by a refusal and `0600` after an adoption.
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::symlink_metadata(path)
        .expect("stat the record")
        .permissions()
        .mode()
        & 0o777
}

fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "root".to_string())
}

/// Serializes every sshd start in THIS process: the test binary runs its tests
/// on parallel threads, and two overlapping `free_port` windows are exactly how
/// a sibling's sshd (or an outgoing connection's ephemeral source port) ends up
/// holding the port this harness just released.
static SSHD_START_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The ed25519 public-key blob the harness's generated host key presents.
fn host_key_blob(host_key: &Path) -> String {
    let pub_path = host_key.with_extension("pub");
    let text = std::fs::read_to_string(&pub_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", pub_path.display()));
    text.split_whitespace()
        .nth(1)
        .unwrap_or_else(|| panic!("{} has no key blob field", pub_path.display()))
        .to_string()
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    listener.local_addr().expect("local addr").port()
}

/// The outcome of waiting for the port our `sshd` was asked to bind.
enum ListenerWait {
    /// Something is accepting on the port — NOT necessarily our sshd; the
    /// caller verifies the host key.
    Open,
    /// OUR `sshd` child exited before it accepted: the port was taken before it
    /// could bind, or `sshd` failed to start.
    ChildExited(String),
    /// Nothing accepted within the timeout.
    TimedOut,
}

/// Wait for a listener on `port`, but NEVER accept success once OUR child has
/// exited: an `sshd` that lost the port race (or failed to start) must be
/// retried on a fresh port, not mistaken for a running listener.
fn wait_for_listener(child: &mut Child, port: u16, timeout: Duration) -> ListenerWait {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(Some(status)) = child.try_wait() {
            return ListenerWait::ChildExited(format!("sshd exited with {status}"));
        }
        if std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return ListenerWait::Open;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    ListenerWait::TimedOut
}

/// Does the ed25519 key the listener on `port` PRESENTS equal `expected_blob`?
///
/// `Ok(true)` — our sshd. `Ok(false)` — a DIFFERENT sshd took the port. `Err` —
/// no ed25519 key could be read (a non-`ssh` listener, or a failed
/// connection). Both non-`true` outcomes are a port TOCTOU, never an
/// authentication problem.
fn listener_presents_expected_key(port: u16, expected_blob: &str) -> Result<bool, String> {
    let out = Command::new("ssh-keyscan")
        .args([
            "-T",
            "2",
            "-p",
            &port.to_string(),
            "-t",
            "ed25519",
            "127.0.0.1",
        ])
        .output()
        .map_err(|e| format!("run ssh-keyscan: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // `host algo blob`: the third field is the key blob.
        if let Some(blob) = line.split_whitespace().nth(2) {
            return Ok(blob == expected_blob);
        }
    }
    Err(format!(
        "ssh-keyscan read no ed25519 key (status {:?}): {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).trim()
    ))
}

/// Start `sshd -D` on a fresh port, wait until it accepts, VERIFY the listener
/// is the child just spawned (by its host key), and return the child and port.
///
/// A bare "the port is open" is not success: `free_port` releases the port
/// before `sshd` binds it, so a sibling's `sshd` — or an outgoing connection's
/// ephemeral source port — can take it. Accepting such a listener would make
/// the client authenticate against a FOREIGN `AuthorizedKeysFile` and fail
/// later as an opaque `Permission denied (publickey)`. Every rejected port is
/// reported as the port TOCTOU it is.
fn start_listener(
    root: &Path,
    host_key: &Path,
    authorized: &Path,
    expected_blob: &str,
) -> (Child, u16) {
    let sshd = find_sshd();
    // Hold the process-wide start lock across port pick + spawn + verify so a
    // sibling test thread cannot take a port this thread just released.
    let _serialized = SSHD_START_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut last_error = String::new();
    for _ in 0..10 {
        let port = free_port();
        let config = root.join(format!("sshd_config.{port}"));
        let log = root.join(format!("sshd.log.{port}"));
        let pid = root.join("sshd.pid");
        let text = format!(
            "Port {port}\n\
             ListenAddress 127.0.0.1\n\
             HostKey {host}\n\
             PidFile {pid}\n\
             AuthorizedKeysFile {auth}\n\
             StrictModes no\n\
             UsePAM no\n\
             PasswordAuthentication no\n\
             KbdInteractiveAuthentication no\n\
             PubkeyAuthentication yes\n\
             PermitRootLogin no\n\
             LogLevel ERROR\n",
            host = host_key.display(),
            pid = pid.display(),
            auth = authorized.display(),
        );
        std::fs::write(&config, text).expect("write sshd_config");
        // sshd refuses a group/world-writable config, and a permissive umask
        // (e.g. the `umask 0002` gate) makes `fs::write` group-writable.
        set_mode(&config, 0o600);
        let mut child = Command::new(&sshd)
            .arg("-f")
            .arg(&config)
            .arg("-E")
            .arg(&log)
            .arg("-p")
            .arg(port.to_string())
            .arg("-D")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sshd");
        match wait_for_listener(&mut child, port, Duration::from_secs(10)) {
            ListenerWait::Open => match listener_presents_expected_key(port, expected_blob) {
                Ok(true) => return (child, port),
                Ok(false) => {
                    last_error = format!(
                        "port {port} was taken by a FOREIGN listener: it presents a different \
                         ed25519 host key than the harness sshd"
                    );
                    eprintln!("storekit real-sshd harness: {last_error}; retrying on a fresh port");
                    let _ = child.kill();
                    let _ = child.wait();
                }
                Err(e) => {
                    last_error = format!(
                        "port {port} was taken by a NON-SSH listener (no ed25519 host key could \
                         be read): {e}"
                    );
                    eprintln!("storekit real-sshd harness: {last_error}; retrying on a fresh port");
                    let _ = child.kill();
                    let _ = child.wait();
                }
            },
            ListenerWait::ChildExited(why) => {
                let _ = child.wait();
                last_error = format!(
                    "{why}: the port {port} released by free_port was taken before sshd could \
                     bind it (or sshd failed to start); log: {}",
                    std::fs::read_to_string(&log).unwrap_or_default().trim()
                );
                eprintln!("storekit real-sshd harness: {last_error}; retrying on a fresh port");
            }
            ListenerWait::TimedOut => {
                let _ = child.kill();
                let _ = child.wait();
                last_error = format!(
                    "sshd on port {port} never accepted within 10s; log: {}",
                    std::fs::read_to_string(&log).unwrap_or_default().trim()
                );
            }
        }
    }
    panic!(
        "could not start a real sshd on any tried port: the ports free_port chose kept being \
         taken by foreign listeners, or sshd failed to start; last error: {last_error}"
    );
}

fn scan_known_hosts(root: &Path, port: u16, expected_blob: &str) -> PathBuf {
    for _ in 0..50 {
        let out = Command::new("ssh-keyscan")
            .args(["-p", &port.to_string(), "-t", "ed25519", "127.0.0.1"])
            .output();
        if let Ok(out) = out
            && out.status.success()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            if text.contains("ssh-ed25519") {
                // The scanned key MUST be the harness's own. A foreign listener
                // that took the port would otherwise be trusted here and fail
                // later as an authentication error.
                if !text.contains(expected_blob) {
                    panic!(
                        "the listener on port {port} presented a FOREIGN ed25519 host key \
                         (the port was taken by another listener); refusing to trust it"
                    );
                }
                let path = root.join("known_hosts");
                std::fs::write(&path, text.as_bytes()).expect("write known_hosts");
                set_mode(&path, 0o600);
                return path;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("ssh-keyscan never returned the ed25519 host key for port {port}");
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

struct Fixture {
    harness: TestSshd,
    src: PathBuf,
    far_root: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        let harness = TestSshd::start();
        let base = harness.dir.path().join("fixture");
        std::fs::create_dir_all(&base).expect("create fixture base");
        let src = base.join("src");
        std::fs::create_dir_all(&src).expect("create source");
        std::fs::write(src.join("file"), b"payload").expect("seed source");
        let far_root = base.join("dst");
        Fixture {
            harness,
            src,
            far_root,
        }
    }

    fn record(&self) -> PathBuf {
        destination_lock_path(&self.far_root).expect("the far root has a sibling record")
    }

    /// Acquire the far-side lock for a PUSH of `self.src` into `self.far_root`.
    fn acquire(&self) -> DestinationOwnership {
        let t = self.harness.transport(&self.far_root);
        DestinationOwnership::lock_remote(Direction::Push, &self.src, &t)
            .expect("acquire the far-side lock")
    }

    fn try_acquire(&self) -> Result<DestinationOwnership, Error> {
        let t = self.harness.transport(&self.far_root);
        DestinationOwnership::lock_remote(Direction::Push, &self.src, &t)
    }

    fn sync_owned(&self, ownership: DestinationOwnership) -> SyncResult {
        let t = self.harness.transport(&self.far_root);
        sync(
            Direction::Push,
            &self.src,
            &t,
            &ReplaceAll,
            Extraneous::Keep,
            ownership,
        )
    }
}

/// Acquire the far-side lock, retrying ONLY on the typed contention outcome
/// until `timeout`. A kill releases the far-side flock asynchronously (the
/// holder process must notice the closed channel), so a test that has just
/// killed a session polls rather than assuming the release already happened.
fn acquire_after_release(fixture: &Fixture, timeout: Duration) -> DestinationOwnership {
    let deadline = Instant::now() + timeout;
    loop {
        match fixture.try_acquire() {
            Ok(o) => return o,
            Err(Error::LockContended(_)) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("unexpected acquisition failure: {e:?}"),
        }
    }
}

/// Every process whose argv STARTS with `program` and carries `fragment`.
/// The fragment is this test's unique record path, so only processes THIS test
/// started can match; `program` selects the local `ssh` client (whose argv
/// starts `ssh `) or, because the sshd is on the same host, the far-side `perl`
/// holder (whose argv starts `perl `).
fn pids_matching(program: &str, fragment: &str) -> Vec<i32> {
    let out = Command::new("ps")
        .args(["-ww", "-eo", "pid=,command="])
        .output()
        .expect("run ps");
    let text = String::from_utf8_lossy(&out.stdout);
    let me = std::process::id() as i32;
    let mut pids = Vec::new();
    for line in text.lines() {
        let line = line.trim_start();
        let mut parts = line.splitn(2, char::is_whitespace);
        let Some(pid) = parts.next() else { continue };
        let Some(cmd) = parts.next() else { continue };
        if !cmd.starts_with(program) || !cmd.contains(fragment) {
            continue;
        }
        let Ok(pid) = pid.trim().parse::<i32>() else {
            continue;
        };
        if pid != me {
            pids.push(pid);
        }
    }
    pids
}

/// SIGKILL every pid named by [`pids_matching`], returning those signalled.
fn kill_matching(program: &str, fragment: &str) -> Vec<i32> {
    let pids = pids_matching(program, fragment);
    for pid in &pids {
        let _ = Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    pids
}

// ---------------------------------------------------------------------------
// 1. Exclusion is real (a test per direction of the hand-off)
// ---------------------------------------------------------------------------

/// A holds the far-side record; B is REFUSED with the typed contended outcome
/// and its far side is untouched; A finishes; B then succeeds and runs to
/// completion.
#[test]
fn far_side_lock_excludes_a_second_owned_run() {
    let f = Fixture::new();
    let record = f.record();

    let a = f.acquire();
    assert!(
        record.exists(),
        "A's acquisition creates the far-side sibling record {record:?}"
    );
    assert!(
        !f.far_root.exists(),
        "taking the far-side lock must not create the destination root"
    );

    // B contends: the typed outcome, no wait.
    let err = f
        .try_acquire()
        .err()
        .expect("a second run must be refused while A holds the record");
    assert!(
        matches!(err, Error::LockContended(_)),
        "B must get the typed contention outcome, got {err:?}"
    );
    assert!(
        !f.far_root.exists(),
        "B's refused acquisition must leave the far side untouched"
    );

    // A finishes (drops the token, releasing the far-side record); B succeeds.
    drop(a);
    let b = acquire_after_release(&f, Duration::from_secs(20));
    let report = f
        .sync_owned(b)
        .expect("B must acquire the released record and complete the push");
    assert!(
        report.applied.iter().any(|p| p == "file"),
        "B's push applies the file: {:?}",
        report.applied
    );
    assert_eq!(
        std::fs::read(f.far_root.join("file")).expect("read the pushed file"),
        b"payload"
    );
}

/// The SAME exclusion with the roles swapped: B holds first, A is refused,
/// B finishes, A succeeds. Exclusion is symmetric, not an artifact of which
/// run acquires first.
#[test]
fn far_side_lock_exclusion_is_symmetric() {
    let f = Fixture::new();
    let b = f.acquire();
    let err = f
        .try_acquire()
        .err()
        .expect("a second run must be refused while the first holds the record");
    assert!(matches!(err, Error::LockContended(_)), "got {err:?}");
    drop(b);
    let a = acquire_after_release(&f, Duration::from_secs(20));
    f.sync_owned(a).expect("A must complete after B releases");
}

// ---------------------------------------------------------------------------
// 2. Acquisition is non-blocking
// ---------------------------------------------------------------------------

/// Acquisition against a HELD record returns PROMPTLY with the contended
/// outcome. The wait is bounded so a regression that waits can never hang the
/// suite.
#[test]
fn far_side_lock_acquisition_is_non_blocking() {
    let f = Fixture::new();
    let holder = f.acquire();

    let started = Instant::now();
    let err = f.try_acquire().err().expect("must contend, not wait");
    let elapsed = started.elapsed();
    assert!(matches!(err, Error::LockContended(_)), "got {err:?}");
    assert!(
        elapsed < Duration::from_secs(5),
        "acquisition against a held record must return promptly (typed contention, never a \
         wait), but took {elapsed:?}"
    );
    drop(holder);
}

// ---------------------------------------------------------------------------
// 3. Release on every path (a failed run releases)
// ---------------------------------------------------------------------------

/// A run that FAILS after the lock is taken (the far-side destination root is
/// a regular file, so `provision_layout`'s `mkdir -p` fails) releases the
/// record: a subsequent acquisition succeeds.
#[test]
fn far_side_lock_is_released_after_a_failed_run() {
    let f = Fixture::new();
    // The destination root is a FILE on the far side: `mkdir -p <root>` fails.
    std::fs::write(&f.far_root, b"i am a file").expect("create a file at the destination root");

    let ownership = f.acquire();
    let result = f.sync_owned(ownership);
    let error = result.expect_err("the run must fail when the destination root is a file");
    let text = error.to_string();
    assert!(
        text.contains("provision") || text.contains("layout") || text.contains("mkdir"),
        "the failure must name the provisioning stage: {text}"
    );

    // The failed run dropped its token, so the record must be released.
    let again = acquire_after_release(&f, Duration::from_secs(20));
    drop(again);
    assert_eq!(
        std::fs::read(&f.far_root).expect("the root file is untouched"),
        b"i am a file"
    );
}

// ---------------------------------------------------------------------------
// 4. Connection loss releases the record and is REPORTED
// ---------------------------------------------------------------------------

/// Killing the connection (the lock session's local `ssh` client) releases the
/// far-side record, and a run handed the dead session reports a typed
/// connection-loss failure instead of a clean success.
#[test]
fn connection_loss_releases_the_far_side_lock_and_is_reported() {
    let f = Fixture::new();
    let record = f.record();
    let ownership = f.acquire();
    assert!(
        record.exists(),
        "the record exists while the session is alive"
    );

    // Kill the HOLDER (the far-side `perl`), so the local ssh client observes
    // the session ending and has something to report. Both processes carry the
    // test's unique record path in argv, so only our own are matched.
    let fragment = record.to_string_lossy().to_string();
    assert!(
        !pids_matching("ssh ", &fragment).is_empty(),
        "premise: the lock session's local ssh client is running"
    );
    let killed = kill_matching("perl ", &fragment);
    assert!(
        !killed.is_empty(),
        "the far-side lock holder (argv carries {record:?}) must be found and killed"
    );
    // The local client exits once the far side does. Wait for it, so the
    // liveness check below observes a dead session rather than racing the exit
    // (a killed client is asynchronous).
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline && !pids_matching("ssh ", &fragment).is_empty() {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        pids_matching("ssh ", &fragment).is_empty(),
        "the lock session's ssh client must exit after the far-side holder is killed"
    );

    // The run is handed the now-dead session. Even if its operations happen to
    // succeed, `sync` must report the lost lock, never a clean success.
    let result = f.sync_owned(ownership);
    let error = result.expect_err("a run whose lock session died must not report success");
    assert_eq!(
        error.error().transport_reason(),
        Some(storekit::TransportKind::BeforeCommand),
        "the loss must be a typed transport failure: {error:?}"
    );
    let text = error.to_string();
    assert!(
        text.contains("far-side operation-lock session"),
        "the report must name the lost lock session: {text}"
    );
    assert!(
        text.contains("cannot outlive its client"),
        "the report must state the not-a-lease consequence: {text}"
    );

    // The record is released: a later run acquires it (polling the async release).
    let again = acquire_after_release(&f, Duration::from_secs(20));
    drop(again);
}

// ---------------------------------------------------------------------------
// 5. A transport that does not implement far-side locking fails closed
// ---------------------------------------------------------------------------

/// The `Remote::lock_far_side` DEFAULT refuses, so a transport that does not
/// override it cannot be used to run an unowned remote destination silently.
/// `LocalTransport` is such a transport: `lock_remote` refuses it (a local
/// destination is refused even earlier, by the local-destination check, so this
/// asserts the refusal is a `Preflight` naming the right remedy).
#[test]
fn lock_remote_refuses_a_local_destination() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(&src).expect("src");
    std::fs::write(src.join("f"), b"x").expect("seed");
    let t = storekit::transport::LocalTransport::new(
        &SysEnv::from_process(),
        dst.clone(),
        Layout::empty(),
    )
    .expect("local transport");
    let err = DestinationOwnership::lock_remote(Direction::Push, &src, &t)
        .err()
        .expect("a local destination must be refused by lock_remote");
    assert_eq!(
        err.preflight_reason(),
        Some(PreflightKind::LocalDestinationViaRemoteLock),
        "the refusal must be the typed local-destination refusal: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// 6. A foreign listener on the chosen port is DIAGNOSED, never trusted
// ---------------------------------------------------------------------------

/// The port-TOCTOU probe itself: a listener that is not our sshd must never be
/// mistaken for it. A plain TCP listener presents no ed25519 host key, so the
/// probe does not return `Ok(true)` AND names the cause; the harness then
/// retries on a fresh port and, if every port is taken, fails naming the port
/// race rather than surfacing an opaque authentication error from the wrong
/// `AuthorizedKeysFile`.
#[test]
fn a_foreign_listener_on_the_chosen_port_is_not_mistaken_for_our_sshd() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a foreign listener");
    let port = listener.local_addr().expect("local addr").port();
    let verdict = listener_presents_expected_key(port, "AAAA-not-our-key");
    match &verdict {
        Ok(true) => panic!("a foreign listener must not be accepted as the harness sshd"),
        Ok(false) => { /* a foreign sshd: a DIFFERENT ed25519 key */ }
        Err(why) => {
            assert!(
                why.contains("no ed25519 key"),
                "the port-race diagnostic must name the cause, got: {why}"
            );
            eprintln!("port {port} rejected as foreign: {why}");
        }
    }
    drop(listener);
}

// ---------------------------------------------------------------------------
// 7. The far-side holder adopts the SAME record the local arm does
// ---------------------------------------------------------------------------

/// The header text the crate writes and recognises ([`storekit::lock`]'s
/// `RECORD_HEADER`). Spelled here as the DOCUMENTED wire text so this test
/// pins the interop between the two arms, not one module's private literal.
const RECORD_HEADER: &[u8] = b"storekit lock record v1\n";

/// A pre-existing FOREIGN record at the far-side path is not a record this
/// crate wrote, so a far-side acquisition REFUSES it with the local arm's
/// typed `LockRecordNotRecognized` and leaves the entry byte-for-byte and
/// mode-for-mode untouched. PRE-FIX the holder unconditionally truncated the
/// entry and wrote a bare op id, destroying the caller's data and returning
/// success.
#[test]
fn far_side_acquisition_refuses_a_foreign_record_without_truncating_it() {
    let f = Fixture::new();
    let record = f.record();
    let mut foreign = b"a caller's data (NOT a storekit lock record)".to_vec();
    foreign.resize(51, b'.');
    assert_eq!(foreign.len(), 51, "the fixture is a 51-byte foreign record");
    std::fs::write(&record, &foreign).expect("plant the foreign record");
    set_mode(&record, 0o644);
    let before_mode = mode_of(&record);

    let err = f
        .try_acquire()
        .err()
        .expect("a far-side acquisition must refuse a pre-existing foreign record");
    assert_eq!(
        err.preflight_reason(),
        Some(PreflightKind::LockRecordNotRecognized),
        "the far-side refusal must be the SAME typed condition the local arm raises: {err:?}"
    );
    assert_eq!(
        std::fs::read(&record).expect("read the record back"),
        foreign,
        "a refused far-side acquisition must not truncate or rewrite the record"
    );
    assert_eq!(
        mode_of(&record),
        before_mode,
        "a refused far-side acquisition must not chmod the record"
    );
    assert!(
        !f.far_root.exists(),
        "the refusal must leave the far side untouched"
    );
}

/// A record the crate wrote (header + op id) is ADOPTED by the far-side holder
/// — the header survives the acquisition — and a later LOCAL
/// `FileLock::acquire` on the same path SUCCEEDS instead of refusing it with
/// `LockRecordNotRecognized`. PRE-FIX the holder rewrote the entry as a bare op
/// id, so the local arm then refused the far side's own record.
#[test]
fn a_crate_written_record_is_adopted_across_the_far_side_and_local_arms() {
    let f = Fixture::new();
    let record = f.record();
    let mut seeded = RECORD_HEADER.to_vec();
    seeded.extend_from_slice(b"pre-seeded op id");
    std::fs::write(&record, &seeded).expect("seed a record this crate wrote");
    set_mode(&record, 0o600);

    let ownership = f.acquire();
    // Snapshot what the FAR-SIDE arm wrote before the local arm rewrites it.
    let after_far_side = std::fs::read(&record).expect("read the far-side record");
    drop(ownership);

    // The flock is released asynchronously. Retry ONLY the typed contention;
    // a LockRecordNotRecognized is the defect and must fail here at once.
    let deadline = Instant::now() + Duration::from_secs(20);
    let local = loop {
        match FileLock::acquire(&record, "local after far side") {
            Ok(guard) => break guard,
            Err(Error::LockContended(_)) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!(
                "a local FileLock::acquire must accept the record the far-side arm wrote, got \
                 {e:?}"
            ),
        }
    };
    drop(local);

    // The far-side arm wrote a record the local arm recognises. PRE-FIX this
    // held a BARE op id (no header), which is why the local acquire above was
    // refused.
    assert!(
        after_far_side.starts_with(RECORD_HEADER),
        "the far-side acquisition must write the crate's record header, got: {:?}",
        String::from_utf8_lossy(&after_far_side)
    );
    assert!(
        !after_far_side[RECORD_HEADER.len()..].is_empty(),
        "the far-side holder must record its op id after the header"
    );
}

/// The far-side acquisition makes the record PRIVATE (`0600`), tightening a
/// record an earlier version left wider — the local arm's chmod, on the same
/// record. PRE-FIX the holder never chmodded, so a `0644` record stayed `0644`.
#[test]
fn a_far_side_acquisition_tightens_the_record_mode_to_0600() {
    let f = Fixture::new();
    let record = f.record();
    let mut seeded = RECORD_HEADER.to_vec();
    seeded.extend_from_slice(b"pre-seeded op id");
    std::fs::write(&record, &seeded).expect("seed a record this crate wrote");
    set_mode(&record, 0o644);
    assert_eq!(
        mode_of(&record),
        0o644,
        "premise: the record starts wider than 0600"
    );

    let ownership = f.acquire();
    assert_eq!(
        mode_of(&record),
        0o600,
        "a far-side acquisition must tighten the record to 0600 like the local arm"
    );
    drop(ownership);
}
