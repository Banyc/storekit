//! The PUBLIC operation-scoped sidecar critical section
//! (`transport::with_operation_lock_sidecar`) must run the crate's ONE
//! reserved-spelling gate BEFORE it creates anything.
//!
//! THE DEFECT this file pins: `with_operation_lock_sidecar` is the ONE public
//! CREATE path that reached `ensure_operation_lock_sidecar_durable` — a raw
//! `create_dir_all` + `create_new` + `chmod` on `base.join(sidecar)` — without
//! `refuse_reserved_mutation`. A caller that passed one of the crate's own
//! bookkeeping spellings as its `sidecar` therefore CREATED a lock record /
//! residue that every guarded path refuses. The guard now runs at the public
//! entry, so the same spelling is refused, typed, with nothing created.
//!
//! The control group below shows the comparator guarded path
//! (`atomic::write_atomic_replace`) already refused these spellings; the fix
//! makes the sidecar helper agree with it.

use std::path::Path;

use storekit::RootedRelativePath;
use storekit::atomic::{ReplaceStage, write_atomic_replace};
use storekit::error::{Error, ReservedKind};
use storekit::reserved::{is_lock_record_name, is_residue_name};
use storekit::transport::{Layout, with_operation_lock_sidecar};

/// A validated root-relative path, parsed at the boundary like a consumer must.
fn rp(s: &str) -> RootedRelativePath {
    RootedRelativePath::parse(Path::new(s)).unwrap()
}

/// Which of the crate's two reserved authorities the gate must name, typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expected {
    /// The lock-record authority: `Error::Conflict`.
    LockRecord,
    /// The residue authority: `Error::Reserved { reason: ResidueBelow }`.
    Residue,
}

/// Every spelling here NAMES or descends THROUGH the crate's own bookkeeping,
/// so a caller-chosen `sidecar` may not spell one: `operation.lock` /
/// `snapshots/.001.operation.lock` / `OPERATION.LOCK` are the application lock
/// record (and its case alias), and `a/b/.sync-aside.1` is a byte-exact
/// reserved residue component in a NON-final position.
const RESERVED: &[(&str, Expected)] = &[
    ("operation.lock", Expected::LockRecord),
    ("OPERATION.LOCK", Expected::LockRecord),
    ("state/operation.lock", Expected::LockRecord),
    ("snapshots/.001.operation.lock", Expected::LockRecord),
    ("a/b/.sync-aside.1", Expected::Residue),
];

/// Assert the gate's refusal is TYPED, not a generic transport error.
fn assert_typed_refusal(spelling: &str, expected: Expected, err: &Error) {
    match expected {
        Expected::LockRecord => assert!(
            matches!(err, Error::Conflict(_)),
            "the lock-record spelling {spelling:?} must be refused typed as \
             Error::Conflict, got {err:?}"
        ),
        Expected::Residue => assert_eq!(
            err.reserved_kind(),
            Some(ReservedKind::ResidueBelow),
            "the residue spelling {spelling:?} must be refused typed as \
             Error::Reserved{{ResidueBelow}}, got {err:?}"
        ),
    }
}

/// THE FIX. Every reserved spelling is refused BEFORE anything is created, and
/// the refusal is typed. Run against the PRE-FIX tree, this fails on the first
/// (and every) entry with `Ok` and an on-disk entry, which is the defect.
#[test]
fn sidecar_helper_refuses_reserved_spellings_before_creating() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).expect("create the root");

    let mut created = Vec::new();
    for (spelling, expected) in RESERVED {
        let sidecar = rp(spelling);
        match with_operation_lock_sidecar(&root, &sidecar, || Ok(())) {
            Ok(()) => created.push(format!(
                "{spelling:?} => Ok(()) and on-disk entry exists={} at {}",
                root.join(spelling).exists(),
                root.join(spelling).display()
            )),
            Err(err) => {
                if root.join(spelling).exists() {
                    created.push(format!(
                        "{spelling:?} => Err but still created {}",
                        root.join(spelling).display()
                    ));
                }
                assert_typed_refusal(spelling, *expected, &err);
            }
        }
    }

    assert!(
        created.is_empty(),
        "with_operation_lock_sidecar accepted and acted on reserved spellings; \
         the ONE public create path bypassed the reserved-spelling gate: {created:#?}"
    );

    // Nothing was created anywhere under the root: the refusal ran up front.
    assert_eq!(
        std::fs::read_dir(&root).expect("read the root").count(),
        0,
        "a refused sidecar spelling must not have created a parent chain or the record"
    );
}

/// THE CONTROL: the guarded comparator path already refused these exact
/// spellings. This stays red-free before and after the fix, and is the
/// evidence that the sidecar helper was the odd one out.
#[test]
fn guarded_comparator_refuses_the_same_spellings() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).expect("create the root");

    let mut accepted = Vec::new();
    for (spelling, _) in RESERVED {
        let path = root.join(spelling);
        match write_atomic_replace(&path, b"x", &mut |_: ReplaceStage| None) {
            Ok(_) => accepted.push(*spelling),
            Err(err) => assert!(
                err.reserved_kind().is_some() || matches!(err, Error::Conflict(_)),
                "the comparator's refusal of {spelling:?} must be typed, got {err:?}"
            ),
        }
        assert!(
            !path.exists(),
            "the guarded comparator must refuse {spelling:?} before creating it"
        );
    }
    assert!(
        accepted.is_empty(),
        "the guarded comparator accepted reserved spellings: {accepted:?}"
    );
}

/// THE PRODUCTION SPELLING. The crate's own default
/// `Layout::empty().lock_sidecar` (`state/operation.lock.mutex`) is NOT a
/// lock-record or residue spelling, so the guard must let it through: the
/// critical section runs and the record is created durably.
#[test]
fn default_layout_sidecar_spelling_still_works() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).expect("create the root");

    let layout = Layout::empty();
    // The production spelling is NEITHER reserved authority: verified, not
    // assumed. (`operation.lock.mutex` is not a lock record — the record is
    // exactly `operation.lock` — and not the sibling `.<name>.operation.lock`
    // shape either; it is not a residue.)
    let spelling = layout
        .lock_sidecar
        .as_path()
        .to_str()
        .expect("the default sidecar spelling is UTF-8");
    for component in layout.lock_sidecar.as_path().components() {
        let name = component.as_os_str().to_str().expect("a UTF-8 component");
        assert!(
            !is_lock_record_name(name),
            "no component of the default sidecar spelling {spelling:?} may be a lock record; \
             {name:?} is"
        );
        assert!(
            !is_residue_name(name),
            "no component of the default sidecar spelling {spelling:?} may be residue; \
             {name:?} is"
        );
    }
    let mut ran = false;
    with_operation_lock_sidecar(&root, &layout.lock_sidecar, || {
        ran = true;
        Ok(())
    })
    .expect("the crate's own default sidecar spelling must still be accepted");

    assert!(ran, "the critical section must actually run");
    let record = root.join(layout.lock_sidecar.as_path());
    assert!(
        record.is_file(),
        "the default sidecar record must be created at {}",
        record.display()
    );
    assert_eq!(
        std::fs::read_to_string(&record).expect("read the empty record"),
        "",
        "the sidecar record is an empty mutex file"
    );
}
