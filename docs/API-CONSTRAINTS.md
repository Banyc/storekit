# API constraints

The public API exists to make the implementation small. A constraint is measured by
what it deletes, not by how elegant it looks.

| # | Constraint | Status |
|---|---|---|
| 1 | **A mutation of a name inside a root is named one way: `(&RootDir, &RootedRelativePath)`.** The path is parsed once, at the boundary, into a validated type, and that type is the only input a root-relative mutating primitive accepts. The mutations that do not take the pair are listed once, in the machine-checked `PAIR-LESS-MUTATIONS` block below. A test derives the public raw-path surface and fails unless every member is named there or carries a stated exemption, so an omission is a failing test. The derivation's class is exactly the syntactic path class: `Path`/`PathBuf`, optionally wrapped in `Option`/`Box`/`Cow`/`Rc`/`Arc`, or in `Vec`, `Result`, a slice, an array or a path-bearing tuple; `impl AsRef<Path>`; `impl Into<PathBuf>`. A string is a path spelling, not a path: `&str`, `String`, `&OsStr`, `OsString` and `&[u8]` are out of class by construction. Sweeping them in would pull the crate's string-taking public functions into the population, and the derivation would stop pinning path mutations. | done |
| 2 | **Ownership is one axis, not six entry points.** `sync`/`push`/`pull` × owned/unowned × local/remote collapse to one function taking a `DestinationOwnership` that only the lock-taking paths can construct. The lock records a run holds are the same axis: `DestinationOwnership::Locked` (local sibling record), `LockedWithInRoot` (sibling plus the caller's in-root `Layout::lock`) and `LockedRemote` (a persistent far-side session) are three unforgeable values of the one enum, produced by `lock`, `lock_with_in_root_lock` and `lock_remote`. A token is bound to the transport that minted it — direction, pinned local root, remote root spelling, endpoint identity, remote localness — each compared and each pinned by a test per direction (`PreflightKind::RunBindingMismatch`, `RemoteRootMismatch`, `EndpointIdentityMismatch`, `RemoteLocalnessMismatch`). | done |
| 3 | **Containment has one authority, and it consumes kinds, not resolutions.** A caller supplies an entry-kind view; it never supplies a resolution function. All three views build `SymlinkContainmentIndex` from kinds — the two canonicalizers from their own entry list (`from_entries`), the atomic tree-copy views from `live_entry_kinds`. The rule and the index are `pub(crate)`, and no API accepts a resolver. | done |
| 4 | **Every condition a caller must branch on is a typed value.** Each class whose conditions a caller must tell apart carries a public kind enum, and every error variant names it: `ReservedKind` (reserved-spelling refusals — a closed set of three, no fallback, accessor `reserved_kind()`), `MaterializationKind` (address-fidelity and wire refusals), `StoreKind` (the tree copy's source-audit refusals, the residue gate, visible-but-unconfirmed durability), `TransportKind` (the manifest-failure layers and the receiver-marker, root, durability and sidecar-wait conditions), and `PreflightKind` (the run-binding conditions: the transport's identity and root, the run the token was minted for, the destination's localness and the lock arm that fits it, the record's own shape, and far-side support). The arm sets are not enumerated here; `src/error.rs` is their definition, and a partial list in this row would read as one. The four that split a heterogeneous class carry an explicit `Unclassified` fallback and the `*_reason()` accessor convention. Messages are preserved verbatim so a text-matching caller keeps working, and `with_context` preserves the kind. | done |
| 5 | **The reserved spellings a break may touch are a value, unforgeable outside the crate.** | done (internal: `Sanction` and `GuardedRel` are `pub(crate)`, so no caller can name them and no public signature takes or returns one; this constraint shapes the implementation, not the public surface) |
| 6 | **Every bound is a constant with its reason stated**, and no derived value feeds a length-limited resource unbounded. | done |
| 7 | **One direction of data flow per type**: a type that is read is not the same type that is written. | done (see below) |
| 8 | **The crate's own contract is not reachable by accident**: the weak, unverified or unenforced path is reachable only through a name that states it. | done (see below; five residuals remain by decision, each named at its item) |

## The pair-less mutations

<!-- PAIR-LESS-MUTATIONS:BEGIN -->
- `atomic::write_atomic_replace` — the unconfined, absolute-path replace; avoid it when the confined `write_atomic_replace_fd` can name the destination
- `atomic::copy_tree_verbatim` — a tolerant verbatim copy whose source may be out of root
- `atomic::copy_dir_recursive_fd` — the same copy, descriptor-confined on the destination
- `sync::retire_destination_lock` — a lock-record removal with its own spelling and identity checks
- `sync::Residue::recover_to` — takes a raw target path, validated at the boundary before use
- `sync::Residue::discard` — takes no path argument; the validated pair lives inside `Residue`
- `lock::FileLock::acquire` — a caller-supplied path, adopted as the record only when it is empty or already holds a record this crate wrote (whose first line is the record header "storekit lock record v1"); any other non-empty entry is refused with the typed PreflightKind::LockRecordNotRecognized and left byte-for-byte and mode-for-mode alone, never truncated
- `lock::AdministrativeRecoveryGuard::acquire` — a record created or adopted at a caller-supplied path, under the same refusal as `lock::FileLock::acquire`
- `platform::chmod` — the one path-based mode authority (the fd-bound `File::set_permissions` is the permitted second form)
- `platform::symlink` — the cross-platform symlink helper
- `Remote::lock_far_side` — the far-side ownership seam (a trait method), whose holder
applies the same record rule as `lock::FileLock::acquire`: it adopts an empty or
header-prefixed entry, refuses any other non-empty one untouched (bytes and mode
unchanged, before any `chmod`) with the same typed `PreflightKind::LockRecordNotRecognized`,
and writes an entry it does adopt at mode `0600`
- `Remote::provision_layout` — the layout and bootstrap-`create_dir_all` seam (a trait method)
- `transport::with_operation_lock_sidecar` — a raw `base` plus a caller-chosen `sidecar` spelling, refused by the reserved-mutation authority if it names the crate's bookkeeping
- `transport::ssh::hostkey::pin_known_hosts` — the drop of a stale pinned host key under the transport's own private `cache_dir`
<!-- PAIR-LESS-MUTATIONS:END -->

This block is the one enumeration of mutations that do not take
`(&RootDir, &RootedRelativePath)`, and it is machine-checked: a test finds exactly one
such block, resolves every name in it to a real item, and separately derives the public
raw-path surface so that a member missing from the block is a failing test. Primitives
that also take a `&Path` but are not name mutations are exempt in that test, each with
a stated reason, in the reviewed exemption list inside `src/atomic/guard.rs`: read-only
probes, path algebra, and the `DestinationOwnership` / `sync` entry points whose
`&Path` is the root rather than the name being mutated. That list, not this row, is the
enumeration: the test derives the population, and the exemptions are the residue to
audit one entry at a time.

## What constraint 8 closed

Every public path whose guarantee is weaker than the crate's default, or that is
unverified. Each was resolved as **N** (the weak path is legitimate and its name states
it), **C** (the path is constrained so it cannot be reached by accident), or **R** (a
residual, named at the item with its reach).

- **C — the wire assembler's completeness precondition.** The raw assemblers take
  `CompleteListing`, whose only constructor refuses a non-zero walk exit with the typed
  `MaterializationKind::IncompleteListing` before assembly, so the exit status cannot
  be omitted from the signature. The public entry points are
  `canonicalize_remote_entries_checked(output, root, exited_zero)` and
  `canonicalize_remote_entries_destination_checked(...)`, and both refuse `false` with
  the same typed refusal; the raw forms are `pub(crate)`, and the crate's own remote
  path passes `out.success()`.
- **N — the unconfined atomic replace.** `atomic::write_atomic_replace(path: &Path)` is
  public and its name states the weakness: the unconfined, absolute-path form. Its
  justification is the consumer's call shape, not this crate's production.
- **N — `Remote::exists`.** A default method delegating to `metadata_opt`, whose doc
  states exactly what a `false` discards (absent conflated with "the probe could not
  tell"), pointing a caller that must distinguish them at `metadata_opt`.
- **N — `Remote::lock_far_side`.** A default that refuses, so a transport that does not
  override it cannot own a remote destination. `DestinationOwnership::lock_remote`
  returns a typed refusal naming the override, and the weaker choice remains the
  explicitly named `Unowned`. The default documents the contract an override must
  satisfy: non-blocking acquisition mapping a live holder to `Error::LockContended`,
  release on every exit path (drop, error, panic) and when the connection dies.
- **N — the enumerated path-based mutators**: the `PAIR-LESS-MUTATIONS` block above.
- **R — `Remote::exec`**: it runs a caller-built command, bypassing the lock, path
  confinement and the operation protocol. It cannot be closed without removing the seam
  every shelling-out default is built on.
- **R — `Remote::fsync_tree` / `fsync_parent` defaults**: both default to a no-op, so a
  production transport that does not override makes nothing durable. Both production
  transports override.
- **R — `Remote::remove_file_if`'s default**: the non-atomic read-compare-remove. The
  production path is bounded to the one record the layout owns by an identity-checked
  `OwnedLockRecord`.
- **R — weaker paths already named at the item**: destination tolerance only through the
  `*_destination` names; the Windows port's weaker guarantees on every primitive and in
  `atomic::COMPONENT_CONFINED`; `Remote::copy_tree`'s SSH `cp -a` asymmetry;
  `EntryPolicy::AppendTail`'s lost-update warning.
- **R — `reserved::is_reserved_name` / `is_reserved_path`** are narrower than
  `is_unaddressable_name` / `is_unaddressable_path`: byte-exact reserved matching, not
  "may I use this name". The sync does not strip with them; it uses the broad pair,
  `is_residue_path` for the destination view and `is_unaddressable_path` for the source
  view.
- **Out of scope — `Tracer::new(enabled: bool)`**: rule 2's boolean clause is about a
  destructive choice; a tracer's `false` is the safe, side-effect-free default.

## What constraint 7 closed

The manifest is produced, serialized, transferred and read back, so the same facts cross
the wire in both directions.

- **The entry kind and mode are validated values, not wire spellings.**
  `TreeEntry.entry_type` is an `EntryKind` and `mode` a `u32`; `"file"`/`"dir"`/`"symlink"`
  and the octal mode exist only across serde. The `unknown manifest entry type` /
  `invalid manifest mode` refusals fire at the one wire boundary, and the reader also
  validates the schema version, the hash algorithm, the tree digest and every entry path
  there, so a record with an unknown kind, an invalid mode, a malformed path or a
  malformed version/algorithm/digest cannot become a `TreeEntry` at all. The per-entry
  content digest and symlink target shapes are not reader-validated: the target's
  containment rule is relative to the entry's own path, so it belongs to
  `verify_tree_metadata`, with the digest recomputation and the duplicate/ordering
  checks.
- **The destination observation is a distinct type.** `DestinationTree`'s payload is
  `pub(crate)`, so it cannot be serialized as a `tree.json` and cannot be passed where a
  canonical source manifest is required. The direction is a type at the entry points
  (`diff_source_and_destination`, `apply_manifests`), proven by two
  `compile_fail,E0308` doctests. Residual: `TreeMetadata` is deliberately not sealed
  (its fields are `pub`), so a caller who rebuilds one from the destination's accessors
  is not stopped by the type system; sealing it would be a breaking consumer change.
- **The strict source walk does not build the destination type.** `UnsupportedSink` is
  a type, so the strict path allocates no `unsupported` list, keeps no per-entry
  "tolerated" state, and returns `TreeMetadata` by construction.

**The mode spelling.** The wire form is exactly four octal digits, the spelling
`serialize` emits, so the accepted set equals the emitted set and the parse is
injective. The digest depends on that, because `compute_tree_digest` hashes the
validated values re-serialized as `{:04o}`. `"0644"`, `"0755"`, `"0000"` and every
value in `0..=0o7777` are accepted; a shorter or over-long octal string, zero-padding,
file-type bits, a leading sign, non-octal text and the empty string are refused. The
rule governs the JSON `tree.json` wire only. The far-side listing frame carries the raw
`st_mode` in hex with the type bits included; the far-side manifest frame is the one
that masks to `0o7777`, and masking is the semantics there.

## Rules for adding a constraint

- **State what it removes.** A constraint that removes no branch is decoration.
- **Prove the delta.** Before replacing a runtime check with a type, enumerate what the
  check refused and what the type refuses; they are rarely equal, and the difference is
  a behaviour change, not a refactor.
- **Flip a looser assertion explicitly**, with the reason recorded. A silent flip is a
  lost assertion.
- **A constraint only the crate can construct is worth more than one the caller can
  build.** Unforgeable is the difference between a rule and a convention.
- **A deletion is justified by a consumer's need**, never by this crate's own
  production, and by the assertions that cover it, never by a preserved test count.
- **A constraint's own claims are subject to the rules in `docs/CONSISTENCY.md`** — in
  particular that a device must be able to fail for the property it names, and that its
  derivation must not come from the thing it certifies.
