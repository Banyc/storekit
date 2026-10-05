//! CONSUMER-FIT GUARD: the public call shapes a CONSUMER's interface requires.
//!
//! This crate exists to be consumed — by `~/code/deploy` (and then
//! `~/code/ckpt`) — and a deletion or a signature change that is invisible to
//! this crate's OWN suite is still a build break for a consumer. A prior pass
//! deleted `Remote::exists` and demoted `atomic::write_atomic_replace(&Path)`,
//! justified by "our production never did"; the population that mattered was
//! the CONSUMER's interface, where deploy's interface requires `exists` (this
//! crate supplies it as a DEFAULT method, so a transport may override it with a
//! cheaper probe) and calls the path-based replace. This file is the class fix:
//! it NAMES the required shapes and exercises them through the PUBLIC API only,
//! so a future deletion of any of them is a loud COMPILE failure here — exactly
//! the failure a consumer would get, and exactly the failure no unit test in
//! this crate can produce.
//!
//! Each group below says WHICH KIND of evidence it has, measured at
//! `~/code/deploy` `@` = `a76da6d4` (tip `2a2af061`):
//!
//! * a name a CURRENT consumer CALLS — `Remote::exists` (called in production),
//!   the absolute-path replace and its `_fd` twin, `copy_dir_recursive_fd`,
//!   `fsync_tree_recursive_fd`, `with_operation_lock_sidecar` with `SIDECAR_*`,
//!   and the `id_newtype!` machinery;
//! * a name the crate HOLDS for a consumer that does not use it YET — the `sync`
//!   entry point with its ownership values, `SyncReport`, `copy_tree_verbatim`.
//!   Measured at that revision: `deploy` contains ZERO occurrences of any of
//!   them, so the pin is not an attestation of a current need; it is a promise
//!   not to drop the surface the planned migration depends on, and it says so
//!   here rather than borrowing the authority of a call site that does not
//!   exist.
//!
//! Deleting any of them is a COMPILE failure here either way, which is the
//! point. Each is cited by NAME, never by `file:line`: a consumer's line numbers
//! move under it, and this crate's own rules forbid resting a claim on one.
//!
//! * `Remote::exists` — `src/remote/transport/mod.rs`, the trait method the
//!   consumer's interface requires, `fn exists(&self, rel: &RootedRelativePath)
//!   -> bool`. Its PRODUCTION caller is `src/store/local/objects.rs`; the two
//!   other files that call it (`src/remote/helper/mod.rs`, `durable.rs`) do so
//!   from `#[cfg(test)]` modules, so they attest the trait method's shape but
//!   not a production need.
//! * the path-based atomic replace — the ABSOLUTE-PATH call shape
//!   `write_atomic_replace(&path, bytes, &mut hook)`, which the consumer's
//!   retention history-floor writer uses (`src/retention/reachability/
//!   history_floor.rs`; the migration deleted `deploy`'s platform-specific store
//!   modules, so the durable evidence is the CALL SHAPE, not a file that moves
//!   under it) — and its confined twin `write_atomic_replace_fd`, which the
//!   consumer's `store/local` writer drives.
//! * the tree pair — `copy_dir_recursive_fd` (out-of-root `&Path` source into a
//!   root-confined `RootedRelativePath` staging destination) then
//!   `fsync_tree_recursive_fd`; recorded in the README's "Design conflicts
//!   the `deploy` migration surfaced" (c).
//! * the ONE `sync` entry point in BOTH ownership states —
//!   `DestinationOwnership::lock(..)` (owned) and
//!   `DestinationOwnership::Unowned` (the explicitly weaker path). HELD FOR THE
//!   PLANNED CONSUMER: no current consumer calls `sync` (see the header).
//!
//! It is fast and hermetic: no network, no sshd — everything runs through
//! `LocalTransport` and the in-crate seams.

// Test fixtures drive the same name-creating primitives the funnel guards.
#![allow(clippy::disallowed_methods)]

use std::path::Path;

use storekit::RootedRelativePath;
use storekit::atomic::{
    ReplaceOutcome, RootDir, copy_dir_recursive_fd, copy_tree_verbatim, fsync_tree_recursive_fd,
    write_atomic_replace, write_atomic_replace_fd,
};
use storekit::env::SysEnv;
use storekit::sync::{DestinationOwnership, Direction, Extraneous, ReplaceAll, SyncReport, sync};
use storekit::transport::{
    Layout, LocalTransport, Remote, SIDECAR_WAIT_TIMEOUT, with_operation_lock_sidecar,
};

/// A validated root-relative path (the only spelling authority for a
/// root-confined mutation), parsed at the boundary like a consumer must.
fn rp(s: &str) -> RootedRelativePath {
    RootedRelativePath::parse(Path::new(s)).unwrap()
}

/// Build a `LocalTransport` rooted at an existing directory.
fn local_transport(base: &Path) -> LocalTransport {
    LocalTransport::new(&SysEnv::from_process(), base.to_path_buf(), Layout::empty())
        .expect("build a local transport")
}

/// `Remote::exists` (deploy's REQUIRED trait method) plus the typed-absence
/// alternative `Remote::metadata_opt` (`Ok(None)` versus `Err`).
///
/// The control is the last block: an UNANSWERABLE probe (a symlinked parent,
/// refused by the confined read) reads as `false` through `exists` and as
/// `Err` through `metadata_opt`. That is the exact contract `exists` documents,
/// and the reason a caller that must distinguish *absent* from *could not tell*
/// is pointed at `metadata_opt`.
#[test]
fn remote_existence_probe_and_typed_absence() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let base = tmp.path().join("dst");
    std::fs::create_dir_all(&base).expect("create the destination root");
    std::fs::write(base.join("present"), b"x").expect("seed a present entry");

    let t = local_transport(&base);

    // The cheap probe: present -> true, absent -> false.
    assert!(
        t.exists(&rp("present")),
        "exists must report a present entry"
    );
    assert!(
        !t.exists(&rp("missing")),
        "exists must report an absent entry"
    );

    // The typed probe a consumer uses when absence must be distinguished from
    // an unanswerable probe.
    assert!(
        t.metadata_opt(&rp("present")).unwrap().is_some(),
        "metadata_opt must report a present entry as Ok(Some)"
    );
    assert!(
        t.metadata_opt(&rp("missing")).unwrap().is_none(),
        "metadata_opt must report a CONFIRMED absence as Ok(None)"
    );

    #[cfg(unix)]
    {
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).expect("create the outside dir");
        std::fs::write(outside.join("planted"), b"OUTSIDE").expect("plant outside content");
        std::os::unix::fs::symlink(&outside, base.join("link")).expect("plant the symlink");

        let link_planted = rp("link/planted");
        assert!(
            t.metadata_opt(&link_planted).is_err(),
            "an unanswerable (refused) probe must be an Err, never Ok(None)"
        );
        assert!(
            !t.exists(&link_planted),
            "the cheap probe conflates the refused probe with absence: exists == false"
        );
    }
}

/// The path-based UNCONFINED atomic replace — the ABSOLUTE-PATH call shape the
/// consumer's retention history-floor writer uses at the revision above (the
/// migration deleted its platform-specific store modules, so the durable evidence
/// is the CALL SHAPE, not a file that moves under it) — AND its confined
/// equivalent `write_atomic_replace_fd` (deploy's `write_atomic_replace_at`
/// resolves through it). Both must be nameable, and both must install the bytes.
#[test]
fn atomic_replace_path_based_and_confined() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root_path = tmp.path().join("root");
    std::fs::create_dir_all(&root_path).expect("create the root");
    let root = RootDir::open(&root_path).expect("open the root");

    // The public, deliberately-named unconfined form: an ABSOLUTE &Path, no
    // (root, rel) pair.
    let absolute = root_path.join("plain.txt");
    let outcome = write_atomic_replace(&absolute, b"PATH-BASED", &mut |_| None)
        .expect("the path-based replace installs");
    assert!(
        matches!(outcome, ReplaceOutcome::ReplacedDurable),
        "the path-based replace must report its commit points explicitly: {outcome:?}"
    );
    assert_eq!(
        std::fs::read(&absolute).unwrap(),
        b"PATH-BASED",
        "the path-based replace must install the bytes"
    );

    // The confined form: a (RootDir, RootedRelativePath) pair.
    let outcome = write_atomic_replace_fd(&root, &rp("confined.txt"), b"CONFINED", &mut |_| None)
        .expect("the confined replace installs");
    assert!(
        matches!(outcome, ReplaceOutcome::ReplacedDurable),
        "the confined replace must report its commit points explicitly: {outcome:?}"
    );
    assert_eq!(
        std::fs::read(root_path.join("confined.txt")).unwrap(),
        b"CONFINED",
        "the confined replace must install the bytes under the root"
    );
}

/// The fd-confined tree pair with the EXACT consumer signatures: an
/// out-of-root `&Path` SOURCE copied into a root-confined `RootedRelativePath`
/// staging destination, then `fsync_tree_recursive_fd` over that destination.
/// The typed error branch proves the refusal is a `StoreKind`, not message
/// text.
#[test]
fn tree_pair_out_of_root_source_into_confined_staging() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root_path = tmp.path().join("root");
    std::fs::create_dir_all(&root_path).expect("create the root");
    let root = RootDir::open(&root_path).expect("open the root");

    // The SOURCE is OUTSIDE the root and named by an ordinary `&Path`.
    let src = tmp.path().join("src");
    std::fs::create_dir_all(src.join("nested")).expect("create the source tree");
    std::fs::write(src.join("a.txt"), b"a").expect("write a");
    std::fs::write(src.join("nested/b.txt"), b"b").expect("write b");

    // The DESTINATION is a root-confined validated relative path.
    copy_dir_recursive_fd(&root, &src, &rp("staging/copy")).expect("copy the tree into staging");
    assert_eq!(
        std::fs::read(root_path.join("staging/copy/a.txt")).unwrap(),
        b"a"
    );
    assert_eq!(
        std::fs::read(root_path.join("staging/copy/nested/b.txt")).unwrap(),
        b"b"
    );

    // The second half of the pair: make the copied tree durable.
    fsync_tree_recursive_fd(&root, &rp("staging/copy")).expect("fsync the copied tree");

    #[cfg(unix)]
    {
        use storekit::StoreKind;
        // A typed refusal by KIND (never by message): a top-level symlink
        // source is refused with a `StoreKind` a consumer branches on.
        let link = tmp.path().join("src-link");
        std::os::unix::fs::symlink(&src, &link).expect("plant the source symlink");
        let err = copy_dir_recursive_fd(&root, &link, &rp("staging/refused"))
            .expect_err("a top-level symlink source must be refused");
        assert_eq!(
            err.store_reason(),
            Some(StoreKind::CopySourceIsSymlink),
            "the refusal must be a typed StoreKind, not message text: {err:?}"
        );
    }
}

/// One consumer-required name and one held for the planned consumer, both
/// exercised through the PUBLIC API only: the operation-scoped sidecar critical
/// section (`deploy`'s `remote::helper::recover` fallback needs a
/// blocking-with-deadline form over the already-open, read-only record — a
/// current CALL SITE), and the tolerant verbatim copy, which `deploy`'s
/// retention checkpoint does NOT use (measured: its live-base clone is a
/// test-only `std::fs::copy`) and which the crate therefore holds for the
/// planned consumer rather than attesting. A future deletion of either is a
/// COMPILE failure here.
#[test]
fn sidecar_critical_section_and_tolerant_clone_are_public() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let base = tmp.path().join("base");
    std::fs::create_dir_all(&base).expect("create the base");

    // The operation-scoped sidecar: the consumer supplies the base root and
    // the layout's sidecar record; the closure runs under the flock.
    let layout = Layout::empty();
    let mut ran = false;
    with_operation_lock_sidecar(&base, &layout.lock_sidecar, || {
        ran = true;
        Ok(())
    })
    .expect("the sidecar critical section runs");
    assert!(ran, "the critical section must run");
    assert_eq!(
        SIDECAR_WAIT_TIMEOUT,
        std::time::Duration::from_secs(2),
        "the public deadline is the record's documented one"
    );

    // The tolerant clone: a reserved spelling is carried verbatim.
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).expect("create the source");
    std::fs::write(src.join("operation.lock"), b"RESERVED").expect("seed the lock record");
    let dst = tmp.path().join("clone");
    copy_tree_verbatim(&src, &dst).expect("the tolerant clone runs");
    assert_eq!(
        std::fs::read(dst.join("operation.lock")).unwrap(),
        b"RESERVED",
        "the tolerant clone carries the reserved name verbatim"
    );
}

/// A `Layout` construction through the PUBLIC field shape (and the
/// convenience constructor), which is what a consumer hands to
/// `LocalTransport::new`.
#[test]
fn layout_is_constructible_from_public_parts() {
    let empty = Layout::empty();
    assert!(empty.bootstrap_dirs.is_empty());
    assert!(empty.receiver_marker.is_none());

    let explicit = Layout {
        bootstrap_dirs: vec![rp("state")],
        lock: rp("state/operation.lock"),
        lock_sidecar: rp("state/operation.lock.mutex"),
        receiver_marker: Some(rp("state/receiver-id")),
    };
    assert_eq!(explicit.bootstrap_dirs, vec![rp("state")]);
    assert_eq!(explicit.lock, rp("state/operation.lock"));
    assert_eq!(explicit.lock_sidecar, rp("state/operation.lock.mutex"));
    assert_eq!(explicit.receiver_marker, Some(rp("state/receiver-id")));
}

/// Run one local push through the ONE `sync` entry point with the given
/// ownership value, returning the report.
fn push_once(src: &Path, dst: &Path, ownership: DestinationOwnership) -> SyncReport {
    let transport = local_transport(dst);
    sync(
        Direction::Push,
        src,
        &transport,
        &ReplaceAll,
        Extraneous::Keep,
        ownership,
    )
    .expect("the local push runs")
}

/// The ONE `sync` entry point in BOTH ownership states: the owned path via
/// `DestinationOwnership::lock(..)` (the crate takes the destination's
/// operation lock) and the explicitly weaker `DestinationOwnership::Unowned`
/// (the caller has taken the destination out of band). Both must be nameable
/// and both must transfer.
#[test]
fn sync_entry_point_in_both_ownership_states() {
    // OWNED: the crate acquires and holds the destination's operation lock.
    {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        std::fs::create_dir_all(&src).expect("create the source root");
        std::fs::create_dir_all(&dst).expect("create the destination root");
        std::fs::write(src.join("f"), b"OWNED").expect("seed the source");

        let transport = local_transport(&dst);
        let ownership = DestinationOwnership::lock(Direction::Push, &src, &transport)
            .expect("acquire the destination's operation lock");
        assert!(
            matches!(ownership, DestinationOwnership::Locked(_)),
            "the acquiring constructor must produce the unforgeable Locked token"
        );

        let report = push_once(&src, &dst, ownership);
        assert!(
            report.applied.iter().any(|p| p == "f"),
            "the owned push must report the applied entry: {:?}",
            report.applied
        );
        assert_eq!(std::fs::read(dst.join("f")).unwrap(), b"OWNED");
    }

    // UNOWNED: the weaker guarantee is named at the call site.
    {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        std::fs::create_dir_all(&src).expect("create the source root");
        std::fs::create_dir_all(&dst).expect("create the destination root");
        std::fs::write(src.join("f"), b"UNOWNED").expect("seed the source");

        let report = push_once(&src, &dst, DestinationOwnership::Unowned);
        assert!(
            report.applied.iter().any(|p| p == "f"),
            "the unowned push must report the applied entry: {:?}",
            report.applied
        );
        assert_eq!(std::fs::read(dst.join("f")).unwrap(), b"UNOWNED");
    }
}
