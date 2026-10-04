//! The PUBLIC, path-based symlink creator (`platform::symlink`) must run the
//! crate's ONE reserved-spelling authority, exactly as its descriptor-confined
//! twin (`atomic::symlink_fd`) does.
//!
//! THE DEFECT this file pins: `platform::symlink` was `pub`, took a raw
//! `&Path` link name, and never ran `refuse_reserved_mutation`. Measured with
//! the public API alone, `platform::symlink("/etc/hostname",
//! root.join("operation.lock"))` returned `Ok` and created the record
//! spelling, while `atomic::symlink_fd` refused the same name. A consumer
//! could therefore create `operation.lock`, `.<name>.operation.lock`, and
//! `.sync-aside.…` names at will. `symlink(2)` fails `EEXIST` on an existing
//! entry, so the defect cannot REPLACE an existing record — it is bounded to a
//! fail-closed confusion, not an inode split — but the class it belongs to (a
//! path-based primitive shipped missing the guard its `_fd` twin had) is
//! closed by the guard these tests exercise.
//!
//! The verbatim COPY keeps its UNGUARDED creator: `copy_tree_verbatim`
//! deliberately carries reserved and temp spellings into its destination, and
//! `platform::symlink_verbatim` is the named (`pub(crate)`) weak path it uses.

#![cfg(unix)]
#![allow(clippy::disallowed_methods)]

use std::path::Path;

use storekit::error::{Error, ReservedKind};
use storekit::platform::symlink;

/// Every reserved spelling is refused, TYPED, through the public surface, and
/// nothing is created — while a legitimate name still becomes a symlink.
#[test]
fn the_public_symlink_creator_refuses_the_crates_reserved_spellings() {
    let tmp = tempfile::tempdir().expect("tempdir");

    // (1) A LEGITIMATE name still works: the guard refuses only the crate's
    // own bookkeeping spellings.
    let ok = tmp.path().join("ordinary-link");
    symlink(Path::new("some-target"), &ok).expect("an ordinary name is still creatable");
    assert!(
        std::fs::symlink_metadata(&ok)
            .expect("the created link")
            .file_type()
            .is_symlink(),
        "a legitimate name must still become a symlink"
    );

    // (2) The LOCK-record authority, typed as `Error::Conflict`.
    for spelling in ["operation.lock", ".store.operation.lock"] {
        let link = tmp.path().join(spelling);
        let error = symlink(Path::new("some-target"), &link)
            .expect_err("a lock-record spelling must be refused");
        assert!(
            matches!(error, Error::Conflict(_)),
            "the lock-record spelling {spelling:?} must be refused typed as Error::Conflict, \
             got {error:?}"
        );
        assert!(
            std::fs::symlink_metadata(&link).is_err(),
            "the refused lock-record spelling {spelling:?} must not be created"
        );
    }

    // (3) The RESIDUE authority, typed as `Error::Reserved{ResidueBelow}`.
    let aside = tmp.path().join(".sync-aside.1");
    let error =
        symlink(Path::new("some-target"), &aside).expect_err("a residue spelling must be refused");
    assert_eq!(
        error.reserved_kind(),
        Some(ReservedKind::ResidueBelow),
        "the residue spelling must be refused typed as Error::Reserved{{ResidueBelow}}, \
         got {error:?}"
    );
    assert!(
        std::fs::symlink_metadata(&aside).is_err(),
        "the refused residue spelling must not be created"
    );
}

/// The comparator: the descriptor-confined twin refuses the SAME spelling, so
/// the public path-based creator and `atomic::symlink_fd` agree.
#[test]
fn the_confined_twin_refuses_the_same_reserved_spelling() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = storekit::atomic::RootDir::open(tmp.path()).expect("open the owned root");
    let rel = storekit::RootedRelativePath::parse(Path::new("operation.lock")).expect("parse");
    let error = storekit::atomic::symlink_fd(&root, Path::new("some-target"), &rel)
        .expect_err("the confined twin must refuse the lock record");
    assert!(
        matches!(error, Error::Conflict(_)),
        "the confined twin must refuse typed as Error::Conflict, got {error:?}"
    );
}
