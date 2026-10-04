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
//! carries source audits. `no_libc_reference_outside_the_funnel` is a TEXT
//! audit; `std_fs_name_mutation_counts_are_pinned` PARSES the crate. Their
//! claim is exactly what they catch:
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
//! * `atomic::guard::tests::std_fs_name_mutation_counts_are_pinned` PARSES every
//!   crate `.rs` file into the crate's module graph with `syn` (after
//!   `#[cfg(test)]` items are removed) and pins the per-file per-symbol counts
//!   of the RESOLVED production calls to the `std::fs` name mutators
//!   (`remove_file` / `remove_dir` / `remove_dir_all` / `rename` / `hard_link`),
//!   so a new DIRECT production call, or a new canonical mutator
//!   occurrence in a macro body, changes a count and forces review. Because the
//!   sources are parsed, the DIRECT spelling of a path is not a variable: a raw
//!   identifier (`std::fs::r#remove_file`, `use r#std as s;`), whitespace,
//!   `self`, a leading `::`, and a DIRECT callee written in parentheses or
//!   behind a reference (`(std::fs::remove_file)(p)`, `(&std::fs::rename)(a, b)`)
//!   all resolve to the one symbol they name. A crate-wide alias table maps an
//!   absolute path such as `crate::a::hidden_fs` to the canonical `std` path it
//!   names, so a `pub(crate) use std::fs as hidden_fs;` declared in ANOTHER
//!   MODULE is resolved at its call sites, and a glob over such a module
//!   (`use crate::alias_a::*;`) propagates its aliases into the importing
//!   module. The same parsed pass reports a production path that reaches one of
//!   the five through an ENUMERATED route — an IMPORTED symbol, a MODULE ALIAS
//!   (including a cross-file re-export and a `std`-crate-root alias), or a GLOB.
//!   It does NOT claim to refuse "every IMPORT route": that completeness job is
//!   the resolved-symbol clippy deny, not this audit. A production file that
//!   holds a `#[path]` attribute or an `include!` invocation is REFUSED — the
//!   audit fails closed — rather than resolved, because either decouples a
//!   module's path from its location and defeats the location-derived alias
//!   table. A file is test-only iff its FIRST path component is one of the
//!   crate-root directories `tests/` / `benches/` / `examples/` (and the path
//!   continues into it), or EVERY `mod` declaration that names it is
//!   `#[cfg(...)]`-gated on `test` — never a name or an interior path component,
//!   so an ungated `src/**/tests.rs` compiled into the lib is PRODUCTION, and a
//!   file declared BOTH `#[cfg(test)] mod x;` and `#[cfg(not(test))] mod x;` is
//!   PRODUCTION, because the second declaration compiles it into the lib.
//!
//! The COMPLETENESS device — "no mutation outside the funnel, whatever the
//! spelling" — is a resolved-symbol clippy deny, not these audits. These audits
//! do what a lint cannot: they notice when the funnel's OWN calls change (the
//! per-file pinned `std::fs` counts and the funnel's pinned `libc` counts) and
//! they resolve the enumerated import routes as a second, independent detector.
//! Neither audit is total, and neither is offered as a spelling oracle.
//!
//! The residual holes these audits CANNOT close, and which no claim above is
//! scoped to include: a proc macro that EMITS a mutator call (its expansion is
//! not source this crate parses); a call made through a function POINTER or
//! `dyn` dispatch — an ALIASED `let f = fx::remove_file;` IS reported as a
//! route, but the audit does not carry a value across a variable or resolve a
//! `dyn` method; a `std::fs` mutator reached inside a MACRO through an
//! alias/import rather than the canonical `std::fs::<symbol>` path (the macro
//! token scan matches the canonical token sequence only, and counts the
//! occurrence ONCE, not once per macro expansion); a raw FFI declaration —
//! `extern "C" { fn unlinkat(…); }` followed by a call — which names neither
//! `libc` nor `std::fs`, so NEITHER audit sees it; and an INODE-PRESERVING
//! content mutation (`std::fs::write`, `std::fs::copy`) on a path that already
//! exists, which cannot split a holder because the flock is attached to the
//! unchanged inode. The mode setter (`std::fs::set_permissions`) and the name
//! CREATORS (`std::fs::create_dir*`, the platform `symlink` wrappers) are NO
//! LONGER in this residue: both are on `clippy.toml`'s deny list, counted by
//! the `std::fs` pin, and covered by the cross-artifact consistency test, so an
//! adopt-a-name or re-mode call outside the funnel fails the lint. The reach of
//! a raw FFI declaration is one declaration the crate's
//! own author writes — exactly the residue a source audit carries and names
//! rather than denies. A foreign process, or a raw `std::fs` call the caller
//! writes itself, is outside the crate entirely and is not stopped by any of
//! this.

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
    #![allow(clippy::disallowed_methods)]
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

    /// The name-mutating `libc` functions the OUTSIDE-the-funnel assertion
    /// refuses: BOTH the `*at` forms and the non-`at` forms the earlier
    /// six-symbol scan missed (`open` with `O_CREAT`/`O_TRUNC`, `rmdir`,
    /// `unlink`, `rename`, `symlink`, `link`, `mkdir`, `remove`). It is the
    /// belt to the outside map's braces: the map already closes the outside
    /// `libc` surface exactly, and this additionally forbids a REVIEWED map
    /// entry from being one of these known mutators. Inside the funnel the
    /// device is the exact per-module `libc` surface pin, not this list, so a
    /// new funnel syscall of ANY name changes the pin.
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

    /// Every `std` symbol whose denial the funnel owns, as its CANONICAL path
    /// (the path the crate's own name resolution produces) paired with the
    /// short symbol name used in the pins. It is one table so the count pin,
    /// the import-route detector, the macro-token scan, and the canonical-
    /// spelling test cannot drift apart:
    ///
    /// * the inode mutators that REMOVE or REPLACE an entry (`remove_file`,
    ///   `remove_dir`, `remove_dir_all`, `rename`, `hard_link`);
    /// * the CREATORS that ADOPT a name (`std::fs::create_dir`,
    ///   `std::fs::create_dir_all`, and the platform symlink creators, which
    ///   live under `std::os::…` and are therefore keyed by their full path);
    /// * the path-based mode setter `std::fs::set_permissions`.
    ///
    /// The creators are here because the guard's reserved-spelling check is
    /// what makes a name-creating call safe: a creation bypasses that check
    /// exactly as a removal does, so `create_dir_fd`/`symlink_fd` refuse the
    /// reserved spellings (`operation.lock`, `.sync-aside.1`) that
    /// `std::fs::create_dir*` and the platform symlink calls would happily
    /// create.
    const NAME_MUTATION_SYMBOLS: &[(&[&str], &str)] = &[
        (&["std", "fs", "remove_file"], "remove_file"),
        (&["std", "fs", "remove_dir"], "remove_dir"),
        (&["std", "fs", "remove_dir_all"], "remove_dir_all"),
        (&["std", "fs", "rename"], "rename"),
        (&["std", "fs", "hard_link"], "hard_link"),
        (&["std", "fs", "create_dir"], "create_dir"),
        (&["std", "fs", "create_dir_all"], "create_dir_all"),
        (&["std", "fs", "set_permissions"], "set_permissions"),
        (&["std", "os", "unix", "fs", "symlink"], "symlink"),
        (
            &["std", "os", "windows", "fs", "symlink_file"],
            "symlink_file",
        ),
        (
            &["std", "os", "windows", "fs", "symlink_dir"],
            "symlink_dir",
        ),
    ];

    /// A production route by which one of [`NAME_MUTATION_SYMBOLS`] can be
    /// reached WITHOUT spelling its canonical path, so the exact-count pin
    /// alone cannot see it.
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

    /// A `use` leaf after expansion: the path it names (with `self` folded and
    /// every raw-identifier `r#` prefix stripped) and the LOCAL name it binds,
    /// when the leaf carries an `as` alias.
    struct UseLeaf {
        segments: Vec<String>,
        local: Option<String>,
    }

    /// A canonical, `std`-rooted path: `["std"]` is the crate root,
    /// `["std","fs"]` the filesystem module, `["std","fs",<symbol>]` a mutator,
    /// and a trailing `"*"` a glob.
    type CanonPath = Vec<String>;

    /// The crate-wide name resolution the PARSED audit is built on. Every
    /// source file is parsed once; its module path comes from its
    /// package-relative location. `aliases` maps an ABSOLUTE canonical path
    /// (`crate::a::hidden_fs`) to the canonical `std` path it names, so a
    /// cross-file `pub(crate) use std::fs as hidden_fs;` is resolved at the
    /// CALLER's path rather than in the declaring file's text.
    #[derive(Default)]
    struct FsIndex {
        aliases: BTreeMap<CanonPath, CanonPath>,
    }

    /// A parsed source file plus the module path it belongs to.
    struct ParsedSource {
        rel: String,
        module: CanonPath,
        file: syn::File,
    }

    impl FsIndex {
        /// Resolve a path written in module `module` to its canonical `std`
        /// path: anchor the leading `crate`/`self`/`super`/`std` (a leading
        /// `::` is consumed by the parser, and the `std`/`crate` arms already
        /// cover it), then repeatedly substitute a known alias and append the
        /// next segment. `crate::a::hidden_fs::remove_file` therefore resolves
        /// through the binding declared in `crate::a`. The hop cap makes a
        /// pathological cyclic import terminate rather than loop.
        fn resolve_path(&self, module: &[String], segments: &[String]) -> CanonPath {
            let (mut path, mut index) = match segments.first().map(String::as_str) {
                Some("std") => (vec!["std".to_string()], 1),
                // `libc` is anchored like `std` so a funnel call can be
                // resolved to its canonical `libc::<symbol>` path (the
                // consistency test compares that surface with `clippy.toml`).
                // No `std::fs` symbol starts with `libc`, so this cannot
                // change the `std::fs` audit.
                Some("libc") => (vec!["libc".to_string()], 1),
                Some("crate") => (vec!["crate".to_string()], 1),
                Some("self") => (module.to_vec(), 1),
                Some("super") => {
                    let mut base = module.to_vec();
                    let mut at = 0usize;
                    while segments.get(at).map(String::as_str) == Some("super") {
                        base.pop();
                        at += 1;
                    }
                    (base, at)
                }
                Some(_) | None => (module.to_vec(), 0),
            };
            let mut hops = 0usize;
            loop {
                hops += 1;
                if hops > 256 {
                    break;
                }
                if let Some(target) = self.aliases.get(&path)
                    && *target != path
                {
                    path = target.clone();
                    continue;
                }
                if index >= segments.len() {
                    break;
                }
                path.push(segments[index].clone());
                index += 1;
            }
            path
        }
    }

    /// The local spelling of an identifier with the raw-identifier prefix
    /// removed, so `r#fx` and `fx` are ONE name: the flag lives on the AST node,
    /// not in the spelling the byte matcher used to compare.
    fn unraw(ident: &syn::Ident) -> String {
        let text = ident.to_string();
        text.strip_prefix("r#").unwrap_or(&text).to_string()
    }

    /// The segment spellings of a parsed path, raw identifiers normalized.
    fn path_segments(path: &syn::Path) -> Vec<String> {
        path.segments
            .iter()
            .map(|segment| unraw(&segment.ident))
            .collect()
    }

    /// Expand a parsed `use` tree into its leaves, folding `self` against the
    /// braced prefix (`std::{self as s}` names `std`), recursing into nested
    /// braces, and turning a glob into a trailing `"*"` segment.
    fn expand_use_tree(tree: &syn::UseTree, prefix: &[String], out: &mut Vec<UseLeaf>) {
        match tree {
            syn::UseTree::Path(path) => {
                let mut next = prefix.to_vec();
                next.push(unraw(&path.ident));
                expand_use_tree(&path.tree, &next, out);
            }
            syn::UseTree::Name(name) => {
                let spelling = unraw(&name.ident);
                let mut segments = prefix.to_vec();
                if spelling != "self" {
                    segments.push(spelling);
                }
                out.push(UseLeaf {
                    segments,
                    local: None,
                });
            }
            syn::UseTree::Rename(rename) => {
                let spelling = unraw(&rename.ident);
                let mut segments = prefix.to_vec();
                if spelling != "self" {
                    segments.push(spelling);
                }
                out.push(UseLeaf {
                    segments,
                    local: Some(unraw(&rename.rename)),
                });
            }
            syn::UseTree::Glob(_) => {
                let mut segments = prefix.to_vec();
                segments.push("*".to_string());
                out.push(UseLeaf {
                    segments,
                    local: None,
                });
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    expand_use_tree(item, prefix, out);
                }
            }
        }
    }

    /// Collect every `use` item of one file as its expanded leaves, tagged with
    /// the module path it is declared in. The walk reaches a `use` at ANY
    /// nesting depth — including a BLOCK-LOCAL `use std::fs as fx;` inside a
    /// function body, whose `fx::remove_file` a top-level-only walk would miss.
    struct UseCollector<'out> {
        module: CanonPath,
        out: &'out mut Vec<(CanonPath, Vec<UseLeaf>)>,
    }

    impl<'ast> syn::visit::Visit<'ast> for UseCollector<'_> {
        fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
            if let Some((_, items)) = &item.content {
                let mut child = self.module.clone();
                child.push(unraw(&item.ident));
                let saved = std::mem::replace(&mut self.module, child);
                for inner in items {
                    self.visit_item(inner);
                }
                self.module = saved;
            }
        }

        fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
            let mut leaves = Vec::new();
            expand_use_tree(&item.tree, &[], &mut leaves);
            self.out.push((self.module.clone(), leaves));
        }
    }

    /// Every `use` leaf of one parsed file, with the module path it is declared
    /// in (an inline `mod` block advances the path; a block-local `use` keeps
    /// its enclosing module's path, which is a conservative over-approximation
    /// of its scope).
    fn collect_uses(file: &syn::File, base: &[String]) -> Vec<(CanonPath, Vec<UseLeaf>)> {
        let mut out = Vec::new();
        let mut collector = UseCollector {
            module: base.to_vec(),
            out: &mut out,
        };
        syn::visit::Visit::visit_file(&mut collector, file);
        out
    }

    /// The canonical `std` name-mutation symbol `path` names, if any, from
    /// [`NAME_MUTATION_SYMBOLS`]: `std::fs::remove_file`, `std::fs::create_dir`,
    /// `std::os::unix::fs::symlink`, … — one table, so the count pin, the route
    /// detector, and the canonical-spelling test cannot drift apart.
    fn mutator_symbol(path: &[String]) -> Option<&'static str> {
        NAME_MUTATION_SYMBOLS.iter().find_map(|(segments, symbol)| {
            (path.len() == segments.len()
                && path
                    .iter()
                    .zip(segments.iter())
                    .all(|(actual, expected)| actual.as_str() == *expected))
            .then_some(*symbol)
        })
    }

    /// Whether a canonical path is a glob over `std` or `std::fs`.
    fn is_fs_glob(path: &[String]) -> bool {
        (path.len() == 2 && path[0] == "std" && path[1] == "*")
            || (path.len() == 3 && path[0] == "std" && path[1] == "fs" && path[2] == "*")
    }

    /// Whether a `use` leaf is a glob (its final expanded segment is `"*"`).
    /// The RAW leaf decides this, not the resolved canonical path: a glob over
    /// a LOCAL re-export module resolves to `crate::a::*`, which is not a
    /// `std::fs` glob but still carries that module's aliases.
    fn is_glob_leaf(leaf: &UseLeaf) -> bool {
        leaf.segments.last().map(String::as_str) == Some("*")
    }

    /// Whether the WRITTEN path is already the canonical spelling of
    /// `symbol` (one of [`NAME_MUTATION_SYMBOLS`]) — `std::fs::remove_file`,
    /// but equally `std::os::unix::fs::symlink` — which the exact-count pin
    /// covers, or reaches the mutator only through an import / alias /
    /// cross-file re-export.
    fn is_canonical_literal(segments: &[String], symbol: &str) -> bool {
        NAME_MUTATION_SYMBOLS.iter().any(|(path, name)| {
            *name == symbol
                && path.len() == segments.len()
                && segments
                    .iter()
                    .zip(path.iter())
                    .all(|(actual, expected)| actual.as_str() == *expected)
        })
    }

    /// The callee path of a call, seeing through the parentheses, references,
    /// and invisible groups that a DIRECT call can spell. `(std::fs::remove_file)(p)`
    /// and `(&std::fs::rename)(a, b)` are the SAME resolved call as the bare
    /// spelling, not a function-pointer residue; only a genuinely non-path
    /// callee (a variable, a field, a closure, a `dyn` method) stays invisible
    /// to the count and is named in the audit's residue.
    fn callee_path(mut expr: &syn::Expr) -> Option<&syn::Path> {
        loop {
            match expr {
                syn::Expr::Path(path) if path.qself.is_none() => return Some(&path.path),
                syn::Expr::Paren(paren) => expr = &paren.expr,
                syn::Expr::Reference(reference) => expr = &reference.expr,
                syn::Expr::Group(group) => expr = &group.expr,
                _ => return None,
            }
        }
    }

    /// The identifiers and single punctuation characters of a macro's token
    /// stream, as a flat list. `syn` does not descend into macro tokens (its
    /// `visit_token_stream` hook is a no-op), so a mutator written inside a
    /// `macro_rules!` body or any macro invocation would otherwise be invisible
    /// to the parse. `TokenStream`'s `Display` separates tokens with whitespace
    /// but does not guarantee the spacing around punctuation, so the rendered
    /// text is re-tokenised here into one entry per identifier or single
    /// punctuation character. An `r#` raw prefix is stripped from an
    /// identifier, matching [`unraw`].
    fn macro_token_list(tokens: impl std::fmt::Display) -> Vec<String> {
        let text = tokens.to_string();
        let bytes = text.as_bytes();
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < bytes.len() {
            let b = bytes[i];
            if b.is_ascii_whitespace() {
                i += 1;
                continue;
            }
            if b.is_ascii_alphabetic() || b == b'_' {
                if b == b'r'
                    && bytes.get(i + 1) == Some(&b'#')
                    && bytes
                        .get(i + 2)
                        .is_some_and(|next| next.is_ascii_alphabetic() || *next == b'_')
                {
                    i += 2;
                }
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                out.push(text[start..i].to_string());
                continue;
            }
            let ch = text[i..].chars().next().unwrap();
            out.push(ch.to_string());
            i += ch.len_utf8();
        }
        out
    }

    /// The identifier a macro token list starts a `::`-joined path at, if one
    /// begins at `i`: `["std","fs","create_dir"]` for
    /// `std :: fs :: create_dir`, as a flat token list. Returns the segments
    /// and the index just past the path, so the caller can continue scanning
    /// after it. Raw identifiers are already stripped by [`macro_token_list`].
    fn token_path_at(list: &[String], mut i: usize) -> Option<(Vec<String>, usize)> {
        let first = list.get(i)?;
        let head = first.chars().next()?;
        if !(head.is_ascii_alphabetic() || head == '_') {
            return None;
        }
        let mut segments = vec![first.clone()];
        i += 1;
        while list.get(i).map(String::as_str) == Some(":")
            && list.get(i + 1).map(String::as_str) == Some(":")
        {
            let Some(next) = list.get(i + 2) else { break };
            let Some(head) = next.chars().next() else {
                break;
            };
            if !(head.is_ascii_alphabetic() || head == '_') {
                break;
            }
            segments.push(next.clone());
            i += 3;
        }
        Some((segments, i))
    }

    /// The `std` name-mutation symbols named by a macro token stream as a
    /// canonical `::`-joined path — `std::fs::<symbol>`, `std::fs::create_dir`,
    /// and the `std::os::…::fs::symlink*` creators alike. This restores the
    /// coverage the parse lost: the old byte count saw the literal
    /// `std::fs::<symbol>(` text inside a `macro_rules!` body, and this scan
    /// sees that sequence again, plus the creation family. It is deliberately
    /// NOT a general resolver — a mutator reached inside a macro through an
    /// alias/import is not matched, and the occurrence is counted once per
    /// macro, not once per expansion; both are named in the audit's residue.
    fn macro_mutator_symbols(tokens: impl std::fmt::Display) -> Vec<&'static str> {
        let list = macro_token_list(tokens);
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < list.len() {
            match token_path_at(&list, i) {
                Some((segments, next)) if next > i => {
                    if let Some(symbol) = mutator_symbol(&segments) {
                        out.push(symbol);
                    }
                    i = next;
                }
                _ => i += 1,
            }
        }
        out
    }

    /// The module path of a package-relative source file: `src/lib.rs` is the
    /// crate root and `src/a/b.rs` / `src/a/b/mod.rs` are `crate::a::b`. This is
    /// the file-layout image of the crate's `mod` declarations (the crate uses
    /// the Rust module-file convention), so `crate::a::b` names exactly the
    /// module the `mod` declarations compose; the declarations' `#[cfg(...)]`
    /// attributes, not this mapping, decide the gated/exempt set (see
    /// [`test_only_gated_paths`]). A file outside `src/` gets a module of its
    /// own, so its `use` items resolve without colliding with a real module.
    fn module_path_from_rel(rel: &str) -> CanonPath {
        if let Some(rest) = rel.strip_prefix("src/")
            && let Some(stem) = rest.strip_suffix(".rs")
        {
            let mut segments = vec!["crate".to_string()];
            for part in stem.split('/') {
                segments.push(part.to_string());
            }
            if segments.len() > 1
                && matches!(
                    segments.last().map(String::as_str),
                    Some("mod" | "lib" | "main")
                )
            {
                segments.pop();
            }
            return segments;
        }
        vec!["crate".to_string(), rel.to_string()]
    }

    /// Parse every file with `syn` after the existing `#[cfg(test)]` removal,
    /// so no test code enters the module graph. A PRODUCTION file that does not
    /// parse is a hard failure (the audit cannot vouch for it); a test-only file
    /// that does not parse contributes no items, since none of its code is
    /// production.
    fn parse_crate(files: &[(String, String)], gated: &BTreeSet<String>) -> Vec<ParsedSource> {
        files
            .iter()
            .map(|(rel, raw)| {
                let production = production_only(&code_only(raw));
                let file = match syn::parse_file(&production) {
                    Ok(file) => file,
                    Err(error) if is_test_only(rel, gated) => {
                        let _ = error;
                        syn::File {
                            shebang: None,
                            attrs: Vec::new(),
                            items: Vec::new(),
                        }
                    }
                    Err(error) => {
                        panic!(
                            "the `std::fs` audit could not parse the production form of {rel}: \
                             {error}"
                        )
                    }
                };
                // FAIL CLOSED on a `#[path]`/`include!` in PRODUCTION source:
                // either decouples a module's path from its location, which is
                // the key the alias table is built from, so the audit cannot
                // vouch for the file. A refusal is reviewable; resolving a
                // wrong path is not.
                if !is_test_only(rel, gated)
                    && let Some(escape) = path_or_include_escape(&file)
                {
                    panic!(
                        "the `std::fs` audit REFUSES {rel}: it contains {escape}, which decouples \
                         a module's path from the file's package-relative location (a `#[path]` \
                         attribute) or pulls in source the walk did not collect (an `include!`), \
                         so the parsed module graph and the location-derived alias table would be \
                         wrong. Remove the `#[path]`/`include!` (or move the file to its module's \
                         conventional location) rather than weakening the audit."
                    );
                }
                ParsedSource {
                    rel: rel.clone(),
                    module: module_path_from_rel(rel),
                    file,
                }
            })
            .collect()
    }

    /// Whether an attribute list carries `#[allow(clippy::disallowed_methods)]`
    /// (or the inner `#![allow(...)]` form). Matching on the token TEXT is
    /// enough for the one lint path this audit cares about, and needs no
    /// `syn` printing feature.
    fn attrs_allow_disallowed(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|attr| {
            attr.path().is_ident("allow")
                && matches!(
                    &attr.meta,
                    syn::Meta::List(list)
                        if list
                            .tokens
                            .to_string()
                            .replace(' ', "")
                            .contains("disallowed_methods")
                )
        })
    }

    /// The FUNNEL modules: every PRODUCTION file whose module-level
    /// `#![allow(clippy::disallowed_methods)]` makes the crate-root
    /// resolved-symbol deny blind inside it. DERIVED from the parsed sources, so
    /// the libc-surface pin in [`no_libc_reference_outside_the_funnel`] follows
    /// the allow instead of a second, hardcoded list that could drift from the
    /// `#[allow]`s. Item-level allows are covered separately by
    /// [`funnel_symbol_surface`], which walks annotated items too.
    fn funnel_modules(files: &[(String, String)], gated: &BTreeSet<String>) -> BTreeSet<String> {
        parse_crate(files, gated)
            .iter()
            .filter(|source| !is_test_only(&source.rel, gated))
            .filter(|source| attrs_allow_disallowed(&source.file.attrs))
            .map(|source| source.rel.clone())
            .collect()
    }

    /// Whether a `syn::Item` carries `#[allow(clippy::disallowed_methods)]`,
    /// directly or as a module-body inner attribute. An annotated item is a
    /// FUNNEL region: the crate-root deny does not reach inside it.
    fn item_allows_disallowed(item: &syn::Item) -> bool {
        let attrs = match item {
            syn::Item::Fn(item) => &item.attrs,
            syn::Item::Mod(item) => &item.attrs,
            syn::Item::Impl(item) => &item.attrs,
            syn::Item::Const(item) => &item.attrs,
            syn::Item::Static(item) => &item.attrs,
            syn::Item::Struct(item) => &item.attrs,
            syn::Item::Enum(item) => &item.attrs,
            syn::Item::Trait(item) => &item.attrs,
            syn::Item::Union(item) => &item.attrs,
            syn::Item::Type(item) => &item.attrs,
            syn::Item::Use(item) => &item.attrs,
            _ => return false,
        };
        attrs_allow_disallowed(attrs)
    }

    /// Whether a parsed production file uses a `#[path]` attribute on a `mod`
    /// declaration or an `include!` invocation. Both decouple a module's path
    /// from the file's package-relative location — the key the alias table is
    /// built from — or pull in source the file walk did not collect, so the
    /// audit refuses the file instead of resolving a wrong module path.
    fn path_or_include_escape(file: &syn::File) -> Option<&'static str> {
        struct EscapeVisitor {
            escape: Option<&'static str>,
        }
        impl<'ast> syn::visit::Visit<'ast> for EscapeVisitor {
            fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
                if self.escape.is_none()
                    && item.attrs.iter().any(|attr| attr.path().is_ident("path"))
                {
                    self.escape = Some("a `#[path]` attribute");
                }
                syn::visit::visit_item_mod(self, item);
            }

            fn visit_macro(&mut self, mac: &'ast syn::Macro) {
                if self.escape.is_none()
                    && mac
                        .path
                        .segments
                        .last()
                        .is_some_and(|segment| segment.ident == "include")
                {
                    self.escape = Some("an `include!` invocation");
                }
                syn::visit::visit_macro(self, mac);
            }
        }
        let mut visitor = EscapeVisitor { escape: None };
        syn::visit::Visit::visit_file(&mut visitor, file);
        visitor.escape
    }

    /// Resolve every `use` alias in the parsed crate TRANSITIVELY: a leaf's
    /// path is re-resolved until the alias table stops changing, so
    /// `use s::fs::…` is resolved even when `use std as s;` appears after it.
    fn build_index(parsed: &[ParsedSource]) -> FsIndex {
        let uses: Vec<Vec<(CanonPath, Vec<UseLeaf>)>> = parsed
            .iter()
            .map(|source| collect_uses(&source.file, &source.module))
            .collect();
        let mut index = FsIndex::default();
        for pass in 0..256 {
            let before = index.aliases.clone();
            for per_file in &uses {
                for (module, leaves) in per_file {
                    // Explicit (non-glob) imports first, so a glob cannot
                    // shadow one: Rust gives an explicit import precedence
                    // over a glob.
                    for leaf in leaves {
                        if is_glob_leaf(leaf) {
                            continue;
                        }
                        let canonical = index.resolve_path(module, &leaf.segments);
                        if is_fs_glob(&canonical) {
                            continue;
                        }
                        // The default local name is the LAST SEGMENT OF THE
                        // WRITTEN PATH, not of the canonical path it names:
                        // `use crate::a::hidden_fs;` binds `hidden_fs`, even
                        // though it names `std::fs`.
                        let local = leaf
                            .local
                            .clone()
                            .unwrap_or_else(|| leaf.segments.last().cloned().unwrap_or_default());
                        if local.is_empty() || local == "*" {
                            continue;
                        }
                        let mut key = module.clone();
                        key.push(local);
                        index.aliases.insert(key, canonical);
                    }
                    // Then globs: import every alias the glob's TARGET module
                    // holds into THIS module, so `use crate::alias_a::*;`
                    // makes `crate::alias_a::hidden_fs` visible as
                    // `crate::alias_b::hidden_fs` and a call through the glob
                    // resolves. `or_insert` keeps an explicit import's
                    // binding over a glob-propagated one.
                    for leaf in leaves {
                        if !is_glob_leaf(leaf) {
                            continue;
                        }
                        let prefix = &leaf.segments[..leaf.segments.len() - 1];
                        let target_module = index.resolve_path(module, prefix);
                        let imported: Vec<(String, CanonPath)> = index
                            .aliases
                            .iter()
                            .filter(|(key, _)| {
                                key.len() == target_module.len() + 1
                                    && key.starts_with(target_module.as_slice())
                            })
                            .map(|(key, target)| {
                                (key.last().cloned().unwrap_or_default(), target.clone())
                            })
                            .collect();
                        for (name, target) in imported {
                            let mut key = module.clone();
                            key.push(name);
                            index.aliases.entry(key).or_insert(target);
                        }
                    }
                }
            }
            if index.aliases == before {
                return index;
            }
            assert!(
                pass < 255,
                "the `std::fs` audit's alias fixpoint did not converge"
            );
        }
        index
    }

    /// The parsed visitor: it records a production CALL to a mutator for the
    /// per-file count and a non-canonical ROUTE to one for the violation list.
    struct FsVisitor<'a> {
        index: &'a FsIndex,
        file: String,
        module: CanonPath,
        calls: BTreeMap<(String, &'static str), usize>,
        routes: BTreeSet<Violation>,
    }

    impl<'a> FsVisitor<'a> {
        fn new(index: &'a FsIndex, file: &str, module: CanonPath) -> Self {
            Self {
                index,
                file: file.to_string(),
                module,
                calls: BTreeMap::new(),
                routes: BTreeSet::new(),
            }
        }

        fn record_route(&mut self, symbol: &'static str, route: FsRoute) {
            self.routes.insert(Violation {
                file: self.file.clone(),
                symbol,
                route,
            });
        }
    }

    impl<'ast> syn::visit::Visit<'ast> for FsVisitor<'_> {
        fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
            if let Some((_, items)) = &item.content {
                let mut child = self.module.clone();
                child.push(unraw(&item.ident));
                let saved = std::mem::replace(&mut self.module, child);
                for inner in items {
                    self.visit_item(inner);
                }
                self.module = saved;
            }
        }

        fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
            let mut leaves = Vec::new();
            expand_use_tree(&item.tree, &[], &mut leaves);
            for leaf in leaves {
                let canonical = self.index.resolve_path(&self.module, &leaf.segments);
                if is_fs_glob(&canonical) {
                    self.record_route("*", FsRoute::Glob);
                } else if let Some(symbol) = mutator_symbol(&canonical) {
                    // Importing a mutating symbol into production code is
                    // itself the route (a call, a function pointer, or a
                    // re-export), so flag it even before a call appears.
                    self.record_route(symbol, FsRoute::Imported);
                }
            }
        }

        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            // `syn::visit` does not descend into a macro's tokens, so scan them
            // here for a canonical `std::fs::<symbol>` sequence (the spelling
            // the old byte count saw). This counts a mutator written in a
            // `macro_rules!` body or any macro invocation; it does NOT resolve
            // an aliased path inside the macro, nor count once per expansion.
            for symbol in macro_mutator_symbols(&mac.tokens) {
                *self.calls.entry((self.file.clone(), symbol)).or_default() += 1;
            }
            syn::visit::visit_macro(self, mac);
        }

        fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
            if let Some(path) = callee_path(&call.func) {
                let segments = path_segments(path);
                let canonical = self.index.resolve_path(&self.module, &segments);
                if let Some(symbol) = mutator_symbol(&canonical) {
                    *self.calls.entry((self.file.clone(), symbol)).or_default() += 1;
                }
            }
            syn::visit::visit_expr_call(self, call);
        }

        fn visit_path(&mut self, path: &'ast syn::Path) {
            let segments = path_segments(path);
            let canonical = self.index.resolve_path(&self.module, &segments);
            if let Some(symbol) = mutator_symbol(&canonical)
                && !is_canonical_literal(&segments, symbol)
            {
                let route = if segments.len() == 1 {
                    FsRoute::Imported
                } else {
                    FsRoute::ModuleAlias
                };
                self.record_route(symbol, route);
            }
            syn::visit::visit_path(self, path);
        }
    }

    /// One parse of the crate yields both halves of the audit: the non-canonical
    /// production ROUTES to a mutator (the violation list) and the resolved
    /// production CALL counts per file and symbol (the pin).
    fn std_fs_audit(
        files: &[(String, String)],
        gated: &BTreeSet<String>,
    ) -> (Vec<Violation>, BTreeMap<(String, &'static str), usize>) {
        let parsed = parse_crate(files, gated);
        let index = build_index(&parsed);
        let mut routes = BTreeSet::new();
        let mut calls: BTreeMap<(String, &'static str), usize> = BTreeMap::new();
        for source in &parsed {
            if is_test_only(&source.rel, gated) {
                continue;
            }
            let mut visitor = FsVisitor::new(&index, &source.rel, source.module.clone());
            syn::visit::Visit::visit_file(&mut visitor, &source.file);
            routes.extend(visitor.routes);
            for (key, count) in visitor.calls {
                *calls.entry(key).or_default() += count;
            }
        }
        (routes.into_iter().collect(), calls)
    }

    /// Reports a `std::fs` inode-mutating CALL or PATH in PRODUCTION code that
    /// reaches one of [`NAME_MUTATION_SYMBOLS`] through an ENUMERATED route the
    /// exact-count pin does not cover: an IMPORTED symbol, a MODULE ALIAS
    /// (including a `std`-crate-ROOT alias and a cross-file `pub(crate)`
    /// re-export resolved through the module graph), or a glob. It is not a
    /// total route detector (see the audit's RESIDUE).
    ///
    /// The sources are parsed, so for a DIRECT call or path the spelling is not
    /// a variable: a raw identifier (`r#std`, `r#fx`, `r#remove_file`),
    /// whitespace, a nested brace, `self`, a leading `::`, and a callee in
    /// parentheses or behind a reference (`(fx::remove_file)(p)`) all resolve
    /// to the symbol they name. It does NOT resolve a value carried through a
    /// variable or a `dyn` method (see the audit's RESIDUE). PURE over
    /// `(package-relative path, raw contents)` pairs so a unit test can drive it
    /// with synthetic sources. Test-only files are skipped by [`is_test_only`],
    /// and a production file holding a `#[path]`/`include!` makes the parse fail
    /// closed.
    fn std_fs_mutation_violations(
        files: &[(String, String)],
        gated: &BTreeSet<String>,
    ) -> Vec<Violation> {
        std_fs_audit(files, gated).0
    }

    fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        i
    }

    /// Walk the WHOLE crate directory (not only `src/`), skipping `target/` and
    /// hidden directories. Covers `build.rs`, `tests/**`, `benches/**`, and
    /// `examples/**`. A production file that reaches for a `#[path]` attribute
    /// or an `include!` is not "covered" by resolving it — the audit REFUSES
    /// such a file (see [`path_or_include_escape`]), because either decouples a
    /// module's path from its package-relative location and the location-based
    /// alias table would then be wrong.
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

    /// Test-only files are outside the production funnel. Exemption has TWO
    /// arms and neither is a NAME or an interior path component:
    ///
    /// * POSITION, restricted to the crate-root directories `tests/`,
    ///   `benches/`, and `examples/`: the FIRST path component must be one of
    ///   those AND the path must continue into it, so `tests/push_atomicity.rs`
    ///   is exempt. A `tests` component anywhere ELSE (a `src/**/tests/`
    ///   directory, or a `src/**/tests.rs` FILE) is PRODUCTION: it is compiled
    ///   into the lib unless EVERY `mod` declaration that names it is cfg-gated.
    /// * GATING, for every other file: `gated` is the set of package-relative
    ///   module paths whose EVERY `mod <name>;` declaration carries a
    ///   `#[cfg(...)]` that IMPLIES `test` (see [`test_only_gated_paths`] and
    ///   [`gated_paths_from_declarations`]). A file declared BOTH
    ///   `#[cfg(test)] mod x;` and `#[cfg(not(test))] mod x;` is PRODUCTION.
    ///
    /// NAME is not a predicate. A suffix rule (`ends_with("regression.rs")`)
    /// or a bare `tests.rs` component is strictly BROADER than the gated set:
    /// it exempts every file of that shape, including a production module
    /// compiled into the lib, which could then issue `libc::unlinkat` with
    /// both audits green. Deriving exemption from the GATING has exactly the
    /// gated set as its extension — exempting a new file under `src/` requires
    /// declaring it under `#[cfg(test)]`, which is the property being checked.
    fn is_test_only(rel: &str, gated: &BTreeSet<String>) -> bool {
        let mut components = rel.split('/');
        let first = components.next().unwrap_or("");
        let in_crate_root_dir = components.next().is_some();
        if in_crate_root_dir && matches!(first, "tests" | "benches" | "examples") {
            return true;
        }
        gated.contains(rel)
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

    /// Split a comma list on TOP-LEVEL commas only, so a nested `all(…)` /
    /// `any(…)` disjunct stays one argument for [`cfg_implies_test`].
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
        let mut declarations: Vec<(String, bool)> = Vec::new();
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
            // nested in a submodule is reached). EVERY declaration is recorded
            // — including a second declaration of the same file — so the fold
            // can see that one of them is production; `seen` only gates the
            // walk, not the recording.
            for (name, cfg_test) in mod_declarations(&source) {
                let base = format!("{dir}{name}");
                for candidate in [format!("{base}.rs"), format!("{base}/mod.rs")] {
                    let path = root.join(&candidate);
                    if !path.exists() {
                        continue;
                    }
                    declarations.push((candidate.clone(), cfg_test));
                    if seen.insert(candidate.clone()) {
                        queue.push(path);
                    }
                }
            }
        }
        gated_paths_from_declarations(declarations)
    }

    /// Fold every `mod <name>;` declaration seen across the crate into the set
    /// of files the audits may skip. A file is exempt only when EVERY
    /// declaration that names it is gated on `test`: `test_gated` collects a
    /// file named by a test-implying declaration, `production_declared`
    /// collects a file named by a declaration that does NOT imply `test`, and
    /// the result is their DIFFERENCE. A file declared BOTH `#[cfg(test)] mod
    /// x;` and `#[cfg(not(test))] mod x;` is therefore PRODUCTION — the second
    /// declaration compiles it into the lib, so the test-implying declaration
    /// cannot exempt it.
    fn gated_paths_from_declarations(
        declarations: impl IntoIterator<Item = (String, bool)>,
    ) -> BTreeSet<String> {
        let mut test_gated: BTreeSet<String> = BTreeSet::new();
        let mut production_declared: BTreeSet<String> = BTreeSet::new();
        for (file, implies_test) in declarations {
            if implies_test {
                test_gated.insert(file);
            } else {
                production_declared.insert(file);
            }
        }
        test_gated
            .difference(&production_declared)
            .cloned()
            .collect()
    }

    fn is_ident_start(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'_'
    }

    /// Strip comments AND string-literal CONTENTS, leaving only code
    /// positions; a raw string becomes `r""` and a plain string `""`. F7: a
    /// `#[doc = "libc::open("]` or a `const` holding that text must not trip a
    /// text audit, and a doc comment that merely MENTIONS a symbol must not
    /// either. Raw strings (`r"…"`, `r#"…"#`) are handled. A char literal is
    /// replaced by the VALID placeholder `'a'` (not `''`), because the parsed
    /// audits feed this output to `syn::parse_file`.
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
                    // Skip the escaped character BEFORE looking for the closing
                    // quote: in `'\''` the escaped character IS a quote, and
                    // treating it as the terminator left a stray `'` (which a
                    // `syn` parse rejects, though a byte match did not).
                    let mut j = i + 3;
                    while j < bytes.len() && bytes[j] != b'\'' {
                        j += 1;
                    }
                    i = (j + 1).min(bytes.len());
                    // A char literal is replaced by a VALID literal (`'a'`),
                    // not by `''`: the parsed audits feed this output to
                    // `syn::parse_file`, which rejects an empty char literal.
                    out.push_str("'a'");
                    continue;
                }
                if let (Some(&c1), Some(&c2)) = (bytes.get(i + 1), bytes.get(i + 2))
                    && c2 == b'\''
                    && c1 != b'\\'
                {
                    i += 3;
                    out.push_str("'a'");
                    continue;
                }
            }
            let ch = text[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    /// Whether one attribute group is the `test` gate whose item this function
    /// removes from production: `#[cfg(test)]` (and its `#![…]` / whitespace
    /// spellings).
    fn attribute_is_cfg_test(text: &str) -> bool {
        let (Some(open), Some(close)) = (text.find('['), text.rfind(']')) else {
            return false;
        };
        matches!(cfg_predicate(&text[open + 1..close]), Some(predicate) if predicate.trim() == "test")
    }

    /// Remove every `#[cfg(test)]`-gated item/statement from already
    /// comment/string-stripped code, leaving PRODUCTION code. F4: counting test
    /// occurrences as production is what made six pin entries advertise a
    /// production call that did not exist. An ATTRIBUTE RUN is buffered whole,
    /// so a `#[cfg(unix)]` that PRECEDES the `#[cfg(test)]` of the same item is
    /// removed with it instead of being left dangling (which a `syn` parse
    /// rejects, though a byte match did not). The skip is brace/paren/bracket
    /// balanced, so an `#[cfg(test)]` on a statement or a parameter is removed
    /// with its expression and a `#[cfg(test)] mod tests { … }` with its whole
    /// body. (Run AFTER `code_only`, so a doc comment that merely mentions
    /// `#[cfg(test)]` is already gone.)
    fn production_only(code: &str) -> String {
        let bytes = code.as_bytes();
        let mut out = String::with_capacity(code.len());
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'#' {
                // A `#` that does not begin an attribute group is an ordinary
                // character (a raw identifier's `r#`, a macro token); copy it
                // and advance, or the run collector would not make progress.
                let mut probe = i + 1;
                if bytes.get(probe) == Some(&b'!') {
                    probe += 1;
                }
                if bytes.get(probe) != Some(&b'[') {
                    out.push('#');
                    i += 1;
                    continue;
                }
                // Collect one attribute run: `#![…] #[…] …`.
                let run_start = i;
                let mut j = i;
                let mut has_cfg_test = false;
                loop {
                    let before_ws = j;
                    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if bytes.get(j) != Some(&b'#') {
                        j = before_ws;
                        break;
                    }
                    let mut k = j + 1;
                    if bytes.get(k) == Some(&b'!') {
                        k += 1;
                    }
                    if bytes.get(k) != Some(&b'[') {
                        j = before_ws;
                        break;
                    }
                    let mut depth = 0i32;
                    while k < bytes.len() {
                        match bytes[k] {
                            b'[' => depth += 1,
                            b']' => {
                                depth -= 1;
                                k += 1;
                                if depth == 0 {
                                    break;
                                }
                                continue;
                            }
                            _ => {}
                        }
                        k += 1;
                    }
                    if attribute_is_cfg_test(&code[j..k]) {
                        has_cfg_test = true;
                    }
                    j = k;
                }
                if !has_cfg_test {
                    out.push_str(&code[run_start..j]);
                    i = j;
                    continue;
                }
                // Skip the gated item/statement: a balanced `{…}` block, or up
                // to a `;` / `,` / enclosing CLOSER at depth 0 for a
                // use/expression/parameter/field form. A `)` or `]` seen at
                // depth 0 closes the ENCLOSING list (e.g. the parameter list
                // of `fn f(p: &Path, #[cfg(test)] swap: Option<&T>)`, whose
                // type has no parentheses of its own), so the skip stops BEFORE
                // it and leaves the closer balanced. An `else` continuation
                // keeps an `if … else …` statement together.
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
                            if depth == 0 {
                                break;
                            }
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

    /// Read an identifier at `*i`, honouring an `r#` raw-identifier prefix, and
    /// return its LOCAL spelling (the prefix stripped) with `*i` advanced past
    /// it. `None` when no identifier starts there. A raw identifier names the
    /// same item as its plain spelling, so `r#rmdir` must not be recorded as
    /// the distinct key `r`.
    fn read_ident(code: &str, i: &mut usize) -> Option<String> {
        let bytes = code.as_bytes();
        if !bytes.get(*i).copied().is_some_and(is_ident_start) {
            return None;
        }
        if bytes.get(*i) == Some(&b'r')
            && bytes.get(*i + 1) == Some(&b'#')
            && bytes
                .get(*i + 2)
                .is_some_and(|next| next.is_ascii_alphabetic() || *next == b'_')
        {
            *i += 2;
        }
        let start = *i;
        while *i < bytes.len() && is_ident_start(bytes[*i]) {
            *i += 1;
        }
        Some(code[start..*i].to_string())
    }

    /// Every reference to `libc` in code form, as a map from the reference
    /// spelling to its count: `libc::<symbol>` for a path, `libc::*` for a glob,
    /// and bare `libc` for a module alias / re-export (`use libc as c;`,
    /// `use libc::{self as c};`). WHITESPACE-INSENSITIVE, so `libc :: unlinkat`
    /// and a newline before `(` are both seen, and RAW-IDENTIFIER-INSENSITIVE,
    /// so `libc::r#rmdir` is the SAME key as `libc::rmdir` (a raw identifier
    /// names the same item; without the `r#` strip it was recorded as the
    /// distinct key `libc::r`, which the funnel's exact `libc::<symbol>` pin did
    /// not see). This is ONE rule instead of a list of mutating-symbol
    /// patterns, so a module alias or a cross-file re-export cannot slip past
    /// it.
    fn libc_references(code: &str) -> BTreeMap<String, usize> {
        let bytes = code.as_bytes();
        let mut map: BTreeMap<String, usize> = BTreeMap::new();
        let mut i = 0usize;
        while i < bytes.len() {
            let Some(name) = read_ident(code, &mut i) else {
                i += 1;
                continue;
            };
            if name != "libc" {
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
                } else {
                    let mut symbol_at = j;
                    match read_ident(code, &mut symbol_at) {
                        Some(symbol) => {
                            *map.entry(format!("libc::{symbol}")).or_default() += 1;
                        }
                        None => {
                            *map.entry("libc".to_string()).or_default() += 1;
                        }
                    }
                }
            } else {
                *map.entry("libc".to_string()).or_default() += 1;
            }
        }
        map
    }

    /// Exemption follows the crate-root DIRECTORY or the GATING, never a name
    /// and never an interior component. A file under `src/**/tests.rs` or
    /// `src/**/tests/` is PRODUCTION unless EVERY `mod` declaration that names
    /// it is gated.
    #[test]
    fn is_test_only_follows_crate_root_position_or_gating() {
        let gated = test_only_gated_paths();
        assert!(!is_test_only("src/latests/evil.rs", &gated));
        assert!(!is_test_only("src/latest.rs", &gated));
        // Crate-root directories are the ONLY positional exemption.
        assert!(is_test_only("tests/push_atomicity.rs", &gated));
        assert!(is_test_only("benches/bench.rs", &gated));
        assert!(is_test_only("examples/demo.rs", &gated));
        assert!(
            !is_test_only("benches.rs", &gated),
            "the positional arm is a DIRECTORY, not a bare crate-root file"
        );
        assert!(
            !is_test_only("other/tests/foo.rs", &gated),
            "a `tests` component below the crate root is not a crate-root tests directory"
        );
        // Under `src/`, position is NOT gating: an ungated `tests.rs` compiles
        // into the lib and is PRODUCTION.
        assert!(
            !is_test_only("src/transport/tests/foo.rs", &gated),
            "src/**/tests/ is production unless its `mod` declaration is gated"
        );
        assert!(
            !is_test_only("src/probe/tests.rs", &gated),
            "src/**/tests.rs is production unless its `mod` declaration is gated"
        );
        // The crate's REAL test modules are exempt by GATING.
        assert!(is_test_only("src/sync/apply/tests.rs", &gated));
        assert!(is_test_only("src/deep_tree_regression.rs", &gated));
        assert!(is_test_only("src/fifo_regression.rs", &gated));
        assert!(is_test_only("src/test_support.rs", &gated));
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

    /// STRUCTURAL AUDIT (libc): the crate's `libc` surface is CLOSED on BOTH
    /// sides of the funnel boundary, and each side is pinned by its EXACT
    /// reference set.
    ///
    /// * OUTSIDE the funnel modules, any reference to `libc` — a mutating
    ///   symbol, a module alias (`use libc as c;`), a re-export, a glob, or a
    ///   newline-separated call — must be on a pinned review list. That list
    ///   holds only AUDITED, non-mutating uses (`libc::flock`, `libc::fstatat`,
    ///   `libc::O_RDONLY`), and an independent assertion refuses a
    ///   [`MUTATING_LIBC_SYSCALLS`] member even when someone pins it, so a
    ///   reviewed pin cannot authorize a name-mutating call.
    /// * INSIDE each funnel module, the WHOLE `libc::<symbol>` reference
    ///   surface is pinned PER MODULE and PER REFERENCE. This is the DERIVED
    ///   device that closes the class: a new raw syscall in the funnel changes
    ///   this map and fails, WHATEVER the symbol — `mknod`, `mkfifo`,
    ///   `renameat2`, `fchmod`/`fchmodat`, `remove`, `syscall`, or one not
    ///   written yet — where the earlier per-symbol pin saw only a fixed
    ///   14-name list and changed no count for anything else. The funnel
    ///   modules are themselves DERIVED from the source: every production file
    ///   carrying the module-level `#![allow(clippy::disallowed_methods)]` that
    ///   makes the crate-root resolve-symbol deny blind inside it (see
    ///   [`funnel_modules`]), so a new funnel module joins this pin without an
    ///   edit.
    ///
    /// The `std::fs` side has the analogous derived pin in
    /// [`std_fs_name_mutation_counts_are_pinned`], and the cross-artifact
    /// closure (the funnel may not adopt a symbol `clippy.toml` does not deny)
    /// is [`every_mutation_symbol_the_funnel_uses_is_denied_crate_wide`].
    #[test]
    fn no_libc_reference_outside_the_funnel() {
        let mut files = Vec::new();
        collect_crate_rs_files(Path::new(env!("CARGO_MANIFEST_DIR")), &mut files);
        assert!(files.len() > 10, "the audit must see the real source tree");
        let gated = test_only_gated_paths();
        let sources: Vec<(String, String)> = files
            .iter()
            .map(|file| {
                (
                    crate_relative(file),
                    std::fs::read_to_string(file).expect("read source file"),
                )
            })
            .collect();
        let funnel_modules = funnel_modules(&sources, &gated);
        for module in [
            "src/atomic/mod.rs",
            "src/atomic/unix.rs",
            "src/atomic/windows.rs",
        ] {
            assert!(
                funnel_modules.contains(module),
                "the module-level `#![allow(clippy::disallowed_methods)]` funnel set lost {module}: \
                 {funnel_modules:?}"
            );
        }

        let mut funnel_surface: BTreeMap<(String, String), usize> = BTreeMap::new();
        let mut outside: BTreeMap<(String, String), usize> = BTreeMap::new();
        for (rel, raw) in &sources {
            // Order matters: strip comments/strings FIRST, then remove
            // `#[cfg(test)]` items, so a doc comment mentioning `#[cfg(test)]`
            // is already gone. F5: `src/atomic/guard.rs` is scanned like any
            // other production file (its `#[cfg(test)]` audit code is removed
            // by `production_only`).
            let code = normalize_ws(&production_only(&code_only(raw)));
            let refs = libc_references(&code);
            if funnel_modules.contains(rel) {
                // DERIVED pin: the funnel's ENTIRE `libc` reference surface,
                // per module. ANY new syscall — whatever its name — changes
                // this map, so the funnel's own libc use cannot drift
                // unnoticed inside the blind spot the crate-root deny has here.
                for (reference, count) in refs {
                    *funnel_surface.entry((rel.clone(), reference)).or_default() += count;
                }
                continue;
            }
            if is_test_only(rel, &gated) {
                continue;
            }
            for (reference, count) in refs {
                let symbol = reference.strip_prefix("libc::").unwrap_or("");
                assert!(
                    !MUTATING_LIBC_SYSCALLS.contains(&symbol)
                        && reference != "libc::*"
                        && reference != "libc",
                    "{rel} references the mutating/aliased libc facility {reference:?}: a \
                     name-mutating syscall may be issued only from the guarded funnel \
                     ({funnel_modules:?}); a `use libc … as alias` or a re-export does not exempt it"
                );
                *outside.entry((rel.clone(), reference)).or_default() += count;
            }
        }

        // The pinned production `libc` surface outside the funnel: the
        // crate's AUDITED, non-mutating uses. Any difference in this map is a
        // new (or removed) reference to `libc` outside the funnel and fails
        // here. None of these is a name-mutating symbol (asserted above).
        let expected: &[(&str, &str, usize)] = &[
            // `src/atomic/mod.rs`'s three references MOVED to the funnel surface
            // pin below when the funnel modules became DERIVED from the
            // module-level allow: `atomic/mod.rs` carries that allow, so its
            // `libc` surface is the funnel's own and is pinned per module.
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
             ({funnel_modules:?}); a NON-mutating one must be reviewed and pinned here"
        );

        // The FUNNEL side: the exact per-module, per-reference `libc` surface.
        // A new raw syscall in a funnel module changes this map and fails, so
        // the class is closed for EVERY symbol name rather than for an
        // enumerated 14. A change here is a deliberate, reviewed addition.
        let expected_funnel: &[(&str, &str, usize)] = &[
            // src/atomic/mod.rs — the cooperative-open flags for the atomic
            // open primitive (moved here from the outside map when the funnel
            // modules became derived from the module-level allow).
            ("src/atomic/mod.rs", "libc::O_CLOEXEC", 1),
            ("src/atomic/mod.rs", "libc::O_DIRECTORY", 1),
            ("src/atomic/mod.rs", "libc::O_NOFOLLOW", 1),
            // src/atomic/unix.rs — the funnel's WHOLE libc surface.
            // The name-mutating syscalls (each still individually reviewed;
            // the point of the pin is that a NEW one changes the map):
            ("src/atomic/unix.rs", "libc::openat", 7),
            ("src/atomic/unix.rs", "libc::unlinkat", 3),
            ("src/atomic/unix.rs", "libc::renameat", 1),
            ("src/atomic/unix.rs", "libc::symlinkat", 1),
            ("src/atomic/unix.rs", "libc::linkat", 1),
            ("src/atomic/unix.rs", "libc::mkdirat", 3),
            // The `openat` flag/`*at` surface (constants and flags).
            ("src/atomic/unix.rs", "libc::AT_REMOVEDIR", 1),
            ("src/atomic/unix.rs", "libc::AT_SYMLINK_NOFOLLOW", 4),
            ("src/atomic/unix.rs", "libc::O_APPEND", 1),
            ("src/atomic/unix.rs", "libc::O_CLOEXEC", 8),
            ("src/atomic/unix.rs", "libc::O_CREAT", 5),
            ("src/atomic/unix.rs", "libc::O_DIRECTORY", 18),
            ("src/atomic/unix.rs", "libc::O_EXCL", 3),
            ("src/atomic/unix.rs", "libc::O_NOFOLLOW", 8),
            ("src/atomic/unix.rs", "libc::O_NONBLOCK", 2),
            ("src/atomic/unix.rs", "libc::O_RDONLY", 29),
            ("src/atomic/unix.rs", "libc::O_RDWR", 1),
            ("src/atomic/unix.rs", "libc::O_TRUNC", 2),
            ("src/atomic/unix.rs", "libc::O_WRONLY", 5),
            ("src/atomic/unix.rs", "libc::PATH_MAX", 1),
            // The mode/kind masks and the C types the syscall signatures need.
            ("src/atomic/unix.rs", "libc::S_IFDIR", 4),
            ("src/atomic/unix.rs", "libc::S_IFLNK", 1),
            ("src/atomic/unix.rs", "libc::S_IFMT", 4),
            ("src/atomic/unix.rs", "libc::S_IFREG", 1),
            ("src/atomic/unix.rs", "libc::c_char", 3),
            ("src/atomic/unix.rs", "libc::mode_t", 2),
            // The READ-ONLY descriptor calls (the `stat` family and the
            // directory-descriptor reader); none can adopt or free a name.
            ("src/atomic/unix.rs", "libc::closedir", 1),
            ("src/atomic/unix.rs", "libc::fcntl", 1),
            ("src/atomic/unix.rs", "libc::fdopendir", 1),
            ("src/atomic/unix.rs", "libc::fstat", 3),
            ("src/atomic/unix.rs", "libc::fstatat", 4),
            ("src/atomic/unix.rs", "libc::readdir", 1),
            ("src/atomic/unix.rs", "libc::readlinkat", 2),
            ("src/atomic/unix.rs", "libc::stat", 8),
            // A macOS-only `fcntl(F_GETPATH)` read of an open descriptor.
            ("src/atomic/unix.rs", "libc::F_GETPATH", 1),
        ];
        let expected_funnel: BTreeMap<(String, String), usize> = expected_funnel
            .iter()
            .map(|(file, reference, count)| {
                (((*file).to_string(), (*reference).to_string()), *count)
            })
            .collect();
        assert_eq!(
            funnel_surface, expected_funnel,
            "the guarded funnel's `libc` reference surface changed — a new raw syscall in a funnel \
             module must be reviewed for the lock-record guard, whatever the symbol. The actual \
             surface is on the LEFT: {funnel_surface:?}"
        );
    }

    /// STRUCTURAL AUDIT (`std::fs`): the `std::fs`/`std::os` calls that can
    /// REMOVE, REPLACE, or CREATE a directory entry — the ones that can free or
    /// swap a lock record's inode, or ADOPT a name the reserved-spelling guard
    /// must refuse — are pinned PER PRODUCTION FILE and PER SYMBOL, and the
    /// same PARSED pass REPORTS the production paths that reach one of them
    /// through an ENUMERATED route the pin does not cover. This audit is the
    /// crate's OWN-call detector and a second, independent route detector; it is
    /// NOT the completeness device (that is the resolved-symbol clippy deny).
    ///
    /// MECHANISM: every `.rs` file under the package directory is parsed with
    /// `syn` after `#[cfg(test)]` items are removed, so the audit sees SYMBOLS,
    /// not bytes: for a DIRECT call or path, a raw identifier
    /// (`std::fs::r#remove_file`, `use r#std as s;`), whitespace, a nested
    /// brace, `self`, a leading `::`, or a callee in parentheses/behind a
    /// reference cannot name a different symbol than the one it resolves to. A
    /// crate-wide alias table, built by walking every `use` tree (with inline
    /// `mod` blocks advancing the module path and glob imports propagating a
    /// local module's aliases) to a fixpoint, maps an absolute path such as
    /// `crate::a::hidden_fs` to the canonical `std` path it names, so a
    /// `pub(crate) use std::fs as hidden_fs;` in ANOTHER FILE is resolved at
    /// its call sites, including through `use crate::a::*;`. A production file
    /// that holds a `#[path]` attribute or an `include!` invocation is REFUSED
    /// (the parse fails closed), because either decouples a module path from
    /// the file's location and the location-derived alias table would be wrong.
    ///
    /// COUNT METHOD: a resolved production CALL — an `ExprCall` whose callee
    /// path resolves to `std::fs::<symbol>` — is one occurrence for that file
    /// and symbol, and a canonical `std::fs::<symbol>` token sequence inside a
    /// macro's tokens is one more. This is a SUPERSET of the old byte count,
    /// which saw only the literal `std::fs::<symbol>(` text: every occurrence
    /// the byte count saw — including one inside a `macro_rules!` body — is
    /// seen here too, plus resolved aliases and parenthesized/referenced direct
    /// callees. It is NOT a superset of every possible call: a mutator reached
    /// inside a macro through an alias, or a call through a function-pointer
    /// variable, is not counted (see RESIDUE), so adding such a call does not
    /// change a count. Every current production call is canonical or in an
    /// ordinary macro body, so the pinned values are unchanged; a change here is
    /// a deliberate, reviewed addition recorded at the pin.
    ///
    /// EXEMPTION: a file is test-only iff its FIRST path component is one of the
    /// crate-root directories `tests/` / `benches/` / `examples/` (and the path
    /// continues into it), or EVERY `mod` declaration that names it is
    /// `#[cfg(...)]`-gated on `test` (see [`is_test_only`] /
    /// [`gated_paths_from_declarations`]). A file declared BOTH
    /// `#[cfg(test)] mod x;` and `#[cfg(not(test))] mod x;` is PRODUCTION. A
    /// file under `src/**/tests.rs` or `src/**/tests/` is PRODUCTION unless
    /// gated; a name or an interior path component never exempts.
    ///
    /// RESIDUE a PARSE cannot see, named not hidden: (1) a proc macro that
    /// EMITS a mutator call, or a `std::fs` mutator reached inside a macro
    /// through an alias/import rather than the canonical path (the macro token
    /// scan matches the canonical `std::fs::<symbol>` sequence only, and counts
    /// the occurrence once per macro, not once per expansion); (2) a call made
    /// through a function POINTER or `dyn` dispatch — an ALIASED
    /// `let f = fx::remove_file;` IS flagged as a route, but the audit does not
    /// carry a value across a variable or resolve a `dyn` method, so `let f =
    /// std::fs::remove_file; f(p)` is neither counted nor reported; (3) a raw
    /// `extern "C"` declaration, `extern "C" { fn unlinkat(dirfd: i32, path:
    /// *const i8, flags: i32) -> i32; }`, followed by a call: it names neither
    /// `libc` nor `std::fs`, so NEITHER this audit nor
    /// `no_libc_reference_outside_the_funnel` sees it. INODE-PRESERVING content
    /// mutations (`std::fs::write`, `std::fs::copy`) are not pinned: on an
    /// EXISTING path they cannot split a holder because the flock stays on the
    /// unchanged inode (the one case where they adopt an absent name is named
    /// in `FUNNEL_SYMBOLS_NOT_DENIED`). The mode setter and the name CREATORS
    /// are pinned now, not residue. The audit resolves the
    /// ENUMERATED routes and NAMES this residue rather than implying totality;
    /// the clippy deny carries the completeness claim.
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

        // The PARSED pass yields BOTH halves: the non-canonical routes the
        // count pin cannot see, and the resolved per-file call counts.
        let (violations, observed) = std_fs_audit(&files, &gated);
        assert!(
            violations.is_empty(),
            "production code reaches a `std::fs` inode mutator through a route the pin does not \
             cover — an imported symbol, a module alias (including a cross-file `pub(crate)` \
             re-export), or a glob — so the lock record could be removed or replaced with the \
             count pin still green; route it through the guarded funnel (src/atomic/unix.rs) \
             instead: {violations:?}"
        );
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
            // --- The CREATION wrappers (round 5): `std::fs::create_dir*` and
            // the platform symlink creators ADOPT a name, so they are on
            // `clippy.toml`'s deny list and this pin tracks their production
            // call counts too. Every site below is either a funnel module
            // (module-level allow), an item-level allow reviewed for the
            // creation it performs, or the ONE platform symlink helper.
            //
            // funnel modules: the fd-confined `create_dir_fd` +
            // `create_dir_all` fallback, and the macOS/Windows ports.
            ("src/atomic/unix.rs", "create_dir", 2),
            ("src/atomic/unix.rs", "create_dir_all", 1),
            ("src/atomic/windows.rs", "create_dir", 3),
            ("src/atomic/windows.rs", "create_dir_all", 5),
            // the ONE platform std-symlink helper (`platform::symlink`), which
            // is the single reviewed allow for all three creators, plus the
            // path-based mode authority (`platform::chmod`).
            ("src/platform.rs", "symlink", 1),
            ("src/platform.rs", "symlink_dir", 1),
            ("src/platform.rs", "symlink_file", 1),
            ("src/platform.rs", "set_permissions", 1),
            // item-level-reviewed creation sites outside the funnel: the
            // destination lock record's parent chain (`create_lock_parent`),
            // the destination root itself (`root_for_mutation`), the sidecar
            // parent chain (`ensure_operation_lock_sidecar_durable`), the
            // local root + layout (`root_dir`/`provision_layout`), and the ssh
            // mux directory (`prepare_identity`) / known-hosts cache
            // (`pin_known_hosts`).
            ("src/sync/apply.rs", "create_dir_all", 2),
            ("src/transport/mod.rs", "create_dir_all", 5),
            ("src/transport/ssh/hostkey.rs", "create_dir_all", 1),
            ("src/transport/ssh/mod.rs", "create_dir_all", 1),
        ];
        let expected: BTreeMap<(String, &'static str), usize> = expected
            .iter()
            .map(|(file, symbol, count)| (((*file).to_string(), *symbol), *count))
            .collect();
        assert_eq!(
            observed, expected,
            "the PRODUCTION `std::fs` mutation counts changed: a new (or removed) removal, \
             creation, or mode call must be reviewed for the lock-record guard — if the new call \
             cannot name the record, update this pin; test-only calls are excluded by construction"
        );
    }

    /// The ENUMERATED import/alias routes by which production code can reach a
    /// `std::fs` inode mutator without spelling `std::fs::<symbol>(` are
    /// reported by [`std_fs_mutation_violations`]: a single and a braced `use`,
    /// an aliased symbol, a module alias (`use std::fs as f` and
    /// `use std::fs;`), a braced self-alias, a glob, the nested
    /// `use std::{fs::…}` form, and — via the TRANSITIVE root aliases —
    /// `use std as s;` / `use ::std as s;` / `use std::{self as s};` followed by
    /// `s::fs::<symbol>(…)`, the second hop `use s::fs::remove_file;` followed
    /// by the bare symbol, and a second `s::fs as f` hop. (These are the routes
    /// the parse can resolve; see the audit's RESIDUE note for what it cannot.)
    /// A legitimate call inside the guarded funnel is NOT reported, so the
    /// scanner is not merely "report everything".
    #[test]
    fn the_std_fs_scanner_resolves_the_enumerated_import_routes() {
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

    /// The THREE routes the text scanner walked around, all reproduced at the
    /// scanner level: a CROSS-FILE module alias (the alias is declared in
    /// another module and invisible to a per-file text match), a RAW-identifier
    /// spelling of any of the three import routes, and a POSITIONAL exemption
    /// (see the exemption test below). The same-file alias is the CONTROL: it
    /// was already caught before the parse, so it must stay caught.
    #[test]
    fn the_parsed_scanner_resolves_cross_file_aliases_and_raw_identifiers() {
        let files: Vec<(String, String)> = [
            // Cross-file: the alias is declared in `crate::a`, used in `crate::b`.
            (
                "src/a.rs",
                "pub(crate) use std::fs as hidden_fs;",
            ),
            (
                "src/b.rs",
                "pub(crate) fn f(p: &Path) { let _ = crate::a::hidden_fs::remove_file(p); }",
            ),
            // Cross-file through `super::` and a bare local binding.
            (
                "src/x/a.rs",
                "pub(crate) use std::fs as hidden_fs;",
            ),
            (
                "src/x/b.rs",
                "pub(crate) fn f(p: &Path) { let _ = super::a::hidden_fs::remove_dir_all(p); }",
            ),
            (
                "src/x/c.rs",
                "use crate::x::a::hidden_fs; pub(crate) fn f(p: &Path) { let _ = hidden_fs::remove_dir(p); }",
            ),
            // Raw identifiers: module alias, imported symbol, crate-root alias,
            // aliased imported symbol.
            (
                "src/raw_module.rs",
                "use std::fs as r#fx; pub(crate) fn f(p: &Path) { let _ = r#fx::remove_file(p); }",
            ),
            (
                "src/raw_symbol.rs",
                "use std::fs::r#remove_file; pub(crate) fn f(p: &Path) { let _ = r#remove_file(p); }",
            ),
            (
                "src/raw_root.rs",
                "use r#std as s; pub(crate) fn f(p: &Path) { let _ = s::fs::remove_dir(p); }",
            ),
            (
                "src/raw_alias.rs",
                "use std::fs::r#remove_dir_all as r#wipe; pub(crate) fn f(p: &Path) { let _ = r#wipe(p); }",
            ),
            // BLOCK-LOCAL `use`: the alias is scoped to a function body, so a
            // top-level-only walk would miss the call.
            (
                "src/block_local.rs",
                "pub(crate) fn f(p: &Path) { use std::fs as fx; let _ = fx::remove_file(p); }",
            ),
            (
                "src/block_local_symbol.rs",
                "pub(crate) fn f(p: &Path) { use std::fs::remove_file; let _ = remove_file(p); }",
            ),
            // Same-file control: caught by the byte matcher too, so it must stay.
            (
                "src/control.rs",
                "use std::fs as hidden_fs; pub(crate) fn f(p: &Path) { let _ = hidden_fs::remove_file(p); }",
            ),
            // NEGATIVE: a declared alias with no call is not itself a route.
            (
                "src/no_call.rs",
                "pub(crate) use std::fs as unused_fs;",
            ),
        ]
        .iter()
        .map(|(rel, body)| ((*rel).to_string(), (*body).to_string()))
        .collect();
        let violations = std_fs_mutation_violations(&files, &BTreeSet::new());
        for (file, symbol, route) in [
            ("src/b.rs", "remove_file", FsRoute::ModuleAlias),
            ("src/x/b.rs", "remove_dir_all", FsRoute::ModuleAlias),
            ("src/x/c.rs", "remove_dir", FsRoute::ModuleAlias),
            ("src/raw_module.rs", "remove_file", FsRoute::ModuleAlias),
            ("src/raw_symbol.rs", "remove_file", FsRoute::Imported),
            ("src/raw_root.rs", "remove_dir", FsRoute::ModuleAlias),
            ("src/raw_alias.rs", "remove_dir_all", FsRoute::Imported),
            ("src/block_local.rs", "remove_file", FsRoute::ModuleAlias),
            (
                "src/block_local_symbol.rs",
                "remove_file",
                FsRoute::Imported,
            ),
            ("src/control.rs", "remove_file", FsRoute::ModuleAlias),
        ] {
            assert!(
                violations
                    .iter()
                    .any(|v| v.file == file && v.symbol == symbol && v.route == route),
                "the parsed scanner missed {route:?} {symbol} in {file}: {violations:?}"
            );
        }
        assert!(
            !violations.iter().any(|v| v.file == "src/no_call.rs"),
            "an unused alias declaration is not a route: {violations:?}"
        );
    }

    /// The EXEMPTION route: a file merely sitting under `src/**/tests.rs` is
    /// PRODUCTION, so its import route is reported; the crate's real gated test
    /// modules are exempt by their `#[cfg(...)]`, not their names or paths.
    #[test]
    fn a_src_tests_file_is_scanned_unless_gated() {
        let gated = test_only_gated_paths();
        let route =
            "use std::fs::remove_file; pub(crate) fn f(p: &Path) { let _ = remove_file(p); }";
        let files: Vec<(String, String)> = [
            "src/probe/tests.rs",
            "src/sync/apply/tests.rs",
            "src/deep_tree_regression.rs",
            "src/fifo_regression.rs",
            "src/test_support.rs",
        ]
        .iter()
        .map(|rel| ((*rel).to_string(), route.to_string()))
        .collect();
        let violations = std_fs_mutation_violations(&files, &gated);
        assert!(
            violations
                .iter()
                .any(|v| v.file == "src/probe/tests.rs" && v.route == FsRoute::Imported),
            "an ungated `src/**/tests.rs` compiled into the lib must be scanned: {violations:?}"
        );
        for exempt in [
            "src/sync/apply/tests.rs",
            "src/deep_tree_regression.rs",
            "src/fifo_regression.rs",
            "src/test_support.rs",
        ] {
            assert!(
                !violations.iter().any(|v| v.file == exempt),
                "the gated test module {exempt} must stay exempt: {violations:?}"
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

    /// The pin's matcher is PARSED, so a call whose callee and paren are
    /// separated by whitespace, a leading-`::` path, and a RAW-identifier
    /// symbol are all the SAME resolved call. (The pin's VALUES are unchanged;
    /// only the matcher that drives them is spelling-insensitive now.)
    #[test]
    fn the_parsed_pin_counts_whitespace_and_raw_spellings() {
        for case in [
            "fn f(a: &Path, b: &Path) -> std::io::Result<()> { std::fs::rename(a, b) }",
            "fn f(a: &Path, b: &Path) -> std::io::Result<()> { std::fs::rename (a, b) }",
            "fn f(a: &Path, b: &Path) -> std::io::Result<()> { std::fs::rename\n\t(a, b) }",
            "fn f(a: &Path, b: &Path) -> std::io::Result<()> { std :: fs :: rename (a, b) }",
            "fn f(a: &Path, b: &Path) -> std::io::Result<()> { ::std::fs::rename(a, b) }",
            "fn f(a: &Path, b: &Path) -> std::io::Result<()> { std::fs::r#rename(a, b) }",
        ] {
            let files = vec![("src/prod/count.rs".to_string(), case.to_string())];
            let (_, counts) = std_fs_audit(&files, &BTreeSet::new());
            assert_eq!(
                counts
                    .get(&("src/prod/count.rs".to_string(), "rename"))
                    .copied()
                    .unwrap_or(0),
                1,
                "the parsed count missed a spelling: {case}"
            );
        }
        // A DIFFERENT symbol and a LONGER identifier must not match.
        let files = vec![(
            "src/prod/count.rs".to_string(),
            "fn f(p: &Path) -> std::io::Result<()> { std::fs::rename_all(p) }".to_string(),
        )];
        let (_, counts) = std_fs_audit(&files, &BTreeSet::new());
        assert_eq!(
            counts
                .get(&("src/prod/count.rs".to_string(), "rename"))
                .copied()
                .unwrap_or(0),
            0
        );
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

    /// DEFECT (exemption): a file declared BOTH `#[cfg(test)] mod trick;` and
    /// `#[cfg(not(test))] mod trick;` is compiled into the non-test library, so
    /// the test-implying declaration must NOT exempt it from either audit. The
    /// fold keeps `test_gated` and `production_declared` apart and subtracts the
    /// latter, so only a file whose EVERY declaration implies `test` is exempt.
    #[test]
    fn a_file_declared_both_test_and_production_is_not_exempt() {
        // CONTROL: declared ONLY under a test-implying cfg -> exempt.
        assert!(
            gated_paths_from_declarations([("src/trick.rs".to_string(), true)])
                .contains("src/trick.rs")
        );
        // CONTROL: declared only in production -> not exempt.
        assert!(
            !gated_paths_from_declarations([("src/trick.rs".to_string(), false)])
                .contains("src/trick.rs")
        );
        // THE HOLE: BOTH declarations. The production arm compiles the file into
        // the lib, so the test arm cannot exempt it.
        let gated = gated_paths_from_declarations([
            ("src/trick.rs".to_string(), true),
            ("src/trick.rs".to_string(), false),
        ]);
        assert!(
            !gated.contains("src/trick.rs"),
            "both declarations must be PRODUCTION: {gated:?}"
        );
        assert!(!is_test_only("src/trick.rs", &gated));
    }

    /// DEFECT (non-path callee): `(std::fs::remove_file)(p)` and
    /// `(&std::fs::rename)(a, b)` are DIRECT calls, not function-pointer
    /// residue, but a visitor that matched only `Expr::Path` callees saw
    /// neither. The callee is unwrapped through parentheses, references, and
    /// invisible groups before counting. The function-pointer-through-a-VARIABLE
    /// form stays uncounted, asserted here as the named residue.
    #[test]
    fn the_parsed_pin_sees_through_a_parenthesized_or_referenced_callee() {
        for (label, case, symbol) in [
            (
                "parenthesized canonical",
                "fn f(p: &Path) -> std::io::Result<()> { (std::fs::remove_file)(p) }",
                "remove_file",
            ),
            (
                "referenced canonical",
                "fn f(a: &Path, b: &Path) -> std::io::Result<()> { (&std::fs::rename)(a, b) }",
                "rename",
            ),
            (
                "parenthesized alias",
                "use std::fs as fx; fn f(p: &Path) -> std::io::Result<()> { (fx::remove_dir_all)(p) }",
                "remove_dir_all",
            ),
            // CONTROL: the bare canonical spelling.
            (
                "bare canonical",
                "fn f(a: &Path, b: &Path) -> std::io::Result<()> { std::fs::hard_link(a, b) }",
                "hard_link",
            ),
        ] {
            let files = vec![("src/prod/count.rs".to_string(), case.to_string())];
            let (_, counts) = std_fs_audit(&files, &BTreeSet::new());
            assert_eq!(
                counts
                    .get(&("src/prod/count.rs".to_string(), symbol))
                    .copied()
                    .unwrap_or(0),
                1,
                "{label} was not counted: {case} -> {counts:?}"
            );
        }
        // RESIDUE: a value carried through a VARIABLE is neither counted nor
        // reported (the callee is an `Expr::Path` to the local, not the
        // mutator).
        let files = vec![(
            "src/prod/count.rs".to_string(),
            "fn f(p: &Path) -> std::io::Result<()> { let g = std::fs::remove_file; g(p) }"
                .to_string(),
        )];
        let (routes, counts) = std_fs_audit(&files, &BTreeSet::new());
        assert!(
            counts.is_empty() && routes.is_empty(),
            "the function-pointer-through-a-variable residue is named, not closed: \
             {routes:?} {counts:?}"
        );
    }

    /// DEFECT (glob over a local re-export): `src/alias_a.rs` re-exports
    /// `std::fs as hidden_fs`, and `src/alias_b.rs` pulls it in with
    /// `use crate::alias_a::*;`. The alias table skipped glob leaves, so
    /// `hidden_fs::remove_file(p)` in `alias_b` resolved to nothing. The glob
    /// now propagates the target module's aliases into the importing module.
    #[test]
    fn a_glob_over_a_local_reexport_module_propagates_its_aliases() {
        let files: Vec<(String, String)> = [
            ("src/alias_a.rs", "pub(crate) use std::fs as hidden_fs;"),
            (
                "src/alias_b.rs",
                "use crate::alias_a::*;\npub(crate) fn f(p: &Path) { let _ = hidden_fs::remove_file(p); }",
            ),
            // CONTROL: the same alias reached through the module path directly.
            (
                "src/control.rs",
                "pub(crate) fn f(p: &Path) { let _ = crate::alias_a::hidden_fs::remove_file(p); }",
            ),
        ]
        .iter()
        .map(|(rel, body)| ((*rel).to_string(), (*body).to_string()))
        .collect();
        let (routes, counts) = std_fs_audit(&files, &BTreeSet::new());
        for file in ["src/alias_b.rs", "src/control.rs"] {
            assert!(
                routes.iter().any(|v| v.file == file
                    && v.symbol == "remove_file"
                    && v.route == FsRoute::ModuleAlias),
                "the {file} glob/alias route was missed: {routes:?}"
            );
            assert_eq!(
                counts
                    .get(&(file.to_string(), "remove_file"))
                    .copied()
                    .unwrap_or(0),
                1,
                "the {file} glob/alias call was not counted: {counts:?}"
            );
        }
    }

    /// DEFECT (raw-identifier libc symbol): `libc::r#rmdir` tokenised to the
    /// key `libc::r`, which the funnel's exact `libc::<symbol>` pin never
    /// looked up, so a raw-identifier spelling of a funnel call changed no
    /// count. The `r#` prefix is now stripped, so the key is `libc::rmdir`.
    #[test]
    fn the_libc_scanner_normalises_a_raw_identifier_symbol() {
        let refs = libc_references(&normalize_ws(&code_only("libc::r#rmdir(p);")));
        assert_eq!(refs.get("libc::rmdir").copied(), Some(1), "{refs:?}");
        assert_eq!(
            refs.get("libc::r").copied(),
            None,
            "the raw spelling must not survive as its own key: {refs:?}"
        );
        // A raw `libc` root and a raw symbol together.
        let refs = libc_references(&normalize_ws(&code_only("r#libc::r#unlinkat(0, 0, 0);")));
        assert_eq!(refs.get("libc::unlinkat").copied(), Some(1), "{refs:?}");
        // CONTROL: the plain spelling maps to the same key.
        let refs = libc_references(&normalize_ws(&code_only("libc::rmdir(p);")));
        assert_eq!(refs.get("libc::rmdir").copied(), Some(1), "{refs:?}");
    }

    /// DEFECT (`#[path]`/`include!`): a `#[path]` attribute makes a file's
    /// module path differ from its package-relative location, which is the key
    /// the alias table is built from, so `crate::aliased_mod::hidden_fs` never
    /// resolved; an `include!` pulls in source the walk did not collect. The
    /// audit now FAILS CLOSED on either in production source.
    #[test]
    #[should_panic(expected = "REFUSES")]
    fn a_path_attribute_in_production_source_is_refused() {
        let files = vec![(
            "src/prod/aliased_mod.rs".to_string(),
            "#[path = \"foo_alias.rs\"] mod aliased_mod;\n\
             pub(crate) fn f(p: &Path) { let _ = crate::aliased_mod::hidden_fs::remove_file(p); }"
                .to_string(),
        )];
        let _ = std_fs_mutation_violations(&files, &BTreeSet::new());
    }

    #[test]
    #[should_panic(expected = "REFUSES")]
    fn an_include_in_production_source_is_refused() {
        let files = vec![(
            "src/prod/includes.rs".to_string(),
            "include!(\"generated.rs\");\n".to_string(),
        )];
        let _ = std_fs_mutation_violations(&files, &BTreeSet::new());
    }

    /// CONTROL: a production file with neither `#[path]` nor `include!` is not
    /// refused, so the refusal is specific to the two escapes.
    #[test]
    fn a_plain_production_file_is_not_refused() {
        let files = vec![(
            "src/prod/plain.rs".to_string(),
            "pub(crate) fn f(p: &Path) { let _ = std::fs::read(p); }".to_string(),
        )];
        assert_eq!(
            std_fs_mutation_violations(&files, &BTreeSet::new()),
            Vec::new()
        );
    }

    /// DEFECT (macro tokens): the old byte count saw a `macro_rules!` body's
    /// literal `std::fs::<symbol>(`; the parse could not, because `syn` does
    /// not descend into a macro's token stream. The macro token scan restores
    /// that coverage for the CANONICAL sequence — the spelling the byte count
    /// saw — in a `macro_rules!` definition and in any macro invocation. An
    /// aliased mutator inside a macro stays a named residue.
    #[test]
    fn the_parsed_pin_counts_a_mutator_inside_a_macro_token_stream() {
        for (label, case, symbol) in [
            (
                "macro_rules body",
                "macro_rules! wipe { ($p:expr) => { std::fs::remove_file($p) } }",
                "remove_file",
            ),
            (
                "macro invocation",
                "fn f() { let _ = vec![std::fs::remove_dir_all(p)]; }",
                "remove_dir_all",
            ),
            (
                "raw symbol in macro",
                "macro_rules! wipe { ($p:expr) => { std::fs::r#rename($p, $p) } }",
                "rename",
            ),
            // CONTROL: the same call as ordinary code is still counted once.
            (
                "direct call",
                "fn f(p: &Path) { let _ = std::fs::remove_file(p); }",
                "remove_file",
            ),
        ] {
            let files = vec![("src/prod/count.rs".to_string(), case.to_string())];
            let (_, counts) = std_fs_audit(&files, &BTreeSet::new());
            assert_eq!(
                counts
                    .get(&("src/prod/count.rs".to_string(), symbol))
                    .copied()
                    .unwrap_or(0),
                1,
                "{label} was not counted exactly once: {case} -> {counts:?}"
            );
        }
        // NEGATIVE / residue: a DIFFERENT symbol and an aliased mutator inside
        // a macro are not counted.
        for case in [
            "macro_rules! m { ($p:expr) => { std::fs::remove_file_extra($p) } }",
            "use std::fs as fx; macro_rules! m { ($p:expr) => { fx::remove_file($p) } }",
        ] {
            let files = vec![("src/prod/count.rs".to_string(), case.to_string())];
            let (_, counts) = std_fs_audit(&files, &BTreeSet::new());
            assert!(
                counts.is_empty(),
                "the residue `{case}` must not be counted: {counts:?}"
            );
        }
    }

    // ---------------------------------------------------------------------
    // CROSS-ARTIFACT CONSISTENCY: the funnel may not adopt a `std::fs`/`libc`
    // call that `clippy.toml` does not deny, or the SAME call would be legal
    // outside the funnel and the completeness device would have a hole.
    // ---------------------------------------------------------------------

    /// The `std::fs` / `libc` call targets the FUNNEL regions use that are NOT
    /// on `clippy.toml`'s deny list, each reviewed as unable to ADOPT a name
    /// the funnel's reserved-spelling guard must refuse, and therefore
    /// deliberately left to the crate-root deny's blind side.
    ///
    /// This list is the READ-ONLY / DESCRIPTOR-BOUND side of the PARTITION that
    /// closes the class. [`every_mutation_symbol_the_funnel_uses_is_denied_crate_wide`]
    /// derives the funnel's whole resolved call surface and insists each symbol
    /// is HERE or DENIED; a symbol in NEITHER is a new funnel primitive that
    /// would escape the crate-wide deny, and the test fails naming it. That is
    /// the mechanical form of "a new funnel primitive is impossible to use
    /// without also being denied outside the funnel": a new symbol must pass
    /// through one of the two review doors — `clippy.toml` (name-mutating) or
    /// this list (cannot adopt a reserved name) — because the test refuses the
    /// third state.
    ///
    /// The entries are DESCRIPTOR-BOUND or NAME-PRESERVING calls:
    /// `std::fs::read`/`read_to_string`/`write`/`metadata`/`symlink_metadata`/
    /// `read_dir`/`read_link`/`canonicalize` act on an entry the funnel already
    /// created or resolved; the `libc` entries (`fstatat`, `fstat`, `readlinkat`,
    /// `readdir`, `fdopendir`, `closedir`, `fcntl`) read or manage an open
    /// descriptor. ONE named nuance: `std::fs::write` on a path that does not
    /// exist DOES adopt a name, and every funnel use targets an already-open
    /// descriptor or a path the funnel itself just created, so the crate
    /// classifies it as inode-preserving; the residual is NAMED here rather than
    /// silently exempted.
    ///
    /// `libc::openat` is deliberately NOT here: it CAN adopt a name (with
    /// `O_CREAT`), so it is on clippy.toml's deny list with the other open
    /// spellings. Every other `libc` entry here is read-only.
    const FUNNEL_SYMBOLS_NOT_DENIED: &[&str] = &[
        "std::fs::read",
        "std::fs::read_to_string",
        "std::fs::write",
        "std::fs::metadata",
        "std::fs::symlink_metadata",
        "std::fs::read_dir",
        "std::fs::read_link",
        "std::fs::canonicalize",
        "libc::fstatat",
        "libc::fstat",
        "libc::readlinkat",
        "libc::readdir",
        "libc::fdopendir",
        "libc::closedir",
        "libc::fcntl",
    ];

    /// The canonical `std`/`libc` FUNCTION path a resolved path names, if it is
    /// a module function this audit tracks: `std::fs::<fn>` (three segments),
    /// a `std::os::{unix,windows}::fs::<symlink*>`, or `libc::<fn>`. An
    /// inherent-type path (`std::fs::File::open`) is NOT a module function and
    /// is excluded.
    fn funnel_symbol(canonical: &[String]) -> Option<String> {
        if canonical.len() == 3 && canonical[0] == "std" && canonical[1] == "fs" {
            return Some(format!("std::fs::{}", canonical[2]));
        }
        if canonical.len() == 5
            && canonical[0] == "std"
            && canonical[1] == "os"
            && (canonical[2] == "unix" || canonical[2] == "windows")
            && canonical[3] == "fs"
        {
            return Some(canonical.join("::"));
        }
        if canonical.len() == 2 && canonical[0] == "libc" {
            return Some(canonical.join("::"));
        }
        None
    }

    /// The `std`/`libc` function paths a macro token stream names as a
    /// canonical `::`-joined path — `std::fs::<fn>`, `libc::<fn>`, and the
    /// `std::os::…::fs::symlink*` creators. The aliased-in-a-macro form stays
    /// the audit's named residue.
    fn macro_call_symbols(tokens: impl std::fmt::Display) -> Vec<String> {
        let list = macro_token_list(tokens);
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < list.len() {
            match token_path_at(&list, i) {
                Some((segments, next)) if next > i => {
                    if let Some(symbol) = funnel_symbol(&segments) {
                        out.push(symbol);
                    }
                    i = next;
                }
                _ => i += 1,
            }
        }
        out
    }

    /// The resolved `std::fs` / `libc` call targets of every FUNNEL region: a
    /// module-level `#![allow(clippy::disallowed_methods)]` file, or any item
    /// carrying the `#[allow(...)]`, together with everything nested inside
    /// them. Resolution uses the SAME alias table the `std::fs` count pin uses,
    /// so an imported or re-exported funnel call is the call too.
    struct FunnelSymbols<'a> {
        index: &'a FsIndex,
        module: CanonPath,
        in_funnel: bool,
        used: BTreeSet<String>,
    }

    impl FunnelSymbols<'_> {
        fn record(&mut self, segments: &[String]) {
            let canonical = self.index.resolve_path(&self.module, segments);
            if let Some(symbol) = funnel_symbol(&canonical) {
                self.used.insert(symbol);
            }
        }
    }

    impl<'ast> syn::visit::Visit<'ast> for FunnelSymbols<'_> {
        fn visit_file(&mut self, file: &'ast syn::File) {
            if attrs_allow_disallowed(&file.attrs) {
                self.in_funnel = true;
            }
            for item in &file.items {
                self.visit_item(item);
            }
        }

        fn visit_item(&mut self, item: &'ast syn::Item) {
            let saved = self.in_funnel;
            if item_allows_disallowed(item) {
                self.in_funnel = true;
            }
            if let syn::Item::Mod(module) = item
                && let Some((_, items)) = &module.content
            {
                let mut child = self.module.clone();
                child.push(unraw(&module.ident));
                let saved_module = std::mem::replace(&mut self.module, child);
                for inner in items {
                    self.visit_item(inner);
                }
                self.module = saved_module;
                self.in_funnel = saved;
                return;
            }
            syn::visit::visit_item(self, item);
            self.in_funnel = saved;
        }

        fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
            if self.in_funnel
                && let Some(path) = callee_path(&call.func)
            {
                let segments = path_segments(path);
                self.record(&segments);
            }
            syn::visit::visit_expr_call(self, call);
        }

        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            if self.in_funnel {
                for symbol in macro_call_symbols(&mac.tokens) {
                    self.used.insert(symbol);
                }
            }
            syn::visit::visit_macro(self, mac);
        }
    }

    /// The union of every funnel region's resolved `std::fs`/`libc` call
    /// targets, derived from the parsed production sources.
    fn funnel_symbol_surface(
        files: &[(String, String)],
        gated: &BTreeSet<String>,
    ) -> BTreeSet<String> {
        let parsed = parse_crate(files, gated);
        let index = build_index(&parsed);
        let mut used = BTreeSet::new();
        for source in &parsed {
            if is_test_only(&source.rel, gated) {
                continue;
            }
            let mut visitor = FunnelSymbols {
                index: &index,
                module: source.module.clone(),
                in_funnel: false,
                used: BTreeSet::new(),
            };
            syn::visit::Visit::visit_file(&mut visitor, &source.file);
            used.extend(visitor.used);
        }
        used
    }

    /// The `path = "…"` entries of `clippy.toml`'s `disallowed-methods`, read
    /// MECHANICALLY so the consistency test compares the file the COMPILER
    /// reads with the funnel's resolved surface, not a hand-kept copy.
    fn denied_symbols_from_clippy_toml() -> BTreeSet<String> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("clippy.toml");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let mut out = BTreeSet::new();
        for line in text.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("{ path = \"") else {
                continue;
            };
            let Some(end) = rest.find('"') else { continue };
            out.insert(rest[..end].to_string());
        }
        assert!(
            out.len() > 15,
            "the clippy.toml deny list must parse to the real list: {out:?}"
        );
        out
    }

    /// THE CLASS CLOSURE. The completeness device is the resolved-symbol deny
    /// in `clippy.toml`, but INSIDE a funnel region that deny is blind. So the
    /// funnel is allowed to use a `std::fs`/`libc` call only if that call is
    /// ALSO denied crate-wide (it is name-mutating and must stay funnel-owned)
    /// or is on the reviewed [`FUNNEL_SYMBOLS_NOT_DENIED`] list. Both sides are
    /// derived mechanically: the deny list from `clippy.toml`, the used side
    /// from the funnel's resolved symbol surface. Removing a symbol from
    /// `clippy.toml` while the funnel still uses it fails this test, naming the
    /// now-undefended symbol — which is exactly how the two artifacts stay from
    /// drifting.
    #[test]
    fn every_mutation_symbol_the_funnel_uses_is_denied_crate_wide() {
        let mut paths = Vec::new();
        collect_crate_rs_files(Path::new(env!("CARGO_MANIFEST_DIR")), &mut paths);
        let sources: Vec<(String, String)> = paths
            .iter()
            .map(|file| {
                (
                    crate_relative(file),
                    std::fs::read_to_string(file).expect("read source file"),
                )
            })
            .collect();
        let gated = test_only_gated_paths();
        let used = funnel_symbol_surface(&sources, &gated);
        let denied = denied_symbols_from_clippy_toml();

        // SANITY: the derivation must see the funnel, or an empty `used` set
        // would make the closure vacuous. `create_dir_all` is a creation
        // wrapper added to the deny list in this change; `openat` is the
        // funnel's canonical open.
        assert!(
            used.contains("std::fs::create_dir_all") && used.contains("libc::openat"),
            "the funnel symbol derivation must see the funnel's own creation and open calls, or \
             the closure is vacuous: {used:?}"
        );

        let mut undefended: Vec<String> = Vec::new();
        for symbol in &used {
            if denied.contains(symbol) || FUNNEL_SYMBOLS_NOT_DENIED.contains(&symbol.as_str()) {
                continue;
            }
            undefended.push(symbol.clone());
        }
        assert!(
            undefended.is_empty(),
            "the FUNNEL uses `std::fs`/`libc` calls that clippy.toml does NOT deny, so a caller \
             could issue the SAME call OUTSIDE the funnel with no lint error — the completeness \
             device would have a hole: {undefended:?}. Add each symbol to clippy.toml's \
             `disallowed-methods` (name-mutating or mode-setting calls) or to \
             FUNNEL_SYMBOLS_NOT_DENIED (reviewed read-only / descriptor-bound calls), so the \
             deny and the funnel cannot drift."
        );

        // The review list must not go stale in either direction.
        for symbol in FUNNEL_SYMBOLS_NOT_DENIED {
            assert!(
                used.contains(*symbol),
                "FUNNEL_SYMBOLS_NOT_DENIED names {symbol}, which the funnel no longer uses; \
                 remove it"
            );
            assert!(
                !denied.contains(*symbol),
                "FUNNEL_SYMBOLS_NOT_DENIED names {symbol}, which clippy.toml now denies; remove it \
                 from the review list"
            );
        }
    }
}
