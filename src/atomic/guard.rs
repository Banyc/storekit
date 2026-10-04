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
//! content mutation on an ALREADY-EXISTING path through a call that stays legal
//! outside the funnel, which cannot split a holder because the flock is
//! attached to the unchanged inode. The mode setter
//! (`std::fs::set_permissions`), the name CREATORS (`std::fs::create_dir*`, the
//! platform `symlink` wrappers), and `std::fs::write` — whose create-on-absent
//! path ADOPTS a name — are NO LONGER in this residue: all are on
//! `clippy.toml`'s deny list, counted by the `std::fs` pin where applicable,
//! and covered by the cross-artifact consistency test, so an adopt-a-name or
//! re-mode call outside the funnel fails the lint. The only `std::fs::write`
//! left in production is the funnel's own `write_file_fd`, which calls
//! [`refuse_reserved_mutation`] first and preserves the inode of an EXISTING
//! entry. The reach of
//! a raw FFI declaration is one declaration the crate's
//! own author writes — exactly the residue a source audit carries and names
//! rather than denies. A foreign process, or a raw `std::fs` call the caller
//! writes itself, is outside the crate entirely and is not stopped by any of
//! this.
//!
//! TWO name-ADOPTING forms joined the deny list in round 8 and are NOT residue:
//! `std::os::{unix,windows}::fs::OpenOptionsExt::custom_flags` (on Unix it
//! forwards ARBITRARY `open(2)` bits and `O_CREAT` through it creates a name;
//! on Windows it forwards `dwFlagsAndAttributes`, where the creation
//! disposition is a separate argument, so the Windows form is denied for parity
//! rather than adoption) and `std::os::unix::net::{UnixListener,UnixDatagram}::bind`
//! (a pathname-socket bind creates a directory entry). One NAMED RESIDUAL remains,
//! stated rather than implied: the deny list names RESOLVED `std`/`libc`
//! symbols, so a Windows named-pipe creator (`CreateNamedPipeW`) or a raw
//! `CreateFileW`/`NtCreateFile` reached through `windows_sys` is outside the
//! clippy deny (and the `libc` belt is libc-specific). Rust's stable `std`
//! exposes no named-pipe creator, and the crate's own `windows_sys` uses today
//! are non-adopting (`GetFileInformationByHandle`, `LockFileEx`/`UnlockFileEx`),
//! so the reach is "a raw Win32 creator the crate would have to add" — the same
//! reach as the raw `extern "C"` declaration above. The residual is repeated at
//! the deny list in `clippy.toml` so the completeness claim and the lint agree
//! about what they do not cover.

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

// NON-ADOPTING `custom_flags` SITE, inside the FUNNEL: the flags are
// `FILE_FLAG_OPEN_REPARSE_POINT` on a READ-ONLY open (`read(true)` +
// `share_mode`, no `create`/`create_new`), so no name is created and no
// creation flag is among the forwarded bits. This file is a CHILD of
// `atomic/mod.rs`, whose module-level `#![allow(clippy::disallowed_methods)]`
// already covers every item here, so no item-level attribute is needed (and
// adding one would be dead weight); a move of this function OUT of the module
// would leave it correctly un-allowed.
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
/// THREE [`GuardedRel`] minting constructors ([`GuardedRel::new`],
/// [`GuardedRel::new_for_owned_lock_record`], [`GuardedRel::new_for_residue`]),
/// so a caller cannot consult the guard
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
/// only way to obtain a value is one of the THREE minting constructors (each runs the
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

    /// The curated NAME-MUTATION family of `libc` syscalls: functions that can
    /// ADOPT a directory entry, FREE or SWAP one, or change the mode /
    /// ownership / xattrs / timestamps attached to a NAME. The `*at` forms, the
    /// plain forms, and the descriptor-bound setters are all present.
    ///
    /// This list is the FAMILY half of the belt; it is NOT the belt. The belt
    /// [`no_libc_reference_outside_the_funnel`] refuses is DERIVED by
    /// [`mutating_libc_belt`], which unions this family with every `libc::<fn>`
    /// CALL the tree references that is not on the reviewed
    /// [`NON_MUTATING_LIBC_CALLS`] list. The family catches a symbol the tree
    /// does not itself reference (a reviewer pinning `libc::mkfifo`,
    /// `libc::mknod`, or `libc::renameat2` in the outside map); the derived
    /// clause catches an unclassified NEW reference. The two are checked
    /// against each other by
    /// `every_libc_call_symbol_the_tree_references_is_classified`, so neither
    /// the family nor the tree's references can silently lag the other.
    ///
    /// ROUND 11: the family is the belt's clause (a), and it is now a SUPERSET
    /// of the independently-specified anchor [`INDEPENDENT_KNOWN_NAME_MUTATORS`]
    /// — the set the belt oracle actually iterates. Clause (a) is the ONLY
    /// device that refuses a symbol the tree does not reference, so an anchor
    /// member absent here would be pinnable outside the funnel with no device
    /// noticing; the oracle fails on exactly that. The family is also LARGER
    /// than the hand list round 10 shipped: the POSIX IPC name API, the
    /// template temp-name creators, the missing time setters, `fchmodat2` and
    /// the ACL setters join it, so a pinned reference to any of them is refused
    /// too.
    const MUTATING_LIBC_SYSCALLS: &[&str] = &[
        // REMOVE / REPLACE / LINK
        "unlink",
        "unlinkat",
        "remove",
        "rmdir",
        "rename",
        "renameat",
        "renameat2",
        "link",
        "linkat",
        "symlink",
        "symlinkat",
        // REMOVE a POSIX IPC name (message queue / semaphore / shared memory).
        "mq_unlink",
        "sem_unlink",
        "shm_unlink",
        // CREATE a name
        "mkdir",
        "mkdirat",
        "open",
        "openat",
        "open64",
        "openat64",
        "openat2",
        "creat",
        "creat64",
        "mknod",
        "mknodat",
        "mkfifo",
        "mkfifoat",
        "clonefile",
        "clonefileat",
        // CREATE a POSIX IPC name.
        "mq_open",
        "sem_open",
        "shm_open",
        // CREATE a name through a template (`mkstemp`/`mkdtemp` and friends
        // both choose a spelling and ADOPT it).
        "mkstemp",
        "mkostemp",
        "mkstemps",
        "mkostemps",
        "mkdtemp",
        // `bind` on an `AF_UNIX` pathname socket CREATES a directory entry at
        // the bound path (the raw-syscall twin of
        // `std::os::unix::net::UnixListener::bind`, which `clippy.toml` denies).
        // For an `AF_INET` bind no name is created, so the belt's refusal
        // outside the funnel is CONSERVATIVE (it refuses the symbol, not the
        // address family) — the safe direction, and the crate references no
        // `libc::bind` today.
        "bind",
        // NAME-ATTACHED METADATA
        "chmod",
        "fchmod",
        "fchmodat",
        "fchmodat2",
        "chown",
        "fchown",
        "lchown",
        "fchownat",
        "truncate",
        "ftruncate",
        "utime",
        "utimes",
        "futimens",
        "utimensat",
        "futimes",
        "futimesat",
        "lutimes",
        "setxattr",
        "lsetxattr",
        "fsetxattr",
        "removexattr",
        "lremovexattr",
        "fremovexattr",
        "chflags",
        "fchflags",
        "lchflags",
        "setattrlist",
        "exchangedata",
        "acl_set_file",
        "acl_set_link_np",
        // FILESYSTEM MOUNTS
        "mount",
        "umount",
        "umount2",
    ];

    /// The NAME-MUTATION family as a LITERAL, kept in step with the belt's
    /// clause (a) by set equality. It is deliberately NOT derived from
    /// [`MUTATING_LIBC_SYSCALLS`] or [`NON_MUTATING_LIBC_CALLS`], so a MOVE of a
    /// member out of the code family into the review list changes this side of
    /// the equality.
    ///
    /// THIS LITERAL IS ONE HALF OF A CO-EDITABLE PAIR and is therefore NOT the
    /// oracle. Round 10 shipped this list and [`INDEPENDENT_KNOWN_NAME_MUTATORS`]
    /// as two hand-written literals in this file and compared them to each
    /// other, so deleting `chown` from both and adding it to
    /// [`NON_MUTATING_LIBC_CALLS`] disarmed the belt with every arm green
    /// (measured; 52 of the 54 members could be moved the same way). The anchor
    /// [`INDEPENDENT_KNOWN_NAME_MUTATORS`] is the THIRD artifact the oracle
    /// iterates, and co-editing this pair against it now fails.
    ///
    /// The justification is the shared CLASS, and every member's individual
    /// class and reason live in [`INDEPENDENT_KNOWN_NAME_MUTATORS`]: a member
    /// can FREE or SWAP a directory entry, ADOPT/CREATE a name, create or
    /// remove a POSIX IPC name, choose-and-adopt a temp name, or change
    /// metadata attached to a NAME (mode/owner/time/xattr/flags/ACL), or change
    /// the mount NAME SPACE. The descriptor-bound variants
    /// (`fchmod`/`fchown`/`ftruncate`/`futimens`) are included because the code
    /// family includes them; reclassifying one demands the change be stated in
    /// the anchor with a new class and reason.
    const KNOWN_NAME_MUTATORS: &[&str] = &[
        // FREE or SWAP a directory entry.
        "unlink",
        "unlinkat",
        "remove",
        "rmdir",
        "rename",
        "renameat",
        "renameat2",
        // FREE a POSIX IPC name.
        "mq_unlink",
        "sem_unlink",
        "shm_unlink",
        // CREATE or replace a LINK at a name.
        "link",
        "linkat",
        "symlink",
        "symlinkat",
        // CREATE a directory entry (directory, device node, FIFO, clone).
        "mkdir",
        "mkdirat",
        "mknod",
        "mknodat",
        "mkfifo",
        "mkfifoat",
        "clonefile",
        "clonefileat",
        // CREATE a POSIX IPC name.
        "mq_open",
        "sem_open",
        "shm_open",
        // CREATE a name through a template.
        "mkstemp",
        "mkostemp",
        "mkstemps",
        "mkostemps",
        "mkdtemp",
        // ADOPT or TRUNCATE a name through an open with O_CREAT/O_TRUNC.
        "open",
        "openat",
        "open64",
        "openat64",
        "openat2",
        "creat",
        "creat64",
        // An AF_UNIX `bind` creates the bound pathname entry.
        "bind",
        // Change metadata attached to a NAME.
        "chmod",
        "fchmod",
        "fchmodat",
        "fchmodat2",
        "chown",
        "fchown",
        "lchown",
        "fchownat",
        "truncate",
        "ftruncate",
        "utime",
        "utimes",
        "futimens",
        "utimensat",
        "futimes",
        "futimesat",
        "lutimes",
        "setxattr",
        "lsetxattr",
        "fsetxattr",
        "removexattr",
        "lremovexattr",
        "fremovexattr",
        "chflags",
        "fchflags",
        "lchflags",
        "setattrlist",
        "exchangedata",
        "acl_set_file",
        "acl_set_link_np",
        // Change the mount NAME SPACE.
        "mount",
        "umount",
        "umount2",
    ];

    /// The CLOSED set of name-mutation CLASSES an anchor entry may claim. The
    /// anchor's class field is checked against this list, so a new member cannot
    /// be admitted under an ad-hoc class string invented at the point of edit;
    /// adding a class is a visible review of the CLASS axis, not just of one
    /// symbol.
    const KNOWN_NAME_MUTATOR_CLASSES: &[&str] = &[
        "free-or-swap-a-directory-entry",
        "link-at-a-name",
        "create-a-directory-entry",
        "adopt-or-truncate-through-an-open",
        "bind-a-pathname-socket",
        "create-or-remove-a-posix-ipc-name",
        "create-a-name-through-a-temp-template",
        "change-metadata-attached-to-a-name",
        "change-the-mount-name-space",
    ];

    /// THE INDEPENDENTLY-SPECIFIED ANCHOR, and the per-symbol class + reason
    /// that justifies membership. This is the artifact the belt oracle
    /// ITERATES; it is not compared to another list of its own kind. Every
    /// member must (1) be on [`MUTATING_LIBC_SYSCALLS`], because clause (a) of
    /// the derived belt is the only device that refuses a symbol the tree does
    /// not reference, (2) be ABSENT from [`NON_MUTATING_LIBC_CALLS`], and (3)
    /// be REFUSED by the DEFAULT-DENY half of the belt when a source references
    /// it, so the anchor is not circular through the family. The set is
    /// strictly LARGER than the 54-member family round 10 shipped: `lutimes`,
    /// `futimes`/`futimesat`, `fchmodat2`, the POSIX IPC names
    /// (`mq_open`/`sem_open`/`shm_open` and their `*_unlink` twins), the
    /// template temp-name creators (`mkstemp`/`mkostemp`/`mkstemps`/
    /// `mkostemps`/`mkdtemp`), and the ACL setters were in NEITHER hand list,
    /// and they are named in the oracle as negative controls so the anchor
    /// cannot be silently narrowed back.
    ///
    /// THE ANCHOR'S OWN BOUNDARY — IT IS A LIST TOO. It covers the
    /// POSIX.1-2017, Linux, and BSD/macOS `libc` name-mutation surface: the
    /// remove/replace/link entry calls, the entry creators (directory, node,
    /// FIFO, clone, template temp names), the adopt/truncate open family, the
    /// pathname-socket bind, the POSIX IPC name API, the name-attached metadata
    /// setters (mode/owner/time/xattr/flags/ACL), and the mount name space. It
    /// does NOT cover, and a mutator OUTSIDE it is a STATED RESIDUAL: (a) a raw
    /// syscall reached by NUMBER or by a local `extern "C"` declaration
    /// (`syscall(SYS_…)`, an `io_uring` submission, a `windows_sys` creator),
    /// which has no `libc::<name>` symbol for this anchor to key on — those are
    /// refused, not enumerated, by the `libc`-reference surface pin in
    /// [`no_libc_reference_outside_the_funnel`] and by the raw-`extern "C"`
    /// residue named in this module's docs; and (b) a `libc` name this anchor
    /// does not list, which is admitted only by ADDING it here with a class and
    /// a reason — the act that reviews it.
    const INDEPENDENT_KNOWN_NAME_MUTATORS: &[(&str, &str, &str)] = &[
        // free-or-swap-a-directory-entry
        (
            "unlink",
            "free-or-swap-a-directory-entry",
            "removes one name from its parent directory",
        ),
        (
            "unlinkat",
            "free-or-swap-a-directory-entry",
            "the dirfd-relative twin of unlink",
        ),
        (
            "remove",
            "free-or-swap-a-directory-entry",
            "removes a name (a file or an empty directory)",
        ),
        (
            "rmdir",
            "free-or-swap-a-directory-entry",
            "removes an empty directory name",
        ),
        (
            "rename",
            "free-or-swap-a-directory-entry",
            "re-links the source name and frees the target name",
        ),
        (
            "renameat",
            "free-or-swap-a-directory-entry",
            "the dirfd-relative twin of rename",
        ),
        (
            "renameat2",
            "free-or-swap-a-directory-entry",
            "rename with flags; can also swap two names",
        ),
        // link-at-a-name
        (
            "link",
            "link-at-a-name",
            "creates a second name for one inode",
        ),
        (
            "linkat",
            "link-at-a-name",
            "the dirfd-relative twin of link",
        ),
        ("symlink", "link-at-a-name", "creates a symlink name"),
        (
            "symlinkat",
            "link-at-a-name",
            "the dirfd-relative twin of symlink",
        ),
        // create-a-directory-entry
        (
            "mkdir",
            "create-a-directory-entry",
            "creates a directory name",
        ),
        (
            "mkdirat",
            "create-a-directory-entry",
            "the dirfd-relative twin of mkdir",
        ),
        (
            "mknod",
            "create-a-directory-entry",
            "creates a device, socket, or FIFO name",
        ),
        (
            "mknodat",
            "create-a-directory-entry",
            "the dirfd-relative twin of mknod",
        ),
        ("mkfifo", "create-a-directory-entry", "creates a FIFO name"),
        (
            "mkfifoat",
            "create-a-directory-entry",
            "the dirfd-relative twin of mkfifo",
        ),
        (
            "clonefile",
            "create-a-directory-entry",
            "creates a new name as a copy-on-write clone",
        ),
        (
            "clonefileat",
            "create-a-directory-entry",
            "the dirfd-relative twin of clonefile",
        ),
        // adopt-or-truncate-through-an-open
        (
            "open",
            "adopt-or-truncate-through-an-open",
            "adopts or truncates a name with O_CREAT/O_TRUNC",
        ),
        (
            "openat",
            "adopt-or-truncate-through-an-open",
            "the dirfd-relative twin of open",
        ),
        (
            "open64",
            "adopt-or-truncate-through-an-open",
            "the large-file-offset spelling of open",
        ),
        (
            "openat64",
            "adopt-or-truncate-through-an-open",
            "the large-file-offset spelling of openat",
        ),
        (
            "openat2",
            "adopt-or-truncate-through-an-open",
            "openat with an extensible how-struct; can adopt a name",
        ),
        (
            "creat",
            "adopt-or-truncate-through-an-open",
            "creates or truncates a name",
        ),
        (
            "creat64",
            "adopt-or-truncate-through-an-open",
            "the large-file-offset spelling of creat",
        ),
        // bind-a-pathname-socket
        (
            "bind",
            "bind-a-pathname-socket",
            "an AF_UNIX bind creates the bound pathname entry",
        ),
        // create-or-remove-a-posix-ipc-name
        (
            "mq_open",
            "create-or-remove-a-posix-ipc-name",
            "creates or opens a POSIX message-queue name",
        ),
        (
            "mq_unlink",
            "create-or-remove-a-posix-ipc-name",
            "removes a POSIX message-queue name",
        ),
        (
            "sem_open",
            "create-or-remove-a-posix-ipc-name",
            "creates or opens a POSIX named-semaphore name",
        ),
        (
            "sem_unlink",
            "create-or-remove-a-posix-ipc-name",
            "removes a POSIX named-semaphore name",
        ),
        (
            "shm_open",
            "create-or-remove-a-posix-ipc-name",
            "creates or opens a POSIX shared-memory name",
        ),
        (
            "shm_unlink",
            "create-or-remove-a-posix-ipc-name",
            "removes a POSIX shared-memory name",
        ),
        // create-a-name-through-a-temp-template
        (
            "mkstemp",
            "create-a-name-through-a-temp-template",
            "chooses a spelling and creates the file name",
        ),
        (
            "mkostemp",
            "create-a-name-through-a-temp-template",
            "mkstemp with extra open flags",
        ),
        (
            "mkstemps",
            "create-a-name-through-a-temp-template",
            "mkstemp with a suffix",
        ),
        (
            "mkostemps",
            "create-a-name-through-a-temp-template",
            "mkostemp with a suffix",
        ),
        (
            "mkdtemp",
            "create-a-name-through-a-temp-template",
            "chooses a spelling and creates the directory name",
        ),
        // change-metadata-attached-to-a-name
        (
            "chmod",
            "change-metadata-attached-to-a-name",
            "sets a mode on a name",
        ),
        (
            "fchmod",
            "change-metadata-attached-to-a-name",
            "sets a mode on a descriptor's open name",
        ),
        (
            "fchmodat",
            "change-metadata-attached-to-a-name",
            "sets a mode on a name relative to a dirfd",
        ),
        (
            "fchmodat2",
            "change-metadata-attached-to-a-name",
            "fchmodat with flags; can act on a symlink's own name",
        ),
        (
            "chown",
            "change-metadata-attached-to-a-name",
            "sets an owner on a name",
        ),
        (
            "fchown",
            "change-metadata-attached-to-a-name",
            "sets an owner on a descriptor's open name",
        ),
        (
            "lchown",
            "change-metadata-attached-to-a-name",
            "sets an owner on a symlink's own name",
        ),
        (
            "fchownat",
            "change-metadata-attached-to-a-name",
            "sets an owner relative to a dirfd, with symlink flags",
        ),
        (
            "truncate",
            "change-metadata-attached-to-a-name",
            "changes a named file's length",
        ),
        (
            "ftruncate",
            "change-metadata-attached-to-a-name",
            "changes a descriptor's open file length",
        ),
        (
            "utime",
            "change-metadata-attached-to-a-name",
            "sets a name's access and modify times",
        ),
        (
            "utimes",
            "change-metadata-attached-to-a-name",
            "sets a name's times with microsecond precision",
        ),
        (
            "futimens",
            "change-metadata-attached-to-a-name",
            "sets a descriptor's open name's times with nanosecond precision",
        ),
        (
            "utimensat",
            "change-metadata-attached-to-a-name",
            "sets a name's times relative to a dirfd",
        ),
        (
            "futimes",
            "change-metadata-attached-to-a-name",
            "sets a descriptor's open file's times",
        ),
        (
            "futimesat",
            "change-metadata-attached-to-a-name",
            "sets a name's times relative to a dirfd",
        ),
        (
            "lutimes",
            "change-metadata-attached-to-a-name",
            "sets a symlink's own times",
        ),
        (
            "setxattr",
            "change-metadata-attached-to-a-name",
            "sets an extended attribute on a name",
        ),
        (
            "lsetxattr",
            "change-metadata-attached-to-a-name",
            "sets an extended attribute on a symlink's own name",
        ),
        (
            "fsetxattr",
            "change-metadata-attached-to-a-name",
            "sets an extended attribute on a descriptor's open name",
        ),
        (
            "removexattr",
            "change-metadata-attached-to-a-name",
            "removes an extended attribute from a name",
        ),
        (
            "lremovexattr",
            "change-metadata-attached-to-a-name",
            "removes an extended attribute from a symlink's own name",
        ),
        (
            "fremovexattr",
            "change-metadata-attached-to-a-name",
            "removes an extended attribute from a descriptor's open name",
        ),
        (
            "chflags",
            "change-metadata-attached-to-a-name",
            "changes a name's file flags",
        ),
        (
            "fchflags",
            "change-metadata-attached-to-a-name",
            "changes a descriptor's open name's file flags",
        ),
        (
            "lchflags",
            "change-metadata-attached-to-a-name",
            "changes a symlink's own name's file flags",
        ),
        (
            "setattrlist",
            "change-metadata-attached-to-a-name",
            "sets name-attached attributes in bulk (macOS)",
        ),
        (
            "exchangedata",
            "change-metadata-attached-to-a-name",
            "swaps the contents of two names (macOS)",
        ),
        (
            "acl_set_file",
            "change-metadata-attached-to-a-name",
            "sets an ACL on a name",
        ),
        (
            "acl_set_link_np",
            "change-metadata-attached-to-a-name",
            "sets an ACL on a symlink's own name",
        ),
        // change-the-mount-name-space
        (
            "mount",
            "change-the-mount-name-space",
            "attaches a filesystem at a name",
        ),
        (
            "umount",
            "change-the-mount-name-space",
            "detaches the filesystem at a name",
        ),
        (
            "umount2",
            "change-the-mount-name-space",
            "the flag-taking twin of umount",
        ),
    ];

    /// The `libc::<fn>` CALL symbols the tree references that are REVIEWED as
    /// unable to ADOPT, FREE, SWAP, or re-attribute a NAME: they act on an open
    /// DESCRIPTOR, on the PROCESS, on a RESOURCE LIMIT, or only READ a name.
    /// [`every_libc_call_symbol_the_tree_references_is_classified`] derives the
    /// tree's `libc::<fn>(…)` call symbols and fails on any that is in neither
    /// this list nor [`MUTATING_LIBC_SYSCALLS`], so the classification cannot
    /// silently lag a new syscall reference.
    const NON_MUTATING_LIBC_CALLS: &[&str] = &[
        // READ side of a name
        "fstatat",
        "fstat",
        "readlinkat",
        "getxattr",
        // Directory-descriptor reader
        "readdir",
        "fdopendir",
        "closedir",
        // Descriptor management / content write on an OPEN descriptor
        "fcntl",
        "flock",
        "close",
        "write",
        "poll",
        // Process, limit, and system control
        "kill",
        "killpg",
        "signal",
        "waitid",
        "pipe",
        "sysctl",
        "getrlimit",
        "setrlimit",
        "getegid",
        "getgroups",
    ];

    /// The namespace-relative KEY the pin uses for a resolved CALL target, and
    /// the membership test that decides whether the pin's syntactic resolver is
    /// allowed to attribute a call to a `clippy.toml` entry at all.
    ///
    /// The namespaces are exactly the ones the resolver produces:
    ///
    /// * `std::fs::…` — the free functions (`std::fs::remove_file`), the
    ///   inherent ASSOCIATED functions (`std::fs::File::create`), and the
    ///   BUILDER methods the method arm attributes to an
    ///   `OpenOptions`/`DirBuilder` owner (`std::fs::OpenOptions::create`);
    ///   the key is the path after `std::fs::`;
    /// * `std::os::{unix,windows}::fs::symlink*` — the platform symlink
    ///   creators, whose key is the bare symbol (`symlink`, `symlink_file`,
    ///   `symlink_dir`);
    /// * `std::os::{unix,windows}::net::{UnixListener,UnixDatagram}::bind` —
    ///   the pathname-socket binds, whose key is `UnixListener::bind` /
    ///   `UnixDatagram::bind`.
    ///
    /// An entry OUTSIDE these namespaces is not attributable by the syntactic
    /// resolver and is deliberately EXCLUDED from the pin. `libc::…` is the one
    /// such family, and it has its OWN device — the per-module surface pin
    /// [`no_libc_reference_outside_the_funnel`] plus the derived belt — so a
    /// `libc` entry is not double-counted here.
    fn pin_key_for(segments: &[String]) -> Option<String> {
        let skip = match segments {
            [std, fs, ..] if std == "std" && fs == "fs" => 2,
            [std, os, unix_or_windows, fs, ..]
                if std == "std"
                    && os == "os"
                    && (unix_or_windows == "unix" || unix_or_windows == "windows")
                    && fs == "fs" =>
            {
                match segments.last().map(String::as_str) {
                    Some("symlink" | "symlink_file" | "symlink_dir") => 4,
                    _ => return None,
                }
            }
            [std, os, unix_or_windows, net, kind, bind]
                if std == "std"
                    && os == "os"
                    && (unix_or_windows == "unix" || unix_or_windows == "windows")
                    && net == "net"
                    && (kind == "UnixListener" || kind == "UnixDatagram")
                    && bind == "bind" =>
            {
                4
            }
            _ => return None,
        };
        Some(segments[skip..].join("::"))
    }

    /// Every `std` symbol whose denial the funnel owns, as its CANONICAL path
    /// (the path the crate's own name resolution produces) paired with the
    /// namespace-relative KEY used in the pins. It is one table so the count
    /// pin, the import-route detector, the macro-token scan, and the canonical-
    /// spelling test cannot drift apart:
    ///
    /// * the inode mutators that REMOVE or REPLACE an entry (`remove_file`,
    ///   `remove_dir`, `remove_dir_all`, `rename`, `hard_link`);
    /// * the CREATORS that ADOPT a name (`std::fs::create_dir`,
    ///   `std::fs::create_dir_all`, the platform symlink creators, which live
    ///   under `std::os::…`, and the pathname-socket binds);
    /// * the path-based mode setter `std::fs::set_permissions`;
    /// * the INHERENT/ASSOCIATED and BUILDER spellings of the same adoption
    ///   (`std::fs::File::create`/`create_new`, `std::fs::OpenOptions::create`/
    ///   `create_new`, `std::fs::DirBuilder::create`, `std::fs::copy`,
    ///   `std::fs::write`).
    ///
    /// The creators are here because the guard's reserved-spelling check is
    /// what makes a name-creating call safe: a creation bypasses that check
    /// exactly as a removal does, so `create_dir_fd`/`symlink_fd` refuse the
    /// reserved spellings (`operation.lock`, `.sync-aside.1`) that
    /// `std::fs::create_dir*` and the platform symlink calls would happily
    /// create.
    ///
    /// DERIVED, NOT HAND-KEPT. The set is read out of `clippy.toml`'s
    /// `disallowed-methods` list by [`reconciled_denied_symbols`] — the same
    /// file the compiler reads — through [`pin_key_for`]. A name-mutating entry
    /// added to `clippy.toml` therefore enters the pin AUTOMATICALLY, so the
    /// cross-artifact completeness device and the count pin cannot drift: the
    /// failure mode this replaces was seven adopting symbols the lint denied
    /// but the pin did not count.
    fn name_mutation_symbols() -> &'static BTreeMap<&'static str, Vec<String>> {
        static TABLE: std::sync::OnceLock<BTreeMap<&'static str, Vec<String>>> =
            std::sync::OnceLock::new();
        TABLE.get_or_init(|| {
            let mut table: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
            for path in reconciled_denied_symbols() {
                let segments: Vec<String> = path.split("::").map(str::to_string).collect();
                let Some(key) = pin_key_for(&segments) else {
                    continue;
                };
                let key: &'static str = Box::leak(key.into_boxed_str());
                if let Some(previous) = table.insert(key, segments.clone()) {
                    panic!(
                        "the derived name-mutation table maps {key:?} to BOTH {previous:?} and \
                         {segments:?}; the pin keys must be unique, or a count would merge two \
                         distinct symbols"
                    );
                }
            }
            assert!(
                table.contains_key("remove_file")
                    && table.contains_key("File::create")
                    && table.contains_key("write"),
                "the derived name-mutation table must cover the removal, the inherent-adoption, \
                 and the free-adoption families: {table:?}"
            );
            table
        })
    }

    /// A production route by which one of [`name_mutation_symbols`] can be
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

    /// A local NON-GENERIC `type` alias (`type ZZBuilder =
    /// std::fs::OpenOptions;`) with the path it names and the module it is
    /// declared in. Round 10's resolver keyed the owner on the literal
    /// last-segment name, so a `type` alias of a builder type was invisible to
    /// BOTH the funnel closure and the count pin: inside a funnel module,
    /// `type ZZBuilder = std::fs::OpenOptions;` followed by
    /// `ZZBuilder::new().write(true).create_new(true).open(p)?` left clippy,
    /// `every_mutation_symbol_the_funnel_uses_is_denied_crate_wide` and
    /// `std_fs_name_mutation_counts_are_pinned` all green. A `use … as` alias
    /// WAS resolved; a `type` alias was not.
    struct TypeAlias {
        module: CanonPath,
        local: String,
        target: Vec<String>,
    }

    /// Collect every local non-generic `type X = <path>;` item of one file,
    /// tagged with the module path it is declared in. The walk mirrors
    /// [`UseCollector`], so a BLOCK-LOCAL alias is collected too. A generic
    /// alias (`type X<T> = …`) is skipped deliberately: the resolver
    /// substitutes a path PREFIX, and a generic alias is not usable as a bare
    /// prefix without turbofish (`X::<T>::new`), which this resolver does not
    /// model; the pair-less boundary names the omission.
    struct TypeAliasCollector<'out> {
        module: CanonPath,
        out: &'out mut Vec<TypeAlias>,
    }

    impl<'ast> syn::visit::Visit<'ast> for TypeAliasCollector<'_> {
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

        fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
            if item.generics.params.is_empty()
                && let syn::Type::Path(type_path) = &*item.ty
                && type_path.qself.is_none()
            {
                self.out.push(TypeAlias {
                    module: self.module.clone(),
                    local: unraw(&item.ident),
                    target: path_segments(&type_path.path),
                });
            }
            syn::visit::visit_item_type(self, item);
        }
    }

    /// Every local non-generic `type` alias of one parsed file, with the module
    /// path it is declared in. The fixpoint in [`build_index`] resolves each
    /// alias's target through the same alias table, so `type A = O;` with
    /// `use std::fs::OpenOptions as O;` resolves to the same canonical builder
    /// owner as the direct spelling.
    fn collect_type_aliases(file: &syn::File, base: &[String]) -> Vec<TypeAlias> {
        let mut out = Vec::new();
        let mut collector = TypeAliasCollector {
            module: base.to_vec(),
            out: &mut out,
        };
        syn::visit::Visit::visit_file(&mut collector, file);
        out
    }

    /// The canonical `std` name-mutation symbol `path` names, if any, from
    /// [`name_mutation_symbols`]: `std::fs::remove_file`, `std::fs::create_dir`,
    /// `std::os::unix::fs::symlink`, …, but equally the adopting spellings the
    /// pin used to miss (`std::fs::File::create`, `std::fs::write`). The table
    /// is DERIVED from `clippy.toml`, so the count pin, the route detector, the
    /// canonical-spelling test, and the deny list cannot drift apart.
    fn mutator_symbol(path: &[String]) -> Option<&'static str> {
        name_mutation_symbols().iter().find_map(|(key, canonical)| {
            (canonical.len() == path.len()
                && canonical
                    .iter()
                    .zip(path.iter())
                    .all(|(expected, actual)| expected == actual))
            .then_some(*key)
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
    /// `symbol` (one of [`name_mutation_symbols`]) — `std::fs::remove_file`,
    /// but equally `std::os::unix::fs::symlink` — which the exact-count pin
    /// covers, or reaches the mutator only through an import / alias /
    /// cross-file re-export.
    fn is_canonical_literal(segments: &[String], symbol: &str) -> bool {
        name_mutation_symbols().get(symbol).is_some_and(|path| {
            path.len() == segments.len()
                && segments
                    .iter()
                    .zip(path.iter())
                    .all(|(actual, expected)| actual == expected)
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

    /// The FUNNEL modules: every PRODUCTION file INSIDE a funnel region, i.e.
    /// whose module is allow-bearing or is nested under a module that is. The
    /// allow is a MODULE attribute, so Rust applies it to every item in the
    /// module AND to every child module declared in it, including a
    /// `mod child;` FILE. This is why `src/atomic/guard.rs` — which carries no
    /// attribute of its own — is inside the region `src/atomic/mod.rs`
    /// declares with its inner `#![allow(clippy::disallowed_methods)]`.
    ///
    /// DERIVED from the parsed sources and the parsed `mod` declarations, so
    /// the libc-surface pin in [`no_libc_reference_outside_the_funnel`] follows
    /// the allow's reach instead of a second, hardcoded file list that could
    /// drift from the `#[allow]`s. Item-level allows that do not sit on a
    /// `mod` are covered separately by [`funnel_symbol_surface`], which walks
    /// annotated items too.
    fn funnel_modules(files: &[(String, String)], gated: &BTreeSet<String>) -> BTreeSet<String> {
        let parsed = parse_crate(files, gated);
        let regions = funnel_region_modules(&parsed, gated);
        parsed
            .iter()
            .filter(|source| !is_test_only(&source.rel, gated))
            .filter(|source| module_is_in_funnel_region(&regions, &source.module))
            .map(|source| source.rel.clone())
            .collect()
    }

    /// The module PATHS that are FUNNEL regions under Rust's attribute
    /// INHERITANCE rule: a module whose FILE carries the inner
    /// `#![allow(clippy::disallowed_methods)]`, or a `mod` item (inline or a
    /// `mod child;` declaration, whose child file is a separate
    /// [`ParsedSource`]) carrying the outer `#[allow(...)]`. Rust applies such
    /// an allow to the module's own items and to every module nested inside it,
    /// so the region is the set of module paths that have one of these as an
    /// ancestor-or-self prefix.
    ///
    /// The old per-file predicate ([`attrs_allow_disallowed`] on one file's own
    /// attrs) could only see a region declared IN that file, so a `mod child;`
    /// whose PARENT module carried the allow was invisible — measured: a
    /// non-adopting `File::options`-chain planted at the top level of
    /// `src/atomic/guard.rs` changed nothing, while the identical call in
    /// `src/error.rs` was a clippy error.
    fn funnel_region_modules(
        parsed: &[ParsedSource],
        gated: &BTreeSet<String>,
    ) -> BTreeSet<CanonPath> {
        let mut regions: BTreeSet<CanonPath> = BTreeSet::new();
        for source in parsed {
            if is_test_only(&source.rel, gated) {
                continue;
            }
            if attrs_allow_disallowed(&source.file.attrs) {
                regions.insert(source.module.clone());
            }
            collect_funnel_mod_regions(&source.file.items, &source.module, &mut regions);
        }
        regions
    }

    /// Record the module path of every `mod` item carrying the funnel
    /// `#[allow]`, recursively, so a `mod`-level allow reaches the child FILE
    /// it declares (a `mod child;` has no inline items here; the child file's
    /// own [`ParsedSource`] is matched against the recorded path by
    /// [`module_is_in_funnel_region`]).
    fn collect_funnel_mod_regions(
        items: &[syn::Item],
        module: &[String],
        out: &mut BTreeSet<CanonPath>,
    ) {
        for item in items {
            let syn::Item::Mod(module_item) = item else {
                continue;
            };
            let mut child = module.to_vec();
            child.push(unraw(&module_item.ident));
            if item_allows_disallowed(item) {
                out.insert(child.clone());
            }
            if let Some((_, inner)) = &module_item.content {
                collect_funnel_mod_regions(inner, &child, out);
            }
        }
    }

    /// Whether a source file's `module` is inside a funnel region: some
    /// ANCESTOR-OR-SELF prefix of its module path carries the allow, which is
    /// exactly Rust's attribute-inheritance reach.
    fn module_is_in_funnel_region(regions: &BTreeSet<CanonPath>, module: &[String]) -> bool {
        (1..=module.len()).any(|end| regions.contains(&module[..end]))
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

    /// Whether a `syn::ImplItem` — a method, const, type, or macro inside an
    /// `impl` block — carries `#[allow(clippy::disallowed_methods)]`. The
    /// `syn::Item` arm above CANNOT see these: an `impl`'s children are
    /// `ImplItem`s, not `Item`s, so before this arm existed an `#[allow]` on a
    /// METHOD was invisible to the surface derivation (round 6 claimed the arm;
    /// it did not exist).
    fn impl_item_allows_disallowed(item: &syn::ImplItem) -> bool {
        let attrs = match item {
            syn::ImplItem::Const(item) => &item.attrs,
            syn::ImplItem::Fn(item) => &item.attrs,
            syn::ImplItem::Type(item) => &item.attrs,
            syn::ImplItem::Macro(item) => &item.attrs,
            _ => return false,
        };
        attrs_allow_disallowed(attrs)
    }

    /// The `syn::TraitItem` twin of [`impl_item_allows_disallowed`]: a trait
    /// method's `#[allow]` is a different `syn` node from both an `Item` and an
    /// `ImplItem`, so it needs its own arm.
    fn trait_item_allows_disallowed(item: &syn::TraitItem) -> bool {
        let attrs = match item {
            syn::TraitItem::Const(item) => &item.attrs,
            syn::TraitItem::Fn(item) => &item.attrs,
            syn::TraitItem::Type(item) => &item.attrs,
            syn::TraitItem::Macro(item) => &item.attrs,
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

    /// Resolve every `use` alias AND every local non-generic `type` alias in
    /// the parsed crate TRANSITIVELY: a leaf's path is re-resolved until the
    /// alias table stops changing, so `use s::fs::…` is resolved even when
    /// `use std as s;` appears after it, and `type A = O;` resolves through
    /// `use std::fs::OpenOptions as O;`.
    fn build_index(parsed: &[ParsedSource]) -> FsIndex {
        let uses: Vec<Vec<(CanonPath, Vec<UseLeaf>)>> = parsed
            .iter()
            .map(|source| collect_uses(&source.file, &source.module))
            .collect();
        let type_aliases: Vec<Vec<TypeAlias>> = parsed
            .iter()
            .map(|source| collect_type_aliases(&source.file, &source.module))
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
            // ROUND-11 P2-C: local non-generic `type` aliases are bindings too.
            // A `type ZZBuilder = std::fs::OpenOptions;` inside a funnel module
            // must resolve to the same canonical builder owner as the direct
            // spelling, or `ZZBuilder::new()…create_new(…)` is invisible to both
            // the closure derivation and the count pin. Resolve the alias TARGET
            // through the same table (so `type A = O;` chains through a `use …
            // as`), and only keep bindings that resolve under `std`/`libc`: the
            // index is a `std::fs`/`libc` resolver, and admitting a local type
            // path would let an unrelated `type` name canonicalize to `std::fs`.
            for aliases in &type_aliases {
                for alias in aliases {
                    let canonical = index.resolve_path(&alias.module, &alias.target);
                    if !matches!(canonical.first().map(String::as_str), Some("std" | "libc")) {
                        continue;
                    }
                    let mut key = alias.module.clone();
                    key.push(alias.local.clone());
                    index.aliases.insert(key, canonical);
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
        /// Per-block `let` bindings that resolve to an `OpenOptions`/`DirBuilder`
        /// builder, so a SPLIT builder spelling (`let mut o =
        /// OpenOptions::new(); o.create(true)`) is attributed to the same owner
        /// as a chained one — the same machinery the closure derivation uses,
        /// via [`builder_owner`].
        locals: Vec<BTreeMap<String, String>>,
        calls: BTreeMap<(String, &'static str), usize>,
        routes: BTreeSet<Violation>,
    }

    impl<'a> FsVisitor<'a> {
        fn new(index: &'a FsIndex, file: &str, module: CanonPath) -> Self {
            Self {
                index,
                file: file.to_string(),
                module,
                locals: Vec::new(),
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

        /// The BUILDER-METHOD arm of the count pin: the ADOPTION decision of
        /// `OpenOptions::create`/`create_new` (and the `custom_flags` bit that
        /// can forward `O_CREAT`) is a METHOD on the builder, not a path callee,
        /// so [`Self::visit_expr_call`] cannot see it. The owner comes from
        /// [`builder_owner`], the SAME resolver the closure derivation uses, so
        /// the closure and the pin cannot disagree about which method a symbol
        /// is. A generic `OpenOptions::new()` symbol is NOT a mutator and is not
        /// counted; only a member of the DERIVED table is.
        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            if let Some(owner) =
                builder_owner(self.index, &self.module, &self.locals, &call.receiver)
            {
                let segments: Vec<String> =
                    ["std", "fs", owner.as_str(), unraw(&call.method).as_str()]
                        .iter()
                        .map(|segment| (*segment).to_string())
                        .collect();
                if let Some(symbol) = mutator_symbol(&segments) {
                    *self.calls.entry((self.file.clone(), symbol)).or_default() += 1;
                }
            }
            syn::visit::visit_expr_method_call(self, call);
        }

        /// A fresh `let`-binding scope per block, matching the closure
        /// derivation: a binding recorded by [`Self::visit_local`] is visible
        /// only inside its block, so a sibling function's same-named local
        /// cannot mis-attribute a `create`/`custom_flags` call.
        fn visit_block(&mut self, block: &'ast syn::Block) {
            self.locals.push(BTreeMap::new());
            syn::visit::visit_block(self, block);
            self.locals.pop();
        }

        fn visit_local(&mut self, local: &'ast syn::Local) {
            record_builder_local(self.index, &self.module, &mut self.locals, local);
            syn::visit::visit_local(self, local);
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
    /// reaches one of [`name_mutation_symbols`] through an ENUMERATED route the
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

    /// The `libc::<symbol>` CALL spellings in `code`: a `libc` ident, `::`, a
    /// symbol ident, optional whitespace, then `(`. Comments and string
    /// literals must already be removed (callers pass [`code_only`] output), so
    /// a mention in prose is not a call. This is the DERIVATION the libc belt
    /// is built from and the candidate set
    /// `every_libc_call_symbol_the_tree_references_is_classified` checks.
    fn libc_call_symbols(code: &str) -> BTreeSet<String> {
        let bytes = code.as_bytes();
        let mut out = BTreeSet::new();
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
            if !(bytes.get(j) == Some(&b':') && bytes.get(j + 1) == Some(&b':')) {
                continue;
            }
            j += 2;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let mut symbol_at = j;
            let Some(symbol) = read_ident(code, &mut symbol_at) else {
                continue;
            };
            let mut k = symbol_at;
            while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                k += 1;
            }
            if bytes.get(k) == Some(&b'(') {
                out.insert(symbol);
            }
        }
        out
    }

    /// The DERIVED `libc` name-mutation belt the OUTSIDE-the-funnel assertion
    /// refuses: the union of
    ///
    /// (a) the curated [`MUTATING_LIBC_SYSCALLS`] family (POSIX/BSD/macOS name
    ///     CREATORS, REPLACERS, REMOVERS, and name-attribute setters), and
    /// (b) DEFAULT-DENY: every `libc::<fn>` CALL the crate's own tree
    ///     references that is not on the reviewed [`NON_MUTATING_LIBC_CALLS`]
    ///     list.
    ///
    /// Clause (b) is what makes the belt DERIVED rather than a hand-kept list:
    /// a reference to an unclassified syscall is refused outside the funnel
    /// even before a reviewer names it. Clause (a) is still required for a
    /// symbol the tree does not reference at all — a reviewer can PIN
    /// `libc::mkfifo` in the outside map without any source reference existing.
    /// The classification test forces every referenced call symbol into (a) or
    /// [`NON_MUTATING_LIBC_CALLS`], so the two cannot drift.
    fn mutating_libc_belt(sources: &[(String, String)]) -> BTreeSet<String> {
        let mut belt: BTreeSet<String> = MUTATING_LIBC_SYSCALLS
            .iter()
            .map(|s| s.to_string())
            .collect();
        for (_, raw) in sources {
            for symbol in libc_call_symbols(&code_only(raw)) {
                if !NON_MUTATING_LIBC_CALLS.contains(&symbol.as_str()) {
                    belt.insert(symbol);
                }
            }
        }
        belt
    }

    /// Whether a `libc` REFERENCE found OUTSIDE the funnel is ALLOWED by the
    /// derived [`mutating_libc_belt`]. Extracted from the audit body so the
    /// ORACLE
    /// [`the_libc_belt_refuses_a_known_name_mutator_pinned_outside_the_funnel`]
    /// drives the SAME predicate the audit uses rather than a re-implementation:
    /// the belt is the device cited to make a REVIEWED PIN safe, so it must be
    /// provable that a pinned family member is refused.
    fn libc_reference_outside_funnel_is_allowed(belt: &BTreeSet<String>, reference: &str) -> bool {
        let symbol = reference.strip_prefix("libc::").unwrap_or("");
        !belt.contains(symbol) && reference != "libc::*" && reference != "libc"
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
        // THE BELT IS DERIVED, not enumerated: the curated name-mutation family
        // unioned with the default-deny of every unclassified `libc::<fn>` CALL
        // the tree references. See [`mutating_libc_belt`].
        let belt = mutating_libc_belt(&sources);
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
                assert!(
                    libc_reference_outside_funnel_is_allowed(&belt, &reference),
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

    /// ROUND-7 REGRESSION ARM: the `libc` belt is DERIVED, not a hand-kept list
    /// that can lag the tree. The candidate set is the `libc::<fn>` CALL symbols
    /// the crate's own sources reference; every one must be classified as
    /// name-mutating ([`MUTATING_LIBC_SYSCALLS`], and therefore refused as a
    /// pinned OUTSIDE entry by [`no_libc_reference_outside_the_funnel`]) or as
    /// reviewed non-mutating ([`NON_MUTATING_LIBC_CALLS`]).
    ///
    /// The reviewer's round-7 repro was a pinned `libc::mkfifo` in the OUTSIDE
    /// map that the old 14-name list did not refuse. The tree already references
    /// `libc::mkfifo` (test FIFO fixtures), so this derivation forces it onto
    /// the belt; REMOVING `mkfifo` from [`MUTATING_LIBC_SYSCALLS`] fails this
    /// test, and the belt's default-deny clause refuses it outside regardless.
    #[test]
    fn every_libc_call_symbol_the_tree_references_is_classified() {
        let mut files = Vec::new();
        collect_crate_rs_files(Path::new(env!("CARGO_MANIFEST_DIR")), &mut files);
        let mut calls: BTreeSet<String> = BTreeSet::new();
        for file in &files {
            let raw = std::fs::read_to_string(file).expect("read source file");
            calls.extend(libc_call_symbols(&code_only(&raw)));
        }
        assert!(
            calls.len() > 5 && calls.contains("openat") && calls.contains("mkfifo"),
            "the derivation must see the tree's real `libc` call surface: {calls:?}"
        );

        let mut unclassified: Vec<String> = Vec::new();
        for symbol in &calls {
            if !MUTATING_LIBC_SYSCALLS.contains(&symbol.as_str())
                && !NON_MUTATING_LIBC_CALLS.contains(&symbol.as_str())
            {
                unclassified.push(symbol.clone());
            }
        }
        assert!(
            unclassified.is_empty(),
            "these `libc::<fn>` CALL symbols are referenced by the crate but classified as NEITHER \
             name-mutating (`MUTATING_LIBC_SYSCALLS`) nor reviewed non-mutating \
             (`NON_MUTATING_LIBC_CALLS`): {unclassified:?}. A name-mutating syscall MUST go on the \
             belt so a pinned OUTSIDE reference is refused by \
             `no_libc_reference_outside_the_funnel`."
        );

        // The anchor of the reviewer's repro: `libc::mkfifo` creates a name,
        // the tree references it, and the DERIVED belt (family + default-deny)
        // refuses it outside the funnel.
        assert!(
            MUTATING_LIBC_SYSCALLS.contains(&"mkfifo")
                && mutating_libc_belt(&[]).contains("mkfifo"),
            "`libc::mkfifo` creates a name and the tree references it; it MUST be on the belt"
        );
    }

    /// ROUND-11 ANCHOR ORACLE. The `libc` belt is the device cited to make a
    /// REVIEWED PIN outside the funnel safe, so it must REFUSE a pinned known
    /// name-mutating syscall even if the two classification lists are edited
    /// against each other. For a symbol the lint does not deny (`libc::chmod`)
    /// the belt is the SOLE device.
    ///
    /// WHAT ROUND 10 GOT WRONG. [`KNOWN_NAME_MUTATORS`] and
    /// [`MUTATING_LIBC_SYSCALLS`] are two hand-written literals IN THIS FILE, and
    /// every arm but (5) compared them to EACH OTHER. A set equality between
    /// things you edit together is not an oracle: deleting `chown` from BOTH
    /// literals and adding it to [`NON_MUTATING_LIBC_CALLS`] disarmed the belt
    /// with every arm green (measured; the same 3-way edit works for 52 of the
    /// 54 round-10 members). Arm (5) hardcoded only `chmod` and `renameat2`, so
    /// it did not notice the other 52.
    ///
    /// THIS ORACLE ITERATES [`INDEPENDENT_KNOWN_NAME_MUTATORS`], the THIRD
    /// artifact, and asserts:
    /// (0) the anchor is non-trivial, its classes come from the CLOSED
    ///     [`KNOWN_NAME_MUTATOR_CLASSES`] set, every entry carries a non-empty
    ///     reason distinct from its class, and the named negative controls
    ///     (`lutimes`, `fchmodat2`, `mq_open`, …) are still present;
    /// (1) the two family literals name the SAME symbols AND the anchor names
    ///     every family member and nothing else — so co-editing BOTH family
    ///     literals leaves the anchor naming a symbol the family lost, and fails
    ///     here; a family member with no anchor entry fails too (membership
    ///     requires an individual class + reason, not merely "it is in the
    ///     list");
    /// (2) DISJOINTNESS from [`NON_MUTATING_LIBC_CALLS`], checked against the
    ///     ANCHOR, so a 3-way MOVE into the review list fails even if both
    ///     family literals were edited to match;
    /// (3) every anchor member is REFUSED by the belt the audit DERIVES from a
    ///     source that references it, via the audit's OWN predicate
    ///     ([`libc_reference_outside_funnel_is_allowed`]);
    /// (4) the DEFAULT-DENY clause ALONE — a belt built with no family
    ///     contribution, so the property does not depend on
    ///     [`MUTATING_LIBC_SYSCALLS`] at all — also refuses every anchor member:
    ///     the anti-circular half, whose only disarm is the exclusion a MOVE into
    ///     the non-mutating review list would add; and
    /// (5) for EVERY anchor member the lint does NOT deny, the belt is the SOLE
    ///     device, with a non-vacuous count, so the arm is not a hardcoded pair.
    #[test]
    fn the_libc_belt_refuses_a_known_name_mutator_pinned_outside_the_funnel() {
        // (0) THE ANCHOR ITSELF: a closed class set, an individual reason per
        // symbol, no duplicates, and the named negative controls that stop the
        // anchor from being narrowed back to the round-10 family.
        let classes: BTreeSet<&str> = KNOWN_NAME_MUTATOR_CLASSES.iter().copied().collect();
        let mut anchor: BTreeSet<&str> = BTreeSet::new();
        for (symbol, class, reason) in INDEPENDENT_KNOWN_NAME_MUTATORS {
            assert!(
                classes.contains(class),
                "the anchor entry {symbol} claims the class {class:?}, which is not in the closed \
                 KNOWN_NAME_MUTATOR_CLASSES set {classes:?}; add the class deliberately or fix the \
                 spelling"
            );
            assert!(
                !reason.trim().is_empty() && reason != class,
                "the anchor entry {symbol} must carry a reason distinct from its class ({class:?})"
            );
            assert!(
                anchor.insert(symbol),
                "the anchor names {symbol} more than once; a duplicate makes the reason table \
                 ambiguous"
            );
        }
        assert!(
            anchor.len() >= 60,
            "the independently-specified anchor must be the reviewed family, not a stub: {}",
            anchor.len()
        );
        // THE NAMED NEGATIVE CONTROLS. These were in NEITHER round-10 hand list;
        // they are the members that make the anchor larger than that family, so
        // removing one is the narrowing this arm exists to fail on.
        for symbol in [
            "chown",
            "mknodat",
            "lutimes",
            "futimes",
            "futimesat",
            "fchmodat2",
            "mq_open",
            "mq_unlink",
            "sem_open",
            "sem_unlink",
            "shm_open",
            "shm_unlink",
            "mkstemp",
            "mkostemp",
            "mkstemps",
            "mkostemps",
            "mkdtemp",
            "acl_set_file",
            "acl_set_link_np",
        ] {
            assert!(
                anchor.contains(symbol),
                "the independently-specified anchor lost the negative control {symbol}; it must \
                 stay anchored OUTSIDE the co-editable family pair"
            );
        }

        // (1) THE FAMILY VS THE ANCHOR. The two family literals must agree with
        // each other, and the anchor must name EVERY family member (the belt's
        // clause (a) is the only device that refuses an unreferenced symbol) and
        // NOTHING outside it.
        let code_family: BTreeSet<&str> = MUTATING_LIBC_SYSCALLS.iter().copied().collect();
        let literal_family: BTreeSet<&str> = KNOWN_NAME_MUTATORS.iter().copied().collect();
        assert_eq!(
            code_family, literal_family,
            "the two family literals disagree: MUTATING_LIBC_SYSCALLS={code_family:?} \
             KNOWN_NAME_MUTATORS={literal_family:?}"
        );
        let missing_from_family: Vec<&&str> = anchor.difference(&code_family).collect();
        assert!(
            missing_from_family.is_empty(),
            "these ANCHOR name mutators are absent from MUTATING_LIBC_SYSCALLS: \
             {missing_from_family:?}. The anchor is the artifact this oracle iterates, so \
             co-editing BOTH family literals to drop a member (the round-10 evasion) fails \
             here: the anchor still names it, and clause (a) of the belt no longer refuses it. \
             Restore the family member or state the reclassification in the anchor."
        );
        let missing_from_anchor: Vec<&&str> = code_family.difference(&anchor).collect();
        assert!(
            missing_from_anchor.is_empty(),
            "MUTATING_LIBC_SYSCALLS names {missing_from_anchor:?}, which the anchor does not. \
             Every family member needs an individual class + reason in \
             INDEPENDENT_KNOWN_NAME_MUTATORS: membership must be a reviewed justification, not \
             merely `it is in the list`."
        );

        // (2) DISJOINTNESS, checked against the ANCHOR, so a MOVE of an anchor
        // member into the review list fails even if both family literals were
        // edited to match the move.
        let overlap: Vec<&&str> = anchor
            .iter()
            .filter(|symbol| NON_MUTATING_LIBC_CALLS.contains(symbol))
            .collect();
        assert!(
            overlap.is_empty(),
            "NON_MUTATING_LIBC_CALLS claims these independently-specified KNOWN name mutators, \
             which disarms the belt by excluding them from its default-deny clause: {overlap:?}"
        );

        // (3)+(4) Each anchor member is refused by the DERIVED belt AND by the
        // default-deny clause ALONE. The synthetic source makes the symbol a
        // referenced `libc::<fn>(…)` call, exactly how the real tree puts a
        // symbol on the belt.
        for symbol in &anchor {
            let sources = vec![(
                "src/prod/synthetic.rs".to_string(),
                format!("unsafe fn f(p: *const libc::c_char) {{ libc::{symbol}(p); }}"),
            )];
            let reference = format!("libc::{symbol}");
            let belt = mutating_libc_belt(&sources);
            assert!(
                !libc_reference_outside_funnel_is_allowed(&belt, &reference),
                "a pinned OUTSIDE reference to the name mutator {reference} was ALLOWED by the \
                 belt; the belt is DERIVED, so a non-funnel production reference must land on it: \
                 {belt:?}"
            );
            let default_deny: BTreeSet<String> = libc_call_symbols(&code_only(&sources[0].1))
                .into_iter()
                .filter(|name| !NON_MUTATING_LIBC_CALLS.contains(&name.as_str()))
                .collect();
            assert!(
                !libc_reference_outside_funnel_is_allowed(&default_deny, &reference),
                "the DEFAULT-DENY clause ALONE (a belt with no family contribution) must refuse \
                 {reference}; it did not, which means the symbol sits on NON_MUTATING_LIBC_CALLS: \
                 {default_deny:?}"
            );
        }

        // (5) For EVERY anchor member the clippy deny does NOT name, the belt is
        // the SOLE device. Derived from the anchor rather than hardcoded to a
        // pair, and the count must be non-vacuous, so the arm cannot pass by
        // naming only symbols the lint already denies.
        let denied = denied_symbols_from_clippy_toml();
        let mut lint_omitted = 0usize;
        for symbol in &anchor {
            if denied.contains(&format!("libc::{symbol}")) {
                continue;
            }
            lint_omitted += 1;
            let sources = vec![(
                "src/prod/synthetic.rs".to_string(),
                format!("unsafe fn f(p: *const libc::c_char) {{ libc::{symbol}(p); }}"),
            )];
            let belt = mutating_libc_belt(&sources);
            assert!(
                !libc_reference_outside_funnel_is_allowed(&belt, &format!("libc::{symbol}")),
                "libc::{symbol} is refused by NEITHER clippy.toml nor the belt; a non-funnel \
                 production reference would be undefended: {belt:?}"
            );
        }
        assert!(
            lint_omitted >= 5,
            "only {lint_omitted} anchor members are outside clippy.toml's deny; this arm exists to \
             prove the BELT refuses a symbol the lint does not, so it must not become vacuous"
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
    /// path resolves to a pinned symbol — is one occurrence for that file and
    /// symbol; a canonical `std::fs::<symbol>` token sequence inside a macro's
    /// tokens is one more; and a BUILDER METHOD call (`OpenOptions::create`,
    /// `DirBuilder::create`, `custom_flags`) whose receiver chain or enclosing
    /// `let` binding resolves to an `OpenOptions`/`DirBuilder` owner is one
    /// more, attributed by the SAME [`builder_owner`] the closure derivation
    /// uses. This is a SUPERSET of the old byte count, which saw only the
    /// literal `std::fs::<symbol>(` text: every occurrence the byte count saw —
    /// including one inside a `macro_rules!` body — is seen here too, plus
    /// resolved aliases, parenthesized/referenced direct callees, and the
    /// inherent/builder adoption forms. It is NOT a superset of every possible
    /// call: a mutator reached inside a macro through an alias, or a call
    /// through a function-pointer variable, is not counted (see RESIDUE), so
    /// adding such a call does not change a count.
    ///
    /// SYMBOL SET: DERIVED from `clippy.toml` ([`name_mutation_symbols`]), not
    /// hand-kept — the round-8 finding was seven ADOPTING symbols the lint
    /// denied but a hand-kept table did not count, so a funnel-module call to
    /// one left this pin green. A new deny entry now changes this pin
    /// automatically. The pinned VALUES changed in round 8 for exactly that
    /// reason (`create_new`, `create`, `custom_flags`, and `write` calls that
    /// were always there are now counted); each new value is a deliberate,
    /// reviewed entry recorded at the pin with its reason.
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
    /// `no_libc_reference_outside_the_funnel` sees it. An INODE-PRESERVING
    /// content mutation on an ALREADY-EXISTING path through a call that is
    /// legal outside the funnel is not pinned: it cannot split a holder because
    /// the flock stays on the unchanged inode. The mode setter, the name
    /// CREATORS, and `std::fs::write` (create-or-truncate, and therefore an
    /// ADOPTION when the path is absent) are DENIED now, not residue. The audit
    /// resolves the
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
            // --- The ADOPTION family the pin MISSED before round 8 (P1-A).
            // These symbols were on `clippy.toml`'s deny list but NOT in the
            // hand-kept pin table, so a call to one INSIDE a funnel module —
            // where the module-level `#[allow]` blinds the lint — left the count
            // pin green. The pin's table is now DERIVED from `clippy.toml`
            // ([`name_mutation_symbols`] -> [`reconciled_denied_symbols`] ->
            // [`denied_symbols_from_clippy_toml`]), so these entries appear
            // automatically and every future deny entry does too. Each site is
            // either a funnel module (module-level allow) or an item-level
            // allow reviewed for the creation it performs. (`File::create` and
            // `std::fs::copy` are in the derived set but have NO production
            // call, so they are absent here by construction — the pin lists
            // nonzero counts only.)
            //
            // `create_new` (O_EXCL creation — "create a name, FAIL if it
            // exists"): the temp create in `write_atomic_replace` (unix +
            // windows), `copy_tree_verbatim`'s destination create (unix +
            // windows), windows' `write_file_new_fd`, the transport's
            // `durable_create_new` + its own temp create, and the known-hosts
            // pin. All run inside the funnel or under an item-level allow.
            ("src/atomic/unix.rs", "OpenOptions::create_new", 2),
            ("src/atomic/windows.rs", "OpenOptions::create_new", 4),
            ("src/transport/mod.rs", "OpenOptions::create_new", 2),
            ("src/transport/ssh/hostkey.rs", "OpenOptions::create_new", 1),
            // `OpenOptions::create`: the LOCK PROTOCOL's own record open
            // (creating the record on first use), item-level allowed in each
            // port and refusing a symlink/reparse point at the record spelling.
            ("src/lock/unix.rs", "OpenOptions::create", 1),
            ("src/lock/windows.rs", "OpenOptions::create", 1),
            // `custom_flags`: the reviewed NON-ADOPTING opens that forward only
            // no-create bits (`O_NOFOLLOW`/`O_CLOEXEC`/`O_NONBLOCK`/
            // `O_DIRECTORY`/`FILE_FLAG_OPEN_REPARSE_POINT`/
            // `FILE_FLAG_BACKUP_SEMANTICS`) — the same reviewed sites that carry
            // the item-level allow for the crate-wide deny. The ADOPTING
            // `custom_flags(O_CREAT)` form is what round 8 (P1-B) proved was
            // reachable with every gate green.
            ("src/atomic/guard.rs", "OpenOptions::custom_flags", 1),
            ("src/atomic/mod.rs", "OpenOptions::custom_flags", 1),
            ("src/atomic/unix.rs", "OpenOptions::custom_flags", 1),
            ("src/atomic/windows.rs", "OpenOptions::custom_flags", 1),
            ("src/lock/unix.rs", "OpenOptions::custom_flags", 1),
            ("src/lock/windows.rs", "OpenOptions::custom_flags", 1),
            ("src/transport/mod.rs", "OpenOptions::custom_flags", 1),
            // `std::fs::write` (create-or-truncate, which ADOPTS an absent
            // name): the Windows `write_file_fd`, which runs
            // `refuse_reserved_mutation` first and preserves the inode of an
            // EXISTING entry. Unix's `write_file_fd` writes through the
            // descriptor, so it has no `std::fs::write` and does not appear.
            ("src/atomic/windows.rs", "write", 1),
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

    /// ROUND-8 P1-A REGRESSION: the count pin's symbol set is DERIVED from
    /// `clippy.toml`, so every ADOPTING symbol the lint denies is counted even
    /// inside a funnel module, where the module-level `#[allow]` blinds the
    /// lint. Before this arm, seven adopting symbols were denied by the lint but
    /// absent from the hand-kept pin table: a funnel-module call to one changed
    /// no count. The CONTROL is a read-only symbol the deny list does NOT name —
    /// it must change no count, so this is not merely "count every `std::fs`
    /// call".
    #[test]
    fn the_count_pin_sees_every_adopting_symbol_the_deny_names() {
        // (1) The two devices cannot drift: every path-shaped `clippy.toml`
        // entry, read INDEPENDENTLY from the raw file — NOT by iterating the
        // function that built `table` — is in the derived pin table, mapped to
        // its canonical path. This arm used to loop over
        // `reconciled_denied_symbols()` and compare against the table that same
        // function produced: a map checked against its own producer, which
        // stayed green while a reformatted entry dropped `std::fs::hard_link`
        // from BOTH sides (measured). The raw read cannot be produced by the
        // table, so losing a symbol now fails here.
        let table = name_mutation_symbols();
        let clippy_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("clippy.toml");
        let clippy_text = std::fs::read_to_string(&clippy_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", clippy_path.display()));
        for path in raw_denied_path_strings(&clippy_text) {
            let canonical = synthetic_builder_spelling(&path).unwrap_or(path);
            let segments: Vec<String> = canonical.split("::").map(str::to_string).collect();
            if let Some(key) = pin_key_for(&segments) {
                assert_eq!(
                    table.get(key.as_str()),
                    Some(&segments),
                    "clippy.toml denies {canonical} but the derived pin table does not map {key:?} \
                     to it: {table:?}"
                );
            }
        }
        assert!(
            !table.contains_key("read"),
            "a read-only symbol the deny list does not name must NOT enter the pin: {table:?}"
        );

        // (2) Each adopting spelling planted in a FUNNEL module (module-level
        // `#[allow]`) is counted — the exact configuration the lint cannot see.
        let funnel = |body: &str| {
            let files = vec![(
                "src/prod/funnel.rs".to_string(),
                format!("#![allow(clippy::disallowed_methods)]\n{body}"),
            )];
            std_fs_audit(&files, &BTreeSet::new()).1
        };
        for (label, body, key) in [
            (
                "File::create",
                "fn f(p: &Path) -> std::io::Result<()> { let _ = std::fs::File::create(p)?; Ok(()) }",
                "File::create",
            ),
            (
                "File::create_new",
                "fn f(p: &Path) -> std::io::Result<()> { let _ = std::fs::File::create_new(p)?; \
                 Ok(()) }",
                "File::create_new",
            ),
            (
                "OpenOptions::create (chain)",
                "fn f(p: &Path) -> std::io::Result<()> { let _ = \
                 std::fs::OpenOptions::new().write(true).create(true).open(p)?; Ok(()) }",
                "OpenOptions::create",
            ),
            (
                "OpenOptions::create_new (split local)",
                "fn f(p: &Path) -> std::io::Result<()> { let mut o = std::fs::OpenOptions::new(); \
                 o.write(true).create_new(true); let _ = o.open(p)?; Ok(()) }",
                "OpenOptions::create_new",
            ),
            (
                "File::options (the stable OpenOptions synonym)",
                "fn f(p: &Path) -> std::io::Result<()> { let _ = \
                 std::fs::File::options().write(true).create(true).open(p)?; Ok(()) }",
                "OpenOptions::create",
            ),
            (
                "OpenOptions::default",
                "fn f(p: &Path) -> std::io::Result<()> { let _ = \
                 std::fs::OpenOptions::default().write(true).create_new(true).open(p)?; Ok(()) }",
                "OpenOptions::create_new",
            ),
            (
                "DirBuilder::default",
                "fn f(p: &Path) -> std::io::Result<()> { std::fs::DirBuilder::default().create(p)?; \
                 Ok(()) }",
                "DirBuilder::create",
            ),
            (
                "DirBuilder::create",
                "fn f(p: &Path) -> std::io::Result<()> { std::fs::DirBuilder::new().create(p)?; \
                 Ok(()) }",
                "DirBuilder::create",
            ),
            (
                "std::fs::copy",
                "fn f(a: &Path, b: &Path) -> std::io::Result<()> { std::fs::copy(a, b)?; Ok(()) }",
                "copy",
            ),
            (
                "std::fs::write",
                "fn f(p: &Path) -> std::io::Result<()> { std::fs::write(p, b\"x\")?; Ok(()) }",
                "write",
            ),
        ] {
            let counts = funnel(body);
            assert_eq!(
                counts
                    .get(&("src/prod/funnel.rs".to_string(), key))
                    .copied(),
                Some(1),
                "the count pin must see {label} planted in a funnel module: {counts:?}"
            );
        }

        // (3) CONTROL: a read-only call changes no count.
        let counts =
            funnel("fn f(p: &Path) -> std::io::Result<()> { let _ = std::fs::read(p)?; Ok(()) }");
        assert!(
            counts.is_empty(),
            "a read-only `std::fs::read` must not be counted: {counts:?}"
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
    /// audited direct `std::fs::rename` the exact-count pin already covers — a
    /// `std::fs::write` inside the funnel (a DENIED symbol whose allowed sites
    /// all carry the allow), and a redundant `use std;` (which
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
            "a legitimate direct call at the guarded funnel and a write inside it must NOT be \
             reported as an import-route violation"
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
    /// `std::fs::read`/`read_to_string`/`metadata`/`symlink_metadata`/
    /// `read_dir`/`read_link`/`canonicalize` act on an entry the funnel already
    /// created or resolved; the `libc` entries (`fstatat`, `fstat`, `readlinkat`,
    /// `readdir`, `fdopendir`, `closedir`, `fcntl`) read or manage an open
    /// descriptor. `std::fs::write` is deliberately NOT here: it CREATES on an
    /// absent path, so it can ADOPT a name and is on `clippy.toml`'s deny list.
    /// The funnel's one production site (`atomic::windows::write_file_fd`)
    /// carries the module-level allow and runs `refuse_reserved_mutation`
    /// FIRST, and the inode of an existing entry is preserved, so its only
    /// adoption is of a name the guard has already cleared.
    ///
    /// `libc::openat` is deliberately NOT here: it CAN adopt a name (with
    /// `O_CREAT`), so it is on clippy.toml's deny list with the other open
    /// spellings. Every other `libc` entry here is read-only.
    ///
    /// TWO more families joined this list when the surface derivation learned
    /// to see inherent forms. The `OpenOptions` BUILDER surface
    /// (`new`/`read`/`write`/`share_mode`/`open`) is here because
    /// the ADOPTION decision is the `create`/`create_new` FLAG, which IS denied
    /// (`std::fs::OpenOptions::create`, `.create_new`); the constructor, the
    /// configuration setters, and the terminal `open` cannot adopt a name on
    /// their own, and a read-only open must stay legal everywhere, so denying
    /// the terminal call would be the wrong device. The ASSOCIATED/CONVERSION
    /// surface (`std::fs::File::open` — read-only;
    /// `std::fs::File::from` — an `OwnedFd`→`File` conversion;
    /// `std::fs::Permissions::from_mode` — a mode-VALUE construction) touches no
    /// name at all.
    ///
    /// `std::fs::OpenOptions::custom_flags` is deliberately NOT here. It
    /// forwards ARBITRARY bits to `open(2)`/`CreateFile`, and `O_CREAT` through
    /// it ADOPTS a name, so it is DENIED crate-wide as the resolved trait
    /// methods `std::os::{unix,windows}::fs::OpenOptionsExt::custom_flags`. The
    /// round-8 finding was that this list carried the synthetic spelling with a
    /// comment claiming the method "cannot adopt": both the entry and the
    /// reason were false. Reviewed non-adopting sites now carry an item-level
    /// `#[allow]` instead, exactly as `set_permissions` does.
    const FUNNEL_SYMBOLS_NOT_DENIED: &[&str] = &[
        "std::fs::read",
        "std::fs::read_to_string",
        "std::fs::metadata",
        "std::fs::symlink_metadata",
        "std::fs::read_dir",
        "std::fs::read_link",
        "std::fs::canonicalize",
        // The `OpenOptions` builder surface (see the doc above): the adoption
        // flags are denied, these are not and cannot adopt.
        "std::fs::OpenOptions::new",
        "std::fs::OpenOptions::read",
        "std::fs::OpenOptions::write",
        "std::fs::OpenOptions::share_mode",
        "std::fs::OpenOptions::open",
        // `std::os::unix::fs::OpenOptionsExt::mode` and `OpenOptions::truncate`,
        // used by the reviewed `lock::unix::open_lock_file` exception (now a
        // funnel region because it carries the `#[allow]`). Both are set on a
        // builder that is about to `open`; NEITHER can adopt a name (`create`
        // and `create_new` are the adoption flags and both ARE denied), and the
        // lock's own open spells `.truncate(false)` explicitly, which is exactly
        // why `OpenOptions::truncate` is reviewed here rather than denied
        // crate-wide (a deny matches the METHOD NAME and would false-positive on
        // every read-only `.truncate(false)`).
        "std::fs::OpenOptions::truncate",
        "std::fs::OpenOptions::mode",
        // Read-only / conversion associated calls: no name is touched.
        "std::fs::File::open",
        "std::fs::File::from",
        "std::fs::Permissions::from_mode",
        "libc::fstatat",
        "libc::fstat",
        "libc::readlinkat",
        "libc::readdir",
        "libc::fdopendir",
        "libc::closedir",
        "libc::fcntl",
    ];

    /// The canonical `std`/`libc` path a resolved path names, if it is a CALL
    /// TARGET this audit tracks: any `std::fs::…` path — the free functions
    /// (`std::fs::remove_file`), the inherent ASSOCIATED functions
    /// (`std::fs::File::create`, `std::fs::OpenOptions::new`), and the builder
    /// methods the [`FunnelSymbols`] method arm records as
    /// `std::fs::OpenOptions::create` — a
    /// `std::os::{unix,windows}::fs::<symlink*>` creator, a
    /// `std::os::{unix,windows}::net::{UnixListener,UnixDatagram}::bind`
    /// pathname-socket creator, or a `libc::<fn>`.
    ///
    /// The segment count of the `std::fs` arm is deliberately NOT pinned to
    /// three: the completeness device's escaping spellings are the INHERENT
    /// forms (`File::create`, `OpenOptions::create`), whose canonical paths
    /// are four segments, so a length filter would exclude exactly the class
    /// this arm must see. The `std::os` arms keep their `unix`/`windows` gate
    /// and their `fs`/`net` discriminator, so a deeper unrelated `std::os`
    /// path cannot be mistaken for a creator.
    fn funnel_symbol(canonical: &[String]) -> Option<String> {
        if canonical.len() >= 3 && canonical[0] == "std" && canonical[1] == "fs" {
            return Some(canonical.join("::"));
        }
        if canonical.len() >= 5
            && canonical[0] == "std"
            && canonical[1] == "os"
            && (canonical[2] == "unix" || canonical[2] == "windows")
            && canonical[3] == "fs"
        {
            return Some(canonical.join("::"));
        }
        // The pathname-socket bind: a `net` creator that ADOPTS a directory
        // entry, so the closure MUST be able to see it if the funnel ever
        // spells one (round 8, P2-A).
        if canonical.len() == 6
            && canonical[0] == "std"
            && canonical[1] == "os"
            && (canonical[2] == "unix" || canonical[2] == "windows")
            && canonical[3] == "net"
            && (canonical[4] == "UnixListener" || canonical[4] == "UnixDatagram")
            && canonical[5] == "bind"
        {
            return Some(canonical.join("::"));
        }
        if canonical.len() == 2 && canonical[0] == "libc" {
            return Some(canonical.join("::"));
        }
        None
    }

    /// Whether `method` is an `OpenOptions`/`DirBuilder` CONFIGURATION call —
    /// one that returns the builder (or `&mut` to it) and so keeps a chain ON
    /// the builder. The TERMINAL calls (`OpenOptions::open`, `DirBuilder::create`)
    /// are deliberately not included: they end the chain, so a method called on
    /// their RESULT (`…open(p).map_err(…)`) must not be attributed to the
    /// builder. `create` IS a configuration call for `OpenOptions` (its terminal
    /// is `open`), so it stays.
    fn is_builder_config_method(method: &str) -> bool {
        matches!(
            method,
            "read"
                | "write"
                | "append"
                | "truncate"
                | "create"
                | "create_new"
                | "mode"
                | "custom_flags"
                | "share_mode"
                | "recursive"
        )
    }

    /// The `std::fs` BUILDER type (`OpenOptions` / `DirBuilder`) a canonical
    /// path names, if it is the type itself or one of its CONSTRUCTORS: both
    /// `std::fs::OpenOptions` and `std::fs::OpenOptions::new` bottom out here,
    /// so the receiver of a builder chain (`OpenOptions::new().create`) and the
    /// receiver of a split `let` binding (`let mut o = OpenOptions::new();
    /// o.create`) resolve to the SAME owner.
    ///
    /// The constructor set is the STABLE SURFACE, not an enumeration of one
    /// spelling: `std::fs::File::options()` is the documented synonym of
    /// `OpenOptions::new()` (both return an `OpenOptions`), and
    /// `OpenOptions::default()` / `DirBuilder::default()` are the `Default`
    /// builders. Before this arm they resolved to NO owner, so an adoption flag
    /// (`create`/`create_new`) reached through `File::options().create(true)`
    /// was invisible to BOTH the closure derivation and the count pin while the
    /// identical `OpenOptions::new().create(true)` was counted and denied.
    ///
    /// `File` itself is still deliberately excluded: its name-mutating
    /// four-segment ASSOCIATED functions (`File::create`, `File::create_new`)
    /// are recorded by the call arm, while an instance method on an open `File`
    /// (`set_permissions`, `set_len`) is descriptor-bound and the crate's
    /// documented permitted side. Only `File::options` — whose result IS an
    /// `OpenOptions` — names the builder owner.
    fn builder_type(canonical: &[String]) -> Option<String> {
        if let [std, fs, owner, ctor] = canonical
            && std == "std"
            && fs == "fs"
        {
            if owner == "File" && ctor == "options" {
                return Some("OpenOptions".to_string());
            }
            if matches!(owner.as_str(), "OpenOptions" | "DirBuilder") && ctor == "default" {
                return Some(owner.clone());
            }
        }
        let owner = match canonical {
            [std, fs, owner] if std == "std" && fs == "fs" => owner,
            [std, fs, owner, ctor] if std == "std" && fs == "fs" && ctor == "new" => owner,
            _ => return None,
        };
        matches!(owner.as_str(), "OpenOptions" | "DirBuilder").then(|| owner.clone())
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
    ///
    /// The surface is the union of three arms, because a name-adopting call has
    /// three spellings and each one has to be SEEN:
    ///
    /// * `visit_expr_call` records a path-callee call (`std::fs::File::create`,
    ///   `std::fs::OpenOptions::new`, `std::fs::copy`) at any path depth;
    /// * `visit_expr_method_call` records a builder method
    ///   (`std::fs::OpenOptions::create`) when [`Self::builder_owner`] resolves
    ///   the receiver to an `OpenOptions`/`DirBuilder` builder;
    /// * `visit_macro` records the canonical paths a macro body spells.
    ///
    /// [`Self::locals`] carries the per-block `let` bindings that make the SPLIT
    /// builder spelling (`let mut o = OpenOptions::new(); o.create(true)`) resolve
    /// to the same owner as the chained one, so neither spelling is a hole.
    struct FunnelSymbols<'a> {
        index: &'a FsIndex,
        module: CanonPath,
        in_funnel: bool,
        used: BTreeSet<String>,
        locals: Vec<BTreeMap<String, String>>,
    }

    /// The `std::fs` builder owner (`OpenOptions` / `DirBuilder`) of a
    /// receiver expression, if any. It walks a method/call chain down to its
    /// root and resolves that root through the alias table OR the enclosing
    /// blocks' `let` bindings, so a builder reached through a local (`let mut o
    /// = OpenOptions::new();`) is the same owner as one reached through a chain
    /// (`OpenOptions::new().create(true)`).
    ///
    /// SHARED by the closure derivation ([`FunnelSymbols`]) and the count pin
    /// ([`FsVisitor`]): both must attribute a builder METHOD to the same owner,
    /// or the closure and the pin would see different symbol sets for one
    /// spelling.
    fn builder_owner(
        index: &FsIndex,
        module: &[String],
        locals: &[BTreeMap<String, String>],
        expr: &syn::Expr,
    ) -> Option<String> {
        match expr {
            syn::Expr::MethodCall(inner) => is_builder_config_method(&unraw(&inner.method))
                .then(|| builder_owner(index, module, locals, &inner.receiver))
                .flatten(),
            syn::Expr::Call(call) => {
                let path = callee_path(&call.func)?;
                let canonical = index.resolve_path(module, &path_segments(path));
                builder_type(&canonical)
            }
            syn::Expr::Path(path) if path.qself.is_none() => {
                let segments = path_segments(&path.path);
                if segments.len() == 1 {
                    for scope in locals.iter().rev() {
                        if let Some(owner) = scope.get(&segments[0]) {
                            return Some(owner.clone());
                        }
                    }
                }
                let canonical = index.resolve_path(module, &segments);
                builder_type(&canonical)
            }
            _ => None,
        }
    }

    /// Record `local`'s `let` binding as a builder local if its initializer
    /// resolves to an `OpenOptions`/`DirBuilder` chain. Shared by
    /// [`FunnelSymbols`] and [`FsVisitor`] for the same reason as
    /// [`builder_owner`].
    fn record_builder_local(
        index: &FsIndex,
        module: &[String],
        locals: &mut [BTreeMap<String, String>],
        local: &syn::Local,
    ) {
        if let Some(init) = &local.init
            && let syn::Pat::Ident(pat) = &local.pat
            && let Some(owner) = builder_owner(index, module, locals, &init.expr)
            && let Some(scope) = locals.last_mut()
        {
            scope.insert(unraw(&pat.ident), owner);
        }
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

        /// An `#[allow]` on an IMPL METHOD is a funnel region exactly as one on
        /// a freestanding `fn` is. `visit_item` cannot see it: an impl method
        /// is a `syn::ImplItem`, which the `Item` arm never reaches with its
        /// own attribute check, so before this override the derived surface
        /// missed every item-level-allow impl method — including the five
        /// PRODUCTION `std::fs::create_dir_all` sites this crate ships.
        fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
            let saved = self.in_funnel;
            if impl_item_allows_disallowed(item) {
                self.in_funnel = true;
            }
            syn::visit::visit_impl_item(self, item);
            self.in_funnel = saved;
        }

        /// The trait-method twin of [`Self::visit_impl_item`]: a default or
        /// required method's `#[allow]` is a `syn::TraitItem`, a third node
        /// shape the `Item` arm cannot reach.
        fn visit_trait_item(&mut self, item: &'ast syn::TraitItem) {
            let saved = self.in_funnel;
            if trait_item_allows_disallowed(item) {
                self.in_funnel = true;
            }
            syn::visit::visit_trait_item(self, item);
            self.in_funnel = saved;
        }

        /// A fresh `let`-binding scope per block: a binding recorded by
        /// [`Self::visit_local`] is visible only inside the block it is
        /// declared in, so a same-named local in a sibling function cannot
        /// mis-attribute an unrelated `create`/`truncate` call.
        fn visit_block(&mut self, block: &'ast syn::Block) {
            self.locals.push(BTreeMap::new());
            syn::visit::visit_block(self, block);
            self.locals.pop();
        }

        fn visit_local(&mut self, local: &'ast syn::Local) {
            record_builder_local(self.index, &self.module, &mut self.locals, local);
            syn::visit::visit_local(self, local);
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

        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            if self.in_funnel
                && let Some(owner) =
                    builder_owner(self.index, &self.module, &self.locals, &call.receiver)
            {
                let symbol = format!("std::fs::{owner}::{}", unraw(&call.method));
                self.used.insert(symbol);
            }
            syn::visit::visit_expr_method_call(self, call);
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
    /// targets, derived from the parsed production sources. The region is
    /// derived by [`funnel_region_modules`], so a child FILE of an
    /// allow-bearing module (`src/atomic/guard.rs` under `src/atomic/mod.rs`)
    /// starts inside the region exactly as an inline `mod { … }` under an
    /// annotated item does; starting the visitor at `false` per file was the
    /// per-file defect that hid every cross-file child.
    fn funnel_symbol_surface(
        files: &[(String, String)],
        gated: &BTreeSet<String>,
    ) -> BTreeSet<String> {
        let parsed = parse_crate(files, gated);
        let index = build_index(&parsed);
        let regions = funnel_region_modules(&parsed, gated);
        let mut used = BTreeSet::new();
        for source in &parsed {
            if is_test_only(&source.rel, gated) {
                continue;
            }
            let mut visitor = FunnelSymbols {
                index: &index,
                module: source.module.clone(),
                in_funnel: module_is_in_funnel_region(&regions, &source.module),
                used: BTreeSet::new(),
                locals: Vec::new(),
            };
            syn::visit::Visit::visit_file(&mut visitor, &source.file);
            used.extend(visitor.used);
        }
        used
    }

    /// Read the DOUBLE-QUOTED string values inside `clippy.toml`'s
    /// `disallowed-methods` ARRAY that begin with a namespace the deny list
    /// tracks (`std::fs::`, `std::os::`, `libc::`).
    ///
    /// A RAW scan, deliberately NOT a TOML parse: it is the INDEPENDENT side of
    /// the cross-check in [`denied_symbols_from_clippy_toml`] and of the count
    /// pin's arm (1), so a path-shaped spelling the parse fails to extract is a
    /// FAILING TEST rather than a silently dropped deny. It reads only the
    /// array body, because the prose above and below it quotes whole phrases
    /// and `extern "C"`; the deny ENTRIES live only inside the array.
    fn raw_denied_path_strings(text: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let Some(header) = text
            .lines()
            .position(|line| line.trim_start().starts_with("disallowed-methods"))
        else {
            return out;
        };
        let array: String = text.lines().skip(header).collect::<Vec<_>>().join("\n");
        let Some(open) = array.find('[') else {
            return out;
        };
        let body = &array[open + 1..];
        let end = body.find(']').unwrap_or(body.len());
        let mut rest = &body[..end];
        while let Some(open) = rest.find('"') {
            let after = &rest[open + 1..];
            let Some(close) = after.find('"') else { break };
            let value = &after[..close];
            if value.starts_with("std::fs::")
                || value.starts_with("std::os::")
                || value.starts_with("libc::")
            {
                out.insert(value.to_string());
            }
            rest = &after[close + 1..];
        }
        out
    }

    /// The `path` values of `clippy.toml`'s `disallowed-methods`, read by
    /// PARSING the file as TOML — the way clippy reads it — so every valid
    /// spelling clippy accepts is a spelling this reader accepts too.
    ///
    /// A line-shaped reader that required one exact spelling
    /// (`line.strip_prefix("{ path = \"")`) silently DROPPED a reformatted
    /// entry, which made the derived pin table and the crate-wide deny
    /// DISAGREE: clippy still refused a non-funnel call (exit 101) while the
    /// derived table lacked the symbol, so the count pin lost it (round 8's
    /// exact failure mode) and the closure reported a FALSE "undefended"
    /// symbol.
    ///
    /// The CROSS-CHECK below compares the parse against
    /// [`raw_denied_path_strings`]: every path-shaped string in the array must
    /// be in the parsed set, so a future spelling this extraction misses fails
    /// a test instead of silently losing a deny.
    fn denied_symbols_from_clippy_toml() -> BTreeSet<String> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("clippy.toml");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let document: toml::Value = toml::from_str(&text)
            .unwrap_or_else(|error| panic!("{} is not valid TOML: {error}", path.display()));
        let entries = document
            .get("disallowed-methods")
            .and_then(toml::Value::as_array)
            .unwrap_or_else(|| {
                panic!(
                    "{} must carry a `disallowed-methods` array of entries",
                    path.display()
                )
            });
        let mut out = BTreeSet::new();
        for entry in entries {
            let denied = entry
                .get("path")
                .and_then(toml::Value::as_str)
                .unwrap_or_else(|| {
                    panic!(
                        "every `disallowed-methods` entry must carry a string `path`; this one \
                         does not: {entry:?}"
                    )
                });
            out.insert(denied.to_string());
        }
        assert!(
            out.len() > 15,
            "the clippy.toml deny list must parse to the real list: {out:?}"
        );
        let found = raw_denied_path_strings(&text);
        let missed: Vec<&String> = found.difference(&out).collect();
        assert!(
            missed.is_empty(),
            "{} carries path-shaped string(s) the TOML parse did not extract as \
             `disallowed-methods` `path` values: {missed:?}. Every spelling clippy accepts must \
             enter the derived deny table, or the count pin and the crate-wide deny disagree; \
             give the spelling a `path` key or remove the stray path-shaped string",
            path.display()
        );
        out
    }

    /// The SYNTACTIC spelling of a denied TRAIT method whose call form on a
    /// `std::fs` builder is a METHOD call the resolver attributes to the
    /// builder's owner. `clippy.toml` must name the RESOLVED trait path
    /// (`std::os::unix::fs::OpenOptionsExt::custom_flags`), because that is
    /// what the compiler resolves; the surface derivation and the count pin
    /// see the SYNTACTIC builder method (`std::fs::OpenOptions::custom_flags`),
    /// because `syn` has no type information. Mapping one to the other keeps
    /// the closure test and the deny from silently disagreeing — the round-8
    /// finding where the closure's review list carried a symbol the deny list
    /// did not name.
    fn synthetic_builder_spelling(path: &str) -> Option<String> {
        let segments: Vec<&str> = path.split("::").collect();
        let [std, os, unix_or_windows, fs, owner, method] = segments.as_slice() else {
            return None;
        };
        if *std != "std"
            || *os != "os"
            || (*unix_or_windows != "unix" && *unix_or_windows != "windows")
            || *fs != "fs"
        {
            return None;
        }
        let owner = match *owner {
            "OpenOptionsExt" => "OpenOptions",
            "DirBuilderExt" => "DirBuilder",
            _ => return None,
        };
        Some(format!("std::fs::{owner}::{method}"))
    }

    /// `clippy.toml`'s deny list AS THE SURFACE DERIVATION AND THE COUNT PIN SEE
    /// IT: every raw `path = "…"` entry, PLUS the SYNTACTIC builder spelling of
    /// each denied trait method ([`synthetic_builder_spelling`]).
    ///
    /// [`denied_symbols_from_clippy_toml`] stays the literal file read; this is
    /// the reconciled view the closure test consumes (so a denied `custom_flags`
    /// cannot be simultaneously "denied" per the lint and "undefended" per the
    /// derivation) and [`name_mutation_symbols`] derives the count pin from it,
    /// so the pin counts the same symbol the closure defends.
    fn reconciled_denied_symbols() -> BTreeSet<String> {
        let mut out = denied_symbols_from_clippy_toml();
        let extras: Vec<String> = out
            .iter()
            .filter_map(|path| synthetic_builder_spelling(path))
            .collect();
        out.extend(extras);
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
    ///
    /// The USED side sees three spellings of a name-adopting call (see
    /// [`FunnelSymbols`]): a path-callee call at any depth (`std::fs::File::create`,
    /// `std::fs::copy`), a builder METHOD call whose receiver chain or enclosing
    /// `let` binding resolves to an `OpenOptions`/`DirBuilder`
    /// (`std::fs::OpenOptions::create_new`), and a canonical path spelled in a
    /// macro body.
    ///
    /// NAMED RESIDUALS of the DERIVATION (not of the deny, whose resolution is
    /// type-based): the visitor recognises a builder by its SYNTACTIC chain or
    /// by a `let` binding in an ENCLOSING block, so a builder that reaches a
    /// method through a FUNCTION PARAMETER or a STRUCT FIELD (`fn f(o: &mut
    /// OpenOptions) { o.create(true) }`), through a FUNCTION RETURN
    /// (`make_opts().create(true)` — measured: such a body puts only
    /// `std::fs::OpenOptions::new` in `used`, not the `OpenOptions::create` the
    /// chain spelling records), through a FUNCTION-POINTER binding (`let c =
    /// std::fs::File::create; c(p)`), or whose method call sits inside a macro
    /// body (`o.create(true)`) is not recorded in `used`. THE MITIGATION IS
    /// NARROWER THAN IT LOOKS: the crate-wide deny is ALLOWED inside a funnel
    /// module — that is what makes the funnel the blind spot this derivation
    /// exists to cover — so a FUNNEL-ONLY use reached through one of these
    /// spellings is invisible to BOTH devices: this derivation does not record
    /// it, and the deny cannot fire where it is allowed. A new funnel primitive
    /// must therefore spell the call directly or as a local binding, both of
    /// which the arms above see; this residual is a real reach INSIDE the
    /// funnel, not a hole the lint closes.
    ///
    /// ROUND 11 P2-C CLOSED ONE SPELLING: a `type` ALIAS of the builder type
    /// used to be a hole in BOTH devices — inside a funnel module,
    /// `type ZZBuilder = std::fs::OpenOptions;` followed by
    /// `ZZBuilder::new().write(true).create_new(true).open(p)?` left clippy,
    /// this derivation and the count pin all green, because the owner was keyed
    /// on the literal last-segment name and only `use … as` aliases were
    /// resolved. [`build_index`] now resolves local non-generic `type` aliases
    /// too, so the spelling IS seen; the direct-spelling control still exists in
    /// [`funnel_symbol_surface_sees_associated_builder_and_local_arms`].
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
        // The RECONCILED view: raw clippy.toml entries plus the syntactic
        // builder spelling of a denied trait method, so a `custom_flags` the
        // deny names under `std::os::…::OpenOptionsExt` is defended under the
        // `std::fs::OpenOptions::custom_flags` spelling the derivation records.
        let denied = reconciled_denied_symbols();

        // SANITY: the derivation must see the funnel, or an empty `used` set
        // would make the closure vacuous. `create_dir_all` is a creation
        // wrapper; `openat` is the funnel's canonical open. The two INHERENT
        // arms are what this change added: `std::fs::File::from`/`File::open`
        // are ASSOCIATED calls (four-segment paths the old three-segment filter
        // dropped) and `std::fs::OpenOptions::create_new`/`open` are BUILDER
        // METHOD calls, so a regression that stopped seeing either would leave
        // the closure silently blind to exactly the escaping spellings.
        assert!(
            used.contains("std::fs::create_dir_all")
                && used.contains("libc::openat")
                && used.contains("std::fs::File::from")
                && used.contains("std::fs::OpenOptions::create_new")
                && used.contains("std::fs::OpenOptions::open"),
            "the funnel symbol derivation must see the funnel's own creation, open, associated, \
             and builder-method calls, or the closure is vacuous: {used:?}"
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

    /// ROUND-8 P1-B RECONCILIATION. `OpenOptionsExt::custom_flags` forwards
    /// ARBITRARY bits to `open(2)`, and `O_CREAT` through it ADOPTS a name. The
    /// lint must name the RESOLVED trait path
    /// (`std::os::{unix,windows}::fs::OpenOptionsExt::custom_flags`); the surface
    /// derivation sees the SYNTACTIC builder spelling
    /// (`std::fs::OpenOptions::custom_flags`). [`synthetic_builder_spelling`]
    /// and [`reconciled_denied_symbols`] make the two agree, so the deny and the
    /// closure test cannot drift — the exact failure this round found, where the
    /// review list carried the synthetic spelling with a false "cannot adopt"
    /// reason while the lint did not name the method at all.
    #[test]
    fn custom_flags_adoption_is_denied_and_reconciled_across_both_spellings() {
        let raw = denied_symbols_from_clippy_toml();
        for trait_path in [
            "std::os::unix::fs::OpenOptionsExt::custom_flags",
            "std::os::windows::fs::OpenOptionsExt::custom_flags",
        ] {
            assert!(
                raw.contains(trait_path),
                "clippy.toml must deny the RESOLVED trait path {trait_path}: {raw:?}"
            );
        }
        let reconciled = reconciled_denied_symbols();
        assert!(
            reconciled.contains("std::fs::OpenOptions::custom_flags"),
            "the reconciled deny view must carry the SYNTACTIC builder spelling the derivation \
             records: {reconciled:?}"
        );
        assert_eq!(
            synthetic_builder_spelling("std::os::unix::fs::OpenOptionsExt::custom_flags")
                .as_deref(),
            Some("std::fs::OpenOptions::custom_flags")
        );
        assert_eq!(
            synthetic_builder_spelling("std::os::windows::fs::OpenOptionsExt::custom_flags")
                .as_deref(),
            Some("std::fs::OpenOptions::custom_flags")
        );
        // The false review entry is GONE, and the synthetic spelling is now a
        // DENIED symbol, so the closure defends it instead of excusing it.
        assert!(
            !FUNNEL_SYMBOLS_NOT_DENIED.contains(&"std::fs::OpenOptions::custom_flags"),
            "the review list must not excuse a symbol the deny list names"
        );
        // The count pin counts the method too, because its table derives from
        // the SAME reconciled view.
        assert_eq!(
            name_mutation_symbols().get("OpenOptions::custom_flags"),
            Some(&vec![
                "std".to_string(),
                "fs".to_string(),
                "OpenOptions".to_string(),
                "custom_flags".to_string(),
            ])
        );
    }

    /// ROUND-8 P2-A REGRESSION: a pathname-socket `bind` CREATES a directory
    /// entry — a name ADOPTION in the same class as `create_dir`/`symlink` — so
    /// it is denied crate-wide for both `UnixListener` and `UnixDatagram`, the
    /// surface derivation can SEE it if the funnel ever uses one, the count pin
    /// counts it, and the raw `libc::bind` syscall is on the `libc` belt. The
    /// Windows named-pipe creator has no stable `std` path and is stated as a
    /// NAMED RESIDUAL in `clippy.toml` rather than left unmentioned.
    #[test]
    fn pathname_socket_binds_are_denied_counted_and_on_the_libc_belt() {
        let raw = denied_symbols_from_clippy_toml();
        for bind in [
            "std::os::unix::net::UnixListener::bind",
            "std::os::unix::net::UnixDatagram::bind",
        ] {
            assert!(raw.contains(bind), "clippy.toml must deny {bind}: {raw:?}");
            let segments: Vec<String> = bind.split("::").map(str::to_string).collect();
            assert_eq!(
                funnel_symbol(&segments).as_deref(),
                Some(bind),
                "the surface derivation must SEE {bind}"
            );
            assert!(
                name_mutation_symbols().contains_key(pin_key_for(&segments).unwrap().as_str()),
                "the pin must count {bind}"
            );
        }
        let files = vec![(
            "src/prod/bind.rs".to_string(),
            "#![allow(clippy::disallowed_methods)]\nfn f(p: &Path) { let _ = \
             std::os::unix::net::UnixListener::bind(p); }"
                .to_string(),
        )];
        let counts = std_fs_audit(&files, &BTreeSet::new()).1;
        assert_eq!(
            counts
                .get(&("src/prod/bind.rs".to_string(), "UnixListener::bind"))
                .copied(),
            Some(1),
            "the pin must count a pathname-socket bind: {counts:?}"
        );
        // The raw syscall is on the belt, so a `libc::bind` reference outside
        // the funnel is refused by `no_libc_reference_outside_the_funnel`.
        assert!(MUTATING_LIBC_SYSCALLS.contains(&"bind"));
        assert!(mutating_libc_belt(&[]).contains("bind"));
    }

    /// The surface derivation must SEE each spelling a name-ADOPTING call can
    /// take inside the funnel — a four-segment ASSOCIATED call, a builder
    /// METHOD on a syntactic chain, and a builder carried by a `let` binding
    /// (including through a `use … as` alias) — or `clippy.toml`'s list could
    /// drift from the code without
    /// [`every_mutation_symbol_the_funnel_uses_is_denied_crate_wide`]
    /// noticing. This is the round-6 finding's regression guard: the old
    /// three-segment filter and path-callee-only visitor saw NEITHER the
    /// inherent nor the builder form.
    #[test]
    fn funnel_symbol_surface_sees_associated_builder_and_local_arms() {
        let funnel = r##"#![allow(clippy::disallowed_methods)]
use std::fs::OpenOptions;
use std::fs::OpenOptions as O;

fn associated(p: &std::path::Path) {
    // 4-segment ASSOCIATED calls: the old 3-segment filter dropped these.
    let _ = std::fs::DirBuilder::new();
    let _ = std::fs::File::from(p);
    let _ = std::fs::File::create_new(p);
}

fn chained(p: &std::path::Path) {
    // The BUILDER form: the flag method is what decides adoption.
    let _ = std::fs::OpenOptions::new().write(true).create_new(true).open(p);
    let _ = OpenOptions::new().append(true).open(p);
    let _ = O::new().truncate(false).open(p);
}

fn stable_constructor_synonyms(p: &std::path::Path) {
    // `File::options()` and `OpenOptions::default()` are the SAME builder as
    // `OpenOptions::new()`, so a chain rooted at either must resolve to the
    // same owner or its `create`/`create_new` flag would be invisible.
    let _ = std::fs::File::options().write(true).create(true).open(p);
    let _ = std::fs::OpenOptions::default().read(true).open(p);
    let _ = std::fs::DirBuilder::default();
}

fn split_local() {
    // The SPLIT builder spelling: the owner rides a `let` binding.
    let mut opts = OpenOptions::new();
    opts.append(true);
}

// ROUND-11 P2-C: a local non-generic `type` alias of the builder type. The
// resolver used to key the owner on the literal last-segment name, so this
// spelling recorded NOTHING and the closure + count pin were both blind to it;
// a `use … as` alias WAS resolved.
type ZZBuilder = std::fs::OpenOptions;

fn type_alias_builder(p: &std::path::Path) {
    let _ = ZZBuilder::new().mode(0o600).create_new(true).open(p);
}

fn block_type_alias_builder(p: &std::path::Path) {
    type ZZLocalBuilder = std::fs::DirBuilder;
    let _ = ZZLocalBuilder::new().recursive(true).create(p);
}

fn not_attributed(p: &std::path::Path) {
    // CONTROLS: a method on the `open` RESULT, and a method on an unrelated
    // local, must NOT be attributed to the builder.
    let _ = OpenOptions::new().write(true).open(p).map_err(|e| e);
    let mut v = Vec::new();
    v.push(1u8);
}
"##;
        let files = vec![("src/prod/arms.rs".to_string(), funnel.to_string())];
        let used = funnel_symbol_surface(&files, &BTreeSet::new());

        for expected in [
            "std::fs::DirBuilder::new",
            "std::fs::File::from",
            "std::fs::File::create_new",
            "std::fs::OpenOptions::create_new",
            "std::fs::OpenOptions::append",
            // The aliased `O::new().truncate(…)` chain must resolve through the
            // alias table to the same `OpenOptions` owner as the bare spelling.
            "std::fs::OpenOptions::truncate",
            "std::fs::OpenOptions::write",
            "std::fs::OpenOptions::open",
            // The STABLE CONSTRUCTOR synonyms, and the flag method whose
            // adoption they expose: without the `builder_type` extension the
            // `File::options().create(true)` spelling recorded only the
            // constructor, never the `OpenOptions::create` flag.
            "std::fs::File::options",
            "std::fs::OpenOptions::default",
            "std::fs::DirBuilder::default",
            "std::fs::OpenOptions::create",
            "std::fs::OpenOptions::read",
            // ROUND-11 P2-C: the builder reached through a local non-generic
            // `type` ALIAS. `mode`/`recursive` are spelled ONLY through the
            // aliases here, so their presence pins the alias resolution rather
            // than the direct spelling beside it.
            "std::fs::OpenOptions::mode",
            "std::fs::DirBuilder::recursive",
        ] {
            assert!(
                used.contains(expected),
                "the derivation must SEE the inherent/builder spelling {expected}: {used:?}"
            );
        }
        assert!(
            !used.contains("std::fs::OpenOptions::map_err")
                && !used.contains("std::fs::OpenOptions::push"),
            "a method on the `open` result / an unrelated local must NOT be attributed to the \
             builder: {used:?}"
        );

        // The arms are gated on the `#[allow]`: a NON-funnel file contributes
        // nothing, so the surface is the funnel's, not the crate's.
        let outside = r#"fn a(p: &std::path::Path) { let _ = std::fs::File::create(p); }"#;
        let files = vec![("src/prod/outside.rs".to_string(), outside.to_string())];
        let used = funnel_symbol_surface(&files, &BTreeSet::new());
        assert!(
            used.is_empty(),
            "a non-funnel file must contribute no funnel symbols: {used:?}"
        );
    }

    /// ROUND-6 REGRESSION ARM (the round whose log CLAIMED this arm): the
    /// surface derivation must see an `#[allow(clippy::disallowed_methods)]`
    /// on an IMPL METHOD and on a TRAIT METHOD exactly as it sees one on a
    /// freestanding `fn`. The impl/trait forms are `syn::ImplItem` /
    /// `syn::TraitItem`, which `visit_item` never reaches, so before the
    /// `visit_impl_item`/`visit_trait_item` overrides the A and B arms below
    /// contributed NOTHING and
    /// `every_mutation_symbol_the_funnel_uses_is_denied_crate_wide` was blind
    /// to their bodies — including the five PRODUCTION create-only impl-method
    /// sites this crate already ships.
    ///
    /// The symbol each body calls — `std::fs::DirBuilder::new` — is neither on
    /// `clippy.toml`'s deny list nor on `FUNNEL_SYMBOLS_NOT_DENIED`, so if it
    /// were reached in the REAL funnel the closure test's `undefended`
    /// assertion would fail. The assertion at the end states those two facts,
    /// which is what makes A/B load-bearing rather than decorative.
    #[test]
    fn funnel_symbol_surface_sees_item_level_allow_on_impl_and_trait_methods() {
        const SYMBOL: &str = "std::fs::DirBuilder::new";
        let surface = |body: &str| {
            let files = vec![("src/prod/item_allow.rs".to_string(), body.to_string())];
            funnel_symbol_surface(&files, &BTreeSet::new())
        };

        // A — an `#[allow]`-ed IMPL METHOD. No module-level allow exists, so
        // this item is the ONLY funnel region in the file.
        let impl_method = r##"
struct S;
impl S {
    #[allow(clippy::disallowed_methods)]
    fn adopt(p: &std::path::Path) {
        let _ = std::fs::DirBuilder::new();
        let _ = p;
    }
}
"##;
        assert!(
            surface(impl_method).contains(SYMBOL),
            "the derivation must see an `#[allow]`-ed impl method's body, or the closure test is \
             blind to every item-level-allow impl method"
        );

        // B — the paired TRAIT-METHOD form: the same `#[allow]` on a different
        // `syn` node shape.
        let trait_method = r##"
trait T {
    #[allow(clippy::disallowed_methods)]
    fn adopt(p: &std::path::Path) {
        let _ = std::fs::DirBuilder::new();
        let _ = p;
    }
}
"##;
        assert!(
            surface(trait_method).contains(SYMBOL),
            "the derivation must see an `#[allow]`-ed trait method's body"
        );

        // C — the CONTROL the reviewers used: the freestanding `#[allow]`
        // form. It was already seen before the fix, so it isolates the
        // impl/trait arms rather than proving the whole file is parsed.
        let freestanding = r##"
#[allow(clippy::disallowed_methods)]
fn adopt(p: &std::path::Path) {
    let _ = std::fs::DirBuilder::new();
    let _ = p;
}
"##;
        assert!(
            surface(freestanding).contains(SYMBOL),
            "the freestanding `#[allow]` control must be seen (it was, before the fix)"
        );

        // D — NEGATIVE CONTROL: the SAME impl method with NO `#[allow]` is not
        // a funnel region, so its body must contribute nothing. Without this,
        // a derivation that treated every method as a funnel would pass A/B
        // vacuously.
        let unallowed_impl_method = r##"
struct S;
impl S {
    fn adopt(p: &std::path::Path) {
        let _ = std::fs::DirBuilder::new();
        let _ = p;
    }
}
"##;
        assert!(
            !surface(unallowed_impl_method).contains(SYMBOL),
            "an impl method with NO `#[allow]` is not a funnel region and must contribute nothing"
        );

        // The two facts that make A/B load-bearing for
        // `every_mutation_symbol_the_funnel_uses_is_denied_crate_wide`: the
        // symbol is IN the derived surface and OUTSIDE both review doors, so
        // the closure test would name it `undefended`.
        let denied = reconciled_denied_symbols();
        assert!(
            !denied.contains(SYMBOL) && !FUNNEL_SYMBOLS_NOT_DENIED.contains(&SYMBOL),
            "{SYMBOL} must stay outside both review doors for this arm to prove the closure test \
             would fail on a reached item-level-allow method"
        );
    }

    /// ROUND-11 P3-D REGRESSION ARM. `funnel_modules` / `funnel_symbol_surface`
    /// treat a CHILD FILE of an allow-bearing parent module as inside the region
    /// by an ANCESTOR-OR-SELF prefix test ([`module_is_in_funnel_region`]), the
    /// exact reach of Rust's attribute inheritance. Round 10 shipped that
    /// derivation with NO test that fails when it regresses to an EXACT-module
    /// match: the only symbol `src/atomic/guard.rs` contributes to the real
    /// closure is already on the deny list, so replacing the prefix test with
    /// equality left all 690 tests green.
    ///
    /// This arm drives the closure over a SYNTHETIC source set — an
    /// allow-bearing parent module (`src/prod/parent/mod.rs`, which is how a
    /// `#![allow(clippy::disallowed_methods)]` in a `mod.rs` reaches its child
    /// FILES) plus a CHILD file (`src/prod/parent/child.rs`) using
    /// `std::fs::DirBuilder::new`, a symbol that is NEITHER denied NOR reviewed.
    /// An exact-match derivation reports the child's use as UNDEFENDED and this
    /// test fails; only the ancestor-or-self prefix test reaches it. The symbol
    /// facts are asserted too, so the arm proves the CLOSURE would name the use
    /// rather than merely that a set contains a string.
    #[test]
    fn funnel_region_closure_reaches_a_child_file_of_an_allow_bearing_parent() {
        const SYMBOL: &str = "std::fs::DirBuilder::new";
        let files = vec![
            (
                "src/prod/parent/mod.rs".to_string(),
                "#![allow(clippy::disallowed_methods)]\nmod child;\n".to_string(),
            ),
            (
                "src/prod/parent/child.rs".to_string(),
                "pub fn adopt() { let _ = std::fs::DirBuilder::new(); }\n".to_string(),
            ),
        ];
        let gated = BTreeSet::new();
        let used = funnel_symbol_surface(&files, &gated);
        assert!(
            used.contains(SYMBOL),
            "the closure derivation must reach a CHILD FILE of an allow-bearing parent module \
             (Rust attribute inheritance), or the region derivation is exact-match only: \
             {used:?}"
        );
        let denied = reconciled_denied_symbols();
        assert!(
            !denied.contains(SYMBOL) && !FUNNEL_SYMBOLS_NOT_DENIED.contains(&SYMBOL),
            "{SYMBOL} must stay outside both review doors, so this arm proves the closure test \
             would report the child-file use as undefended"
        );
    }

    // ---------------------------------------------------------------------
    // CONSTRAINT #1 (docs/API-CONSTRAINTS.md): the pair-less mutation
    // enumeration is DERIVED from the public surface, not enumerated by hand.
    // ---------------------------------------------------------------------

    /// The marker pair the ONE machine-readable block in
    /// `docs/API-CONSTRAINTS.md` uses.
    const PAIR_LESS_BEGIN: &str = "<!-- PAIR-LESS-MUTATIONS:BEGIN -->";
    const PAIR_LESS_END: &str = "<!-- PAIR-LESS-MUTATIONS:END -->";

    /// A fn or method keyed by the triple a `module::[Owner::]name` path names,
    /// so a curated block name resolves to the ITEM it names rather than being
    /// matched by string.
    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    struct AuditedFn {
        module: Vec<String>,
        owner: Option<String>,
        name: String,
    }

    impl AuditedFn {
        fn rendered(&self) -> String {
            let mut out = self.module.join("::");
            if let Some(owner) = &self.owner {
                if !out.is_empty() {
                    out.push_str("::");
                }
                out.push_str(owner);
            }
            if !out.is_empty() {
                out.push_str("::");
            }
            out.push_str(&self.name);
            out
        }
    }

    /// One production fn/method plus the two facts the derivation needs.
    #[derive(Debug, Clone)]
    struct CollectedFn {
        item: AuditedFn,
        public: bool,
        takes_raw_path: bool,
    }

    fn type_last_segment(ty: &syn::Type) -> Option<String> {
        match ty {
            syn::Type::Path(type_path) => type_path
                .path
                .segments
                .last()
                .map(|segment| unraw(&segment.ident)),
            syn::Type::Reference(reference) => type_last_segment(&reference.elem),
            syn::Type::Paren(paren) => type_last_segment(&paren.elem),
            syn::Type::Group(group) => type_last_segment(&group.elem),
            _ => None,
        }
    }

    fn bound_is_raw_path(bound: &syn::TypeParamBound) -> bool {
        let syn::TypeParamBound::Trait(trait_bound) = bound else {
            return false;
        };
        let Some(last) = trait_bound.path.segments.last() else {
            return false;
        };
        if !matches!(unraw(&last.ident).as_str(), "AsRef" | "Into") {
            return false;
        }
        let syn::PathArguments::AngleBracketed(arguments) = &last.arguments else {
            return false;
        };
        arguments.args.iter().any(|argument| match argument {
            syn::GenericArgument::Type(ty) => type_is_raw_path(ty),
            _ => false,
        })
    }

    /// Whether `ty` IS a raw path argument: `&Path`, `&mut Path`, `PathBuf`,
    /// `&PathBuf`, or `Path`/`PathBuf` nested through a PATH-BEARING
    /// CONTAINER — `Option`, `Box`, `Cow`, `Rc`, `Arc`, `Vec`, `Result`, a
    /// slice/array (`&[PathBuf]`, `[PathBuf; N]`), or a tuple that has a path
    /// element (`(PathBuf,)`) — plus `impl AsRef<Path>` / `impl Into<PathBuf>`
    /// and a type parameter bounded the same way.
    ///
    /// OUT OF CLASS, deliberately: a bare `&str`, `String`, `&OsStr`, or
    /// `OsString` (a string is a path SPELLING, not a path); a CONTAINER of
    /// those spellings (`&[String]`, `Vec<String>`, `(String,)`,
    /// `Result<String>`), because the type system does not distinguish a
    /// path-spelled string from any other string and this crate has public fns
    /// taking one for a non-path reason; any type not on the container list
    /// above (`HashMap<_, PathBuf>`, `BTreeMap<_, PathBuf>`); and any ALIAS of
    /// `Path`/`PathBuf` — BOTH a `use … as` alias and a LOCAL non-generic
    /// `type` alias. The predicate matches the last-segment NAME, so
    /// `use std::path::Path as ZP;` and `type ZP = std::path::Path;` both leave
    /// `&ZP` outside the class. The boundary is a stated property pinned in
    /// BOTH directions by
    /// [`pair_less_derivation_boundary_is_the_syntactic_path_class`], not an
    /// implication of this list. The alias case is measured, not assumed:
    /// `audited_fns` resolves neither an import nor a `type` alias, so the
    /// predicate is deliberately name-based and the statement says so. (The
    /// FsIndex-backed derivations — the funnel surface and the `std::fs` count
    /// pin — DO resolve local non-generic `type` aliases; this syntactic
    /// derivation deliberately does not, and the `type` case is pinned in both
    /// directions to keep the statement matching the code.)
    fn type_is_raw_path(ty: &syn::Type) -> bool {
        match ty {
            syn::Type::Reference(reference) => type_is_raw_path(&reference.elem),
            syn::Type::Paren(paren) => type_is_raw_path(&paren.elem),
            syn::Type::Group(group) => type_is_raw_path(&group.elem),
            syn::Type::Slice(slice) => type_is_raw_path(&slice.elem),
            syn::Type::Array(array) => type_is_raw_path(&array.elem),
            syn::Type::Tuple(tuple) => tuple.elems.iter().any(type_is_raw_path),
            syn::Type::ImplTrait(impl_trait) => impl_trait.bounds.iter().any(bound_is_raw_path),
            syn::Type::TraitObject(trait_object) => {
                trait_object.bounds.iter().any(bound_is_raw_path)
            }
            syn::Type::Path(type_path) => {
                let Some(segment) = type_path.path.segments.last() else {
                    return false;
                };
                let name = unraw(&segment.ident);
                if matches!(name.as_str(), "Path" | "PathBuf") {
                    return true;
                }
                if !matches!(
                    name.as_str(),
                    "Option" | "Box" | "Cow" | "Rc" | "Arc" | "Vec" | "Result"
                ) {
                    return false;
                }
                let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
                    return false;
                };
                arguments.args.iter().any(|argument| match argument {
                    syn::GenericArgument::Type(inner) => type_is_raw_path(inner),
                    _ => false,
                })
            }
            _ => false,
        }
    }

    fn sig_takes_raw_path(signature: &syn::Signature) -> bool {
        let path_type_params = path_bounded_type_params(signature);
        signature.inputs.iter().any(|argument| match argument {
            syn::FnArg::Typed(pat_type) => {
                type_is_raw_path(&pat_type.ty)
                    || matches!(
                        &*pat_type.ty,
                        syn::Type::Path(type_path)
                            if type_path.qself.is_none()
                                && type_path.path.segments.len() == 1
                                && path_type_params
                                    .contains(&unraw(&type_path.path.segments[0].ident))
                    )
            }
            syn::FnArg::Receiver(_) => false,
        })
    }

    /// Type parameters bounded by `AsRef<Path>`/`Into<PathBuf>`, in the generic
    /// list or the `where` clause, so a param written `p: P where P: AsRef<Path>`
    /// is a raw path argument too.
    fn path_bounded_type_params(signature: &syn::Signature) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        for param in &signature.generics.params {
            if let syn::GenericParam::Type(type_param) = param
                && type_param.bounds.iter().any(bound_is_raw_path)
            {
                names.insert(unraw(&type_param.ident));
            }
        }
        if let Some(where_clause) = &signature.generics.where_clause {
            for predicate in &where_clause.predicates {
                if let syn::WherePredicate::Type(predicate_type) = predicate
                    && let syn::Type::Path(type_path) = &predicate_type.bounded_ty
                    && let Some(segment) = type_path.path.segments.last()
                    && predicate_type.bounds.iter().any(bound_is_raw_path)
                {
                    names.insert(unraw(&segment.ident));
                }
            }
        }
        names
    }

    fn collect_audited_fns(items: &[syn::Item], module: &[String], out: &mut Vec<CollectedFn>) {
        for item in items {
            match item {
                syn::Item::Fn(function) => out.push(CollectedFn {
                    item: AuditedFn {
                        module: module.to_vec(),
                        owner: None,
                        name: unraw(&function.sig.ident),
                    },
                    public: matches!(function.vis, syn::Visibility::Public(_)),
                    takes_raw_path: sig_takes_raw_path(&function.sig),
                }),
                syn::Item::Mod(module_item) => {
                    if let Some((_, inner)) = &module_item.content {
                        let mut child = module.to_vec();
                        child.push(unraw(&module_item.ident));
                        collect_audited_fns(inner, &child, out);
                    }
                }
                syn::Item::Impl(impl_item) => {
                    let owner = type_last_segment(&impl_item.self_ty);
                    for impl_child in &impl_item.items {
                        if let syn::ImplItem::Fn(function) = impl_child {
                            out.push(CollectedFn {
                                item: AuditedFn {
                                    module: module.to_vec(),
                                    owner: owner.clone(),
                                    name: unraw(&function.sig.ident),
                                },
                                // A TRAIT impl's methods are the TRAIT
                                // declaration's; that declaration is walked
                                // separately, so counting these public would
                                // double-count the same API item.
                                public: impl_item.trait_.is_none()
                                    && matches!(function.vis, syn::Visibility::Public(_)),
                                takes_raw_path: sig_takes_raw_path(&function.sig),
                            });
                        }
                    }
                }
                syn::Item::Trait(trait_item) => {
                    let public = matches!(trait_item.vis, syn::Visibility::Public(_));
                    let owner = unraw(&trait_item.ident);
                    for trait_child in &trait_item.items {
                        if let syn::TraitItem::Fn(function) = trait_child {
                            out.push(CollectedFn {
                                item: AuditedFn {
                                    module: module.to_vec(),
                                    owner: Some(owner.clone()),
                                    name: unraw(&function.sig.ident),
                                },
                                public,
                                takes_raw_path: sig_takes_raw_path(&function.sig),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn audited_fns(parsed: &[ParsedSource], gated: &BTreeSet<String>) -> Vec<CollectedFn> {
        let mut out = Vec::new();
        for source in parsed {
            if is_test_only(&source.rel, gated) {
                continue;
            }
            // `module_path_from_rel` anchors every `src/**` module at `crate`;
            // the block and exemption names are written without that root, so
            // strip it here and compare like for like.
            let mut module = source.module.clone();
            if module.first().map(String::as_str) == Some("crate") {
                module.remove(0);
            }
            collect_audited_fns(&source.file.items, &module, &mut out);
        }
        out
    }

    /// Resolve a `module::[Owner::]name` block/exemption name to the parsed
    /// items it names. The OWNER is the last segment before the fn name when it
    /// starts uppercase (`Residue::recover_to`, `Remote::lock_far_side`); the
    /// MODULE PREFIX is everything before that, and a name resolves when the
    /// item's source module STARTS WITH that prefix (so `sync::Residue::…`
    /// finds the `Residue` in `sync::residue`, its public re-export path).
    fn resolve_audited_name(name: &str, all: &[CollectedFn]) -> Vec<AuditedFn> {
        let segments: Vec<&str> = name
            .split("::")
            .filter(|segment| !segment.is_empty())
            .collect();
        let Some((fn_name, rest)) = segments.split_last() else {
            return Vec::new();
        };
        let (module_prefix, owner): (Vec<String>, Option<String>) = match rest.split_last() {
            Some((last, head))
                if last
                    .chars()
                    .next()
                    .is_some_and(|first| first.is_ascii_uppercase()) =>
            {
                (
                    head.iter().map(|segment| (*segment).to_string()).collect(),
                    Some((*last).to_string()),
                )
            }
            _ => (
                rest.iter().map(|segment| (*segment).to_string()).collect(),
                None,
            ),
        };
        all.iter()
            .filter(|collected| {
                collected.item.name == *fn_name
                    && collected.item.owner == owner
                    && collected.item.module.len() >= module_prefix.len()
                    && collected
                        .item
                        .module
                        .iter()
                        .zip(module_prefix.iter())
                        .all(|(actual, expected)| actual == expected)
            })
            .map(|collected| collected.item.clone())
            .collect()
    }

    /// The REVIEWED exemptions: public raw-path fns/methods that are
    /// deliberately NOT pair-less mutations, each with its reason. The
    /// derivation below requires the block to cover every OTHER public
    /// raw-path item, so this list is the residue a reviewer checks one entry
    /// at a time.
    const PAIR_LESS_EXEMPTIONS: &[(&str, &str)] = &[
        (
            "atomic::path_state",
            "lstat-style state read; no mutation (round 3)",
        ),
        (
            "atomic::temp_name_for",
            "derives a temp SPELLING; issues no syscall (round 3)",
        ),
        (
            "RootDir::open",
            "opens/validates the ROOT directory; creates nothing",
        ),
        (
            "atomic::symlink_fd",
            "the raw `&Path` is the symlink TARGET (content); the mutated NAME is the \
             `&RootedRelativePath`",
        ),
        (
            "manifest::canonicalize_tree",
            "read-only walk/hash of a source root",
        ),
        (
            "manifest::canonicalize_tree_destination",
            "read-only walk/hash of a destination root",
        ),
        (
            "manifest::canonicalize_remote_entries_checked",
            "assembles a manifest from already-collected far-side output; read-only",
        ),
        (
            "manifest::canonicalize_remote_entries_destination_checked",
            "the destination twin of the above; read-only",
        ),
        (
            "manifest::verify_tree_metadata",
            "read-only verification of a stored manifest",
        ),
        (
            "platform::file_mode",
            "reads an entry's mode; no mutation (round 3)",
        ),
        (
            "relpath::RootedRelativePath::parse",
            "parses a spelling into the validated type; issues no syscall",
        ),
        (
            "relpath::RootedRelativePath::join",
            "path algebra on the validated type; issues no syscall",
        ),
        (
            "OwnedRoot::parse",
            "parses an endpoint + path; issues no syscall",
        ),
        (
            "sync::apply::destination_lock_path",
            "COMPUTES the sibling record path; creates nothing",
        ),
        (
            "sync::diff::local_manifest",
            "read-only walk/hash of a source root (round 3)",
        ),
        (
            "sync::Residue::detect",
            "read-only detection of a stranded residue",
        ),
        (
            "sync::sync",
            "OPERATION ENTRY POINT: takes the LOCAL ROOT path, not a name inside it; every \
             mutation goes through the pair-based primitives internally",
        ),
        (
            "DestinationOwnership::lock",
            "OPERATION ENTRY POINT: takes the LOCAL ROOT path and mints the ownership token; \
             delegates every name mutation to the pair-based primitives",
        ),
        (
            "DestinationOwnership::lock_remote",
            "the far-side twin of `DestinationOwnership::lock`",
        ),
        (
            "DestinationOwnership::lock_with_in_root_lock",
            "the composed twin of `DestinationOwnership::lock`",
        ),
        (
            "transport::LocalTransport::new",
            "constructor that STORES the base path; issues no mutation",
        ),
        (
            "transport::LocalTransport::with_exec",
            "constructor that STORES the base path and the exec seam; issues no mutation",
        ),
        (
            "ChildRunner::new",
            "constructor that STORES a cwd; issues no mutation",
        ),
        (
            "transport::ssh::SshTransport::new",
            "constructor that STORES the deploy dir and known-hosts paths; issues no mutation",
        ),
        (
            "SshTransport::with_identity_file",
            "builder that STORES an identity path; issues no mutation",
        ),
        (
            "Remote::symlink",
            "the raw `&Path` is the symlink TARGET (content); the mutated NAME is the \
             `&RootedRelativePath`",
        ),
    ];

    /// ROUND-8 P2-B / ROUND-10 P3-B BOUNDARY. The pair-less derivation is
    /// SYNTACTIC on the parameter type, and its class is exactly
    /// `Path`/`PathBuf`, the same nested through a PATH-BEARING CONTAINER
    /// (`Option`/`Box`/`Cow`/`Rc`/`Arc`/`Vec`/`Result`, a slice/array, or a
    /// tuple with a path element), `impl AsRef<Path>`, `impl Into<PathBuf>`, or
    /// a type parameter bounded the same way. A bare `&str`, `String`,
    /// `&OsStr`, and `OsString` are NOT in the class, and NEITHER are CONTAINERS
    /// of those spellings (`&[String]`, `Vec<String>`, `(String,)`,
    /// `Result<String>`): a string is a path SPELLING, not a path, and this
    /// crate already has public fns that take one for non-path reasons
    /// (`is_reserved_name`, `valid_hex_digest`, the error constructors), so
    /// sweeping strings in would make the derivation a superset that no longer
    /// pins path mutations. A type not on the container list above
    /// (`HashMap<_, PathBuf>`) is likewise OUT. A `use … as` ALIAS of
    /// `Path`/`PathBuf` is NOT in the class: the predicate matches the
    /// last-segment NAME, and `audited_fns` resolves no imports, so `use
    /// std::path::Path as ZP; pub fn f(p: &ZP)` is out of class BY CONSTRUCTION.
    /// The boundary is therefore STATED, not implied, and this test pins that
    /// the code does what the statement says — both directions, for the
    /// containers, the strings-in-containers, and the alias case — so the
    /// statement cannot rot into a false claim about the code.
    ///
    /// The CONTAINER extension (round 10) is the fix for a measured hole: a
    /// `pub fn f(paths: &[PathBuf])`, `(PathBuf,)`, or `Result<PathBuf>` was
    /// neither in the stated in-class set nor the out-of-class set, and the
    /// derivation passed for all three while the `&Path` control failed. A
    /// container that carries a path IS a path-bearing public surface, so the
    /// predicate now recurses through it; the element-type recursion is pinned
    /// in BOTH directions (`&[Path]` in, `&[String]` out) rather than only the
    /// positive one.
    #[test]
    fn pair_less_derivation_boundary_is_the_syntactic_path_class() {
        let derived_in = |prelude: &str, param: &str| {
            let source = format!("{prelude}\npub fn probe{param} {{}}");
            let files = vec![("src/prod/boundary.rs".to_string(), source)];
            let parsed = parse_crate(&files, &BTreeSet::new());
            let fns = audited_fns(&parsed, &BTreeSet::new());
            fns.into_iter()
                .find(|collected| collected.item.name == "probe")
                .map(|collected| collected.takes_raw_path)
                .expect("the synthetic `probe` must parse")
        };
        let derived = |param: &str| derived_in("", param);
        // IN CLASS: a raw path, directly or through a path-bearing container.
        for param in [
            "(p: &Path)",
            "(p: PathBuf)",
            "(p: Option<&Path>)",
            "(p: Box<PathBuf>)",
            "(p: Rc<Path>)",
            "(p: Arc<PathBuf>)",
            "(p: Cow<'static, Path>)",
            "(p: Vec<PathBuf>)",
            "(p: &[Path])",
            "(p: &[PathBuf])",
            "(p: [PathBuf; 1])",
            "(p: (PathBuf,))",
            "(p: (u32, PathBuf))",
            "(p: Result<PathBuf>)",
            "(p: Result<PathBuf, std::io::Error>)",
            "(p: impl AsRef<Path>)",
            "(p: impl Into<PathBuf>)",
        ] {
            assert!(
                derived(param),
                "{param} is a raw path (directly or through a path-bearing container) and must be \
                 in the derived class"
            );
        }
        // OUT OF CLASS, the STATED boundary: a string spelling is not a path,
        // and a CONTAINER of string spellings is not a path either.
        for param in [
            "(name: &str)",
            "(name: String)",
            "(name: &OsStr)",
            "(name: OsString)",
            "(name: &[u8])",
            "(name: &[String])",
            "(name: Vec<String>)",
            "(name: (String,))",
            "(name: Result<String>)",
            "(name: std::collections::HashMap<String, PathBuf>)",
        ] {
            assert!(
                !derived(param),
                "{param} is the stated out-of-class boundary and must NOT be derived"
            );
        }
        // ELEMENT-TYPE RECURSION, both directions: the SAME container is in
        // class with a path element and out of class with a string element, so
        // the IN/OUT lists above pin the recursion rather than the container
        // NAME alone (`&[Path]`/`Vec<PathBuf>` in; `&[String]`/`Vec<String>`
        // out).
        // OUT OF CLASS: a `use … as` ALIAS of `Path`/`PathBuf`. The predicate
        // matches the last-segment NAME, not the resolution, so an aliased
        // spelling of a raw path is out of class BY CONSTRUCTION. Both
        // directions: the alias case is refused, and the canonical name beside
        // the same alias is still seen, so this pins the NAME-based predicate
        // rather than the presence of a `use`.
        for (prelude, param) in [
            ("use std::path::Path as ZP;", "(p: &ZP)"),
            ("use std::path::PathBuf as ZP;", "(p: ZP)"),
            // The ALIAS inside a container is the container twin of the same
            // boundary: the element is name-based too.
            ("use std::path::PathBuf as ZP;", "(p: &[ZP])"),
        ] {
            assert!(
                !derived_in(prelude, param),
                "{param} is a `use … as` ALIAS of a raw path and is the stated out-of-class \
                 boundary; it must NOT be derived"
            );
        }
        for (prelude, param) in [
            ("use std::path::Path as ZP;", "(p: &Path)"),
            ("use std::path::PathBuf as ZP;", "(p: PathBuf)"),
            ("use std::path::PathBuf as ZP;", "(p: &[PathBuf])"),
        ] {
            assert!(
                derived_in(prelude, param),
                "{param} names the canonical raw path and must be in the derived class even beside \
                 an alias"
            );
        }
        // OUT OF CLASS: a LOCAL non-generic `type` ALIAS of `Path`/`PathBuf`
        // (round 11 P2-E/P3-E). Like `use … as`, the predicate matches the
        // last-segment NAME, so `type ZP = std::path::Path;` leaves `&ZP`
        // outside the class BY CONSTRUCTION. The FsIndex-backed derivations DO
        // resolve `type` aliases; this syntactic derivation deliberately does
        // not, and the boundary statement says so. Both directions: the alias
        // is refused, the canonical name beside it is still seen, and the
        // literal spelling `Path` beside an UNRELATED `type` alias is
        // unaffected.
        for (prelude, param) in [
            ("type ZP = std::path::Path;", "(p: &ZP)"),
            ("type ZP = std::path::PathBuf;", "(p: ZP)"),
            ("type ZP = std::path::PathBuf;", "(p: &[ZP])"),
            // A CHAINED `type` alias must not leak either: `ZQ` names `ZP`,
            // which names `Path`, and the name-based predicate still refuses it.
            ("type ZP = std::path::Path;\ntype ZQ = ZP;", "(p: &ZQ)"),
        ] {
            assert!(
                !derived_in(prelude, param),
                "{param} is a local `type` ALIAS of a raw path / an unrelated type and is the \
                 stated out-of-class boundary; it must NOT be derived"
            );
        }
        for (prelude, param) in [
            ("type ZP = std::path::Path;", "(p: &Path)"),
            ("type ZP = std::path::PathBuf;", "(p: PathBuf)"),
            ("type ZP = std::path::PathBuf;", "(p: &[PathBuf])"),
        ] {
            assert!(
                derived_in(prelude, param),
                "{param} names the canonical raw path and must be in the derived class even beside \
                 a local `type` alias"
            );
        }
    }

    /// The token streams of every `macro_rules!` DEFINITION in `file`, rendered
    /// as text. `syn` does not descend into a macro's token stream (its
    /// `visit_token_stream` hook is a no-op), so this is the only way into a
    /// macro body; the emitted items are then inspected by
    /// [`macro_body_public_path_fns`].
    fn macro_rules_bodies(file: &syn::File) -> Vec<String> {
        struct Macros(Vec<String>);
        impl<'ast> syn::visit::Visit<'ast> for Macros {
            fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
                if item.mac.path.is_ident("macro_rules") {
                    self.0.push(item.mac.tokens.to_string());
                }
                syn::visit::visit_item_macro(self, item);
            }
        }
        let mut macros = Macros(Vec::new());
        syn::visit::Visit::visit_file(&mut macros, file);
        macros.0
    }

    /// The index of the closer matching the group opener at `open` in a flat
    /// token list, or `None` when unbalanced.
    fn match_group(list: &[String], open: usize) -> Option<usize> {
        let (open_token, close_token) = match list.get(open).map(String::as_str)? {
            "(" => ("(", ")"),
            "[" => ("[", "]"),
            "{" => ("{", "}"),
            _ => return None,
        };
        let mut depth = 0usize;
        for (offset, token) in list[open..].iter().enumerate() {
            if token == open_token {
                depth += 1;
            } else if token == close_token {
                depth -= 1;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
        }
        None
    }

    /// The index of the `>` matching the `<` at `open` in a flat token list, or
    /// `None` when unbalanced. `->` is TWO flat tokens (`-`, `>`), so the `>`
    /// that closes an arrow is not counted; a nested `>` (as in `Vec<Vec<T>>`)
    /// is.
    fn match_angle_group(list: &[String], open: usize) -> Option<usize> {
        if list.get(open).map(String::as_str) != Some("<") {
            return None;
        }
        let mut depth = 0usize;
        let mut previous = "";
        for (offset, token) in list[open..].iter().enumerate() {
            match token.as_str() {
                "<" => depth += 1,
                ">" if previous != "-" => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(open + offset);
                    }
                }
                _ => {}
            }
            previous = token;
        }
        None
    }

    /// The names of `pub fn` items a macro TOKEN STREAM spells with a
    /// `Path`/`PathBuf` token inside their parameter list. This is the
    /// token-level backstop for [`production_macro_bodies_emitting_public_path_fns_are_refused`];
    /// it deliberately scans TOKENS rather than parsing items, so it still sees
    /// a `pub fn $name(p: &Path)` whose metavariable would make an item parse
    /// fail. It is conservative: a `Path` token anywhere in a `pub fn`
    /// parameter list, a trailing `where` clause, or (round 11) behind a
    /// `<…>` GENERIC-PARAMETER group is enough, so it cannot pass by accident.
    fn macro_body_public_path_fns(tokens: impl std::fmt::Display) -> Vec<String> {
        let list = macro_token_list(tokens);
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < list.len() {
            if list[i] != "pub" {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            if list.get(j).map(String::as_str) == Some("(") {
                j = match_group(&list, j).map_or(j + 1, |close| close + 1);
            }
            while matches!(
                list.get(j).map(String::as_str),
                Some("async" | "unsafe" | "const" | "extern")
            ) {
                j += 1;
            }
            if list.get(j).map(String::as_str) != Some("fn") {
                i += 1;
                continue;
            }
            let mut k = j + 1;
            if list.get(k).map(String::as_str) == Some("$") {
                k += 1;
            }
            let Some(name) = list.get(k).cloned() else {
                break;
            };
            k += 1;
            // ROUND-11 P2-B: skip a `<…>` GENERIC-PARAMETER group after the
            // name. Round 10 expected `(` IMMEDIATELY, so
            // `pub fn zz_generic_path_fn<T>(_p: &std::path::Path) {}` was
            // skipped entirely — a real public raw-path mutator with the
            // tripwire AND the pair-less derivation both green.
            if list.get(k).map(String::as_str) == Some("<") {
                match match_angle_group(&list, k) {
                    Some(close) => k = close + 1,
                    None => {
                        i += 1;
                        continue;
                    }
                }
            }
            if list.get(k).map(String::as_str) != Some("(") {
                i += 1;
                continue;
            }
            let close = match match_group(&list, k) {
                Some(close) => close,
                None => {
                    i += 1;
                    continue;
                }
            };
            // ROUND-11 P2-B: the round-10 scan stopped at the parameter list,
            // so `pub fn f<T>(p: T) where T: AsRef<Path> {}` was invisible too.
            // Extend the scanned range across a trailing `where` clause, up to
            // the body opener or the declaration terminator.
            let mut end = close + 1;
            if list.get(end).map(String::as_str) == Some("where") {
                let mut depth = 0i32;
                while end < list.len() {
                    match list[end].as_str() {
                        "{" | ";" if depth == 0 => break,
                        "(" | "[" => depth += 1,
                        ")" | "]" => depth -= 1,
                        _ => {}
                    }
                    end += 1;
                }
            }
            if list[k + 1..end]
                .iter()
                .any(|token| token == "Path" || token == "PathBuf")
            {
                out.push(name);
            }
            i = end;
        }
        out
    }

    /// ROUND-10 P3-A TRIPWIRE. `collect_audited_fns` walks the `syn` item
    /// graph, and `syn` does NOT descend into a macro's token stream (its
    /// `visit_token_stream` hook is a no-op), so a `macro_rules!` body that
    /// emits a `pub fn` taking a raw path is NOT derived: the walk sees the
    /// macro ITEM and stops. The measured repro was
    ///
    /// ```text
    /// macro_rules! zz_make { () => { pub fn zz_macro_mutate(_p: &Path) {} }; }
    /// zz_make!();
    /// ```
    ///
    /// in production: `zz_macro_mutate` is publicly reachable, and while the
    /// direct control `pub fn zz_direct_mutate(_p: &Path)` fails
    /// `pair_less_mutation_enumeration_is_the_derived_public_surface`, the
    /// macro-generated fn passed every device. The class is stated OUT-OF-CLASS
    /// in the derivation's boundary: a macro body is a TOKEN stream, not a
    /// `syn::Item` sequence, and it may carry metavariables, fragment
    /// specifiers, and repetitions, so parsing it as items is unsound in
    /// general. The existing `std::fs` token walker CANNOT be reused soundly
    /// here: it flattens tokens into a `Vec<String>` and recognises only
    /// canonical `::`-joined paths, so it has no notion of an ITEM, a
    /// VISIBILITY, or a parameter TYPE and cannot recover a signature.
    ///
    /// Because the class is out of class, this TRIPWIRE is the control that
    /// keeps the stated boundary honest: it fails the moment a PRODUCTION
    /// `macro_rules!` body spells a `pub fn` whose parameter list mentions
    /// `Path`/`PathBuf`. Adding such a macro is therefore a visible decision —
    /// extend the derivation to expand the body, or enumerate the emitted item
    /// — rather than a silent omission.
    ///
    /// RESIDUE, named: the scan is TOKEN-level, so a macro that builds a path
    /// type out of a metavariable (`pub fn f(p: &$ty)`) or an alias
    /// (`pub fn f(p: ZP)` under `use std::path::Path as ZP`) is not seen. The
    /// scan is conservative in the other direction (a `Path`/`PathBuf` token
    /// anywhere in a `pub fn` parameter list, a trailing `where` clause, or
    /// behind a `<…>` generic-parameter group is refused), so it cannot pass by
    /// accident.
    ///
    /// ROUND 11 P2-B: the round-10 scan required `(` IMMEDIATELY after the fn
    /// name, so `pub fn zz_generic_path_fn<T>(_p: &std::path::Path) {}` was
    /// skipped, and it stopped at the parameter-list close, so a
    /// `where T: AsRef<Path>` bound was invisible too. Both are now scanned and
    /// both are pinned as test arms below.
    #[test]
    fn production_macro_bodies_emitting_public_path_fns_are_refused() {
        // ROUND-11 P2-B ARMS: the token scan must see a `pub fn` whose name is
        // followed by a `<…>` generic-parameter group and/or a `where` clause.
        // Before the fix these returned EMPTY, which is why a generic
        // `pub fn zz<T>(_p: &Path)` planted in a production `macro_rules!` body
        // passed with the tripwire green.
        assert_eq!(
            macro_body_public_path_fns("pub fn zz_generic_path_fn<T>(_p: &std::path::Path) {}"),
            vec!["zz_generic_path_fn".to_string()],
            "a `pub fn` followed by a `<…>` generic-parameter group must still be scanned"
        );
        assert_eq!(
            macro_body_public_path_fns("pub fn zz_lifetime_path_fn<'a>(_p: &'a Path) {}"),
            vec!["zz_lifetime_path_fn".to_string()],
            "a lifetime-parameter group is a `<…>` group too"
        );
        assert_eq!(
            macro_body_public_path_fns("pub fn zz_where_path_fn<T>(_p: T) where T: AsRef<Path> {}"),
            vec!["zz_where_path_fn".to_string()],
            "the scan must reach a trailing `where` clause"
        );
        // NEGATIVE CONTROLS: a generic fn with NO path token, and a generic fn
        // whose `->` return arrow must not unbalance the `<>` scan.
        assert!(
            macro_body_public_path_fns("pub fn zz_generic_plain_fn<T>(_p: &T) {}").is_empty(),
            "a generic fn with no Path/PathBuf token must NOT be an offender"
        );
        assert!(
            macro_body_public_path_fns("pub fn zz_nonpath_fn<T: Into<u64>>(_p: T) -> u64 { 0 }")
                .is_empty(),
            "a `->` arrow's `>` must not unbalance the generic-group scan"
        );

        let mut files = Vec::new();
        collect_crate_rs_files(Path::new(env!("CARGO_MANIFEST_DIR")), &mut files);
        let sources: Vec<(String, String)> = files
            .iter()
            .map(|file| {
                (
                    crate_relative(file),
                    std::fs::read_to_string(file).expect("read source file"),
                )
            })
            .collect();
        let gated = test_only_gated_paths();
        let parsed = parse_crate(&sources, &gated);
        let mut offenders: Vec<String> = Vec::new();
        for source in &parsed {
            if is_test_only(&source.rel, &gated) {
                continue;
            }
            for body in macro_rules_bodies(&source.file) {
                for name in macro_body_public_path_fns(&body) {
                    offenders.push(format!("{}: pub fn {name}", source.rel));
                }
            }
        }
        offenders.sort();
        offenders.dedup();
        assert!(
            offenders.is_empty(),
            "these PRODUCTION `macro_rules!` bodies emit a public fn with a `Path`/`PathBuf` \
             parameter, and `collect_audited_fns` does NOT expand macro bodies, so the \
             pair-less-mutation derivation is blind to them: {offenders:?}. Extend the derivation \
             to expand the body (and prove the parse is sound for metavariables/repetitions), or \
             enumerate the emitted item and say so at the derivation's boundary."
        );
    }

    /// FIX 4: the constraint-1 enumeration is DERIVED, not enumerated. This
    /// test (a) requires EXACTLY ONE marked block in `docs/API-CONSTRAINTS.md`,
    /// (b) resolves every name in it to a real fn/method in the parsed
    /// production item graph, and (c) derives every PUBLIC fn/method that takes
    /// a raw path argument and requires each to be named in the block or in the
    /// reviewed [`PAIR_LESS_EXEMPTIONS`] list. A SIXTH omission is therefore a
    /// failing test rather than a reviewer's find: adding a new public
    /// raw-path mutating fn makes `uncovered` non-empty until the block (or an
    /// exemption with a reason) names it.
    ///
    /// THE DERIVATION'S BOUNDARY, stated rather than implied:
    /// * it is SYNTACTIC on the parameter TYPE. IN CLASS: `Path`/`PathBuf`,
    ///   the same nested through a PATH-BEARING CONTAINER
    ///   (`Option`/`Box`/`Cow`/`Rc`/`Arc`/`Vec`/`Result`, a slice/array, or a
    ///   tuple with a path element), `impl AsRef<Path>` / `impl Into<PathBuf>`,
    ///   and a generic type param bounded the same way. OUT OF CLASS,
    ///   explicitly: `&str`, `String`, `&OsStr`, `OsString`, `&[u8]`, a
    ///   CONTAINER of those spellings (`&[String]`, `Vec<String>`, `(String,)`,
    ///   `Result<String>`), a type not on the container list
    ///   (`HashMap<_, PathBuf>`), any `use … as` ALIAS of `Path`/`PathBuf`, and a
    ///   LOCAL non-generic `type` ALIAS of `Path`/`PathBuf` (`type ZP =
    ///   std::path::Path;`) — for both aliases the predicate matches the NAME,
    ///   not the resolution, and `audited_fns` resolves neither. A STRING IS A
    ///   PATH SPELLING, NOT A PATH: the
    ///   type system does not distinguish a path-spelled string from any other
    ///   string, this crate already has public fns taking a string for a
    ///   non-path reason (`is_reserved_name`, `valid_hex_digest`, the error
    ///   constructors), and extending the class would make the derivation a
    ///   SUPERSET that no longer pins path mutations. A `pub fn f(name: &str)`
    ///   that builds `Path::new(name)` and mutates is therefore out of class BY
    ///   CONSTRUCTION; `pair_less_derivation_boundary_is_the_syntactic_path_class`
    ///   pins both directions of that statement against the code. The same
    ///   reason covers `RootedRelativePath::with_file_name`'s `impl AsRef<OsStr>`
    ///   (it is a name COMPONENT, and the mutation is performed by the returned
    ///   validated type);
    /// * a `macro_rules!` BODY is not walked, so a `pub fn` it emits is NOT
    ///   derived: a macro body is a TOKEN stream, not a `syn::Item` sequence,
    ///   and it may carry metavariables, fragment specifiers, and repetitions,
    ///   so parsing it as items is unsound in general. The `std::fs` macro
    ///   token walker cannot be reused here soundly — it flattens tokens and
    ///   recognises canonical `::`-joined paths, with no notion of an item,
    ///   visibility, or parameter type. This class is therefore STATED out of
    ///   class, and its control is the TRIPWIRE
    ///   `production_macro_bodies_emitting_public_path_fns_are_refused`,
    ///   which fails when a production `macro_rules!` body spells a `pub fn`
    ///   with a `Path`/`PathBuf` parameter;
    /// * a TRAIT-impl method (`impl Remote for X { fn symlink … }`) is not
    ///   counted directly, because the TRAIT DECLARATION is counted instead; a
    ///   FOREIGN trait's methods would be out of class, and this crate has
    ///   none;
    /// * a `mod` nested inside a fn BODY is not walked; this crate has none.
    ///
    /// A member of any of those classes is added to
    /// [`PAIR_LESS_EXEMPTIONS`] with its reason rather than left implicit.
    #[test]
    fn pair_less_mutation_enumeration_is_the_derived_public_surface() {
        // (a) EXACTLY ONE marked block.
        let doc_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/API-CONSTRAINTS.md");
        let doc = std::fs::read_to_string(&doc_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", doc_path.display()));
        let begins = doc.matches(PAIR_LESS_BEGIN).count();
        let ends = doc.matches(PAIR_LESS_END).count();
        assert_eq!(
            begins, 1,
            "docs/API-CONSTRAINTS.md must carry EXACTLY ONE {PAIR_LESS_BEGIN}; found {begins}"
        );
        assert_eq!(
            ends, 1,
            "docs/API-CONSTRAINTS.md must carry EXACTLY ONE {PAIR_LESS_END}; found {ends}"
        );
        let start = doc.find(PAIR_LESS_BEGIN).unwrap() + PAIR_LESS_BEGIN.len();
        let end = doc.find(PAIR_LESS_END).unwrap();
        assert!(
            start < end,
            "the pair-less-mutation markers are out of order"
        );
        let block = &doc[start..end];

        // Each entry is `- \`item::path\` — reason`.
        let mut block_names: Vec<String> = Vec::new();
        for line in block.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("- `") else {
                continue;
            };
            let Some(close) = rest.find('`') else {
                panic!("a pair-less-mutation block entry has an unclosed backtick: {line:?}");
            };
            block_names.push(rest[..close].to_string());
        }
        assert!(
            !block_names.is_empty(),
            "the marked block must name at least one mutation"
        );

        // (b)+(c) the parsed production item graph.
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
        let parsed = parse_crate(&sources, &gated);
        let all = audited_fns(&parsed, &gated);
        let is_public_key = |key: &AuditedFn| {
            all.iter()
                .any(|collected| &collected.item == key && collected.public)
        };

        // (b) every block name RESOLVES to a real fn/method.
        let mut block_keys: BTreeSet<AuditedFn> = BTreeSet::new();
        for name in &block_names {
            let resolved = resolve_audited_name(name, &all);
            assert!(
                !resolved.is_empty(),
                "the pair-less-mutation block names {name:?}, which does NOT resolve to any fn or \
                 method in the parsed production item graph; a stale member must fail"
            );
            assert!(
                resolved.iter().any(is_public_key)
                    || name == "transport::ssh::hostkey::pin_known_hosts",
                "the block names {name:?}, which resolves only to NON-public items; the block is \
                 the crate's public mutation inventory, and \
                 `transport::ssh::hostkey::pin_known_hosts` (pub(crate) in a private module) is the \
                 ONE named non-public residual"
            );
            block_keys.extend(resolved);
        }

        let mut exempt_keys: BTreeSet<AuditedFn> = BTreeSet::new();
        for (name, reason) in PAIR_LESS_EXEMPTIONS {
            assert!(
                !reason.trim().is_empty(),
                "the exemption {name:?} must state its reason"
            );
            let resolved = resolve_audited_name(name, &all);
            assert!(
                !resolved.is_empty(),
                "the exemption {name:?} does NOT resolve to any fn or method in the parsed \
                 production item graph"
            );
            assert!(
                resolved.iter().any(is_public_key),
                "the exemption {name:?} resolves only to NON-public items"
            );
            exempt_keys.extend(resolved);
        }

        // (c) the derived PUBLIC raw-path population.
        let derived: Vec<AuditedFn> = all
            .iter()
            .filter(|collected| collected.public && collected.takes_raw_path)
            .map(|collected| collected.item.clone())
            .collect();
        assert!(
            derived.len() >= 15,
            "the derivation must see the crate's public raw-path surface, not a near-empty set: \
             {derived:?}"
        );

        let mut uncovered: Vec<String> = derived
            .iter()
            .filter(|item| !block_keys.contains(*item) && !exempt_keys.contains(*item))
            .map(AuditedFn::rendered)
            .collect();
        uncovered.sort();
        uncovered.dedup();
        assert!(
            uncovered.is_empty(),
            "these PUBLIC fns/methods take a raw path argument and are in NEITHER the \
             docs/API-CONSTRAINTS.md block NOR the reviewed PAIR_LESS_EXEMPTIONS list, so a SIXTH \
             enumeration omission would not be caught: {uncovered:?}"
        );

        // No stale exemptions: each must cover at least one derived item.
        for (name, _) in PAIR_LESS_EXEMPTIONS {
            let covers = resolve_audited_name(name, &all)
                .iter()
                .any(|key| derived.contains(key));
            assert!(
                covers,
                "the exemption {name:?} covers no derived public raw-path item; remove it"
            );
        }

        // The block must still cover real derived mutations (a block that named
        // only non-path items would not be the enumeration this test guards).
        let block_covered = block_names
            .iter()
            .filter(|name| {
                resolve_audited_name(name, &all)
                    .iter()
                    .any(|key| derived.contains(key))
            })
            .count();
        assert!(
            block_covered >= 3,
            "the block must cover real public raw-path mutations; only {block_covered} of {} do",
            block_names.len()
        );
    }
}
