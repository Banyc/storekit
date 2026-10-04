//! THE crate's one lock-record mutation guard, and the unforgeable
//! capability that any name-mutating operation must present.
//!
//! The single-holder guarantee of [`crate::lock::FileLock`] rests on the
//! lock record's INODE never changing under its path: a later acquisition
//! must flock the SAME inode the live holder holds. Any operation that
//! unlinks, replaces, renames, or truncates the record breaks that, so the
//! crate must have no reachable path to such a mutation.
//!
//! Four earlier passes each enumerated the call sites they could find and
//! missed one (a second symlink implementation, the raw rename primitive
//! under the guarded rename, a raw truncating open, and the Windows
//! transport's direct `std::fs` seams). Enumeration is the wrong shape.
//! This module makes the guard STRUCTURAL instead:
//!
//! * [`refuse_lock_record`] is the ONE spelling authority. Every
//!   name-mutating primitive in [`crate::atomic`] consults it, and it folds
//!   the spelling ALIASES (case; and the Win32 trailing dot/space) through the
//!   reserved-name authority, so a case or trailing-dot spelling cannot slip
//!   past it.
//! * [`GuardedRel`] is an unforgeable proof that the guard ran, carrying the
//!   [`GuardScope`] the guard authorized. Its fields are private to THIS
//!   module, so no other module — not [`super`] itself, not [`super::unix`] /
//!   [`super::windows`], not `transport` / `sync` — can build one except
//!   through the THREE minting constructors [`GuardedRel::new`],
//!   [`GuardedRel::new_for_owned_lock_record`], and
//!   [`GuardedRel::new_for_residue`], each of which runs the ONE reserved-
//!   spelling authority [`refuse_reserved_mutation`]. The rel-path mutators
//!   take one by value, so a NEW primitive cannot name a multi-component
//!   mutation without first proving it guarded.
//!
//! # What this does NOT cover (the honest residual)
//!
//! A developer who writes a brand-new direct `libc::unlinkat` /
//! `libc::renameat` / `libc::open` / `libc::rmdir` call into a module that is
//! not [`super::unix`] is not stopped by the type system: `libc` is an ordinary
//! dependency and Rust cannot forbid a call to it. That is why the crate ALSO
//! carries source audits. They are TEXT audits, and their claim is exactly what
//! they catch:
//!
//! * `atomic::guard::tests::no_libc_reference_outside_the_funnel` scans EVERY
//!   `.rs` file under the package directory (not only `src/`) and fails on ANY
//!   reference to `libc` — a mutating symbol, a module alias (`use libc as c;`),
//!   a braced self-alias, a re-export, a glob, or a call broken across a
//!   newline — outside `src/atomic/unix.rs` or a test-only file. The crate's
//!   audited, non-mutating `libc` surface outside the funnel is pinned by
//!   spelling and count, so an added, removed, or renamed reference fails here
//!   too. The funnel's own mutating-symbol counts are pinned. (A shared
//!   textual scan cannot literally forbid a legitimate `libc::flock` or
//!   `libc::fstatat`, so those are pinned by name and count, and the audit
//!   independently refuses any pinned-or-new MUTATING symbol.)
//! * `atomic::guard::tests::std_fs_name_mutation_counts_are_pinned` pins the per-file
//!   per-symbol counts of the `std::fs` calls that can REMOVE or REPLACE a
//!   directory entry (`remove_file` / `remove_dir` / `remove_dir_all` /
//!   `rename` / `hard_link`) in PRODUCTION code (every `#[cfg(test)]` item is
//!   removed first), so a new one changes a count and forces review, and a
//!   test-only call is never counted as production. The count is matched over
//!   the TOKENISED path `std::fs::<symbol>(`, so whitespace before the call
//!   paren does not evade it. The same audit runs `std_fs_mutation_violations`,
//!   which refuses every IMPORT route to those five symbols — a braced or
//!   single `use std::fs::{…}`, a `use std::fs as <alias>` / `use std::fs;`
//!   module alias, a `use std::fs::*` glob, the bare or aliased imported
//!   symbol, and the `std`-crate-ROOT aliases (`use std as s;`,
//!   `use ::std as s;`, `use std::{self as s};`) resolved TRANSITIVELY, so
//!   `s::fs::remove_file(…)` and `use s::fs::remove_file;` are refused too.
//!   Test-only-ness is derived from the GATING (`#[cfg(...)]` implying `test`
//!   on the module's `mod` declaration), never from a file NAME.
//!
//! The residual holes a text audit CANNOT close, and which the claim above is
//! scoped NOT to include: code produced by a MACRO (`macro_rules!` or a proc
//! macro), code pulled in by `include!` from OUTSIDE the package directory, a
//! call made through a function POINTER / `dyn` dispatch, and any mutation that
//! preserves the entry's inode (`std::fs::write`, `std::fs::copy`,
//! `std::fs::set_permissions`, `std::fs::create_dir*`) — those cannot split a
//! holder because the flock is attached to the unchanged inode, so they are not
//! counted. A raw FFI declaration — `extern "C" { fn unlinkat(…); }` followed
//! by a call — names neither `libc` nor `std::fs`, so NEITHER text scanner sees
//! it; its reach is one declaration the crate's own author writes, which is
//! exactly the residue a source-matching audit carries and names rather than
//! denies. A foreign process, or a raw `std::fs` call the caller writes
//! itself, is outside the crate entirely and is not stopped by any of this.
//! Together the private funnel and the audits mean the "obvious way" to add a
//! mutation — calling the crate's own wrapper, or reaching for a raw syscall, a
//! `std::fs` removal, or an IMPORT ALIAS of one — either presents the
//! capability or fails a test.

use crate::error::{Error, Result};
use std::path::{Component, Path, PathBuf};

/// The on-disk identity of a directory entry: the pair `(device, inode)` on
/// Unix, `(volume serial, file index)` on Windows. Two spellings with the
/// same identity ARE one entry; two spellings with different identities are
/// DISTINCT entries even when a case/dot fold maps one onto the other.
#[cfg(unix)]
pub(crate) type EntryIdentity = (u64, u64);
#[cfg(windows)]
pub(crate) type EntryIdentity = (u64, u64);
#[cfg(not(any(unix, windows)))]
pub(crate) type EntryIdentity = (u64, u64);

/// Resolve `path` to its on-disk identity WITHOUT following a final symlink
/// (`lstat`, and `FILE_FLAG_OPEN_REPARSE_POINT` on Windows): a symlink is its
/// own entry, matching the crate's `O_NOFOLLOW` confinement. `Ok(None)` is a
/// confirmed absence; any other error is returned so a caller can fail closed.
#[cfg(unix)]
pub(crate) fn entry_identity(path: &Path) -> std::io::Result<Option<EntryIdentity>> {
    use std::os::unix::fs::MetadataExt;
    match std::fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some((meta.dev(), meta.ino()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(windows)]
pub(crate) fn entry_identity(path: &Path) -> std::io::Result<Option<EntryIdentity>> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_FLAG_OPEN_REPARSE_POINT, GetFileInformationByHandle,
    };
    // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE: a concurrent
    // holder of the record must not make the identity probe fail.
    const SHARE_ALL: u32 = 0x1 | 0x2 | 0x4;
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_ALL)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let index = ((info.nFileIndexHigh as u64) << 32) | u64::from(info.nFileIndexLow);
    Ok(Some((u64::from(info.dwVolumeSerialNumber), index)))
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn entry_identity(_path: &Path) -> std::io::Result<Option<EntryIdentity>> {
    // Neither supported port of the substrate is selected, so no identity is
    // available and ownership falls back to byte-exact spelling.
    Ok(None)
}

/// The ONE lock record the crate's protocol OWNS ([`crate::transport::Layout::lock`]),
/// as an AUTHORITY rather than a caller-chosen path. This is what makes the
/// ownership grant unforgeable: [`GuardedRel::new_for_owned_lock_record`]
/// takes one of THESE, never a raw path, so the candidate cannot also be
/// passed as its own authority (`new_for_owned_lock_record(rel, rel)` does not
/// typecheck — the second parameter is not a `Path`).
///
/// The two constructors both read the record out of the transport's own
/// [`crate::transport::Layout`], so the authority is derived from the trusted
/// deployment layout, not from whatever path is being mutated:
///
/// * [`OwnedLockRecord::local`] carries the LOCAL root and resolves the
///   record's `(device, inode)` against the candidate's, which is the actual
///   ownership decision (see [`Self::owns`]);
/// * [`OwnedLockRecord::remote`] carries no local root — a remote transport
///   cannot `stat` the far side from here — so it grants ONLY a byte-exact
///   spelling. That is strictly more conservative (more refusals), never an
///   escalation, because a case/dot alias of a REMOTE entry may describe a
///   distinct far-side entry the local process cannot see.
///
/// An in-crate author who wants to repeat the old hole must now explicitly
/// DECLARE a fraudulent layout (`Layout { lock: <victim>, ..Layout::empty() }`)
/// and build an authority from it; they can no longer name the candidate
/// itself as the owned record in one expression. The single legitimate
/// construction site is the transport that holds the layout.
pub(crate) struct OwnedLockRecord {
    /// The local root the record lives under, or `None` for a REMOTE record.
    root: Option<PathBuf>,
    /// The record's root-relative spelling (`Layout::lock`).
    rel: PathBuf,
}

impl OwnedLockRecord {
    /// The authority for a LOCAL transport rooted at `root`, owning
    /// `layout.lock`.
    pub(crate) fn local(root: &Path, layout: &crate::transport::Layout) -> Self {
        Self {
            root: Some(root.to_path_buf()),
            rel: layout.lock.as_path().to_path_buf(),
        }
    }

    /// The authority for a REMOTE transport, owning `layout.lock` by spelling
    /// only (the far side is not stat-able from here).
    pub(crate) fn remote(layout: &crate::transport::Layout) -> Self {
        Self {
            root: None,
            rel: layout.lock.as_path().to_path_buf(),
        }
    }

    /// Whether `candidate` denotes THE SAME on-disk entry as the owned record.
    ///
    /// * Both exist: grant iff their resolved identities are EQUAL. A case or
    ///   trailing-dot/space alias that resolves to the same inode is the owned
    ///   record; one that resolves to a DISTINCT inode (the constant case on a
    ///   case-sensitive filesystem, and the trailing-dot case on macOS and
    ///   Linux alike) is not.
    /// * The candidate does not exist: grant iff its spelling is byte-equal to
    ///   the owned record's. This is the record-CREATION case — there is no
    ///   entry yet to compare, and creating any other lock-record spelling is
    ///   not the protocol's own record.
    /// * Anything else (the candidate exists but the owned record does not, or
    ///   an identity probe failed) is NOT the owned record, so a lock-record
    ///   spelling is refused by [`refuse_lock_record`].
    pub(crate) fn owns(&self, candidate: &Path) -> bool {
        let Some(root) = self.root.as_deref() else {
            return candidate == self.rel.as_path();
        };
        let owned_path = root.join(&self.rel);
        let candidate_path = root.join(candidate);
        match (entry_identity(&owned_path), entry_identity(&candidate_path)) {
            (Ok(Some(owned)), Ok(Some(candidate))) => owned == candidate,
            (_, Ok(None)) => candidate == self.rel.as_path(),
            _ => false,
        }
    }
}

/// Refuse `rel` when any component names one of the crate's LOCK-RECORD
/// spellings ([`crate::reserved::is_lock_record_name`]) — the application
/// lock record `operation.lock` or the sibling record
/// `.<name>.operation.lock`, in byte-exact, case-alias, or trailing-dot/space
/// alias form. The check consults EVERY component, so a path that NAMES or
/// descends THROUGH a record is refused, not only a path whose final component
/// is the record.
///
/// PRIVATE to this module by design: the only in-crate ways to run it are the
/// two [`GuardedRel`] constructors, so a caller cannot consult the guard
/// without minting the capability the mutators demand.
fn refuse_lock_record(rel: &Path) -> Result<()> {
    for component in rel.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        if !name
            .to_str()
            .is_some_and(crate::reserved::is_lock_record_name)
        {
            continue;
        }
        return Err(Error::conflict(format!(
            "refusing to mutate the crate's lock record in {}: the record's stable inode is what \
             makes two simultaneous holders impossible, so removing, replacing, or renaming it \
             would admit a second holder",
            rel.display()
        )));
    }
    Ok(())
}

/// Which reserved spelling, if any, a mutation is SANCTIONED to break.
///
/// The crate has exactly TWO reserved-spelling authorities: the LOCK-record
/// spelling ([`crate::reserved::is_lock_record_name`]) and the RESIDUE
/// spelling ([`crate::reserved::is_residue_name`], an unaddressable,
/// non-temp name that HOLDS a stranded original). A primitive that consulted
/// one and not the other is exactly how a stranded ORIGINAL was destroyed, so
/// there is ONE gate ([`refuse_reserved_mutation`]) and this parameter names
/// the ONE reserved spelling it may let through.
#[derive(Clone, Copy)]
pub(crate) enum Sanction<'a> {
    /// No reserved spelling may be broken. Every name-mutating primitive that
    /// is not one of the two deliberate exceptions presents this.
    None,
    /// The ONE lock record the presented [`OwnedLockRecord`] owns (by resolved
    /// identity, or by the byte-exact creation spelling while no entry exists).
    /// EVERY other lock-record spelling and EVERY residue spelling is still
    /// refused.
    OwnedLockRecord(&'a OwnedLockRecord),
    /// The residue at the FINAL component only: the explicit discard path
    /// (`sync::Residue::discard` -> `atomic::remove_residue_*`). The LOCK
    /// authority still refuses a lock-record spelling (a record is also a
    /// residue spelling), and a residue in any NON-final component is still
    /// refused, so a discard can never touch a NESTED strand.
    FinalResidue,
    /// EVERY residue spelling is permitted, because the operation cannot
    /// DESTROY an existing strand BY ITS SPELLING: it only CREATES a fresh
    /// entry (a directory or a symlink) or MOVES one (a rename endpoint).
    /// `mkdir`/`symlink` fail on an existing entry, and a rename only replaces
    /// what the caller explicitly names — so the engine's own claim-aside
    /// renames and case-probe directory still work, while the LOCK authority
    /// is still enforced. The public rename nevertheless applies the residue
    /// authority (a rename CAN replace a strand), so this sanction is used
    /// only by the crate's own sanctioned rename/creation primitives.
    Residue,
}

/// THE one reserved-spelling gate. Every name-mutating primitive passes
/// through it, and it runs BOTH authorities:
///
/// * the LOCK-record authority ([`refuse_lock_record`]) unless `sanction`
///   proves the candidate IS the owned record; and
/// * the RESIDUE authority ([`crate::reserved::is_residue_name`]) on EVERY
///   component, except (a) the owned lock record itself and (b) the FINAL
///   component under [`Sanction::FinalResidue`].
///
/// A primitive therefore CANNOT carry the lock authority and skip the residue
/// authority: this function is the only reachable way to run either check
/// (`refuse_lock_record` is private to this module, and the residue loop lives
/// here), and it always runs both. A future mutator that wants a lock-record
/// grant presents [`Sanction::OwnedLockRecord`] and still gets the residue
/// refusal on every spelling that is not the granted record; one that wants the
/// explicit discard presents [`Sanction::FinalResidue`] and still gets the lock
/// refusal and the nested-residue refusal. [`GuardedRel`] is the unforgeable
/// proof that this gate ran.
pub(crate) fn refuse_reserved_mutation(rel: &Path, sanction: Sanction<'_>) -> Result<()> {
    let owned_grant = match sanction {
        Sanction::OwnedLockRecord(owned) => owned.owns(rel),
        _ => false,
    };
    if !owned_grant {
        refuse_lock_record(rel)?;
    }
    if matches!(sanction, Sanction::Residue) {
        return Ok(());
    }
    let last = rel.components().count().checked_sub(1);
    for (index, component) in rel.components().enumerate() {
        let Component::Normal(name) = component else {
            continue;
        };
        if !name.to_str().is_some_and(crate::reserved::is_residue_name) {
            continue;
        }
        if owned_grant {
            continue;
        }
        if matches!(sanction, Sanction::FinalResidue) && Some(index) == last {
            continue;
        }
        return Err(super::residue_refusal(rel));
    }
    Ok(())
}

/// The scope of a mutation the guard authorized: ordinary content, or the ONE
/// lock record the crate's own protocol OWNS ([`crate::transport::Layout::lock`]).
/// Carried by [`GuardedRel`] so a caller can pick the sidecar-serialized route
/// for the owned record without re-deriving the comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GuardScope {
    /// An ordinary path: not any lock record.
    Ordinary,
    /// The ONE lock record the protocol is authorized to break.
    OwnedLockRecord,
}

/// An unforgeable proof that the guard ran on a root-relative path, together
/// with the scope it authorized. The fields are PRIVATE to this module, so the
/// only way to obtain a value is one of the two constructors (each runs the
/// guard) — no other module, including a future mutation primitive, can
/// construct one directly, and the rel-path mutators accept nothing else.
#[derive(Clone, Copy)]
pub(crate) struct GuardedRel<'a> {
    rel: &'a Path,
    scope: GuardScope,
}

impl<'a> GuardedRel<'a> {
    /// THE ordinary-content constructor: run the guard on `rel`, then mint the
    /// proof. Refuses EVERY lock-record spelling. There is deliberately no
    /// unchecked constructor and no `Default`.
    ///
    /// COMPILE-LEVEL ARGUMENT: the fields are private to this module, so a
    /// struct-literal construction (`GuardedRel { rel, scope }`) anywhere else
    /// — including in [`super::unix`] / [`super::windows`] — is `E0451`
    /// ("field is private"), and no `unsafe`/`transmute` route exists in safe
    /// code. This constructor is one of the THREE minting constructors (with
    /// [`Self::new_for_owned_lock_record`] and [`Self::new_for_residue`]), the
    /// only minters of the capability: every one runs
    /// [`refuse_reserved_mutation`], the ONE reserved-spelling authority, so a
    /// value of this type IS proof the guard ran.
    pub(crate) fn new(rel: &'a Path) -> Result<Self> {
        refuse_reserved_mutation(rel, Sanction::None)?;
        Ok(Self {
            rel,
            scope: GuardScope::Ordinary,
        })
    }

    /// The crate's OWN lock-protocol constructor. `owned` is the unforgeable
    /// authority for the ONE record the protocol may break, built from the
    /// transport's own [`crate::transport::Layout`]
    /// ([`OwnedLockRecord::local`] / [`OwnedLockRecord::remote`]) — the
    /// caller cannot pass the candidate path as its own authority, because the
    /// parameter is an [`OwnedLockRecord`], not a `Path`.
    ///
    /// `rel` is recognized as the owned record ONLY when it denotes THE SAME
    /// ON-DISK ENTRY as the owned record — the identical resolved
    /// `(device, inode)` ([`OwnedLockRecord::owns`]) — or, when no entry exists
    /// yet, when its spelling is byte-equal (the record-creation case). The
    /// reserved-name module's Unicode/trailing-dot FOLD is deliberately NOT
    /// used here: a fold is sound for DENIAL ([`refuse_lock_record`]) but
    /// unsound for PERMISSION, because it grants ownership over a spelling that
    /// may be a DISTINCT on-disk entry (a live second holder's record).
    ///
    /// EVERY other lock-record spelling — a bare `operation.lock`, a nested
    /// `snapshots/.001.operation.lock`, an interior component, or an alias that
    /// resolves elsewhere — is refused by the same [`refuse_lock_record`]
    /// authority the ordinary constructor uses. A future `*_if`-style
    /// primitive that reaches for this constructor therefore still cannot smash
    /// a foreign record, and one that reaches for [`Self::new`] cannot smash
    /// any.
    pub(crate) fn new_for_owned_lock_record(
        rel: &'a Path,
        owned: &OwnedLockRecord,
    ) -> Result<Self> {
        let grant = owned.owns(rel);
        refuse_reserved_mutation(rel, Sanction::OwnedLockRecord(owned))?;
        Ok(Self {
            rel,
            scope: if grant {
                GuardScope::OwnedLockRecord
            } else {
                GuardScope::Ordinary
            },
        })
    }

    /// The SANCTIONED residue-movement constructor: run the gate with
    /// [`Sanction::Residue`] (every residue spelling is permitted; the lock
    /// authority is still enforced). Used ONLY by the crate's own rename of a
    /// claim-aside and by `sync::Residue::recover_to`, where the residue
    /// spelling is the point of the operation. The PUBLIC
    /// [`GuardedRel::new`] refuses residue spellings, so a caller that reaches
    /// for the ordinary rename cannot move or replace a strand.
    pub(crate) fn new_for_residue(rel: &'a Path) -> Result<Self> {
        refuse_reserved_mutation(rel, Sanction::Residue)?;
        Ok(Self {
            rel,
            scope: GuardScope::Ordinary,
        })
    }

    /// The guarded root-relative path.
    pub(crate) fn as_path(self) -> &'a Path {
        self.rel
    }

    /// Whether this proof is for the ONE owned lock record (the caller must
    /// serialize the mutation through the sidecar).
    pub(crate) fn is_owned_lock_record(self) -> bool {
        self.scope == GuardScope::OwnedLockRecord
    }
}

#[cfg(test)]
mod tests {
    use super::GuardedRel;
    // `OwnedLockRecord` is exercised only by the identity/fold tests below,
    // which need a real (device, inode) pair: a Unix filesystem property.
    #[cfg(unix)]
    use super::OwnedLockRecord;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    /// The ONE guard refuses every lock-record spelling — the application
    /// record, the sibling record, and their case aliases — in ANY component,
    /// and the capability cannot be minted for any of them. This is the
    /// authority every name-mutating primitive consults.
    #[test]
    fn the_guard_refuses_every_lock_record_spelling_in_any_component() {
        for bad in [
            "operation.lock",
            ".dest.operation.lock",
            "OPERATION.LOCK",
            ".DEST.OPERATION.LOCK",
            "a/operation.lock",
            "a/.dest.operation.lock/b",
            "operation.lock/c",
        ] {
            assert!(
                GuardedRel::new(Path::new(bad)).is_err(),
                "{bad:?} names the lock record and must be refused"
            );
        }
        for ok in [
            ".operation.lock.tmp.1.0",
            "snapshots/001/x",
            "operation.locked",
            "my.operation.lock",
        ] {
            assert!(
                GuardedRel::new(Path::new(ok)).is_ok(),
                "{ok:?} does not name the record and must be accepted"
            );
        }
    }

    /// F1/F2, at the authority: the owned-record grant is decided by IDENTITY,
    /// not by a spelling fold, and the authority is built from the transport's
    /// own `Layout` (a candidate path cannot be its own authority — the guard's
    /// second parameter is an [`OwnedLockRecord`], not a `Path`, so the old
    /// `new_for_owned_lock_record(rel, rel)` does not typecheck).
    #[cfg(unix)]
    #[test]
    fn ownership_is_decided_by_identity_not_by_a_fold() {
        use crate::transport::{Layout, RootedRelativePath};
        use std::os::unix::fs::MetadataExt;

        let dir = crate::test_support::fixture_tmpdir(&crate::test_support::fixture_env()).unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("state")).unwrap();
        let record = root.join("state/operation.lock");
        std::fs::write(&record, b"A").unwrap();
        let layout = Layout {
            lock: RootedRelativePath::parse(Path::new("state/operation.lock")).unwrap(),
            ..Layout::empty()
        };
        let owned = OwnedLockRecord::local(&root, &layout);

        // (a) the byte-exact owned spelling is the owned record.
        let exact =
            GuardedRel::new_for_owned_lock_record(Path::new("state/operation.lock"), &owned)
                .expect("the owned spelling is admitted");
        assert!(
            exact.is_owned_lock_record(),
            "the byte-exact owned spelling must be the OwnedLockRecord scope"
        );

        // (a) a case alias resolving to the SAME inode is the owned record; on a
        // case-SENSITIVE filesystem it is a DISTINCT (absent) entry and refused.
        let case_alias = root.join("state/OPERATION.LOCK");
        match std::fs::metadata(&case_alias) {
            Ok(m) if m.ino() == std::fs::metadata(&record).unwrap().ino() => {
                let g = GuardedRel::new_for_owned_lock_record(
                    Path::new("state/OPERATION.LOCK"),
                    &owned,
                )
                .expect("a same-inode alias is the owned record");
                assert!(
                    g.is_owned_lock_record(),
                    "same-inode alias -> OwnedLockRecord"
                );
            }
            _ => {
                assert!(
                    GuardedRel::new_for_owned_lock_record(
                        Path::new("state/OPERATION.LOCK"),
                        &owned
                    )
                    .is_err(),
                    "a case alias that is a DISTINCT on-disk entry must be refused"
                );
            }
        }

        // (a) a DISTINCT on-disk entry is refused, whatever it folds to.
        std::fs::write(root.join("state/operation.lock."), b"B").unwrap();
        assert!(
            GuardedRel::new_for_owned_lock_record(Path::new("state/operation.lock."), &owned)
                .is_err(),
            "a trailing-dot spelling that is a distinct entry must be refused"
        );
        // (b) a lock-record spelling that does not exist and is not byte-equal is
        // refused (it is not the record being created).
        assert!(
            GuardedRel::new_for_owned_lock_record(
                Path::new("snapshots/.001.operation.lock"),
                &owned
            )
            .is_err(),
            "creating some OTHER lock-record spelling is not the protocol's record"
        );

        // (b) the byte-exact spelling with NO entry yet is the creation case.
        let fresh =
            crate::test_support::fixture_tmpdir(&crate::test_support::fixture_env()).unwrap();
        let owned_fresh = OwnedLockRecord::local(fresh.path(), &layout);
        let creating =
            GuardedRel::new_for_owned_lock_record(Path::new("state/operation.lock"), &owned_fresh)
                .expect("the byte-exact spelling may be created");
        assert!(
            creating.is_owned_lock_record(),
            "the byte-exact spelling is the record being created"
        );

        // A REMOTE authority cannot stat the far side, so ownership is byte-exact
        // ONLY: a case/dot alias is refused even though the local FS folds.
        let remote = OwnedLockRecord::remote(&layout);
        assert!(remote.owns(Path::new("state/operation.lock")));
        assert!(!remote.owns(Path::new("state/operation.lock.")));
        assert!(!remote.owns(Path::new("state/OPERATION.LOCK")));
    }

    /// The name-mutating `libc` functions the audit tracks: BOTH the `*at`
    /// forms and the non-`at` forms the earlier six-symbol scan missed
    /// (`open` with `O_CREAT`/`O_TRUNC`, `rmdir`, `unlink`, `rename`,
    /// `symlink`, `link`, `mkdir`, `remove`).
    const MUTATING_LIBC_SYSCALLS: [&str; 14] = [
        "unlinkat",
        "renameat",
        "symlinkat",
        "linkat",
        "mkdirat",
        "openat",
        "unlink",
        "rename",
        "symlink",
        "link",
        "mkdir",
        "rmdir",
        "open",
        "remove",
    ];

    /// The `std::fs` calls that can REMOVE or REPLACE an existing directory
    /// entry — and therefore change or free a lock record's inode. Truncating
    /// or chmodding operations (`write`, `copy`, `set_permissions`) preserve the
    /// entry's inode, so they cannot split a holder and are deliberately NOT
    /// counted (see the audit's scope note).
    const FS_INODE_MUTATORS: [&str; 5] = [
        "remove_file",
        "remove_dir",
        "remove_dir_all",
        "rename",
        "hard_link",
    ];

    /// A production route by which one of [`FS_INODE_MUTATORS`] can be reached
    /// WITHOUT spelling `std::fs::<symbol>(`, so the exact-count pin alone
    /// cannot see it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum FsRoute {
        /// `use std::fs::remove_file [as x];` (single) or one entry of a
        /// braced `use std::fs::{…}` — the bare or aliased imported symbol.
        Imported,
        /// `use std::fs;` / `use std::fs as f;` / `use std::fs::{self as f};`
        /// followed by `f::remove_file(…)`, or a `std`-crate-root alias
        /// (`use std as s;` / `use ::std as s;` / `use std::{self as s};`)
        /// followed by `s::fs::remove_file(…)`.
        ModuleAlias,
        /// `use std::fs::*;` — the glob itself is the route, since any of the
        /// five symbols (and a function pointer to one) comes into scope.
        Glob,
    }

    impl FsRoute {
        fn as_str(self) -> &'static str {
            match self {
                FsRoute::Imported => "imported symbol",
                FsRoute::ModuleAlias => "module alias",
                FsRoute::Glob => "glob import",
            }
        }
    }

    /// One production use of a `std::fs` inode mutator through a route the
    /// exact-count pin does not cover.
    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    struct Violation {
        file: String,
        symbol: &'static str,
        route: FsRoute,
    }

    /// Every `std::fs` inode mutation in PRODUCTION code that reaches one of
    /// [`FS_INODE_MUTATORS`] through a non-canonical route: a `use`d symbol, a
    /// module alias (including a `std`-crate-ROOT alias such as `use std as s;`
    /// followed by `s::fs::remove_file(…)`), or a glob. This is the half the
    /// exact-count pin misses — `use std::fs::remove_file; remove_file(p)`
    /// spells no `std::fs::` call, so a pin over `std::fs::{symbol}(` stays
    /// green while the record is unlinked. WHITESPACE-INSENSITIVE, because the
    /// code is normalized first.
    ///
    /// PURE over `(package-relative path, raw contents)` pairs so a unit test
    /// can drive it with synthetic sources. Test-only files are skipped: a path
    /// COMPONENT equal to `tests` (the integration/bench/example roots or
    /// `tests.rs`), or a module named in `gated` (see [`is_test_only`]).
    fn std_fs_mutation_violations(
        files: &[(String, String)],
        gated: &BTreeSet<String>,
    ) -> Vec<Violation> {
        let mut out = Vec::new();
        for (rel, raw) in files {
            if is_test_only(rel, gated) {
                continue;
            }
            let code = normalize_ws(&production_only(&code_only(raw)));
            out.extend(std_fs_violations_in_code(rel, &code));
        }
        out.sort();
        out.dedup();
        out
    }

    /// The violation scan for one already-comment/string-stripped,
    /// `#[cfg(test)]`-free, whitespace-normalized source.
    fn std_fs_violations_in_code(rel: &str, code: &str) -> Vec<Violation> {
        // (`local name`, mutator symbol) bindings of `std::fs::<symbol>`, the
        // `std::fs` module aliases, and the `std` crate-ROOT aliases, all
        // resolved TRANSITIVELY; plus whether a glob names the module.
        let mut imported: Vec<(String, &'static str)> = Vec::new();
        let mut module_aliases: Vec<String> = Vec::new();
        let mut root_aliases: Vec<String> = Vec::new();
        let mut glob = false;
        resolve_std_fs_bindings(
            &std_fs_use_items(code),
            &mut imported,
            &mut module_aliases,
            &mut root_aliases,
            &mut glob,
        );

        let mut out = Vec::new();
        // Importing a mutating symbol into production code is itself the route
        // (a call, a function pointer, or a re-export), so flag it even before
        // a call appears. This covers the second alias hop
        // (`use std as s; use s::fs::remove_file;`).
        for (_, symbol) in &imported {
            out.push(Violation {
                file: rel.to_string(),
                symbol,
                route: FsRoute::Imported,
            });
        }
        // `f::remove_file(…)` after `use std::fs as f;` (or a transitive
        // `use s::fs as f;`).
        for alias in &module_aliases {
            for symbol in FS_INODE_MUTATORS {
                if count_path_ref(code, alias, symbol) > 0 {
                    out.push(Violation {
                        file: rel.to_string(),
                        symbol,
                        route: FsRoute::ModuleAlias,
                    });
                }
            }
        }
        // `s::fs::remove_file(…)` after `use std as s;` / `use ::std as s;` /
        // `use std::{self as s};`.
        for alias in &root_aliases {
            for symbol in FS_INODE_MUTATORS {
                if count_root_fs_ref(code, alias, symbol) > 0 {
                    out.push(Violation {
                        file: rel.to_string(),
                        symbol,
                        route: FsRoute::ModuleAlias,
                    });
                }
            }
        }
        if glob {
            // A `use std::fs::*` / `use s::fs::*` / `use std::*` glob is the
            // route itself: any of the five symbols (and a bare function
            // pointer to one) is in scope, so flag the import whether or not a
            // call is visible.
            out.push(Violation {
                file: rel.to_string(),
                symbol: "*",
                route: FsRoute::Glob,
            });
        }
        out.sort();
        out.dedup();
        out
    }

    /// The text of every `use` item (the text between `use` and `;`). Only
    /// `use` ITEMS are returned, so a bare `std::fs::remove_file(…)` call is
    /// never mistaken for an import; [`parse_std_fs_use`] decides which items
    /// reach `std::fs`.
    fn std_fs_use_items(code: &str) -> Vec<String> {
        let bytes = code.as_bytes();
        let mut items = Vec::new();
        let mut i = 0usize;
        while i < bytes.len() {
            if !is_ident_start(bytes[i]) {
                i += 1;
                continue;
            }
            let start = i;
            while i < bytes.len() && (is_ident_start(bytes[i]) || bytes[i] == b'_') {
                i += 1;
            }
            if &code[start..i] == "use" && bytes.get(i).is_some_and(|b| b.is_ascii_whitespace()) {
                let body_start = i;
                let mut j = i;
                while j < bytes.len() && bytes[j] != b';' {
                    j += 1;
                }
                let body = code[body_start..j].trim();
                items.push(body.to_string());
                i = j;
            }
        }
        items
    }

    /// Resolve every `use` item to the `std` path it names. A `use std as s;`,
    /// `use ::std as s;` or `use std::{self as s};` binds the crate ROOT under
    /// `s`, so a later `use s::fs::remove_file;` binds the mutator to its bare
    /// name and `s::fs::remove_file(…)` is a route. Resolution is TRANSITIVE
    /// and iterated to a fixpoint because `use` order is insignificant in Rust:
    /// `use s::fs::…` may legally precede `use std as s;`.
    fn resolve_std_fs_bindings(
        items: &[String],
        imported: &mut Vec<(String, &'static str)>,
        module_aliases: &mut Vec<String>,
        root_aliases: &mut Vec<String>,
        glob: &mut bool,
    ) {
        let mut roots: BTreeSet<String> = BTreeSet::new();
        let mut modules: BTreeSet<String> = BTreeSet::new();
        let mut symbols: BTreeMap<String, &'static str> = BTreeMap::new();
        let mut glob_found = false;

        loop {
            let mut changed = false;
            for item in items {
                let mut leaves = Vec::new();
                expand_use_tree(item, &[], &mut leaves);
                for (raw, local) in leaves {
                    let Some(resolved) = resolve_use_path(&raw, &roots, &modules, &symbols) else {
                        continue;
                    };
                    let local =
                        local.unwrap_or_else(|| resolved.last().cloned().unwrap_or_default());
                    if local.is_empty() {
                        continue;
                    }
                    if path_is(&resolved, &["std"]) {
                        // `std` is the CANONICAL root: `std::fs::symbol(` is the
                        // pin's own spelling, not an import route, so a binding
                        // that keeps the name `std` must not be flagged.
                        if local != "std" {
                            changed |= roots.insert(local);
                        }
                    } else if path_is(&resolved, &["std", "fs"]) {
                        changed |= modules.insert(local);
                    } else if (path_is(&resolved, &["std", "fs", "*"])
                        || path_is(&resolved, &["std", "*"]))
                        && !glob_found
                    {
                        glob_found = true;
                        changed = true;
                    } else if resolved.len() == 3
                        && resolved[0] == "std"
                        && resolved[1] == "fs"
                        && let Some(&symbol) = FS_INODE_MUTATORS.iter().find(|&&s| resolved[2] == s)
                        && !symbols.contains_key(&local)
                    {
                        symbols.insert(local, symbol);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }

        imported.extend(symbols);
        module_aliases.extend(modules);
        root_aliases.extend(roots);
        *glob = glob_found;
    }

    /// Whether `path` is exactly the canonical sequence `expected`.
    fn path_is(path: &[String], expected: &[&str]) -> bool {
        path.len() == expected.len()
            && path
                .iter()
                .zip(expected)
                .all(|(segment, want)| segment.as_str() == *want)
    }

    /// Expand one `use` body into `(raw path segments, local name)` leaves,
    /// folding `self` against the enclosing prefix so `std::{self as s}`
    /// becomes the path `std` bound to `s`. Nested-brace forms are recursed.
    fn expand_use_tree(
        item: &str,
        prefix: &[String],
        out: &mut Vec<(Vec<String>, Option<String>)>,
    ) {
        let item = item.trim();
        if item.is_empty() {
            return;
        }
        if let Some(brace) = find_top_level_brace(item) {
            let head_raw = item[..brace].trim_end_matches(':').trim();
            let absolute = item[..brace].trim_start().starts_with("::");
            let close = matching_brace(item, brace).unwrap_or(item.len());
            let inner = &item[brace + 1..close];
            let mut next_prefix: Vec<String> = if absolute {
                Vec::new()
            } else {
                prefix.to_vec()
            };
            for segment in head_raw.split("::") {
                let segment = segment.trim();
                if !segment.is_empty() {
                    next_prefix.push(segment.to_string());
                }
            }
            for entry in split_top_level_commas(inner) {
                expand_use_tree(entry, &next_prefix, out);
            }
            return;
        }
        let (path_text, local) = split_use_alias(item);
        let path_text = path_text.trim();
        let absolute = path_text.starts_with("::");
        let path_text = path_text.trim_start_matches(':');
        let mut segments: Vec<String> = if absolute {
            Vec::new()
        } else {
            prefix.to_vec()
        };
        for segment in path_text.split("::") {
            let segment = segment.trim();
            if segment.is_empty() || segment == "self" {
                continue;
            }
            segments.push(segment.to_string());
        }
        let local = local
            .map(|alias| alias.trim().to_string())
            .filter(|alias| !alias.is_empty());
        out.push((segments, local));
    }

    /// Split a brace-free use tree into its path and an `as` alias, if any.
    fn split_use_alias(item: &str) -> (&str, Option<&str>) {
        let bytes = item.as_bytes();
        let mut depth = 0i32;
        let mut i = 0usize;
        while i < bytes.len() {
            match bytes[i] {
                b'{' | b'(' | b'[' => depth += 1,
                b'}' | b')' | b']' => depth -= 1,
                b'a' if depth == 0 && item[i..].starts_with("as") => {
                    let before_ok = i == 0 || bytes[i - 1].is_ascii_whitespace();
                    let after = i + 2;
                    let after_ok = after >= bytes.len() || bytes[after].is_ascii_whitespace();
                    if before_ok && after_ok {
                        return (&item[..i], Some(&item[after..]));
                    }
                }
                _ => {}
            }
            i += 1;
        }
        (item, None)
    }

    /// The first `{` at brace depth zero, if the use tree is braced.
    fn find_top_level_brace(text: &str) -> Option<usize> {
        let bytes = text.as_bytes();
        let mut depth = 0i32;
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'{' if depth == 0 => return Some(i),
                b'{' | b'(' | b'[' => depth += 1,
                b'}' | b')' | b']' => depth -= 1,
                _ => {}
            }
        }
        None
    }

    /// The index of the `}` matching the `{` at `open`.
    fn matching_brace(text: &str, open: usize) -> Option<usize> {
        let bytes = text.as_bytes();
        let mut depth = 0i32;
        for (offset, &b) in bytes[open..].iter().enumerate() {
            match b {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(open + offset);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Canonicalize a raw use path against the aliases already known. Only the
    /// HEAD can be an alias, because a `use` path is absolute from its root:
    /// substitution rewrites the head to the canonical path it names (the
    /// `std` crate, the `std::fs` module, or one mutator). `None` means the
    /// path never reaches `std`.
    fn resolve_use_path(
        raw: &[String],
        roots: &BTreeSet<String>,
        modules: &BTreeSet<String>,
        symbols: &BTreeMap<String, &'static str>,
    ) -> Option<Vec<String>> {
        let head = raw.first()?;
        let mut out: Vec<String> = Vec::with_capacity(raw.len() + 2);
        if head == "std" || roots.contains(head) {
            out.push("std".to_string());
        } else if modules.contains(head) {
            out.push("std".to_string());
            out.push("fs".to_string());
        } else {
            let symbol = symbols.get(head)?;
            out.push("std".to_string());
            out.push("fs".to_string());
            out.push((*symbol).to_string());
        }
        out.extend(raw[1..].iter().cloned());
        Some(out)
    }

    /// Split a braced `use` list on TOP-LEVEL commas only, so
    /// `fs::{remove_file, rename}` is two entries, not three.
    fn split_top_level_commas(text: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let mut depth = 0i32;
        let mut start = 0usize;
        for (i, c) in text.char_indices() {
            match c {
                '{' | '(' | '[' => depth += 1,
                '}' | ')' | ']' => depth -= 1,
                ',' if depth == 0 => {
                    out.push(&text[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        out.push(&text[start..]);
        out
    }

    /// Count `qualifier::symbol` references where `qualifier` is a complete
    /// path head (so a module alias `fs` does not re-count `std::fs::symbol`).
    /// WHITESPACE-INSENSITIVE between the segments.
    fn count_path_ref(code: &str, qualifier: &str, symbol: &str) -> usize {
        count_path_segments(code, &[qualifier, symbol], None, true)
    }

    /// Count `root::fs::symbol` references for a `std`-crate-ROOT alias.
    fn count_root_fs_ref(code: &str, root: &str, symbol: &str) -> usize {
        count_path_segments(code, &[root, "fs", symbol], None, true)
    }

    /// Count `std::fs::symbol(` CALLS, whitespace-insensitive between `std`,
    /// `::`, `fs`, `::`, `symbol`, and the opening `(`. This is the matcher the
    /// exact-count pin uses, so `std::fs::rename (a, b)` cannot evade it.
    fn count_std_fs_calls(code: &str, symbol: &str) -> usize {
        count_path_segments(code, &["std", "fs", symbol], Some(b'('), false)
    }

    /// Count occurrences of the token sequence `segments` joined by `::`
    /// (whitespace allowed around each `::`), optionally followed by `trailing`
    /// (whitespace allowed before it). The first segment must be a complete
    /// path HEAD: not preceded by an identifier character and — when
    /// `reject_leading_colon` — not preceded by `:` either, so a module alias
    /// `fs` does not match the `fs` in `std::fs::remove_file`.
    fn count_path_segments(
        code: &str,
        segments: &[&str],
        trailing: Option<u8>,
        reject_leading_colon: bool,
    ) -> usize {
        let bytes = code.as_bytes();
        let mut count = 0usize;
        let mut i = 0usize;
        while i < bytes.len() {
            if !is_ident_start(bytes[i]) {
                i += 1;
                continue;
            }
            let head_ok = i == 0
                || (!is_ident_start(bytes[i - 1])
                    && !(reject_leading_colon && bytes[i - 1] == b':'));
            if !head_ok {
                i += 1;
                continue;
            }
            let mut j = i;
            let mut matched = true;
            for (index, segment) in segments.iter().enumerate() {
                let start = j;
                while j < bytes.len() && is_ident_start(bytes[j]) {
                    j += 1;
                }
                if &code[start..j] != *segment {
                    matched = false;
                    break;
                }
                if index + 1 < segments.len() {
                    let after = skip_ws(bytes, j);
                    if !(bytes.get(after) == Some(&b':') && bytes.get(after + 1) == Some(&b':')) {
                        matched = false;
                        break;
                    }
                    j = skip_ws(bytes, after + 2);
                }
            }
            if matched && bytes.get(j).is_some_and(|&b| is_ident_start(b)) {
                matched = false;
            }
            if matched && let Some(last) = trailing {
                let after = skip_ws(bytes, j);
                if bytes.get(after) != Some(&last) {
                    matched = false;
                }
            }
            if matched {
                count += 1;
            }
            i += 1;
        }
        count
    }

    fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        i
    }

    /// Walk the WHOLE crate directory (not only `src/`), skipping `target/` and
    /// hidden directories. Covers `build.rs`, `tests/**`, `benches/**`,
    /// `examples/**`, and any `#[path]`-included file that lives inside the
    /// package tree.
    fn collect_crate_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read crate dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == "target" || name.starts_with('.') {
                    continue;
                }
                collect_crate_rs_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    /// The package-relative, `/`-separated path of a scanned file.
    fn crate_relative(file: &Path) -> String {
        file.strip_prefix(Path::new(env!("CARGO_MANIFEST_DIR")))
            .expect("strip manifest prefix")
            .to_string_lossy()
            .replace('\\', "/")
    }

    /// Test-only files are outside the production funnel. A path COMPONENT
    /// equal to `tests` (the integration/bench/example roots, `tests.rs`, or a
    /// `tests/` directory) decides from POSITION; a unit-test-only module is
    /// decided from its GATING, never its NAME: `gated` is the set of
    /// package-relative module paths whose `mod <name>;` declaration carries a
    /// `#[cfg(...)]` that IMPLIES `test` (see [`test_only_gated_paths`]).
    ///
    /// NAME is not a predicate. A suffix rule (`ends_with("regression.rs")`)
    /// is strictly BROADER than the gated set: it exempts every file of that
    /// shape, including a production `src/review_regression.rs` compiled into
    /// the lib, which could then issue `libc::unlinkat` with the audit green.
    /// Deriving exemption from the GATING has exactly the gated set as its
    /// extension — exempting a new production file requires declaring it under
    /// `#[cfg(test)]`, which is the property being checked.
    fn is_test_only(rel: &str, gated: &BTreeSet<String>) -> bool {
        let first = rel.split('/').next().unwrap_or("");
        if matches!(first, "tests" | "benches" | "examples") {
            return true;
        }
        rel.split('/').any(|c| c == "tests" || c == "tests.rs") || gated.contains(rel)
    }

    /// Every `mod <name>;` declaration in one source file, with whether the
    /// attribute run before it has a `#[cfg(...)]` that IMPLIES `test`, in
    /// source order. A `#[cfg(test)]` is held across visibility (`pub(crate) `)
    /// and cleared by the `;` / `}` that ends any OTHER item, so it never leaks
    /// onto a later plain `mod`.
    fn mod_declarations(source: &str) -> Vec<(String, bool)> {
        let code = code_only(source);
        let bytes = code.as_bytes();
        let mut declarations = Vec::new();
        let mut pending_cfg_test = false;
        let mut depth = 0i32;
        let mut i = 0usize;
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'#' && bytes.get(i + 1) == Some(&b'[') {
                let (body, end) = attribute_body(&code, i);
                if let Some(cfg) = cfg_predicate(&body)
                    && cfg_implies_test(cfg)
                {
                    pending_cfg_test = true;
                }
                i = end;
                continue;
            }
            match b {
                b'{' | b'(' | b'[' => {
                    depth += 1;
                    i += 1;
                }
                b'}' | b')' | b']' => {
                    depth -= 1;
                    if b == b'}' && depth <= 0 {
                        pending_cfg_test = false;
                    }
                    i += 1;
                }
                b';' => {
                    if depth <= 0 {
                        pending_cfg_test = false;
                    }
                    i += 1;
                }
                _ if is_ident_start(b) => {
                    let start = i;
                    let mut j = i;
                    while j < bytes.len() && is_ident_start(bytes[j]) {
                        j += 1;
                    }
                    if &code[start..j] == "mod" {
                        let name_start = skip_ws(bytes, j);
                        let mut name_end = name_start;
                        while name_end < bytes.len() && is_ident_start(bytes[name_end]) {
                            name_end += 1;
                        }
                        let name = &code[name_start..name_end];
                        let terminator = skip_ws(bytes, name_end);
                        if !name.is_empty() && bytes.get(terminator) == Some(&b';') {
                            declarations.push((name.to_string(), pending_cfg_test));
                        }
                    }
                    i = j;
                }
                _ => i += 1,
            }
        }
        declarations
    }

    /// The body of the attribute group starting at `#[` (the text between the
    /// brackets) and the index just past its closing `]`.
    fn attribute_body(code: &str, start: usize) -> (String, usize) {
        let bytes = code.as_bytes();
        let begin = (start + 2).min(bytes.len());
        let mut i = begin;
        let mut depth = 1i32;
        while i < bytes.len() {
            match bytes[i] {
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        let end = i.min(bytes.len());
        (code[begin..end].to_string(), (end + 1).min(bytes.len()))
    }

    /// The predicate inside a `cfg(...)` attribute body, if the body is one.
    fn cfg_predicate(body: &str) -> Option<&str> {
        let rest = body.trim().strip_prefix("cfg")?;
        let rest = rest.trim_start().strip_prefix('(')?;
        rest.strip_suffix(')')
    }

    /// Whether a `cfg(...)` predicate is satisfiable ONLY together with `test`.
    /// `test` itself and `all(<… test …>)` qualify; `any(...)` qualifies only
    /// when EVERY disjunct does; `not(...)`, a bare `unix`/`windows`, and a
    /// `feature = "…"` do not. A conservative `false` only makes the audit scan
    /// MORE files, never fewer, so it cannot exempt production code.
    fn cfg_implies_test(cfg: &str) -> bool {
        let cfg = cfg.trim();
        if cfg == "test" {
            return true;
        }
        for (name, all_of_them) in [("all", true), ("any", false)] {
            let Some(inner) = cfg
                .strip_prefix(name)
                .map(str::trim_start)
                .and_then(|rest| rest.strip_prefix('('))
                .and_then(|rest| rest.strip_suffix(')'))
            else {
                continue;
            };
            let args: Vec<&str> = split_top_level_commas(inner)
                .into_iter()
                .map(str::trim)
                .filter(|arg| !arg.is_empty())
                .collect();
            if args.is_empty() {
                return false;
            }
            return if all_of_them {
                args.iter().any(|arg| cfg_implies_test(arg))
            } else {
                args.iter().all(|arg| cfg_implies_test(arg))
            };
        }
        false
    }

    /// The directory (with a trailing `/`) in which a source file's CHILD
    /// module files live: `foo.rs` uses `foo/`, while `mod.rs` / `lib.rs` /
    /// `main.rs` use their own directory.
    fn module_child_dir(rel: &str) -> String {
        let (dir, name) = match rel.rfind('/') {
            Some(pos) => (&rel[..=pos], &rel[pos + 1..]),
            None => ("", rel),
        };
        match name {
            "mod.rs" | "lib.rs" | "main.rs" => dir.to_string(),
            _ => match name.strip_suffix(".rs") {
                Some(stem) => format!("{dir}{stem}/"),
                None => dir.to_string(),
            },
        }
    }

    /// Every package-relative module path in the crate that is gated under a
    /// `#[cfg(...)]` implying `test`, walked TRANSITIVELY from `src/lib.rs` (so
    /// a gated `mod` declared in a submodule — `transport/mod.rs`'s `scripted`
    /// — is found too). This set is the production/test boundary the audits
    /// use; it comes from the source's GATING, never from a file NAME.
    fn test_only_gated_paths() -> BTreeSet<String> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut gated = BTreeSet::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        seen.insert("src/lib.rs".to_string());
        let mut queue = vec![root.join("src/lib.rs")];
        while let Some(file) = queue.pop() {
            let Ok(source) = std::fs::read_to_string(&file) else {
                continue;
            };
            let rel = crate_relative(&file);
            let dir = module_child_dir(&rel);
            // Walk EVERY declared child module (the walk is how a gated `mod`
            // nested in a submodule is reached); only the `test`-gated ones are
            // exempt, and only they are inserted into `gated`.
            for (name, cfg_test) in mod_declarations(&source) {
                let base = format!("{dir}{name}");
                for candidate in [format!("{base}.rs"), format!("{base}/mod.rs")] {
                    let path = root.join(&candidate);
                    if !path.exists() {
                        continue;
                    }
                    if cfg_test {
                        gated.insert(candidate.clone());
                    }
                    if seen.insert(candidate.clone()) {
                        queue.push(path);
                    }
                }
            }
        }
        gated
    }

    fn is_ident_start(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'_'
    }

    /// Strip comments AND string/char-literal CONTENTS, leaving only code
    /// positions. F7: a `#[doc = "libc::open("]` or a `const` holding that text
    /// must not trip a text audit, and a doc comment that merely MENTIONS a
    /// symbol must not either. Raw strings (`r"…"`, `r#"…"#`) are handled.
    fn code_only(text: &str) -> String {
        let bytes = text.as_bytes();
        let mut out = String::with_capacity(text.len());
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                let mut depth = 0usize;
                while i < bytes.len() {
                    if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                        depth += 1;
                        i += 2;
                    } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                out.push(' ');
                continue;
            }
            // Raw string: `r"…"` or `r#…"…"#` (and the byte/raw-byte variants,
            // whose leading ident is copied by the default arm).
            if bytes[i] == b'r'
                && (bytes.get(i + 1) == Some(&b'"') || bytes.get(i + 1) == Some(&b'#'))
            {
                let mut j = i + 1;
                let mut hashes = 0usize;
                while bytes.get(j) == Some(&b'#') {
                    hashes += 1;
                    j += 1;
                }
                if bytes.get(j) == Some(&b'"') {
                    j += 1;
                    loop {
                        if j >= bytes.len() {
                            break;
                        }
                        if bytes[j] == b'"' {
                            let mut k = j + 1;
                            let mut h = 0usize;
                            while bytes.get(k) == Some(&b'#') && h < hashes {
                                h += 1;
                                k += 1;
                            }
                            if h == hashes {
                                j = k;
                                break;
                            }
                        }
                        j += 1;
                    }
                    out.push_str("r\"\"");
                    i = j;
                    continue;
                }
            }
            if bytes[i] == b'"' {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b'"' {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                out.push_str("\"\"");
                continue;
            }
            if bytes[i] == b'\'' {
                if bytes.get(i + 1) == Some(&b'\\') {
                    let mut j = i + 2;
                    while j < bytes.len() && bytes[j] != b'\'' {
                        j += 1;
                    }
                    i = (j + 1).min(bytes.len());
                    out.push_str("''");
                    continue;
                }
                if let (Some(&c1), Some(&c2)) = (bytes.get(i + 1), bytes.get(i + 2))
                    && c2 == b'\''
                    && c1 != b'\\'
                {
                    i += 3;
                    out.push_str("''");
                    continue;
                }
            }
            let ch = text[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    /// Remove every `#[cfg(test)]`-gated item/statement from already
    /// comment/string-stripped code, leaving PRODUCTION code. F4: counting test
    /// occurrences as production is what made six pin entries advertise a
    /// production call that did not exist. The skip is brace/paren/bracket
    /// balanced, so an `#[cfg(test)]` on a statement or a parameter is removed
    /// with its expression and a `#[cfg(test)] mod tests { … }` with its whole
    /// body. (Run AFTER `code_only`, so a doc comment that merely mentions
    /// `#[cfg(test)]` is already gone.)
    fn production_only(code: &str) -> String {
        let bytes = code.as_bytes();
        let mut out = String::with_capacity(code.len());
        let mut i = 0usize;
        while i < bytes.len() {
            if code[i..].starts_with("#[cfg(test)]") {
                let mut j = i + "#[cfg(test)]".len();
                // Skip any further attributes gated by this one.
                loop {
                    let before_ws = j;
                    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if bytes.get(j) == Some(&b'#') && bytes.get(j + 1) == Some(&b'[') {
                        let mut depth = 0i32;
                        while j < bytes.len() {
                            match bytes[j] {
                                b'[' => depth += 1,
                                b']' => {
                                    depth -= 1;
                                    j += 1;
                                    if depth == 0 {
                                        break;
                                    }
                                    continue;
                                }
                                _ => {}
                            }
                            j += 1;
                        }
                    } else {
                        j = before_ws;
                        break;
                    }
                }
                // Skip the gated item/statement: a balanced `{…}` block, or up
                // to a `;` / `,` at depth 0 for a use/expression form. Only a
                // CLOSING BRACE returns the item to depth 0 (a `)` of a
                // signature or a `)` of a call must not end the skip), and an
                // `else` continuation keeps an `if … else …` statement
                // together.
                let mut depth = 0i32;
                loop {
                    if j >= bytes.len() {
                        break;
                    }
                    match bytes[j] {
                        b'{' | b'(' | b'[' => {
                            depth += 1;
                            j += 1;
                        }
                        b')' | b']' => {
                            depth -= 1;
                            j += 1;
                        }
                        b'}' => {
                            depth -= 1;
                            j += 1;
                            if depth <= 0 {
                                let mut k = j;
                                while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                                    k += 1;
                                }
                                if code[k..].starts_with("else") {
                                    j = k;
                                    continue;
                                }
                                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                                    j += 1;
                                }
                                if bytes.get(j) == Some(&b';') {
                                    j += 1;
                                }
                                break;
                            }
                        }
                        b';' | b',' if depth == 0 => {
                            j += 1;
                            break;
                        }
                        _ => j += 1,
                    }
                }
                i = j;
                continue;
            }
            let ch = code[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    /// Collapse every whitespace run (newlines included) to one space, so a
    /// rule cannot be evaded by breaking `libc::unlinkat` and `(` across lines.
    fn normalize_ws(code: &str) -> String {
        code.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Every reference to `libc` in code form, as a map from the reference
    /// spelling to its count: `libc::<symbol>` for a path, `libc::*` for a glob,
    /// and bare `libc` for a module alias / re-export (`use libc as c;`,
    /// `use libc::{self as c};`). WHITESPACE-INSENSITIVE, so `libc :: unlinkat`
    /// and a newline before `(` are both seen. This is ONE rule instead of a
    /// list of mutating-symbol patterns, so a module alias or a cross-file
    /// re-export cannot slip past it.
    fn libc_references(code: &str) -> BTreeMap<String, usize> {
        let bytes = code.as_bytes();
        let mut map: BTreeMap<String, usize> = BTreeMap::new();
        let mut i = 0usize;
        while i < bytes.len() {
            if !is_ident_start(bytes[i]) {
                i += 1;
                continue;
            }
            let start = i;
            while i < bytes.len() && (is_ident_start(bytes[i]) || bytes[i] == b'_') {
                i += 1;
            }
            if &code[start..i] != "libc" {
                continue;
            }
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if bytes.get(j) == Some(&b':') && bytes.get(j + 1) == Some(&b':') {
                j += 2;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if bytes.get(j) == Some(&b'*') {
                    *map.entry("libc::*".to_string()).or_default() += 1;
                } else if j < bytes.len() && is_ident_start(bytes[j]) {
                    let s = j;
                    while j < bytes.len() && (is_ident_start(bytes[j]) || bytes[j] == b'_') {
                        j += 1;
                    }
                    *map.entry(format!("libc::{}", &code[s..j])).or_default() += 1;
                } else {
                    *map.entry("libc".to_string()).or_default() += 1;
                }
            } else {
                *map.entry("libc".to_string()).or_default() += 1;
            }
        }
        map
    }

    /// F6 regression: a production file whose NAME merely contains `tests` is
    /// NOT test-only; only a path COMPONENT that is exactly `tests` (or the
    /// integration/bench/example roots) is.
    #[test]
    fn is_test_only_is_a_component_check_not_a_substring() {
        let gated = test_only_gated_paths();
        assert!(!is_test_only("src/latests/evil.rs", &gated));
        assert!(!is_test_only("src/latest.rs", &gated));
        assert!(is_test_only("tests/push_atomicity.rs", &gated));
        assert!(is_test_only("src/transport/tests/foo.rs", &gated));
        assert!(is_test_only("src/sync/apply/tests.rs", &gated));
        assert!(is_test_only("src/deep_tree_regression.rs", &gated));
        assert!(is_test_only("src/test_support.rs", &gated));
        assert!(is_test_only("benches/bench.rs", &gated));
        assert!(is_test_only("examples/demo.rs", &gated));
    }

    /// F7 regression: string-literal and comment contents are removed before an
    /// audit matches, so a `#[doc = "libc::open("]` or a `const` holding the
    /// text does not fail the gate, while a real call still does.
    #[test]
    fn the_audit_ignores_symbol_mentions_in_strings_and_comments() {
        let benign = "#[doc = \"libc::open(\"]\nconst T: &str = \"libc::unlinkat(\";\n// libc::rmdir(x)\nlet r = r#\"libc::rename(\"#;\n";
        let code = normalize_ws(&code_only(benign));
        assert!(
            libc_references(&code).is_empty(),
            "a symbol MENTION inside a string/comment/raw string is not a reference: {:?}",
            libc_references(&code)
        );
        let real = "unsafe { libc::open(p, 0) };\n";
        let refs = libc_references(&normalize_ws(&code_only(real)));
        assert_eq!(refs.get("libc::open").copied(), Some(1));
    }

    /// F3 regression: the ONE libc reference scanner sees every route the
    /// pattern-based scan missed — a module alias, a braced self-alias, a
    /// cross-file re-export, a glob, and a newline between the path and `(`.
    #[test]
    fn the_libc_reference_scanner_sees_every_alias_route() {
        for (label, text, expected) in [
            (
                "module alias",
                "use libc as c; c::unlinkat(0, 0, 0);",
                "libc",
            ),
            (
                "braced self alias",
                "use libc::{self as c}; c::unlinkat(0, 0, 0);",
                "libc",
            ),
            (
                "cross-file re-export",
                "pub use libc::unlinkat;",
                "libc::unlinkat",
            ),
            ("glob", "use libc::*;", "libc::*"),
            (
                "newline before paren",
                "libc::unlinkat\n\t(0, 0, 0);",
                "libc::unlinkat",
            ),
            (
                "spaced path",
                "libc :: unlinkat (0, 0, 0);",
                "libc::unlinkat",
            ),
        ] {
            let refs = libc_references(&normalize_ws(&code_only(text)));
            assert!(
                refs.contains_key(expected),
                "{label}: expected {expected:?} in {refs:?}"
            );
        }
    }

    /// STRUCTURAL AUDIT (libc): outside the ONE funnel module
    /// (`src/atomic/unix.rs`), the crate's `libc` surface is CLOSED to a pinned
    /// review list. ANY reference to `libc` — a mutating symbol, a module alias
    /// (`use libc as c;`), a re-export, a glob, or a newline-separated call —
    /// that is not on that list fails, and so does a mutation of the pinned
    /// counts. This replaces the old mutating-symbol-only pattern scan, which
    /// missed a module alias (`c::unlinkat`) that genuinely split a live holder.
    ///
    /// WHAT THIS FORBIDS, exactly: outside `src/atomic/unix.rs` and test-only
    /// code, the set of `libc` references is closed — adding one, renaming one,
    /// or removing one changes the pinned map and fails here. The pinned
    /// entries are the crate's AUDITED, non-mutating uses (e.g. `libc::flock`,
    /// `libc::fstatat`, `libc::O_RDONLY`); none is in
    /// [`MUTATING_LIBC_SYSCALLS`], and that is asserted independently, so even a
    /// reviewed pin cannot authorize a name-mutating call. The funnel's own
    /// mutating-symbol counts are pinned as before.
    #[test]
    fn no_libc_reference_outside_the_funnel() {
        const FUNNEL: &str = "src/atomic/unix.rs";
        let mut files = Vec::new();
        collect_crate_rs_files(Path::new(env!("CARGO_MANIFEST_DIR")), &mut files);
        assert!(files.len() > 10, "the audit must see the real source tree");
        let gated = test_only_gated_paths();

        let mut funnel_counts: BTreeMap<&str, usize> = BTreeMap::new();
        let mut outside: BTreeMap<(String, String), usize> = BTreeMap::new();
        for file in &files {
            let rel = crate_relative(file);
            let raw = std::fs::read_to_string(file).expect("read source file");
            // Order matters: strip comments/strings FIRST, then remove
            // `#[cfg(test)]` items, so a doc comment mentioning `#[cfg(test)]`
            // is already gone. F5: `src/atomic/guard.rs` is scanned like any
            // other production file (its `#[cfg(test)]` audit code is removed
            // by `production_only`).
            let code = normalize_ws(&production_only(&code_only(&raw)));
            let refs = libc_references(&code);
            if rel == FUNNEL {
                for symbol in MUTATING_LIBC_SYSCALLS {
                    let count = refs.get(&format!("libc::{symbol}")).copied().unwrap_or(0);
                    if count > 0 {
                        funnel_counts.insert(symbol, count);
                    }
                }
                continue;
            }
            if is_test_only(&rel, &gated) {
                continue;
            }
            for (reference, count) in refs {
                let symbol = reference.strip_prefix("libc::").unwrap_or("");
                assert!(
                    !MUTATING_LIBC_SYSCALLS.contains(&symbol)
                        && reference != "libc::*"
                        && reference != "libc",
                    "{rel} references the mutating/aliased libc facility {reference:?}: a \
                     name-mutating syscall may be issued only from the guarded funnel ({FUNNEL}); \
                     a `use libc … as alias` or a re-export does not exempt it"
                );
                *outside.entry((rel.clone(), reference)).or_default() += count;
            }
        }

        // The pinned production `libc` surface outside the funnel: the
        // crate's AUDITED, non-mutating uses. Any difference in this map is a
        // new (or removed) reference to `libc` outside the funnel and fails
        // here. None of these is a name-mutating symbol (asserted above).
        let expected: &[(&str, &str, usize)] = &[
            ("src/atomic/mod.rs", "libc::O_CLOEXEC", 1),
            ("src/atomic/mod.rs", "libc::O_DIRECTORY", 1),
            ("src/atomic/mod.rs", "libc::O_NOFOLLOW", 1),
            ("src/lock/unix.rs", "libc::EAGAIN", 1),
            ("src/lock/unix.rs", "libc::ELOOP", 1),
            ("src/lock/unix.rs", "libc::EWOULDBLOCK", 2),
            ("src/lock/unix.rs", "libc::LOCK_EX", 1),
            ("src/lock/unix.rs", "libc::LOCK_NB", 1),
            ("src/lock/unix.rs", "libc::LOCK_UN", 1),
            ("src/lock/unix.rs", "libc::O_CLOEXEC", 1),
            ("src/lock/unix.rs", "libc::O_NOFOLLOW", 1),
            ("src/lock/unix.rs", "libc::flock", 2),
            ("src/sync/apply.rs", "libc::O_DIRECTORY", 2),
            ("src/sync/apply.rs", "libc::O_RDONLY", 4),
            ("src/transport/mod.rs", "libc::AT_SYMLINK_NOFOLLOW", 1),
            ("src/transport/mod.rs", "libc::EINTR", 1),
            ("src/transport/mod.rs", "libc::EISDIR", 1),
            ("src/transport/mod.rs", "libc::ELOOP", 1),
            ("src/transport/mod.rs", "libc::ENOENT", 1),
            ("src/transport/mod.rs", "libc::ENOTDIR", 1),
            ("src/transport/mod.rs", "libc::O_CLOEXEC", 2),
            ("src/transport/mod.rs", "libc::O_DIRECTORY", 2),
            ("src/transport/mod.rs", "libc::O_NOFOLLOW", 4),
            ("src/transport/mod.rs", "libc::O_NONBLOCK", 1),
            ("src/transport/mod.rs", "libc::O_RDONLY", 7),
            ("src/transport/mod.rs", "libc::S_IFDIR", 1),
            ("src/transport/mod.rs", "libc::S_IFLNK", 1),
            ("src/transport/mod.rs", "libc::S_IFMT", 1),
            ("src/transport/mod.rs", "libc::S_IFREG", 1),
            ("src/transport/mod.rs", "libc::fstatat", 1),
            ("src/transport/mod.rs", "libc::stat", 1),
            ("src/transport/runner/mod.rs", "libc::CTL_KERN", 1),
            ("src/transport/runner/mod.rs", "libc::KERN_PROC", 1),
            ("src/transport/runner/mod.rs", "libc::KERN_PROC_PID", 1),
            ("src/transport/runner/mod.rs", "libc::SIGKILL", 1),
            ("src/transport/runner/mod.rs", "libc::sysctl", 1),
            ("src/transport/runner/unix.rs", "libc::ECHILD", 1),
            ("src/transport/runner/unix.rs", "libc::F_GETFL", 1),
            ("src/transport/runner/unix.rs", "libc::F_SETFL", 1),
            ("src/transport/runner/unix.rs", "libc::O_NONBLOCK", 1),
            ("src/transport/runner/unix.rs", "libc::POLLIN", 2),
            ("src/transport/runner/unix.rs", "libc::P_PID", 1),
            ("src/transport/runner/unix.rs", "libc::SIGKILL", 2),
            ("src/transport/runner/unix.rs", "libc::SIGTERM", 2),
            ("src/transport/runner/unix.rs", "libc::WEXITED", 1),
            ("src/transport/runner/unix.rs", "libc::WNOHANG", 1),
            ("src/transport/runner/unix.rs", "libc::WNOWAIT", 1),
            ("src/transport/runner/unix.rs", "libc::fcntl", 2),
            ("src/transport/runner/unix.rs", "libc::killpg", 1),
            ("src/transport/runner/unix.rs", "libc::pid_t", 2),
            ("src/transport/runner/unix.rs", "libc::poll", 2),
            ("src/transport/runner/unix.rs", "libc::pollfd", 2),
            ("src/transport/runner/unix.rs", "libc::siginfo_t", 3),
            ("src/transport/runner/unix.rs", "libc::waitid", 1),
            ("src/transport/ssh/mod.rs", "libc::sockaddr_un", 2),
            ("src/transport/ssh/runner/unix.rs", "libc::ESRCH", 2),
            ("src/transport/ssh/runner/unix.rs", "libc::SIGKILL", 1),
            ("src/transport/ssh/runner/unix.rs", "libc::SIGTERM", 1),
        ];
        let expected: BTreeMap<(String, String), usize> = expected
            .iter()
            .map(|(file, reference, count)| {
                (((*file).to_string(), (*reference).to_string()), *count)
            })
            .collect();
        assert_eq!(
            outside, expected,
            "the `libc` references outside the funnel changed: a new reference — a mutating \
             symbol, a module alias, a re-export, or a glob — must be moved behind the funnel \
             (src/atomic/unix.rs); a NON-mutating one must be reviewed and pinned here"
        );

        for (symbol, expected) in [
            // THREE `unlinkat` references since R6: `unlinkat_fd_io` (guarded),
            // the `unlinkat(AT_REMOVEDIR)` rmdir wrapper, and
            // `unlinkat_fd_owned` (the capability-gated retirement of a lock
            // record the caller's own authority owns, reachable only through
            // `remove_owned_lock_record_fd`). The third is the ONE deliberate
            // bypass of the guard, reviewed and pinned here.
            ("unlinkat", 3usize),
            ("renameat", 1),
            ("symlinkat", 1),
            ("linkat", 1),
            ("mkdirat", 3),
            // FIVE until the copy's identity-based overlap refusal added TWO
            // read-only component opens (`O_RDONLY | O_DIRECTORY | O_NOFOLLOW`):
            // one in `open_destination_anchor` (the deepest existing directory
            // on `dst_rel`, resolved from the owned root descriptor) and one in
            // `dir_chain_contains` (the `..` ancestry step). Neither can CREATE,
            // REPLACE, or TRUNCATE an entry, so neither needs the lock-record
            // guard; both are reviewed and pinned here.
            ("openat", 7),
            ("unlink", 0),
            ("rename", 0),
            ("symlink", 0),
            ("link", 0),
            ("mkdir", 0),
            // CHANGED DELIBERATELY (constraint #1): the only `libc::rmdir`
            // reference was the path-based `remove_dir_all_path`, now
            // `#[cfg(test)]` (no production caller). No production rmdir
            // remains; a re-added one must be reviewed and pinned here.
            ("rmdir", 0),
            // CHANGED DELIBERATELY (constraint #1): the only `libc::open` path
            // reference was the path-based `remove_dir_all_path`, now
            // `#[cfg(test)]`. Production opens all go through the guarded
            // `openat` funnel instead.
            ("open", 0),
            ("remove", 0),
        ] {
            assert_eq!(
                funnel_counts.get(symbol).copied().unwrap_or(0),
                expected,
                "the guarded funnel's libc::{symbol} reference count changed: a new raw syscall in \
                 src/atomic/unix.rs must be reviewed for the lock-record guard"
            );
        }
    }

    /// STRUCTURAL AUDIT (`std::fs`): the `std::fs` calls that can REMOVE or
    /// REPLACE a directory entry — the ones that can free or swap a lock
    /// record's inode — are pinned PER PRODUCTION FILE and PER SYMBOL. F4: the
    /// count is taken over PRODUCTION code only (every `#[cfg(test)]` item is
    /// removed first), so a unit-test module inside a production file no longer
    /// contributes — the old pin advertised production calls that had zero
    /// production occurrences and would not have moved if a production call were
    /// replaced by a test one. Import/alias routes are refused by
    /// [`std_fs_mutation_violations`], called below.
    ///
    /// SCOPE, stated exactly: the pin covers these five inode-mutating calls
    /// spelled `std::fs::<symbol>(` in the production code of every `.rs` file
    /// under the package directory except test-only files (a path COMPONENT
    /// equal to `tests`, or a module whose `mod` declaration is `#[cfg(...)]`-
    /// gated on `test`). The count matcher is TOKENISED, so `std::fs::rename (…)`
    /// is counted too. The same audit additionally refuses every IMPORT route to
    /// the same five symbols — a braced or single `use std::fs::{…}`, a
    /// `use std::fs as <alias>` / `use std::fs;` module alias, a `use std::fs::*`
    /// glob, the nested `use std::{fs::…}`, the bare or aliased imported symbol,
    /// and the `std`-crate-ROOT alias (`use std as s;` / `use ::std as s;` /
    /// `use std::{self as s};`) resolved TRANSITIVELY (so `s::fs::remove_file(…)`
    /// and `use s::fs::remove_file;` are both refused) — because those spell no
    /// `std::fs::<symbol>(` call and would otherwise leave the pin green while
    /// the record was unlinked. Neither half covers a MACRO that expands to one
    /// of these calls, an `include!`d file OUTSIDE the package, or a call made
    /// through a function pointer / `dyn` dispatch. Inode-PRESERVING mutations
    /// (`std::fs::write`, `std::fs::copy`, `std::fs::set_permissions`,
    /// `std::fs::create_dir*`) are not pinned: they cannot split a holder
    /// because the flock stays on the unchanged inode.
    ///
    /// RESIDUE, named not hidden: this audit matches SOURCE TEXT, so it has a
    /// residue — any mutation whose source never spells a `std::fs` path or
    /// import. The sharpest instance is a raw FFI declaration,
    /// `extern "C" { fn unlinkat(dirfd: i32, path: *const i8, flags: i32) ->
    /// i32; }`, followed by a call: it names neither `libc` nor `std::fs`, so
    /// NEITHER this audit nor `no_libc_reference_outside_the_funnel` sees it.
    /// Its reach is one declaration in the crate's own source (a dependency's
    /// or a foreign process's mutation is outside the crate entirely). This
    /// audit closes the ordinary route — a `use` alias or a fully-qualified
    /// call — and NAMES the exotic residue rather than implying totality.
    #[test]
    fn std_fs_name_mutation_counts_are_pinned() {
        let mut paths = Vec::new();
        collect_crate_rs_files(Path::new(env!("CARGO_MANIFEST_DIR")), &mut paths);
        let files: Vec<(String, String)> = paths
            .iter()
            .map(|file| {
                (
                    crate_relative(file),
                    std::fs::read_to_string(file).expect("read source file"),
                )
            })
            .collect();
        let gated = test_only_gated_paths();

        // The import/alias routes the exact-count pin below cannot see. This is
        // the enforcement half: `use std::fs::remove_file; remove_file(p)`
        // spells no `std::fs::{symbol}(` call, so it must be caught HERE, or
        // the record can be unlinked with every pin still green.
        let violations = std_fs_mutation_violations(&files, &gated);
        assert!(
            violations.is_empty(),
            "production code reaches a `std::fs` inode mutator through a route the pin does not \
             cover — a `use`d symbol, a module alias, or a glob — so the lock record could be \
             removed or replaced with the count pin still green; route it through the guarded \
             funnel (src/atomic/unix.rs) instead: {violations:?}"
        );

        let mut observed: BTreeMap<(String, &str), usize> = BTreeMap::new();
        for (rel, raw) in &files {
            if is_test_only(rel, &gated) {
                continue;
            }
            let code = normalize_ws(&production_only(&code_only(raw)));
            for symbol in FS_INODE_MUTATORS {
                let count = count_std_fs_calls(&code, symbol);
                if count > 0 {
                    observed.insert((rel.clone(), symbol), count);
                }
            }
        }
        let expected: &[(&str, &str, usize)] = &[
            ("src/atomic/mod.rs", "remove_file", 1),
            // CHANGED DELIBERATELY (constraint #1): the path-based
            // `remove_dir_all_path` became `#[cfg(test)]` (it has no
            // production caller; the fd-confined `remove_dir_all_fd` is the
            // production authority), so its `std::fs::remove_file` left the
            // PRODUCTION count.
            //
            // The `rename` below is the path-based `write_atomic_replace`'s
            // commit-point-1 rename. It was briefly dropped from this pin when
            // that replace was demoted to `#[cfg(test)]`/`pub(crate)`; the
            // demotion was REVERTED because a CONSUMER's interface requires
            // the name (deploy's Windows port calls the path-based replace —
            // see docs/CONSISTENCY.md axis M), so the rename is production code
            // again and is pinned here. The guard still runs on the function.
            ("src/atomic/unix.rs", "rename", 1),
            ("src/atomic/windows.rs", "remove_dir", 2),
            // TWO production `remove_dir_all` calls since R2: the implicit
            // recursive removal (`remove_dir_all_fd`, refused for residue) and
            // the EXPLICIT discard (`remove_residue_dir_all_fd`), whose caller
            // has already decided the strand is disposable. Both run the lock
            // authority on the whole path/tree before the call, so neither can
            // name the record; the count is raised deliberately, not silently.
            ("src/atomic/windows.rs", "remove_dir_all", 2),
            // SEVEN production `remove_file` calls: the pre-A1 five plus the two
            // sanctioned claim-aside walk primitives (`remove_claim_file_fd`
            // and the discard `remove_residue_file_fd`), whose paths include
            // the claim root and therefore permit residues.
            ("src/atomic/windows.rs", "remove_file", 7),
            ("src/atomic/windows.rs", "rename", 2),
            ("src/transport/mod.rs", "remove_file", 8),
            ("src/transport/mod.rs", "rename", 1),
            ("src/transport/mod.rs", "hard_link", 1),
            ("src/transport/ssh/hostkey.rs", "remove_file", 1),
        ];
        let expected: BTreeMap<(String, &str), usize> = expected
            .iter()
            .map(|(file, symbol, count)| (((*file).to_string(), *symbol), *count))
            .collect();
        assert_eq!(
            observed, expected,
            "the PRODUCTION `std::fs` removal/replace/rename counts changed: a new (or removed) \
             call must be reviewed for the lock-record guard — if the new call cannot name the \
             record, update this pin; test-only calls are excluded by construction"
        );
    }

    /// Every route by which production code can reach a `std::fs` inode
    /// mutator without spelling `std::fs::<symbol>(` is reported by
    /// [`std_fs_mutation_violations`]: a single and a braced `use`, an aliased
    /// symbol, a module alias (`use std::fs as f` and `use std::fs;`), a braced
    /// self-alias, a glob, the nested `use std::{fs::…}` form, and — via the
    /// TRANSITIVE root aliases — `use std as s;` / `use ::std as s;` /
    /// `use std::{self as s};` followed by `s::fs::<symbol>(…)`, the second hop
    /// `use s::fs::remove_file;` followed by the bare symbol, and a second
    /// `s::fs as f` hop. A legitimate call inside the guarded funnel is NOT
    /// reported, so the scanner is not merely "report everything".
    #[test]
    fn the_std_fs_scanner_sees_every_import_alias_route() {
        let fixtures: &[(&str, &str, FsRoute)] = &[
            (
                "src/prod/single_import.rs",
                "use std::fs::remove_file; fn f(p: &Path) -> std::io::Result<()> { remove_file(p) }",
                FsRoute::Imported,
            ),
            (
                "src/prod/braced_import.rs",
                "use std::fs::{remove_file, rename as mv}; fn f(a: &Path, b: &Path) -> std::io::Result<()> { mv(a, b) }",
                FsRoute::Imported,
            ),
            (
                "src/prod/aliased_import.rs",
                "use std::fs::remove_dir_all as wipe; fn f(p: &Path) -> std::io::Result<()> { wipe(p) }",
                FsRoute::Imported,
            ),
            (
                "src/prod/module_alias.rs",
                "use std::fs as fx; fn f(p: &Path) -> std::io::Result<()> { fx::remove_file(p) }",
                FsRoute::ModuleAlias,
            ),
            (
                "src/prod/braced_self_alias.rs",
                "use std::fs::{self as fx}; fn f(a: &Path, b: &Path) -> std::io::Result<()> { fx::rename(a, b) }",
                FsRoute::ModuleAlias,
            ),
            (
                "src/prod/bare_module_import.rs",
                "use std::fs; fn f(p: &Path) -> std::io::Result<()> { fs::remove_dir_all(p) }",
                FsRoute::ModuleAlias,
            ),
            (
                "src/prod/glob_import.rs",
                "use std::fs::*; fn f(p: &Path) -> std::io::Result<()> { remove_file(p) }",
                FsRoute::Glob,
            ),
            (
                "src/prod/nested_std_use.rs",
                "use std::{fs::remove_file}; fn f(p: &Path) -> std::io::Result<()> { remove_file(p) }",
                FsRoute::Imported,
            ),
            (
                "src/prod/nested_std_alias.rs",
                "use std::{fs as fx}; fn f(p: &Path) -> std::io::Result<()> { fx::hard_link(p, p) }",
                FsRoute::ModuleAlias,
            ),
            (
                "src/prod/root_alias_two_step.rs",
                "use std as s; use s::fs::remove_file; fn f(p: &Path) -> std::io::Result<()> { remove_file(p) }",
                FsRoute::Imported,
            ),
            (
                "src/prod/root_alias_import_reversed.rs",
                "use s::fs::remove_file; use std as s; fn f(p: &Path) -> std::io::Result<()> { remove_file(p) }",
                FsRoute::Imported,
            ),
            (
                "src/prod/root_alias_direct.rs",
                "use std as s; fn f(p: &Path) -> std::io::Result<()> { s::fs::remove_file(p) }",
                FsRoute::ModuleAlias,
            ),
            (
                "src/prod/root_alias_leading_colons.rs",
                "use ::std as s; fn f(a: &Path, b: &Path) -> std::io::Result<()> { s :: fs :: rename (a, b) }",
                FsRoute::ModuleAlias,
            ),
            (
                "src/prod/root_alias_braced_self.rs",
                "use std::{self as s}; fn f(p: &Path) -> std::io::Result<()> { s::fs::remove_dir(p) }",
                FsRoute::ModuleAlias,
            ),
            (
                "src/prod/root_alias_second_hop_module.rs",
                "use std as s; use s::fs as fx; fn f(p: &Path) -> std::io::Result<()> { fx::remove_dir_all(p) }",
                FsRoute::ModuleAlias,
            ),
            (
                "src/prod/root_alias_braced_root.rs",
                "use {std as s, std::fs}; fn f(p: &Path) -> std::io::Result<()> { fs::hard_link(p, p) }",
                FsRoute::ModuleAlias,
            ),
            (
                "src/prod/root_alias_glob.rs",
                "use std as s; use s::fs::*; fn f(p: &Path) -> std::io::Result<()> { remove_file(p) }",
                FsRoute::Glob,
            ),
            (
                "src/prod/std_root_glob.rs",
                "use std::*; fn f(p: &Path) -> std::io::Result<()> { fs::remove_file(p) }",
                FsRoute::Glob,
            ),
        ];
        let files: Vec<(String, String)> = fixtures
            .iter()
            .map(|(rel, body, _)| ((*rel).to_string(), (*body).to_string()))
            .collect();
        let violations = std_fs_mutation_violations(&files, &BTreeSet::new());
        for (rel, _, route) in fixtures {
            assert!(
                violations
                    .iter()
                    .any(|v| v.file == *rel && v.route == *route),
                "the scanner missed the {} route in {rel}: {violations:?}",
                route.as_str()
            );
        }
    }

    /// The NEGATIVE fixture: a legitimate mutation at the guarded funnel — the
    /// audited direct `std::fs::rename` the exact-count pin already covers — an
    /// inode-PRESERVING `std::fs::write`, and a redundant `use std;` (which
    /// must NOT turn the canonical `std::fs::…` spelling into an alias route)
    /// all yield NO violation, so the scanner does not merely report every
    /// `std::fs` mention. (The direct call is still counted by the pin; this
    /// test is about the import scanner, not the pin.)
    #[test]
    fn the_std_fs_scanner_does_not_report_the_guarded_funnel() {
        let files = vec![(
            "src/atomic/unix.rs".to_string(),
            "use std;\nfn legit(a: &Path, b: &Path) -> std::io::Result<()> {\n    std::fs::rename(a, b)\n}\n\
             fn keep(p: &Path) -> std::io::Result<()> { std::fs::write(p, b\"x\") }\n"
                .to_string(),
        )];
        assert_eq!(
            std_fs_mutation_violations(&files, &BTreeSet::new()),
            Vec::new(),
            "a legitimate direct call at the guarded funnel and an inode-preserving write must \
             NOT be reported as an import-route violation"
        );
    }

    /// The exact-count pin's matcher is TOKENISED, so a `std::fs::rename (…)`
    /// whose callee and paren are separated by whitespace is counted exactly as
    /// the unspaced spelling is. (The pin's VALUES are unchanged; only the
    /// matcher that drives them is whitespace-insensitive.)
    #[test]
    fn the_exact_count_pin_counts_whitespace_before_the_call() {
        for case in [
            "fn f(a: &Path, b: &Path) -> std::io::Result<()> { std::fs::rename(a, b) }",
            "fn f(a: &Path, b: &Path) -> std::io::Result<()> { std::fs::rename (a, b) }",
            "fn f(a: &Path, b: &Path) -> std::io::Result<()> { std::fs::rename\n\t(a, b) }",
            "fn f(a: &Path, b: &Path) -> std::io::Result<()> { std :: fs :: rename (a, b) }",
            "fn f(a: &Path, b: &Path) -> std::io::Result<()> { ::std::fs::rename(a, b) }",
        ] {
            let code = normalize_ws(&code_only(case));
            assert_eq!(
                count_std_fs_calls(&code, "rename"),
                1,
                "the exact-count matcher missed a spelling: {case} -> {code}"
            );
        }
        // A DIFFERENT symbol and a LONGER identifier must not match.
        let other = normalize_ws(&code_only(
            "fn f(p: &Path) -> std::io::Result<()> { std::fs::rename_all(p) }",
        ));
        assert_eq!(count_std_fs_calls(&other, "rename"), 0);
    }

    /// DEFECT 2 regression: test-only-ness comes from the GATING, not the NAME.
    /// A production `src/review_regression.rs` — a name the old suffix rule
    /// exempted — is scanned and its import route is reported. The two real
    /// gated modules stay exempt, and the exemption follows the `#[cfg]` rather
    /// than the suffix: `not(test)` and a bare `unix` do NOT exempt.
    #[test]
    fn a_production_regression_suffixed_path_is_scanned() {
        let gated = test_only_gated_paths();
        assert!(
            gated.contains("src/deep_tree_regression.rs"),
            "lib.rs's `#[cfg(all(test, unix))] mod deep_tree_regression;` gates it: {gated:?}"
        );
        assert!(gated.contains("src/fifo_regression.rs"));
        assert!(gated.contains("src/test_support.rs"));
        assert!(
            gated.contains("src/transport/scripted.rs"),
            "a gated `mod` nested in a submodule is reached by walking every declaration"
        );
        assert!(!gated.contains("src/review_regression.rs"));
        assert!(!is_test_only("src/review_regression.rs", &gated));

        let files = vec![(
            "src/review_regression.rs".to_string(),
            "use std::fs::remove_file; fn f(p: &Path) -> std::io::Result<()> { remove_file(p) }"
                .to_string(),
        )];
        let violations = std_fs_mutation_violations(&files, &gated);
        assert!(
            violations
                .iter()
                .any(|v| v.file == "src/review_regression.rs" && v.route == FsRoute::Imported),
            "a production `*_regression.rs` path must be scanned: {violations:?}"
        );

        // The gate, not the name: a negative or unrelated cfg does not exempt.
        assert!(!cfg_implies_test("not(test)"));
        assert!(!cfg_implies_test("unix"));
        assert!(!cfg_implies_test("feature = \"x\""));
        assert!(!cfg_implies_test("any(test, unix)"));
        assert!(cfg_implies_test("test"));
        assert!(cfg_implies_test("all(test, unix)"));
        assert!(cfg_implies_test("all(unix, all(test, windows))"));
        assert!(cfg_implies_test("any(all(test, unix), all(test, windows))"));

        let source = "#[cfg(test)] mod alpha;\n#[cfg(not(test))] mod beta;\n\
                      #[cfg(unix)] mod gamma;\npub(crate) mod delta;\n\
                      #[cfg(all(test, unix))] pub(crate) mod epsilon;\n";
        let gated_names: Vec<String> = mod_declarations(source)
            .into_iter()
            .filter(|(_, gated)| *gated)
            .map(|(name, _)| name)
            .collect();
        assert_eq!(
            gated_names,
            vec!["alpha".to_string(), "epsilon".to_string()]
        );
    }
}
