//! Regression: a FIFO in a store must not hang the read-side, listing, or
//! removal primitives.
//!
//! `path_state_fd`, `read_fd`/`read_json_fd`, `write_atomic_cas_fd`, and
//! `write_atomic_if_match_fd` all used to open the final entry `O_RDONLY`
//! without `O_NONBLOCK`. `open(2)` of a FIFO read-only BLOCKS until a writer
//! appears, so ONE FIFO in a store — from another tool, or an attacker — hung
//! a consumer's verify/recovery and a sync's `AppendTail` compare FOREVER: an
//! unbounded hang on user data. The primitives now open with `O_NONBLOCK` (a
//! no-op for a regular file) and classify the OPENED inode, refusing a
//! FIFO/socket/device with a clear error.
//!
//! The pre-fix failure is a HANG, so an in-process assertion cannot observe it
//! (the test process would block). These tests therefore RE-EXEC this test
//! binary as a child (behind [`MODE_ENV`]) that performs its probe, and the
//! parent enforces a HARD WALL-CLOCK TIMEOUT: a child that does not finish
//! is KILLED and reported, so the suite itself can never hang.
//!
//! Three more shapes shared the defect. [`crate::transport::Remote::list`]
//! (the Unix `_confined` arm) opened every non-symlink child `O_RDONLY` to
//! `fstat` it, so one FIFO in a listed directory blocked the listing FOREVER;
//! it now classifies each child first and NEVER opens a non-regular entry, so
//! a FIFO is LISTED as an `Other` entry — `is_dir`/`is_symlink` false with the
//! entry's own size and mode — exactly as the path-based and far-side arms
//! list it. [`crate::transport::Remote::set_mode`] (the `_confined` arm) had
//! the same blocking open and now opens with `O_NONBLOCK` and REFUSES a
//! FIFO/socket/device with the typed
//! [`crate::error::MaterializationKind::SpecialFile`] kind, never blocking and
//! never chmodding one. The removal-failure `NotFound` existence probe
//! (`remove_dir_all`) also opened `O_RDONLY` and now opens `O_NONBLOCK`, so a
//! FIFO is a prompt failure instead of a hang. Each has its OWN re-exec child
//! and hard-timeout parent below, so a pre-fix hang in any of them fails on
//! its own.
#![allow(clippy::disallowed_methods)]
#![cfg(unix)]

use std::ffi::CString;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::atomic::RootDir;
use crate::env::SysEnv;
use crate::error::MaterializationKind;
use crate::relpath::RootedRelativePath;
use crate::transport::{Layout, LocalTransport, Remote, RemoteEntry};

/// The child re-exec reads its work mode from this.
const MODE_ENV: &str = "STOREKIT_FIFO_MODE";
/// The child creates its FIFO under this directory (set by the parent).
const ROOT_ENV: &str = "STOREKIT_FIFO_ROOT";
/// The exact libtest name of [`fifo_child`] (for the child's `--exact`).
const CHILD_TEST: &str = "fifo_regression::fifo_child";
/// The child's work mode.
const CHILD_MODE: &str = "probe";
/// Printed by the child only after ALL FOUR probes returned; the parent
/// requires it so a child that ran no test can never pass this suite.
const DONE_MARKER: &str = "STOREKIT_FIFO_CHILD_DONE";
/// The exact libtest name of [`fifo_list_child`].
const LIST_CHILD_TEST: &str = "fifo_regression::fifo_list_child";
/// The list child's work mode.
const LIST_CHILD_MODE: &str = "list";
/// Printed by the list child after the listing returned AND the confined arm
/// was shown to agree with the path-based arm.
const LIST_DONE_MARKER: &str = "STOREKIT_FIFO_LIST_CHILD_DONE";
/// The exact libtest name of [`fifo_set_mode_child`].
const CHMOD_CHILD_TEST: &str = "fifo_regression::fifo_set_mode_child";
/// The set_mode child's work mode.
const CHMOD_CHILD_MODE: &str = "set_mode";
/// Printed by the set_mode child after the refusal returned with the typed
/// kind and the FIFO's mode was shown unchanged.
const CHMOD_DONE_MARKER: &str = "STOREKIT_FIFO_SET_MODE_CHILD_DONE";
/// The exact libtest name of [`fifo_remove_dir_all_child`].
const REMOVE_CHILD_TEST: &str = "fifo_regression::fifo_remove_dir_all_child";
/// The remove_dir_all child's work mode.
const REMOVE_CHILD_MODE: &str = "remove_dir_all";
/// Printed by the remove_dir_all child after the prompt refusal returned with
/// the FIFO still in place.
const REMOVE_DONE_MARKER: &str = "STOREKIT_FIFO_REMOVE_DIR_ALL_CHILD_DONE";
/// The per-probe wall-clock bound the CHILD asserts. Well above any scheduling
/// jitter, far below a hang.
const PROBE_LIMIT: Duration = Duration::from_secs(5);
/// The parent's HARD wall-clock cap on a child: the probes at [`PROBE_LIMIT`]
/// plus process startup fit comfortably; a pre-fix child (which blocks on the
/// probing open) is killed here and the test fails.
const CHILD_LIMIT: Duration = Duration::from_secs(20);

/// A FIFO in a store is refused promptly by every read-side primitive, never
/// opened blocking.
#[test]
fn fifo_entries_do_not_hang_the_read_side_primitives() {
    let tmp = crate::test_support::fixture_tmpdir(&SysEnv::from_process())
        .expect("tempdir for the FIFO probe");
    let root = tmp.path().to_path_buf();
    let stdout = assert_child_reached(
        run_child(CHILD_TEST, CHILD_MODE, &root, CHILD_LIMIT),
        CHILD_LIMIT,
        DONE_MARKER,
        "read-side primitive",
    );
    let probes = stdout
        .lines()
        .filter(|line| line.contains("FIFO_PROBE"))
        .count();
    assert_eq!(
        probes, 4,
        "all four read-side primitives must have been probed:\n{stdout}"
    );
    // Say WHICH platform ran the probe.
    println!(
        "FIFO regression probes ran on platform={} (4 primitives refused a FIFO promptly)",
        std::env::consts::OS
    );
}

/// A directory containing a FIFO LISTS promptly, and the Unix `_confined` arm
/// lists the FIFO EXACTLY as the path-based arm does (an `Other` entry: not a
/// directory, not a symlink, with its own size and mode).
#[test]
fn listing_a_directory_with_a_fifo_does_not_hang() {
    let tmp = crate::test_support::fixture_tmpdir(&SysEnv::from_process())
        .expect("tempdir for the list FIFO probe");
    let root = tmp.path().to_path_buf();
    let _stdout = assert_child_reached(
        run_child(LIST_CHILD_TEST, LIST_CHILD_MODE, &root, CHILD_LIMIT),
        CHILD_LIMIT,
        LIST_DONE_MARKER,
        "list",
    );
    println!(
        "FIFO list probe ran on platform={} (a FIFO is listed, never opened blocking)",
        std::env::consts::OS
    );
}

/// `set_mode` of a FIFO is REFUSED promptly with the typed special-file kind,
/// never blocking on `open(2)` and never chmodding the FIFO.
#[test]
fn set_mode_on_a_fifo_is_refused_not_blocked() {
    let tmp = crate::test_support::fixture_tmpdir(&SysEnv::from_process())
        .expect("tempdir for the set_mode FIFO probe");
    let root = tmp.path().to_path_buf();
    let _stdout = assert_child_reached(
        run_child(CHMOD_CHILD_TEST, CHMOD_CHILD_MODE, &root, CHILD_LIMIT),
        CHILD_LIMIT,
        CHMOD_DONE_MARKER,
        "set_mode",
    );
    println!(
        "FIFO set_mode probe ran on platform={} (a FIFO is refused, never chmodded)",
        std::env::consts::OS
    );
}

/// `remove_dir_all` of a FIFO fails promptly (a FIFO is not a directory) — its
/// `NotFound` existence probe never blocks on the FIFO — and does not remove
/// the FIFO.
#[test]
fn remove_dir_all_on_a_fifo_does_not_hang() {
    let tmp = crate::test_support::fixture_tmpdir(&SysEnv::from_process())
        .expect("tempdir for the remove_dir_all FIFO probe");
    let root = tmp.path().to_path_buf();
    let _stdout = assert_child_reached(
        run_child(REMOVE_CHILD_TEST, REMOVE_CHILD_MODE, &root, CHILD_LIMIT),
        CHILD_LIMIT,
        REMOVE_DONE_MARKER,
        "remove_dir_all",
    );
    println!(
        "FIFO remove_dir_all probe ran on platform={} (the FIFO probe does not block)",
        std::env::consts::OS
    );
}

/// The child worker: no-op unless the parent set [`MODE_ENV`], so an ordinary
/// `cargo test` only lists it as ignored. The parent re-execs the binary with
/// `--ignored --exact` and the env switch.
#[test]
#[ignore = "spawned by the FIFO regression test via MODE_ENV"]
fn fifo_child() {
    let Ok(mode) = std::env::var(MODE_ENV) else {
        return;
    };
    assert_eq!(mode, CHILD_MODE, "unexpected FIFO child mode {mode:?}");
    let root = PathBuf::from(std::env::var_os(ROOT_ENV).expect("ROOT_ENV is set by the parent"));
    std::fs::create_dir_all(&root).expect("create the FIFO root");
    let fifo = root.join("fifo");
    let c = CString::new(fifo.as_os_str().as_bytes()).expect("no NUL in the FIFO path");
    if unsafe { libc::mkfifo(c.as_ptr(), 0o600) } != 0 {
        panic!(
            "mkfifo {}: {}",
            fifo.display(),
            std::io::Error::last_os_error()
        );
    }
    let owned = RootDir::open(&root).expect("open the owned root");
    let rel = &crate::relpath::RootedRelativePath::parse(Path::new("fifo")).unwrap();

    // FOUR probes, each of which must return promptly: a FIFO must never block
    // an open.
    probe("state", || crate::atomic::path_state_fd(&owned, rel));
    probe("read", || crate::atomic::read_fd(&owned, rel));
    probe("cas", || {
        crate::atomic::write_atomic_cas_fd(&owned, rel, b"x")
    });
    probe("ifmatch", || {
        crate::atomic::write_atomic_if_match_fd(&owned, rel, b"x", b"y")
    });

    println!("{DONE_MARKER} platform={}", std::env::consts::OS);
}

/// The child worker for the `list` probe: no-op unless the parent set
/// [`MODE_ENV`] to [`LIST_CHILD_MODE`]. It lists a directory that contains a
/// FIFO through the Unix `_confined` arm (a NON-EMPTY root-relative path) and
/// requires the listing to return promptly, to LIST the FIFO (never refuse
/// it), and to agree field-for-field with the ARM-SELECTING path-based
/// listing of the SAME directory (a transport rooted at the directory itself,
/// listed through the EMPTY root-relative path). `RemoteEntry` has no
/// `PartialEq`, so the comparable shape is compared as sorted tuples.
///
/// Pre-fix this child BLOCKS in the `list` call (the `_confined` open of the
/// FIFO), so the parent kills it and reports the hang.
#[test]
#[ignore = "spawned by the FIFO list regression test via MODE_ENV"]
fn fifo_list_child() {
    let Ok(mode) = std::env::var(MODE_ENV) else {
        return;
    };
    assert_eq!(mode, LIST_CHILD_MODE, "unexpected FIFO child mode {mode:?}");
    let root = PathBuf::from(std::env::var_os(ROOT_ENV).expect("ROOT_ENV is set by the parent"));
    let dir = root.join("dir");
    std::fs::create_dir_all(&dir).expect("create the directory the listing reads");
    let fifo = dir.join("pipe");
    let c = CString::new(fifo.as_os_str().as_bytes()).expect("no NUL in the FIFO path");
    if unsafe { libc::mkfifo(c.as_ptr(), 0o600) } != 0 {
        panic!(
            "mkfifo {}: {}",
            fifo.display(),
            std::io::Error::last_os_error()
        );
    }
    std::fs::write(dir.join("plain.txt"), b"hello").expect("write the regular child");
    let env = SysEnv::from_process();

    // The `_confined` arm: a NON-EMPTY root-relative path lists the child.
    let confined = LocalTransport::new(&env, root.clone(), Layout::empty())
        .expect("build the confined transport");
    let rel_dir = RootedRelativePath::parse(Path::new("dir")).expect("parse dir");
    println!(
        "FIFO_LIST probing the confined arm at {}",
        rel_dir.display()
    );
    std::io::stdout().flush().expect("flush the pre-probe line");
    let started = Instant::now();
    let listed = confined
        .list(&rel_dir)
        .expect("list a directory containing a FIFO");
    let elapsed = started.elapsed();
    assert!(
        elapsed < PROBE_LIMIT,
        "the list probe took {elapsed:?}, over the {PROBE_LIMIT:?} bound: the FIFO open was not \
         non-blocking"
    );
    let pipe = listed
        .iter()
        .find(|e| e.name == "pipe")
        .expect("the FIFO must be LISTED, not refused or dropped");
    assert!(
        !pipe.is_dir && !pipe.is_symlink,
        "a FIFO is neither a directory nor a symlink, got {pipe:?}"
    );

    // The ARM-SELECTING path-based listing of the SAME directory: a transport
    // rooted at the directory, listed through the EMPTY root-relative path.
    let path_based = LocalTransport::new(&env, dir.clone(), Layout::empty())
        .expect("build the path-based transport");
    let empty = RootedRelativePath::from_validated(PathBuf::new());
    let listed_path = path_based
        .list(&empty)
        .expect("path-based list of the same directory");
    assert_eq!(
        listing_shape(&listed),
        listing_shape(&listed_path),
        "the `_confined` arm must list a directory containing a FIFO EXACTLY as the path-based \
         arm does"
    );
    println!("FIFO_LIST confined={pipe:?}");
    println!("{LIST_DONE_MARKER} platform={}", std::env::consts::OS);
    std::io::stdout().flush().expect("flush the done marker");
}

/// The child worker for the `set_mode` probe: no-op unless the parent set
/// [`MODE_ENV`] to [`CHMOD_CHILD_MODE`]. It gives the FIFO a DISTINCT mode,
/// then requires `set_mode` on the FIFO path to return promptly, to fail with
/// the crate's typed special-file kind, and to have left the FIFO's mode
/// EXACTLY as it was (never chmodded).
///
/// Pre-fix this child BLOCKS in the `set_mode` call (the `_confined` open of
/// the FIFO), so the parent kills it and reports the hang.
#[test]
#[ignore = "spawned by the FIFO set_mode regression test via MODE_ENV"]
fn fifo_set_mode_child() {
    use std::os::unix::fs::PermissionsExt;

    let Ok(mode) = std::env::var(MODE_ENV) else {
        return;
    };
    assert_eq!(
        mode, CHMOD_CHILD_MODE,
        "unexpected FIFO child mode {mode:?}"
    );
    let root = PathBuf::from(std::env::var_os(ROOT_ENV).expect("ROOT_ENV is set by the parent"));
    std::fs::create_dir_all(&root).expect("create the FIFO root");
    let fifo = root.join("pipe");
    let c = CString::new(fifo.as_os_str().as_bytes()).expect("no NUL in the FIFO path");
    if unsafe { libc::mkfifo(c.as_ptr(), 0o600) } != 0 {
        panic!(
            "mkfifo {}: {}",
            fifo.display(),
            std::io::Error::last_os_error()
        );
    }
    // A mode DISTINCT from the one `set_mode` is asked for, so "unchanged" is
    // observable: a chmod that went through would move it to 0o600.
    let before = 0o640;
    std::fs::set_permissions(&fifo, std::fs::Permissions::from_mode(before))
        .expect("set the FIFO's distinct mode");

    let transport = LocalTransport::new(&SysEnv::from_process(), root.clone(), Layout::empty())
        .expect("build the transport");
    let rel = RootedRelativePath::parse(Path::new("pipe")).expect("parse pipe");
    println!("FIFO_CHMOD probing set_mode on a FIFO");
    std::io::stdout().flush().expect("flush the pre-probe line");
    let started = Instant::now();
    let outcome = transport.set_mode(&rel, 0o600);
    let elapsed = started.elapsed();
    assert!(
        elapsed < PROBE_LIMIT,
        "the set_mode probe took {elapsed:?}, over the {PROBE_LIMIT:?} bound: the FIFO open was \
         not non-blocking"
    );
    let err = outcome.expect_err("set_mode must REFUSE a FIFO, never chmod it");
    assert_eq!(
        err.materialization_reason(),
        Some(MaterializationKind::SpecialFile),
        "set_mode must refuse the FIFO with the typed special-file kind, got: {err:?}"
    );
    let after = std::fs::symlink_metadata(&fifo)
        .expect("stat the FIFO after the refusal")
        .permissions()
        .mode()
        & 0o7777;
    assert_eq!(
        after, before,
        "the refused set_mode must not have chmodded the FIFO"
    );
    println!("FIFO_CHMOD refused={err}");
    println!("{CHMOD_DONE_MARKER} platform={}", std::env::consts::OS);
    std::io::stdout().flush().expect("flush the done marker");
}

/// The child worker for the `remove_dir_all` probe: no-op unless the parent
/// set [`MODE_ENV`] to [`REMOVE_CHILD_MODE`]. It requires `remove_dir_all` of a
/// FIFO to return promptly, to FAIL (a FIFO is not a directory), and to have
/// left the FIFO in place. The removal-failure path's `NotFound` existence
/// probe used to open the FIFO `O_RDONLY`, so pre-fix this child BLOCKS in the
/// probe and the parent kills it.
#[test]
#[ignore = "spawned by the FIFO remove_dir_all regression test via MODE_ENV"]
fn fifo_remove_dir_all_child() {
    let Ok(mode) = std::env::var(MODE_ENV) else {
        return;
    };
    assert_eq!(
        mode, REMOVE_CHILD_MODE,
        "unexpected FIFO child mode {mode:?}"
    );
    let root = PathBuf::from(std::env::var_os(ROOT_ENV).expect("ROOT_ENV is set by the parent"));
    std::fs::create_dir_all(&root).expect("create the FIFO root");
    let fifo = root.join("pipe");
    let c = CString::new(fifo.as_os_str().as_bytes()).expect("no NUL in the FIFO path");
    if unsafe { libc::mkfifo(c.as_ptr(), 0o600) } != 0 {
        panic!(
            "mkfifo {}: {}",
            fifo.display(),
            std::io::Error::last_os_error()
        );
    }
    let transport = LocalTransport::new(&SysEnv::from_process(), root.clone(), Layout::empty())
        .expect("build the transport");
    let rel = RootedRelativePath::parse(Path::new("pipe")).expect("parse pipe");
    println!("FIFO_REMOVE probing remove_dir_all on a FIFO");
    std::io::stdout().flush().expect("flush the pre-probe line");
    let started = Instant::now();
    let outcome = transport.remove_dir_all(&rel);
    let elapsed = started.elapsed();
    assert!(
        elapsed < PROBE_LIMIT,
        "the remove_dir_all probe took {elapsed:?}, over the {PROBE_LIMIT:?} bound: the FIFO open \
         was not non-blocking"
    );
    outcome.expect_err("remove_dir_all of a FIFO must fail: a FIFO is not a directory");
    assert!(
        std::fs::symlink_metadata(&fifo).is_ok(),
        "the failed remove_dir_all must not have removed the FIFO"
    );
    println!("{REMOVE_DONE_MARKER} platform={}", std::env::consts::OS);
    std::io::stdout().flush().expect("flush the done marker");
}

/// Run one probe, require it to return PROMPTLY and to REFUSE the FIFO, and
/// print its result (flushed, so a later hang is visible to the parent).
fn probe<T: std::fmt::Debug>(label: &str, f: impl FnOnce() -> crate::Result<T>) {
    let start = Instant::now();
    let outcome = f();
    let elapsed = start.elapsed();
    assert!(
        elapsed < PROBE_LIMIT,
        "the {label} probe took {elapsed:?}, over the {PROBE_LIMIT:?} bound: the FIFO open was \
         not non-blocking"
    );
    match outcome {
        Err(e) => println!("FIFO_PROBE {label} elapsed={elapsed:?} refused={e}"),
        Ok(value) => panic!(
            "the {label} probe must REFUSE the FIFO, not open and report it, got Ok({value:?})"
        ),
    }
    std::io::stdout()
        .flush()
        .expect("flush the probe result to the parent");
}

/// The parent's verdict on a child run.
enum ChildOutcome {
    Exited(std::process::Output),
    TimedOut(std::process::Output),
}

/// The comparable shape of a listing: [`RemoteEntry`] carries no `PartialEq`,
/// and the two list arms must agree on EVERY field, so the entries are mapped
/// to tuples and sorted by name before comparison.
fn listing_shape(entries: &[RemoteEntry]) -> Vec<(String, bool, bool, u64, u32)> {
    let mut out: Vec<(String, bool, bool, u64, u32)> = entries
        .iter()
        .map(|e| (e.name.clone(), e.is_dir, e.is_symlink, e.size, e.mode))
        .collect();
    out.sort();
    out
}

/// Turn a child's verdict into the assertions every parent test makes: a
/// child past the HARD cap was killed (a pre-fix hang) and is reported as
/// such; a child that exited non-zero is reported; a child that exited 0
/// without reaching `done` ran no assertion and is reported rather than
/// passing vacuously. Returns the child's stdout on success.
fn assert_child_reached(outcome: ChildOutcome, limit: Duration, done: &str, what: &str) -> String {
    match outcome {
        ChildOutcome::TimedOut(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            panic!(
                "the {what} child did not finish within {limit:?} and was killed: it opened the \
                 FIFO blocking (the pre-fix hang). A hang is an unbounded failure on user data \
                 and is never acceptable.\n\
                 --- child stdout ---\n{stdout}\n--- child stderr ---\n{stderr}"
            );
        }
        ChildOutcome::Exited(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(
                out.status.success(),
                "the {what} child failed: status={:?}\n--- child stdout ---\n{stdout}\n\
                 --- child stderr ---\n{stderr}",
                out.status
            );
            assert!(
                stdout.contains(done),
                "the {what} child exited 0 but never reached {done}, so it ran no assertion and \
                 this suite would pass vacuously:\n--- child stdout ---\n{stdout}\n\
                 --- child stderr ---\n{stderr}"
            );
            stdout.into_owned()
        }
    }
}

/// Spawn the parent's own test binary as a child (running `child_test` under
/// the `mode` value of [`MODE_ENV`]) and wait for it with a HARD wall-clock
/// cap: past the cap the child is killed and its (partial) output returned, so
/// a pre-fix hang fails the test instead of hanging the suite.
fn run_child(child_test: &str, mode: &str, root: &Path, limit: Duration) -> ChildOutcome {
    let exe = std::env::current_exe().expect("the running test binary path");
    let mut child = std::process::Command::new(exe)
        .args(["--exact", child_test, "--ignored", "--nocapture"])
        .env(MODE_ENV, mode)
        .env(ROOT_ENV, root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn the FIFO child test binary");
    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return ChildOutcome::Exited(child.wait_with_output().expect("output")),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                return ChildOutcome::TimedOut(child.wait_with_output().expect("output"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => panic!("waiting on the FIFO child: {e}"),
        }
    }
}
