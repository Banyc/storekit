//! The destination-ownership token is bound to the transport's ENDPOINT, not
//! only to its root spelling.
//!
//! # The defect these tests pin
//!
//! `DestinationOwnership` records the destination identity so a caller cannot
//! take the lock on one destination and mutate another. Before the fix the
//! recorded identity was the PATH spelling only: `Prepared` had no transport
//! identity, and `Prepared::matches` compared the direction, the local root
//! spelling, the normalized remote ROOT, and `remote.is_local()`. A token
//! minted against host A was therefore accepted for a run against host B
//! whenever both reported the same `root()` spelling — and for
//! `Direction::Pull`, where the remote is the SOURCE, `matches` ignored the
//! remote entirely, so a plan read from source A was applied against a live
//! source B.
//!
//! # The double
//!
//! [`EndpointRemote`] is a transport whose DATA operations delegate to a real
//! [`LocalTransport`] at a per-endpoint base directory, while `root()` reports
//! a SHARED layout spelling and `endpoint_identity()` states the endpoint. Two
//! instances therefore model the same layout path on two different hosts. The
//! far-side manifest command (`Remote::exec`, the perl verify script) is
//! rewritten to run against the endpoint's own base directory, so the
//! manifests reflect that endpoint's data — exactly as a real far side would.
//!
//! These tests use ONLY the public API.

#![allow(clippy::disallowed_methods)]
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use storekit::env::SysEnv;
use storekit::error::PreflightKind;
use storekit::sync::{DestinationOwnership, Direction, Extraneous, ReplaceAll, sync};
use storekit::transport::{
    CreateNewVerdict, ExecOutcome, FarSideLockSession, FsBytes, Layout, LocalTransport, Remote,
    RemoteEntry, RemoteMeta, RootedRelativePath,
};

/// A transport for ONE endpoint: its data lives in `data_root`, its reported
/// root spelling is `reported_root` (deliberately decoupled), and its endpoint
/// identity is `identity`.
struct EndpointRemote {
    inner: LocalTransport,
    data_root: PathBuf,
    reported_root: PathBuf,
    identity: Option<String>,
    far_side_lock_calls: Arc<AtomicUsize>,
    session_alive: Arc<AtomicBool>,
}

impl EndpointRemote {
    fn new(data_root: PathBuf, reported_root: PathBuf, identity: Option<&str>) -> EndpointRemote {
        std::fs::create_dir_all(&data_root).expect("create the endpoint's data dir");
        let inner =
            LocalTransport::new(&SysEnv::from_process(), data_root.clone(), Layout::empty())
                .expect("a LocalTransport over the endpoint's data dir");
        EndpointRemote {
            inner,
            data_root,
            reported_root,
            identity: identity.map(str::to_string),
            far_side_lock_calls: Arc::new(AtomicUsize::new(0)),
            session_alive: Arc::new(AtomicBool::new(true)),
        }
    }

    /// A path INSIDE this endpoint's own data directory.
    fn data(&self, rel: &str) -> PathBuf {
        self.data_root.join(rel)
    }

    fn far_side_lock_calls(&self) -> usize {
        self.far_side_lock_calls.load(Ordering::SeqCst)
    }
}

impl Remote for EndpointRemote {
    fn root(&self) -> &Path {
        &self.reported_root
    }

    fn is_local(&self) -> bool {
        // A remote endpoint: the far-side manifest goes through `exec`.
        false
    }

    fn endpoint_identity(&self) -> Option<String> {
        self.identity.clone()
    }

    fn read(&self, rel: &RootedRelativePath) -> storekit::Result<Vec<u8>> {
        self.inner.read(rel)
    }

    fn write(&self, rel: &RootedRelativePath, data: &[u8], mode: u32) -> storekit::Result<()> {
        self.inner.write(rel, data, mode)
    }

    fn try_write_new(
        &self,
        rel: &RootedRelativePath,
        data: &[u8],
    ) -> storekit::Result<CreateNewVerdict> {
        self.inner.try_write_new(rel, data)
    }

    fn create_dir(&self, rel: &RootedRelativePath) -> storekit::Result<()> {
        self.inner.create_dir(rel)
    }

    fn create_dir_all(&self, rel: &RootedRelativePath) -> storekit::Result<()> {
        self.inner.create_dir_all(rel)
    }

    fn set_mode(&self, rel: &RootedRelativePath, mode: u32) -> storekit::Result<()> {
        self.inner.set_mode(rel, mode)
    }

    fn list(&self, rel: &RootedRelativePath) -> storekit::Result<Vec<RemoteEntry>> {
        self.inner.list(rel)
    }

    fn rename(&self, from: &RootedRelativePath, to: &RootedRelativePath) -> storekit::Result<()> {
        self.inner.rename(from, to)
    }

    fn rename_aside(
        &self,
        from: &RootedRelativePath,
        to: &RootedRelativePath,
    ) -> storekit::Result<()> {
        self.inner.rename_aside(from, to)
    }

    fn symlink(&self, target: &Path, link: &RootedRelativePath) -> storekit::Result<()> {
        self.inner.symlink(target, link)
    }

    fn read_link(&self, rel: &RootedRelativePath) -> storekit::Result<PathBuf> {
        self.inner.read_link(rel)
    }

    fn remove_file(&self, rel: &RootedRelativePath) -> storekit::Result<()> {
        self.inner.remove_file(rel)
    }

    fn remove_dir_all(&self, rel: &RootedRelativePath) -> storekit::Result<()> {
        self.inner.remove_dir_all(rel)
    }

    fn remove_dir(&self, rel: &RootedRelativePath) -> storekit::Result<()> {
        self.inner.remove_dir(rel)
    }

    fn metadata(&self, rel: &RootedRelativePath) -> storekit::Result<RemoteMeta> {
        self.inner.metadata(rel)
    }

    fn exec(&self, argv: &[String], timeout: Duration) -> storekit::Result<ExecOutcome> {
        // The far-side manifest command names `root()` (the SHARED spelling).
        // This endpoint's data lives at `data_root`, so rewrite that argument
        // before running the command locally: the manifest describes THIS
        // endpoint's tree, exactly as a real far side would.
        let spelled = self.reported_root.to_string_lossy().into_owned();
        let rewritten: Vec<String> = argv
            .iter()
            .map(|arg| {
                if *arg == spelled {
                    self.data_root.to_string_lossy().into_owned()
                } else {
                    arg.clone()
                }
            })
            .collect();
        self.inner.exec(&rewritten, timeout)
    }

    fn filesystem_bytes(&self) -> storekit::Result<FsBytes> {
        self.inner.filesystem_bytes()
    }

    fn lock_far_side(
        &self,
        record: &Path,
        _op_id: &str,
    ) -> storekit::Result<Box<dyn FarSideLockSession>> {
        self.far_side_lock_calls.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FakeFarSideSession {
            record: record.to_path_buf(),
            alive: Arc::clone(&self.session_alive),
        }))
    }
}

/// A live far-side holder that never touches a real record: enough for
/// `DestinationOwnership::lock_remote` to mint a token, and `sync` to see the
/// session as alive for the whole run.
struct FakeFarSideSession {
    record: PathBuf,
    alive: Arc<AtomicBool>,
}

impl FarSideLockSession for FakeFarSideSession {
    fn record(&self) -> &Path {
        &self.record
    }
    fn is_alive(&mut self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
    fn loss_detail(&mut self) -> String {
        String::new()
    }
    fn local_pid(&self) -> u32 {
        std::process::id()
    }
    fn release(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
    }
}

/// A source tree, a local destination, and the SHARED layout spelling two
/// endpoints report.
struct Fixture {
    dir: tempfile::TempDir,
    src: PathBuf,
    shared_root: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).expect("create the source");
    std::fs::write(src.join("f"), b"payload").expect("seed the source");
    let shared_root = dir.path().join("srv").join("store");
    Fixture {
        dir,
        src,
        shared_root,
    }
}

// ---------------------------------------------------------------------------
// PUSH: the token binds the HOST (the remote is the destination)
// ---------------------------------------------------------------------------

/// THE REPRO. A token minted against host A is accepted pre-fix for a run
/// against host B when both report the SAME root spelling; the run then
/// mutates host B while host A is untouched. Post-fix it is REFUSED, naming
/// the endpoint axis, and host B is unmutated.
#[test]
fn a_remote_token_is_refused_across_endpoints_with_the_same_root() {
    let f = fixture();
    let host_a = EndpointRemote::new(
        f.dir.path().join("data-a"),
        f.shared_root.clone(),
        Some("ssh://host-a:22"),
    );
    let host_b = EndpointRemote::new(
        f.dir.path().join("data-b"),
        f.shared_root.clone(),
        Some("ssh://host-b:22"),
    );

    let token = DestinationOwnership::lock_remote(Direction::Push, &f.src, &host_a)
        .expect("mint a remote token against host A");
    assert_eq!(
        host_a.far_side_lock_calls(),
        1,
        "the mint acquired host A's far-side record"
    );
    assert_eq!(
        host_b.far_side_lock_calls(),
        0,
        "the mint must not touch host B"
    );

    let error = sync(
        Direction::Push,
        &f.src,
        &host_b,
        &ReplaceAll,
        Extraneous::Keep,
        token,
    )
    .expect_err("a token minted against host A must be refused for host B");
    assert_eq!(
        error.error().preflight_reason(),
        Some(PreflightKind::EndpointIdentityMismatch),
        "the refusal must be the typed ENDPOINT-identity mismatch, not only a root refusal: {error:?}"
    );
    assert!(
        !host_b.data("f").exists(),
        "host B must be unmutated by the refused run"
    );
    assert!(
        !host_a.data("f").exists(),
        "host A is untouched: the token was only minted, never run"
    );
}

/// The ROOT axis is compared too: two endpoints with the SAME identity but
/// different root spellings are refused (pre-fix this was already caught for a
/// PUSH by the destination-root comparison; the test pins that it stays
/// caught, now through the explicit root axis).
#[test]
fn a_remote_token_is_refused_across_roots_with_the_same_endpoint() {
    let f = fixture();
    let host_a = EndpointRemote::new(
        f.dir.path().join("data-a"),
        f.shared_root.clone(),
        Some("ssh://shared-host:22"),
    );
    let host_b = EndpointRemote::new(
        f.dir.path().join("data-b"),
        f.dir.path().join("other").join("store"),
        Some("ssh://shared-host:22"),
    );

    let token = DestinationOwnership::lock_remote(Direction::Push, &f.src, &host_a)
        .expect("mint a remote token against host A");
    let error = sync(
        Direction::Push,
        &f.src,
        &host_b,
        &ReplaceAll,
        Extraneous::Keep,
        token,
    )
    .expect_err("a token minted for another root spelling must be refused");
    assert_eq!(
        error.error().preflight_reason(),
        Some(PreflightKind::RemoteRootMismatch),
        "the refusal must be the typed ROOT-spelling mismatch: {error:?}"
    );
    assert!(!host_b.data("f").exists(), "host B must be unmutated");
}

/// The LEGITIMATE case still works: the very transport that acquired the token
/// is accepted, and the push completes into that endpoint.
#[test]
fn the_transport_that_minted_a_push_token_is_accepted() {
    let f = fixture();
    let host_a = EndpointRemote::new(
        f.dir.path().join("data-a"),
        f.shared_root.clone(),
        Some("ssh://host-a:22"),
    );
    let token = DestinationOwnership::lock_remote(Direction::Push, &f.src, &host_a)
        .expect("mint a remote token against host A");
    let report = sync(
        Direction::Push,
        &f.src,
        &host_a,
        &ReplaceAll,
        Extraneous::Keep,
        token,
    )
    .expect("the transport that minted the token must be accepted");
    assert!(report.conflicts.is_empty(), "{:?}", report.conflicts);
    assert!(
        report.verify_failures.is_empty(),
        "{:?}",
        report.verify_failures
    );
    assert_eq!(
        std::fs::read(host_a.data("f")).expect("the pushed file"),
        b"payload"
    );
}

// ---------------------------------------------------------------------------
// PULL: the token binds the SOURCE (the remote is the source)
// ---------------------------------------------------------------------------

/// THE PULL REPRO. A token minted against source A is accepted pre-fix for a
/// run whose live source is B (the same root spelling, a different endpoint),
/// so the plan read from A is applied against B. Post-fix it is REFUSED.
#[test]
fn a_pull_token_is_refused_against_a_different_source_endpoint() {
    let f = fixture();
    let local_dst = f.dir.path().join("local-dst");
    std::fs::create_dir_all(&local_dst).expect("create the local destination");

    let source_a = EndpointRemote::new(
        f.dir.path().join("src-a"),
        f.shared_root.clone(),
        Some("ssh://source-a:22"),
    );
    let source_b = EndpointRemote::new(
        f.dir.path().join("src-b"),
        f.shared_root.clone(),
        Some("ssh://source-b:22"),
    );
    std::fs::write(source_a.data("x"), b"from-a").expect("seed source A");
    std::fs::write(source_b.data("x"), b"from-b").expect("seed source B");

    let token = DestinationOwnership::lock(Direction::Pull, &local_dst, &source_a)
        .expect("mint a pull token against source A");
    let error = sync(
        Direction::Pull,
        &local_dst,
        &source_b,
        &ReplaceAll,
        Extraneous::Keep,
        token,
    )
    .expect_err("a pull token minted against source A must be refused for source B");
    assert_eq!(
        error.error().preflight_reason(),
        Some(PreflightKind::EndpointIdentityMismatch),
        "the refusal must be the typed ENDPOINT-identity mismatch: {error:?}"
    );
    assert!(
        !local_dst.join("x").exists(),
        "the local destination must be unmutated by the refused pull"
    );
}

/// The ROOT axis for a PULL: pre-fix `matches` ignored the remote entirely, so
/// even a different source root was accepted. Post-fix it is REFUSED.
#[test]
fn a_pull_token_is_refused_against_a_different_source_root() {
    let f = fixture();
    let local_dst = f.dir.path().join("local-dst");
    std::fs::create_dir_all(&local_dst).expect("create the local destination");

    let source_a = EndpointRemote::new(
        f.dir.path().join("src-a"),
        f.shared_root.clone(),
        Some("ssh://source:22"),
    );
    let source_b = EndpointRemote::new(
        f.dir.path().join("src-b"),
        f.dir.path().join("other").join("store"),
        Some("ssh://source:22"),
    );
    std::fs::write(source_a.data("x"), b"from-a").expect("seed source A");
    std::fs::write(source_b.data("x"), b"from-b").expect("seed source B");

    let token = DestinationOwnership::lock(Direction::Pull, &local_dst, &source_a)
        .expect("mint a pull token against source A");
    let error = sync(
        Direction::Pull,
        &local_dst,
        &source_b,
        &ReplaceAll,
        Extraneous::Keep,
        token,
    )
    .expect_err("a pull token minted for another source root must be refused");
    assert_eq!(
        error.error().preflight_reason(),
        Some(PreflightKind::RemoteRootMismatch),
        "the refusal must be the typed ROOT-spelling mismatch: {error:?}"
    );
    assert!(
        !local_dst.join("x").exists(),
        "the local destination must be unmutated"
    );
}

/// The LOCAL-ROOT axis for a PULL, through the PUBLIC API. `Prepared::matches`
/// step 1 combines the direction check with the local-root comparison; the
/// PUSH half is pinned by
/// `sync::apply::tests::a_destination_ownership_token_is_bound_to_its_local_root`
/// and the PULL half by its twin there. This is the public-API twin: a token
/// minted for destination root A, replayed with destination root B and the
/// SAME source transport, is refused with the typed RUN-BINDING mismatch and
/// neither root is mutated.
///
/// DIRECTION GATE. Gating the ROOT half of step 1 on
/// `direction == Direction::Push` leaves the whole suite green without this
/// test. The impact is BOUNDED and stated: the run uses the TOKEN's
/// `prepared.local` (A), so the `local_root` argument (B) is a silently IGNORED
/// argument, not a containment breach; A is the root the un-pinned run would
/// mutate.
#[test]
fn a_pull_token_is_refused_against_a_different_local_destination_root() {
    let f = fixture();
    let local_dst_a = f.dir.path().join("local-dst-a");
    let local_dst_b = f.dir.path().join("local-dst-b");
    std::fs::create_dir_all(&local_dst_a).expect("create the local destination A");
    std::fs::create_dir_all(&local_dst_b).expect("create the local destination B");
    std::fs::write(local_dst_a.join("f"), b"destination-a").expect("seed destination A");
    std::fs::write(local_dst_b.join("f"), b"destination-b").expect("seed destination B");

    let source = EndpointRemote::new(
        f.dir.path().join("src-a"),
        f.shared_root.clone(),
        Some("ssh://source:22"),
    );
    std::fs::write(source.data("f"), b"from-source").expect("seed the source");

    let token = DestinationOwnership::lock(Direction::Pull, &local_dst_a, &source)
        .expect("mint a pull token for destination A");
    let error = sync(
        Direction::Pull,
        &local_dst_b,
        &source,
        &ReplaceAll,
        Extraneous::Keep,
        token,
    )
    .expect_err("a pull token minted for destination A must be refused for destination B");
    assert_eq!(
        error.error().preflight_reason(),
        Some(PreflightKind::RunBindingMismatch),
        "the refusal must be the typed RUN-BINDING mismatch, not a source root or endpoint \
         mismatch: {error:?}"
    );
    assert_eq!(
        std::fs::read(local_dst_a.join("f")).expect("read destination A"),
        b"destination-a",
        "the token's OWN destination root (A) must be unmutated by the refused pull"
    );
    assert_eq!(
        std::fs::read(local_dst_b.join("f")).expect("read destination B"),
        b"destination-b",
        "the destination root named on the call (B) must be unmutated"
    );
}

/// The LEGITIMATE PULL case still works: the source that minted the token is
/// accepted and its content is copied into the local destination.
#[test]
fn the_transport_that_minted_a_pull_token_is_accepted() {
    let f = fixture();
    let local_dst = f.dir.path().join("local-dst");
    std::fs::create_dir_all(&local_dst).expect("create the local destination");

    let source = EndpointRemote::new(
        f.dir.path().join("src-a"),
        f.shared_root.clone(),
        Some("ssh://source-a:22"),
    );
    std::fs::write(source.data("x"), b"from-a").expect("seed source");

    let token = DestinationOwnership::lock(Direction::Pull, &local_dst, &source)
        .expect("mint a pull token against the source");
    let report = sync(
        Direction::Pull,
        &local_dst,
        &source,
        &ReplaceAll,
        Extraneous::Keep,
        token,
    )
    .expect("the source that minted the token must be accepted");
    assert!(report.conflicts.is_empty(), "{:?}", report.conflicts);
    assert_eq!(
        std::fs::read(local_dst.join("x")).expect("the pulled file"),
        b"from-a"
    );
}

// ---------------------------------------------------------------------------
// The mint refuses a transport that cannot state an endpoint
// ---------------------------------------------------------------------------

/// A transport whose `endpoint_identity()` is `None` cannot mint a remote
/// ownership token: the mint REFUSES (typed) naming the override it needs and
/// the explicitly-weaker `Unowned` path, and it does so BEFORE the far-side
/// acquisition or any mutation.
#[test]
fn a_transport_without_an_endpoint_identity_cannot_mint_a_remote_token() {
    let f = fixture();
    let host = EndpointRemote::new(f.dir.path().join("data"), f.shared_root.clone(), None);

    let error = match DestinationOwnership::lock_remote(Direction::Push, &f.src, &host) {
        Err(error) => error,
        Ok(_) => panic!("a transport without an endpoint identity must not mint a token"),
    };
    assert_eq!(
        error.preflight_reason(),
        Some(PreflightKind::EndpointIdentityUnavailable),
        "the refusal must be the typed ENDPOINT-identity-unavailable condition: {error:?}"
    );
    assert_eq!(
        host.far_side_lock_calls(),
        0,
        "the refusal must precede the far-side acquisition"
    );
    assert!(
        !host.data("f").exists(),
        "the refused mint must mutate nothing"
    );
}

// ---------------------------------------------------------------------------
// Mutation control: the distinguished refusals carry DISTINCT typed kinds
// ---------------------------------------------------------------------------

/// MUTATION CONTROL for the typed-preflight constraint: the two conditions a
/// caller must tell apart — a transport that states no ENDPOINT IDENTITY, and
/// a transport whose ROOT spelling differs from the token's — carry DIFFERENT
/// `PreflightKind`s, and neither is `Unclassified`. Collapsing them onto one
/// kind, or leaving the class untyped, fails here even though a test that only
/// matched message substrings would still pass.
#[test]
fn the_endpoint_and_root_refusals_carry_distinct_typed_kinds() {
    // (a) The missing-endpoint-identity refusal.
    let f = fixture();
    let anonymous = EndpointRemote::new(f.dir.path().join("data"), f.shared_root.clone(), None);
    let missing_identity =
        match DestinationOwnership::lock_remote(Direction::Push, &f.src, &anonymous) {
            Err(error) => error,
            Ok(_) => panic!("a transport without an endpoint identity must not mint a token"),
        };
    let identity_kind = missing_identity.preflight_reason();

    // (b) The remote-root-mismatch refusal: mint against root A, run against
    // root B on the SAME endpoint identity.
    let f = fixture();
    let host_a = EndpointRemote::new(
        f.dir.path().join("data-a"),
        f.shared_root.clone(),
        Some("ssh://shared-host:22"),
    );
    let host_b = EndpointRemote::new(
        f.dir.path().join("data-b"),
        f.dir.path().join("other").join("store"),
        Some("ssh://shared-host:22"),
    );
    let token = DestinationOwnership::lock_remote(Direction::Push, &f.src, &host_a)
        .expect("mint a remote token against host A");
    let root_mismatch = sync(
        Direction::Push,
        &f.src,
        &host_b,
        &ReplaceAll,
        Extraneous::Keep,
        token,
    )
    .expect_err("a token minted for another root spelling must be refused");
    let root_kind = root_mismatch.error().preflight_reason();

    assert_eq!(
        identity_kind,
        Some(PreflightKind::EndpointIdentityUnavailable)
    );
    assert_eq!(root_kind, Some(PreflightKind::RemoteRootMismatch));
    assert_ne!(
        identity_kind, root_kind,
        "the endpoint-identity and root-spelling refusals must have DISTINCT kinds"
    );
    assert!(
        identity_kind != Some(PreflightKind::Unclassified)
            && root_kind != Some(PreflightKind::Unclassified),
        "each distinguished condition must name its kind, never the fallback"
    );
}

// ---------------------------------------------------------------------------
// PULL: the token binds the SOURCE's LOCALNESS (the `None`-identity hole)
// ---------------------------------------------------------------------------

/// The `None`-IDENTITY REPRO. A token minted against a LOCAL source records no
/// endpoint identity (a `LocalTransport` states none, exactly as a path on
/// this host needs none) and `Prepared::matches` step 3 compared the identity
/// only as an `Option`, so `None == None` PASSED. Step 4 derives
/// `dest_is_local` as `remote.is_local()` for a PUSH but HARDCODES `true` for a
/// PULL, so a LOCAL->NON-LOCAL flip of the remote was never compared on a
/// PULL: a non-local source that states no identity and reports the same root
/// spelling was accepted, and the plan read from the local source was applied
/// against the non-local one.
///
/// MISMATCHED CONTENT: before the fix this run mutates the destination with
/// the NON-LOCAL source's bytes and fails only afterwards on the
/// post-transfer digest, so `preflight_reason()` is `None` and no ownership
/// refusal ever fires. Post-fix the remote's LOCALNESS is its own recorded
/// axis, compared for BOTH directions, and the run is REFUSED with the typed
/// localness mismatch before anything is mutated.
#[test]
fn a_pull_token_is_refused_when_the_source_flips_from_local_to_non_local() {
    let f = fixture();
    let local_dst = f.dir.path().join("local-dst");
    std::fs::create_dir_all(&local_dst).expect("create the local destination");
    // The LOCAL source the token is minted against. Its root spelling is the
    // SHARED layout spelling, so LOCALNESS is the only axis that distinguishes
    // it from the non-local source below.
    std::fs::create_dir_all(&f.shared_root).expect("create the shared root");
    std::fs::write(f.shared_root.join("x"), b"from-local").expect("seed the local source");
    let local_source = LocalTransport::new(
        &SysEnv::from_process(),
        f.shared_root.clone(),
        Layout::empty(),
    )
    .expect("a LocalTransport over the shared root");
    // The non-local source the run is handed: the SAME root spelling, NO
    // endpoint identity, and DIFFERENT data.
    let non_local_source =
        EndpointRemote::new(f.dir.path().join("src-b"), f.shared_root.clone(), None);
    std::fs::write(non_local_source.data("x"), b"from-non-local")
        .expect("seed the non-local source");

    let token = DestinationOwnership::lock(Direction::Pull, &local_dst, &local_source)
        .expect("mint a pull token against the LOCAL source");
    let error = sync(
        Direction::Pull,
        &local_dst,
        &non_local_source,
        &ReplaceAll,
        Extraneous::Keep,
        token,
    )
    .expect_err("a pull token minted against a local source must be refused for a non-local one");
    assert_eq!(
        error.error().preflight_reason(),
        Some(PreflightKind::RemoteLocalnessMismatch),
        "the refusal must be the typed LOCALNESS mismatch, not a post-transfer digest failure: {error:?}"
    );
    assert!(
        !local_dst.join("x").exists(),
        "the local destination must be unmutated by the refused pull"
    );
}

/// The EQUAL-CONTENT variant of the same hole: when the two sources' content
/// is byte-identical the post-transfer digest cannot see the substitution, so
/// before the fix `sync` returned **`Ok`** — the plan read from the LOCAL
/// source was applied against the non-local source silently. The localness
/// axis is what makes this run refusable at all.
#[test]
fn a_pull_token_is_refused_when_equal_content_hides_the_local_to_non_local_flip() {
    let f = fixture();
    let local_dst = f.dir.path().join("local-dst");
    std::fs::create_dir_all(&local_dst).expect("create the local destination");
    std::fs::create_dir_all(&f.shared_root).expect("create the shared root");
    std::fs::write(f.shared_root.join("x"), b"same").expect("seed the local source");
    let local_source = LocalTransport::new(
        &SysEnv::from_process(),
        f.shared_root.clone(),
        Layout::empty(),
    )
    .expect("a LocalTransport over the shared root");
    let non_local_source =
        EndpointRemote::new(f.dir.path().join("src-b"), f.shared_root.clone(), None);
    std::fs::write(non_local_source.data("x"), b"same").expect("seed the non-local source");

    let token = DestinationOwnership::lock(Direction::Pull, &local_dst, &local_source)
        .expect("mint a pull token against the LOCAL source");
    let error = sync(
        Direction::Pull,
        &local_dst,
        &non_local_source,
        &ReplaceAll,
        Extraneous::Keep,
        token,
    )
    .expect_err(
        "even EQUAL content must not hide a local->non-local source flip: returning `Ok` here \
         means the local source's plan was applied against a different, non-local source",
    );
    assert_eq!(
        error.error().preflight_reason(),
        Some(PreflightKind::RemoteLocalnessMismatch),
        "the refusal must be the typed LOCALNESS mismatch: {error:?}"
    );
    assert!(
        !local_dst.join("x").exists(),
        "the local destination must be unmutated by the refused pull"
    );
}

// ---------------------------------------------------------------------------
// PUSH: the token binds the DESTINATION's LOCALNESS (the other direction of
// the same `None`-identity hole)
// ---------------------------------------------------------------------------

/// The sorted entry names directly under `dir`, for an UNMUTATED comparison.
fn entry_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("read the destination directory")
        .map(|entry| {
            entry
                .expect("read a destination entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// THE PUSH TWIN of
/// [`a_pull_token_is_refused_when_the_source_flips_from_local_to_non_local`].
///
/// `Prepared::matches` step 4 compares the remote's LOCALNESS for BOTH
/// directions, but before this test only the PULL direction was pinned: gating
/// step 4 on `direction == Direction::Pull` left the ENTIRE suite green, so a
/// future edit could re-open the PUSH half of the localness binding. A token minted
/// against a LOCAL destination — `DestinationOwnership::lock(Direction::Push,
/// src, &LocalTransport over P)` records `remote_is_local == true` and states
/// no endpoint identity — is replayed against a NON-LOCAL third-party `Remote`
/// that states the SAME root spelling and NO identity. Direction, local root,
/// remote root and identity (`None == None`) therefore all match, and ONLY the
/// localness axis can refuse.
///
/// Before the fix this run MUTATES the non-local destination (the token's
/// lock was taken on a different, local tree). The refusal now precedes
/// `run`, so the non-local destination is byte-for-byte unmutated.
#[test]
fn a_push_token_is_refused_when_the_destination_flips_from_local_to_non_local() {
    let f = fixture();
    // The LOCAL destination the token is minted against. Its root spelling is
    // the SHARED layout spelling, so LOCALNESS is the only axis that
    // distinguishes it from the non-local destination below.
    std::fs::create_dir_all(&f.shared_root).expect("create the shared root");
    std::fs::write(f.shared_root.join("sentinel"), b"local-only")
        .expect("seed the local destination");
    let local_dest = LocalTransport::new(
        &SysEnv::from_process(),
        f.shared_root.clone(),
        Layout::empty(),
    )
    .expect("a LocalTransport over the shared root");

    let token = DestinationOwnership::lock(Direction::Push, &f.src, &local_dest)
        .expect("mint a push token against the LOCAL destination");

    // The non-local destination the run is handed: the SAME root spelling, NO
    // endpoint identity, and DIFFERENT content.
    let non_local_dest =
        EndpointRemote::new(f.dir.path().join("dst-b"), f.shared_root.clone(), None);
    std::fs::write(non_local_dest.data("sentinel"), b"non-local-bytes")
        .expect("seed the non-local destination");
    let before = entry_names(&non_local_dest.data_root);

    let error = sync(
        Direction::Push,
        &f.src,
        &non_local_dest,
        &ReplaceAll,
        Extraneous::Keep,
        token,
    )
    .expect_err(
        "a push token minted against a LOCAL destination must be refused for a non-local one",
    );
    assert_eq!(
        error.error().preflight_reason(),
        Some(PreflightKind::RemoteLocalnessMismatch),
        "the refusal must be the typed LOCALNESS mismatch, not a post-transfer verification \
         failure or a silent success: {error:?}"
    );
    assert_eq!(
        std::fs::read(non_local_dest.data("sentinel")).expect("the non-local sentinel"),
        b"non-local-bytes",
        "the non-local destination must be UNMUTATED by the refused push"
    );
    assert_eq!(
        entry_names(&non_local_dest.data_root),
        before,
        "the non-local destination's entry set must be unchanged by the refused push"
    );
    assert!(
        !non_local_dest.data("f").exists(),
        "the refused push must not copy the source entry into the non-local destination"
    );
}

/// The EQUAL-CONTENT variant of the PUSH hole: when the non-local destination
/// already holds byte-identical content, the run has nothing left to write, so
/// before the fix `sync` would verify cleanly and return **`Ok`** — the token
/// minted for a LOCAL destination applied to a non-local one with nothing to
/// catch it. The localness axis is what makes this run refusable at all.
#[test]
fn a_push_token_is_refused_when_equal_content_hides_the_local_to_non_local_flip() {
    let f = fixture();
    std::fs::create_dir_all(&f.shared_root).expect("create the shared root");
    // The local destination holds exactly the source content.
    std::fs::write(f.shared_root.join("f"), b"payload").expect("seed the local destination");
    let local_dest = LocalTransport::new(
        &SysEnv::from_process(),
        f.shared_root.clone(),
        Layout::empty(),
    )
    .expect("a LocalTransport over the shared root");

    let token = DestinationOwnership::lock(Direction::Push, &f.src, &local_dest)
        .expect("mint a push token against the LOCAL destination");

    let non_local_dest =
        EndpointRemote::new(f.dir.path().join("dst-b"), f.shared_root.clone(), None);
    std::fs::write(non_local_dest.data("f"), b"payload")
        .expect("seed the non-local destination with equal content");
    let before = entry_names(&non_local_dest.data_root);

    let error = sync(
        Direction::Push,
        &f.src,
        &non_local_dest,
        &ReplaceAll,
        Extraneous::Keep,
        token,
    )
    .expect_err(
        "even EQUAL content must not hide a local->non-local destination flip: returning `Ok` \
         here means the token minted for a local destination was applied to a non-local one",
    );
    assert_eq!(
        error.error().preflight_reason(),
        Some(PreflightKind::RemoteLocalnessMismatch),
        "the refusal must be the typed LOCALNESS mismatch: {error:?}"
    );
    assert_eq!(
        std::fs::read(non_local_dest.data("f")).expect("the non-local content"),
        b"payload",
        "the non-local destination must be UNMUTATED by the refused push"
    );
    assert_eq!(
        entry_names(&non_local_dest.data_root),
        before,
        "the non-local destination's entry set must be unchanged by the refused push"
    );
}
