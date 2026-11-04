//! The `tolerant-staging-docs` contract, end to end through the PUBLIC API.
//!
//! The strict source walk refuses a hard link (on Unix) and an absolute or
//! escaping symlink. [`storekit::atomic::copy_tree_verbatim`] is NOT a
//! normalizer: on Unix it refuses a hard link rather than splitting it, and it
//! reproduces an absolute symlink target VERBATIM rather than materialising or
//! relative-ising it. So a staged tree can still hold the very entry the strict
//! walk refuses, and the external normalization step (`rsync -a --copy-links`
//! without `-H`, or GNU tar's `--dereference --hard-dereference`) is the
//! caller's decision, documented in [`storekit::sync`].
//!
//! Unix only: a hard link's `nlink` and an absolute symlink need POSIX
//! semantics to build, and the refusals under test are the Unix port's. The
//! Windows gate compiles this file out rather than pretending to test it, as
//! the other Unix-specific test files do.

#![cfg(unix)]
#![allow(clippy::disallowed_methods)]

use std::os::unix::fs::MetadataExt;
use std::path::Path;

use storekit::atomic::copy_tree_verbatim;
use storekit::env::SysEnv;
use storekit::error::{MaterializationKind, StoreKind};
use storekit::sync::{
    DestinationOwnership, Direction, ReplaceAll, SyncError, SyncOptions, SyncReport, sync,
};
use storekit::transport::{Layout, LocalTransport};

fn env() -> SysEnv {
    SysEnv::from_process()
}

fn transport(root: &Path) -> LocalTransport {
    LocalTransport::new(&env(), root.to_path_buf(), Layout::empty()).unwrap()
}

fn write(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, bytes).unwrap();
}

/// Run the OWNED path: acquire the destination's operation lock, then [`sync`].
fn owned(
    direction: Direction,
    local_root: &Path,
    remote: &dyn storekit::transport::Remote,
    options: &SyncOptions,
) -> Result<SyncReport, SyncError> {
    let ownership = DestinationOwnership::lock(direction, local_root, remote, &options.exclusions)?;
    sync(
        direction,
        local_root,
        remote,
        &ReplaceAll,
        options.clone(),
        ownership,
    )
}

/// The strict source walk refuses a HARD LINK with the typed materialization
/// kind, and `copy_tree_verbatim` refuses the SAME source instead of splitting
/// the link into two independent regular files.
///
/// The copy refusal in isolation is the unit test
/// `copy_tree_verbatim_refuses_what_it_cannot_copy_faithfully` in
/// `src/atomic/unix.rs`; this test pins it for the staging flow's source so the
/// two refusals are stated together as the documented limit.
#[test]
fn sync_refuses_a_hard_link_and_the_verbatim_copy_does_too() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    write(&src.join("a"), b"hard-linked\n");
    std::fs::hard_link(src.join("a"), src.join("b")).unwrap();
    std::fs::create_dir_all(&dst).unwrap();

    let refusal = owned(
        Direction::Push,
        &src,
        &transport(&dst),
        &SyncOptions::default(),
    )
    .expect_err("a hard link in the source must refuse the strict walk");
    assert_eq!(
        refusal.error().materialization_reason(),
        Some(MaterializationKind::HardLink),
        "the refusal must be the typed hard-link condition: {refusal:?}"
    );

    let staging = tmp.path().join("staging");
    let error = copy_tree_verbatim(&src, &staging)
        .expect_err("the verbatim copy must refuse a hard link, not duplicate it");
    assert_eq!(
        error.store_reason(),
        Some(StoreKind::CopyHardLink),
        "the copy refusal must be the typed hard-link condition: {error:?}"
    );
    assert_eq!(
        std::fs::metadata(src.join("a")).unwrap().nlink(),
        2,
        "the refused copy must not touch the source's link"
    );
}

/// An absolute symlink SURVIVES `copy_tree_verbatim` (the target is reproduced
/// verbatim), and the strict walk over the STAGED tree still refuses it. This
/// is the documented limit: the crate's own copy cannot normalize a link.
#[test]
fn a_verbatim_copy_reproduces_an_absolute_symlink_that_sync_refuses() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let staging = tmp.path().join("staging");
    let dst = tmp.path().join("dst");
    write(&src.join("f"), b"payload");
    std::os::unix::fs::symlink("/etc/passwd", src.join("absolute-link")).unwrap();
    std::fs::create_dir_all(&dst).unwrap();

    let refusal = owned(
        Direction::Push,
        &src,
        &transport(&dst),
        &SyncOptions::default(),
    )
    .expect_err("an absolute symlink in the source must refuse the strict walk");
    assert_eq!(
        refusal.error().materialization_reason(),
        Some(MaterializationKind::AbsoluteSymlink),
        "the refusal must be the typed absolute-symlink condition: {refusal:?}"
    );

    copy_tree_verbatim(&src, &staging)
        .expect("the verbatim copy reproduces the absolute target; it does not normalize it");
    assert_eq!(
        std::fs::read_link(staging.join("absolute-link")).unwrap(),
        Path::new("/etc/passwd"),
        "the target is carried VERBATIM, not materialised or relative-ised"
    );

    let refusal = owned(
        Direction::Push,
        &staging,
        &transport(&dst),
        &SyncOptions::default(),
    )
    .expect_err(
        "the staged tree still holds the absolute symlink, so the strict walk still refuses it",
    );
    assert_eq!(
        refusal.error().materialization_reason(),
        Some(MaterializationKind::AbsoluteSymlink),
        "a staged tree is an ordinary directory, so the refusal is the symlink's, not the \
         staging root's: {refusal:?}"
    );
    assert!(
        !dst.join("f").exists(),
        "the refused run must not transfer anything"
    );
}
