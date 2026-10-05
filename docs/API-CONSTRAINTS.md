# API constraints

The public API exists to make the implementation small. Every constraint here removes
branches, checks and sentences from `src/` — the measure of a constraint is not
elegance, it is what it deletes — and each was earned by a defect that its *absence*
made possible.

| # | Constraint | Removes from the impl | Status |
|---|---|---|---|
| 1 | **A mutation OF A NAME INSIDE A ROOT is named one way: `(&RootDir, &RootedRelativePath)`.** The path is parsed once, at the boundary, into a validated type, and that type is the only input a ROOT-RELATIVE mutating primitive accepts. The mutations that do NOT take the pair are enumerated ONCE, in the MACHINE-CHECKED `PAIR-LESS-MUTATIONS` block below (search that marker); a test derives the public raw-path surface and fails unless every member is named there or carries a stated exemption, so an omission is a failing test rather than a silent omission. The derivation's class is EXACTLY the syntactic PATH class — `Path`/`PathBuf` (optionally wrapped in `Option`/`Box`/`Cow`/`Rc`/`Arc`, or in `Vec`, `Result`, a slice, an array or a path-bearing tuple), `impl AsRef<Path>`, `impl Into<PathBuf>` — and a STRING is a path SPELLING, not a path: `&str`, `String`, `&OsStr`, `OsString` and `&[u8]` are OUT of class by construction (sweeping them in would pull the crate's string-taking public functions into the population, and the derivation would stop pinning path mutations). | the private `validate_rel` guard (its looser rule was re-derived at every `_fd` primitive and at the Windows port's `rel_join`); the "which spelling did the caller use" branches; the public path-based `set_private`, `sync_parent_dir`, `ensure_private_dir(_durable)` and `remove_dir_all_path` spellings (the first three remain `pub(crate)` for the crate's own paths; `remove_dir_all_path` is `#[cfg(test)]`, with no production caller; `ensure_private_dir` exists on the Windows port only); and the *class* where a path-based primitive shipped missing the guard its `_fd` twin had. | **done** |
| 2 | **Ownership is one axis, not six entry points.** `sync`/`push`/`pull` × owned/unowned × local/remote collapse to one function taking a `DestinationOwnership` value that only the lock-taking paths can construct. The lock records a run holds are the SAME axis: `DestinationOwnership::Locked` (local sibling record), `LockedWithInRoot` (sibling + the caller's in-root `Layout::lock`), and `LockedRemote` (a persistent far-side session) are three unforgeable values of the one enum, produced by `lock`, `lock_with_in_root_lock` and `lock_remote`. A token is bound to the transport that minted it — direction, pinned local root, remote root spelling, endpoint identity, remote localness — each compared, each pinned by a test per direction. | five near-duplicate entry bodies; the "call the right one" prose; a documented limitation that exists only because the weak path is a separate function; a `bool` parameter on the acquiring constructor (the composition and the remote/local choice are named enum values, never flags). | **done** |
| 3 | **Containment has one authority and it consumes kinds, not resolutions.** A caller supplies an entry-kind view; it never supplies a resolution function. All three views build `SymlinkContainmentIndex` from KINDS — the two canonicalizers from their own entry list (`from_entries`), the atomic tree-copy views from `live_entry_kinds` — the rule and the index are `pub(crate)`, and no API accepts a resolver. | three hand-rolled resolvers, one of which projected from the live source tree and accepted an escape while the other two refused it. | **done** |
| 4 | **Every condition a caller must branch on is a typed value.** Each class whose conditions a caller must tell apart carries a public kind enum, and every error variant names it: `ReservedKind` (reserved-spelling refusals — a CLOSED set of three, no fallback, accessor `reserved_kind()`), `MaterializationKind` (address-fidelity and wire refusals), `StoreKind` (the tree copy's source-audit refusals, the residue gate, visible-but-unconfirmed durability), `TransportKind` (the manifest-failure layers and the receiver-marker, root, durability and sidecar-wait conditions), and `PreflightKind` (the run-binding conditions — the transport's identity and root, the run the token was minted for, the destination's localness and the lock arm that fits it, the record's own shape, and far-side support).. The ARM SETS are not enumerated here: `src/error.rs` is their definition, and a partial list in this row would read as one (that is how `SidecarWaitTimeout` and `IncompleteListing` went unmentioned). The four that split a heterogeneous class carry an explicit `Unclassified` fallback and the `*_reason()` accessor convention; messages are preserved VERBATIM so a text-matching caller keeps working, and `with_context` preserves the kind. | string matching in callers, and the mutation where two layers collapsed onto one kind stayed green under message assertions but is caught by the kind assertions. | **done** |
| 5 | **The reserved spellings a break may touch are a value, unforgeable outside the crate.** | the residual list as prose. | **done** (INTERNAL: `Sanction` and `GuardedRel` are `pub(crate)`, so no caller can name them and no public signature takes or returns one — this constraint shapes the implementation, not the public surface) |
| 6 | **Every bound is a constant with its reason stated**, and no derived value feeds a length-limited resource unbounded. | ad-hoc length arithmetic at each site. | **done** |
| 7 | **One direction of data flow per type**: a type that is read is not the same type that is written. | mode/kind re-reads, and the checks that exist only to catch a caller passing the wrong one. | **done** (see below) |
| 8 | **The crate's own contract is not reachable by accident**: the weak, unverified or unenforced path is reachable only through a name that states it. | the "documented but not enforced" bullets. | **done** (see below; FIVE residuals remain by decision, each named at its item) |

## The pair-less mutations

<!-- PAIR-LESS-MUTATIONS:BEGIN -->
- `atomic::write_atomic_replace` — the UNCONFINED, absolute-path replace; the one to avoid when the confined `write_atomic_replace_fd` can name the destination
- `atomic::copy_tree_verbatim` — a tolerant verbatim copy whose SOURCE may be out of root
- `atomic::copy_dir_recursive_fd` — the same copy, descriptor-confined on the destination
- `sync::retire_destination_lock` — a lock-record removal with its own spelling and identity checks
- `sync::Residue::recover_to` — takes a raw target path, validated at the boundary before use
- `sync::Residue::discard` — takes NO path argument; the validated pair lives inside `Residue`
- `lock::FileLock::acquire` — a caller-supplied path, adopted as the record only when it is empty or already holds a record this crate wrote (whose first line is the record header "storekit lock record v1"); any other non-empty entry is REFUSED with the typed PreflightKind::LockRecordNotRecognized and left byte-for-byte and mode-for-mode alone, never truncated
- `lock::AdministrativeRecoveryGuard::acquire` — a record created or adopted at a caller-supplied path, under the same refusal as `lock::FileLock::acquire`
- `platform::chmod` — the ONE path-based mode authority (the FD-bound `File::set_permissions` is the permitted second form)
- `platform::symlink` — the cross-platform symlink helper
- `Remote::lock_far_side` — the far-side ownership seam (a trait method), whose holder
applies the SAME record rule as `lock::FileLock::acquire`: it adopts an empty or
header-prefixed entry, refuses any other non-empty one untouched (bytes AND mode
unchanged, before any `chmod`) with the same typed `PreflightKind::LockRecordNotRecognized`,
and writes an entry it does adopt at mode `0600`
- `Remote::provision_layout` — the layout and bootstrap-`create_dir_all` seam (a trait method)
- `transport::with_operation_lock_sidecar` — a raw `base` plus a caller-chosen `sidecar` spelling, refused by the reserved-mutation authority if it names the crate's bookkeeping
- `transport::ssh::hostkey::pin_known_hosts` — the drop of a stale pinned host key under the transport's OWN private `cache_dir`
<!-- PAIR-LESS-MUTATIONS:END -->

This block is the ONE enumeration of the mutations that do NOT take
`(&RootDir, &RootedRelativePath)`, and it is MACHINE-CHECKED: a test finds exactly one
such block, resolves every name in it to a real item, and separately derives the public
raw-path surface so that a member missing from the block is a FAILING TEST. The
primitives that also take a `&Path` but are NOT name mutations are exempt in that test,
each with a stated reason, in the reviewed exemption list inside `src/atomic/guard.rs` —
read-only probes, path algebra, and the `DestinationOwnership` / `sync` entry points
whose `&Path` is the ROOT rather than the name being mutated. That list, not this row,
is the enumeration: the test derives the population, and the exemptions are the residue
to audit one entry at a time.

## What constraint 8 closed

Every PUBLIC path whose guarantee is weaker than the crate's default, or that is
unverified. Resolved as **N** (the weak path is legitimate and its NAME states it),
**C** (the path is constrained so it cannot be reached by accident), or **R** (a
residual, named AT the item with its reach).

- **C — the wire assembler's completeness precondition.** `canonicalize_remote_entries`
  needs the caller to have checked the walk's exit status, which its signature did not
  carry. The public entry points are `canonicalize_remote_entries_checked(output, root,
  exited_zero)` and `…_destination_checked(...)`, and both refuse `false` with the typed
  `MaterializationKind::IncompleteListing` BEFORE assembly; the raw forms are
  `pub(crate)`, and the crate's own remote path passes `out.success()`.
- **N — the unconfined atomic replace.** `atomic::write_atomic_replace(path: &Path)` is
  public and its NAME states the weakness: the UNCONFINED, absolute-path form. Its
  justification is the CONSUMER's call shape, not this crate's production.
- **N — `Remote::exists`.** A default method delegating to `metadata_opt`, whose doc
  states exactly what a `false` discards (absent conflated with *the probe could not
  tell*), pointing a caller that must distinguish them at `metadata_opt`.
- **N — `Remote::lock_far_side`.** A DEFAULT that REFUSES, so a transport that does not
  override it cannot own a remote destination; `DestinationOwnership::lock_remote`
  returns a typed refusal naming the override, and the weaker choice remains the
  explicitly-named `Unowned`. The default documents the contract an override must
  satisfy: NON-BLOCKING acquisition mapping a live holder to `Error::LockContended`,
  release on every exit path (drop, error, panic) and when the connection dies.
- **N — the enumerated path-based mutators**: the `PAIR-LESS-MUTATIONS` block above.
- **R — `Remote::exec`**: it runs a caller-built command, bypassing the lock, path
  confinement and the operation protocol; it cannot be closed without removing the seam
  every shelling-out default is built on.
- **R — `Remote::fsync_tree` / `fsync_parent` defaults**: both default to a no-op; a
  production transport that does not override makes nothing durable. Both production
  transports override.
- **R — `Remote::remove_file_if`'s default**: the NON-ATOMIC read-compare-remove. The
  production path is bounded to the ONE record the layout OWNS by an identity-checked
  `OwnedLockRecord`.
- **R — weaker paths already named at the item**: destination tolerance only through the
  `*_destination` names; the Windows port's weaker guarantees on every primitive and in
  `atomic::COMPONENT_CONFINED`; `Remote::copy_tree`'s SSH `cp -a` asymmetry;
  `EntryPolicy::AppendTail`'s lost-update warning.
- **R — `reserved::is_reserved_name` / `is_reserved_path`** are NARROWER than
  `is_unaddressable_name` / `is_unaddressable_path`: byte-exact reserved MATCHING, not
  "may I use this name". The sync does NOT strip with them — it uses the BROAD pair,
  `is_residue_path` for the destination view and `is_unaddressable_path` for the source
  view.
- **Out of scope — `Tracer::new(enabled: bool)`**: rule 2's boolean clause is about a
  DESTRUCTIVE choice; a tracer's `false` is the safe, side-effect-free default.

## What constraint 7 closed

The manifest is produced, serialized, transferred and read back, so the same facts cross
the wire in both directions. Three places let one type carry both directions; each is
now two.

- **The entry kind and mode are VALIDATED values, not wire spellings.**
  `TreeEntry.entry_type` is an `EntryKind` and `mode` a `u32`; `"file"`/`"dir"`/`"symlink"`
  and the octal mode exist only across serde. The per-consumer projections (nine
  `EntryKind::of` sites, two of which silently dropped a path; seven `parse_mode`
  re-reads) are gone, and the `unknown manifest entry type` / `invalid manifest mode`
  refusals fire at the ONE wire boundary, and the reader ALSO validates the schema
  version, the hash algorithm, the tree digest and every entry PATH there, so a record
  with an unknown kind, an invalid mode, a malformed path or a malformed
  version/algorithm/digest cannot become a `TreeEntry` at all. The per-entry CONTENT
  digest and symlink TARGET shapes are NOT reader-validated: the target's containment
  rule is relative to the entry's own path, so it belongs to `verify_tree_metadata`,
  with the digest recomputation and the duplicate/ordering checks.
- **The destination observation is a distinct type.** `DestinationTree`'s payload is
  `pub(crate)`, so it cannot be serialized as a `tree.json` and cannot be passed where a
  canonical source manifest is required; the direction is a TYPE at the entry points
  (`diff_source_and_destination`, `apply_manifests`), proven by two `compile_fail,E0308`
  doctests. RESIDUAL: `TreeMetadata` is deliberately not sealed (its fields are `pub`),
  so a caller who REBUILDS one from the destination's accessors is not stopped by the
  type system; sealing it would be a breaking consumer change.
- **The strict SOURCE walk no longer builds the destination type.** `UnsupportedSink` is
  a type, so the strict path allocates no `unsupported` list, keeps no per-entry
  "tolerated" state, and returns `TreeMetadata` by construction.

**The delta, measured, for the mode spelling.** The wire form is exactly four octal
digits — the spelling `serialize` emits — so the accepted set equals the emitted set and
the parse is INJECTIVE (which the digest depends on, since `compute_tree_digest` hashes
the VALIDATED values re-serialized as `{:04o}`). Narrowed on the READ path:
`"644"`, `"00644"`, `"10644"` (the setuid form: it was masked to `0o644`), `"+755"`,
`"37777777777"`, `"77777"`, `"07777"`, `"17777"` — eight distinct spellings, in four
shapes: too few digits, zero-padded, carrying file-type bits, and a sign or a value
beyond `0o7777` — used to load and now fail to load; nothing old-refused is now
accepted. `"0644"`,
`"0755"`, `"0000"` and every value in `0..=0o7777` are unchanged. The rule governs the
JSON `tree.json` wire ONLY. The far-side LISTING frame carries the raw `st_mode` in hex
with the type bits INCLUDED; the far-side MANIFEST frame is the one that masks to
`0o7777`, and masking is the semantics THERE.

## Rules for adding a constraint

- **State what it removes.** A constraint that removes no branch is decoration.
- **Prove the delta.** Before replacing a runtime check with a type, enumerate what the
  check refused and what the type refuses — they are rarely equal, and the difference is
  a behaviour change, not a refactor.
- **Flip a looser assertion explicitly**, with the reason recorded. A silent flip is a
  lost assertion.
- **A constraint only the crate can construct is worth more than one the caller can
  build**: unforgeable is the difference between a rule and a convention.
- **A deletion is justified by a CONSUMER's need**, never by this crate's own production,
  and by the ASSERTIONS that cover it, never by a preserved test count.
- **A constraint's own claims are subject to the rules in `docs/CONSISTENCY.md`** — in
  particular that a device must be able to fail for the property it names, and that its
  derivation must not come from the thing it certifies.
