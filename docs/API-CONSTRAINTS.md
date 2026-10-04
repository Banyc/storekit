# API constraints

The public API exists to make the implementation small. Every constraint added
here is one the impl would otherwise re-derive, re-check, or document — and
each of the constraints below was earned by a defect that the *absence* of the
constraint made possible.

The measure of a constraint is not elegance; it is how many branches, checks and
sentences disappear from `src/`.

| # | Constraint | Removes from the impl | Status |
|---|---|---|---|
| 1 | **A mutation OF A NAME INSIDE A ROOT is named one way: `(&RootDir, &RootedRelativePath)`.** The path is parsed once, at the boundary, into a validated type, and that type is the only input a ROOT-RELATIVE mutating primitive accepts. The mutations that do NOT take the pair are enumerated ONCE, in the MACHINE-CHECKED `PAIR-LESS-MUTATIONS` block below (search that marker), and that block — not this row and not the constraint-8 bullet — is the list: a test derives the public raw-path surface and fails unless every member is named there or carries a stated exemption, so a SEVENTH omission is a failing test rather than a reviewer's find (six were found by hand, across rounds 1, 2, 3, 5, 6 and 7). An earlier version of this row claimed "ONE tolerated exception": its population was the crate's own root-relative primitives, not the whole public surface (axis M) — and the NEXT version still missed the sidecar helper, which is the same error twice, and is why the rule is stated as a rule about ROOT-RELATIVE mutation with its exceptions ENUMERATED rather than as a count. | the private `validate_rel` guard (its looser rule was re-derived at every `_fd` primitive and at the Windows port's `rel_join`); the "which spelling did the caller use" branches; the public path-based `set_private`, `sync_parent_dir`, `ensure_private_dir(_durable)` and `remove_dir_all_path` spellings; and the *class* where a path-based primitive shipped missing the guard its `_fd` twin had. | **done** |
| 2 | **Ownership is one axis, not six entry points.** `sync`/`push`/`pull` × owned/unowned × local/remote collapse to one function taking an `Ownership` value that only the lock-taking paths can construct. The lock records a run holds are part of the SAME axis, not a parallel API: `DestinationOwnership::Locked` (LOCAL sibling record), `DestinationOwnership::LockedWithInRoot` (sibling record + the caller's in-root `Layout::lock`), and `DestinationOwnership::LockedRemote` (a persistent FAR-SIDE lock session) are three unforgeable values of the one enum, produced by `DestinationOwnership::lock`, `DestinationOwnership::lock_with_in_root_lock`, and `DestinationOwnership::lock_remote`. | five near-duplicate entry bodies; the "call the right one" prose; a documented limitation that exists only because the weak path is a separate function; a `bool` parameter on the acquiring constructor (the composition and the remote/local choice are named enum values, never flags). | **done** (`DestinationOwnership`; `DestinationOwnership::lock` is the unforgeable acquiring constructor; `lock_with_in_root_lock` is the composed one; `lock_remote` is the far-side one) |
| 3 | **Containment has one authority and it consumes kinds, not resolutions.** A caller supplies an entry-kind view; it never supplies a resolution function. | three hand-rolled resolvers, one of which projected from the live source tree and accepted an escape while the other two refused it. Verified: all three views build `SymlinkContainmentIndex` from `live_entry_kinds`, the rule and the index are `pub(crate)`, and no API accepts a resolver. | **done** |
| 4 | **Every condition a caller must branch on is a typed value.** Each class whose conditions a caller must tell apart carries a public kind enum and every error variant names it: `ReservedKind` (reserved-spelling refusals), `MaterializationKind` (address-fidelity and wire refusals, plus `RootsOverlap`/`ParentNotClosed`), `StoreKind` (the tree copy's source-audit refusals, the residue gate, and visible-but-unconfirmed durability), `TransportKind` (the manifest-failure LAYERS — unreachable host vs far-side script vs missing `perl` vs output-drain vs undetermined — the receiver-marker conditions, a non-directory root, and remote durability), and `PreflightKind` (the RUN-BINDING conditions: an endpoint identity unavailable vs mismatched, a remote root mismatch, a run-binding mismatch, a remote destination through the local lock and a local one through the far-side lock, an unlockable remote destination, the composed form's local-only requirement, far-side locking unsupported, and a lock record or its parent being a symlink). The message is preserved VERBATIM so a text-matching caller keeps working; `with_context` preserves the kind. Of these five enums, the FOUR that split a heterogeneous class — `MaterializationKind`, `StoreKind`, `TransportKind`, `PreflightKind` — carry an explicit `Unclassified` fallback for the conditions no caller branches on, and their accessors follow the `*_reason()` convention (`materialization_reason`, `store_reason`, `transport_reason`, `preflight_reason`); `ReservedKind` is a CLOSED set of three conditions (`ResidueBelow`, `NotResidue`, `RecoverTargetOccupied`) with no fallback needed and the accessor `reserved_kind()`. An earlier version of this sentence said "each enum carries an explicit `Unclassified` fallback" and named the `*_reason()` convention without exception, which is false for `ReservedKind`. | string matching in callers (the manifest-failure layer tests, the copy-source-audit tests, the roots-overlap test, the receiver-marker tests and — until this row was completed — every endpoint/root/run-binding refusal had to match message substrings to tell two conditions apart), and the mutation where two layers collapsed onto one kind stayed green under message assertions but is caught by the kind assertions. **This row was marked done while the FIFTH class had no kind at all**: `PreflightKind` did not exist, and the crate's own newest tests were telling its conditions apart by substring — the constraint's stated product (no text matching in callers) was violated by the crate itself. Typing it EXPOSED a mis-specified test: `a_destination_ownership_token_is_bound_to_its_run` matched `"destination ownership was taken for"`, an opening BOTH the root and the run-binding refusals share, so it had been asserting the ROOT refusal while claiming to assert the run binding; it now asserts `RemoteRootMismatch`, and a new test covers `RunBindingMismatch`. The class is a **breaking change to a consumer**: `deploy`'s bridge destructured the tuple variant and needed `{ message, .. }`. | **done** (`ReservedKind`, `MaterializationKind`, `StoreKind`, `TransportKind`, `PreflightKind`) |
| 5 | **The reserved spellings a break may touch are a value, unforgeable outside the crate.** | the residual list as prose. | **done** (INTERNAL: `Sanction` and `GuardedRel` are `pub(crate)`, so no caller can name them and no public signature takes or returns one — this constraint shapes the implementation, not the public surface) |
| 6 | **Every bound is a constant with its reason stated**, and no derived value feeds a length-limited resource unbounded. | ad-hoc length arithmetic at each site. | **done** |
| 7 | **One direction of data flow per type**: a type that is read is not the same type that is written. | mode/kind re-reads, and the checks that exist only to catch a caller passing the wrong one. | **done** (see "What constraint 7 closed" below) |
| 8 | **The crate's own contract is not reachable by accident**: the weak, unverified or unenforced path is reachable only through a name that states it. | the "documented but not enforced" bullets. | **done** (see "What constraint 8 closed" below; one stated residual remains by decision) |

## The pair-less mutations

`
<!-- PAIR-LESS-MUTATIONS:BEGIN -->
- `atomic::write_atomic_replace` — the UNCONFINED, absolute-path replace
- `atomic::copy_tree_verbatim` — a tolerant verbatim copy whose SOURCE may be out of root
- `atomic::copy_dir_recursive_fd` — the same copy, descriptor-confined on the destination
- `sync::retire_destination_lock` — a lock-record removal with its own spelling and identity checks
- `sync::Residue::recover_to` — takes a raw target path, validated at the boundary before use
- `sync::Residue::discard` — takes NO path argument; the validated pair lives inside `Residue`
- `lock::FileLock::acquire` — the record path the lock protocol computes
- `lock::AdministrativeRecoveryGuard::acquire` — a record created or truncated at a caller-supplied path
- `platform::chmod` — the ONE path-based mode authority (FD-bound `File::set_permissions` is the permitted second form)
- `platform::symlink` — the cross-platform symlink helper
- `Remote::lock_far_side` — the far-side ownership seam (a trait method)
- `Remote::provision_layout` — the layout and bootstrap-`create_dir_all` seam (a trait method)
- `transport::with_operation_lock_sidecar` — a raw `base` plus a caller-chosen `sidecar` spelling, refused by the reserved-mutation authority if it names the crate's bookkeeping
- `transport::ssh::hostkey::pin_known_hosts` — the drop of a stale pinned host key under the transport's OWN private `cache_dir`
<!-- PAIR-LESS-MUTATIONS:END -->
`

This block is the ONE enumeration of the mutations that do NOT take
`(&RootDir, &RootedRelativePath)`, and it is MACHINE-CHECKED: a test in
`atomic::guard::tests` finds exactly one such block, resolves every name in it to
a real item, and separately derives the public raw-path surface so that a member
missing from the block is a FAILING TEST. The read-only primitives that also take
a `&Path` (`platform::file_mode`, `atomic::path_state`, `sync::diff::local_manifest`,
`manifest::canonicalize_tree(_destination)`, `manifest::verify_tree_metadata`,
`RootDir::open`, `OwnedRoot::parse`, `atomic::temp_name_for`) are exempt in that
test, each with its reason, because they neither mutate a name nor escape the
confined primitives.

## What constraint 8 closed

The audit is every PUBLIC path whose guarantee is weaker than the crate's
default, or that is unverified/unenforced. Each item is resolved as **N** (the
weak path is legitimate and its NAME now states it), **C** (the path is
constrained so it cannot be reached by accident), or **R** (a stated residual,
named AT the item with its reach).

* **C — the wire assembler's completeness precondition.**
  `manifest::canonicalize_remote_entries` takes the far side's stdout as `&str`
  but REQUIRES the caller to have checked the walk's exit status, which the
  signature does not carry. The public entry points are now
  `canonicalize_remote_entries_checked(output, root, exited_zero)` and
  `canonicalize_remote_entries_destination_checked(...)`; both refuse `false`
  with the TYPED `MaterializationKind::IncompleteListing` BEFORE assembly. The
  raw `(&str, &Path)` forms are `pub(crate)`, and the crate's OWN remote path
  passes `out.success()` at the call site, so the precondition is enforced by
  the type of call, not a paragraph. The pre-fix hole — "the string alone was
  enough to assemble an incomplete listing" — is closed by the API shape, and
  `checked_assembler_refuses_a_nonzero_walk_exit` pins the refusal and its
  typed kind.
* **N — the unconfined atomic replace.** `atomic::write_atomic_replace(path:
  &Path)` is ONE OF the enumerated public mutations that do not take `(&RootDir,
  &RootedRelativePath)` (the enumeration is row 1's; an earlier version of this
  bullet said "the ONE public mutation", a count row 1's own history had already
  refuted, and it survived here for another round as the fourth place the count
  was written). Its first resolution here was **C** (demoted to
  `pub(crate)`) on the ground that no production body needed it — but that
  ground covered THIS CRATE's production, not the CONSUMER's interface. The
  reason C was wrong, in one line: **the justification covered the crate's own
  production, not the consumer's interface** (deploy's Windows port called the
  path-based `write_atomic_replace` — at the revision this was measured, in its
  own `store::atomic::windows`; that module has since been replaced by a
  re-export, so a reader checking the citation at deploy's HEAD will not find it.
  The SUBSTANTIVE evidence is the CALL SHAPE, and a cross-repo citation must be
  read at the revision it names: axis A's rule about citations applies across
  repositories too, and this one drifted for reasons this crate never saw). It is
  PUBLIC again, and its NAME states the weakness: the UNCONFINED,
  absolute-path form, the one to avoid when the confined
  `write_atomic_replace_fd` can name the destination. `docs/CONSISTENCY.md`
  axis M records the population error.
* **N — `Remote::exists`.** A `bool` existence probe that swallows EVERY error
  (permission, transport fault) as absence. Its first resolution here was
  **C** (deleted from the trait) on the ground that the crate's own production
  and tests never needed it — the same population error: **the justification
  covered the crate's own production, not the consumer's interface.** deploy's
  own transport trait DECLARED `exists` as a REQUIRED method when this was
  measured and its production calls it; at deploy's current HEAD that trait is
  itself a re-export of `storekit::transport::Remote`, so the citation moved
  under the claim, and the evidence that still holds is the CALL SITES (267
  `.exists(` occurrences, with `fn exists` defined in its `impl Remote` blocks,
  which would not compile if the name left the trait). It is a DEFAULT method
  again, delegating to `metadata_opt` (so no
  implementor is forced to write it, and an implementor may override with a
  cheaper probe), and its doc states EXACTLY what it discards — a `false`
  conflates *absent* with *the probe could not tell* — pointing a caller that
  must distinguish the two at `metadata_opt`. Naming the weakness, not deleting
  the name, is what the constraint requires.
* **N — `Remote::lock_far_side`.** The far-side ownership seam has a DEFAULT
  that REFUSES, so a transport that does not override it cannot be used to OWN
  a remote destination: `DestinationOwnership::lock_remote` returns a typed
  `Preflight` refusal naming the override it needs, and the weaker choice
  remains the explicitly-named `DestinationOwnership::Unowned`. The trait method
  documents the full contract a third-party override must satisfy: a
  NON-BLOCKING acquisition that maps a live holder to
  `Error::LockContended`, release on EVERY exit path (drop, error, panic),
  release when the connection dies, and `is_alive() == false` once the session
  is gone. `SshTransport` overrides it with a persistent far-side `perl`
  `flock` session (`DestinationOwnership::lock_remote`).
* **N — the enumerated path-based mutators.** Constraint #1's rule is about
  ROOT-RELATIVE mutations; the mutations that take a raw `&Path` are legitimate
  for what they do, and each is a NAMED weak path rather than an unnamed one. The
  list exists ONCE — the machine-checked `PAIR-LESS-MUTATIONS` block above — and
  this bullet deliberately does not restate it: an earlier version did, and the
  two copies diverged within one round (each omitted members the other listed),
  which is the same "a list standing in for a rule" error that had already cost
  six hand-found omissions. The READ-ONLY primitives that also take a `&Path`
  (`platform::file_mode`, `atomic::path_state`, `sync::diff::local_manifest`,
  `manifest::canonicalize_tree(_destination)`, `manifest::verify_tree_metadata`,
  `RootDir::open`, `OwnedRoot::parse`, `atomic::temp_name_for`) are not mutations
  and are not exceptions to this rule; the derived test exempts each with a stated
  reason.
* **R — `Remote::exec`.** The raw command seam: it runs a caller-built
  command, bypassing the operation lock, path confinement and the crate's own
  operation protocol. It cannot be closed without removing the seam every
  shelling-out `Remote` default is built on, so the reach is stated at the
  trait method.
* **R — `Remote::fsync_tree` / `Remote::fsync_parent` defaults.** Both default
  to a no-op; a production `Remote` that does not override silently makes
  nothing durable. The reach is named at each default (the production
  transports override both).
* **R — `Remote::remove_file_if`'s default.** The trait default is the
  NON-ATOMIC read-compare-remove; the reach is named at the default. The
  production path is additionally bounded to the ONE lock record the layout
  OWNS by an identity-checked `OwnedLockRecord` capability, so a public caller
  cannot break a record the protocol does not own.
* **R — `remote` weaker paths already named at the item**: the destination
  tolerance is reachable only through the `*_destination` names (the tolerant
  sink `RecordUnsupported` is private); the Windows port's weaker guarantees
  are stated on every
  primitive and in `atomic::COMPONENT_CONFINED`; `Remote::copy_tree`'s SSH
  `cp -a` asymmetry is stated on the trait method; `EntryPolicy::AppendTail`
  carries its lost-update warning.
* **R — `reserved::is_reserved_name` / `is_reserved_path`** are NARROWER than
  `is_unaddressable_name` / `is_unaddressable_path` by design (byte-exact
  reserved MATCHING for the sync's strip, versus the identity rule). The
  README names this at the authority; they are not the "may I use this name"
  oracle.
* **Out of scope — `Tracer::new(enabled: bool)`.** Rule 2's boolean clause is
  about a DESTRUCTIVE choice; a tracer's `false` is the safe, side-effect-free
  default, so there is no weaker guarantee to name.

The pin move this pass first made — gating the unconfined replace test-only on
Unix removed its `std::fs::rename` from the production count — was REVERSED
when the demotion was; `docs/CONSISTENCY.md` ("I") records both the move and
the reversal.

## What constraint 7 closed

The manifest is produced, serialized, transferred, and read back, so the same
facts cross the wire in both directions. Three places let one type carry both
directions; each is now two.

* **The entry kind and mode are VALIDATED values, not wire spellings.**
  `TreeEntry.entry_type` is an `EntryKind` and `TreeEntry.mode` is a `u32`;
  the strings `"file"`/`"dir"`/`"symlink"` and the four-digit octal mode exist
  only across `Serialize`/`Deserialize`. **Removed:** the
  per-consumer projections — nine `EntryKind::of` sites (six fallible, two
  silently DROPPING a path with `let Ok(..) else { continue }`, one filtering)
  and seven `parse_mode(&entry.mode)` re-reads, plus the raw kind-string
  comparisons the typed field replaces. **Refusals moved:** the
  `unknown manifest entry type` refusal and the `invalid manifest mode`
  refusal now fire at the ONE wire boundary (`Deserialize`), so a malformed
  record cannot become a `TreeEntry` at all. **Wire format:**
  byte-identical; pinned by
  `manifest_entries_serialize_to_the_same_wire_strings` (the `type` string and
  the octal `mode`) and by the local/remote byte-identity test.
  `EntryKind` now lives in `manifest` (re-exported from `sync::diff`), and
  `EntryKind::of`/`from_manifest` are gone — the field IS the value.
* **The destination observation is a distinct type.** `DestinationTree`'s
  canonical payload is `pub(crate)`, so a destination observation cannot be
  SERIALIZED as a `tree.json` and cannot be PASSED where a canonical source
  manifest is required. **The direction is a TYPE at the entry points:**
  `diff_source_and_destination(&TreeMetadata, &DestinationTree)` and
  `apply_manifests(&TreeMetadata, &DestinationTree)`
  take the destination type in the destination position, while `diff_trees`
  stays for two canonical manifests. Two `compile_fail` doctests prove the
  wrong direction does not typecheck, each `E0308`
  (`expected &TreeMetadata, found &DestinationTree`; and the swap is
  `expected &DestinationTree, found &TreeMetadata`). `DestinationTree.meta`
  becoming `pub(crate)` is a breaking change to that field — the direction is
  the point — and `unsupported` stays public; `tests/consumer_fit.rs` does not
  use either and still compiles and passes.
* **The strict SOURCE walk no longer builds the destination type.**
  `UnsupportedPolicy` (a runtime mode, re-read at three `match` sites in each
  of the local walk and the wire assembler) and the deferred
  `unsupported_reason: Option<...>` per-entry state are gone. The policy is a
  TYPE (`UnsupportedSink`: `RefuseUnsupported` / `RecordUnsupported`), so the
  strict path allocates no `unsupported` list, keeps no per-entry "was this
  tolerated" state, and returns `TreeMetadata` by construction —
  `canonicalize_tree` and `canonicalize_remote_entries` cannot produce a
  `DestinationTree`.

**The delta, measured.** The typed field now IS stricter at the boundary, and
this paragraph was REWRITTEN because its first version claimed a strictness the
code did not have: `mode_octal::deserialize` had carried `& 0o7777` over from
`parse_mode`, so it MASKED, and `from_str_radix` also accepted a leading `+` and
any length — `"10644"` loaded as `0o644` with the setuid bit silently dropped,
and `"+755"` loaded at all. The refusal is now real: `mode_octal::deserialize`
accepts EXACTLY the spelling `serialize` emits — four octal digits, no sign — so
the accepted set equals the emitted set and the wire parse is INJECTIVE. That
injectivity is load-bearing rather than tidy: `compute_tree_digest` hashes the
VALIDATED values re-serialized as `{:04o}`, so two spellings aliasing to one
value would yield one `tree_sha256` from two different wire records.

| spelling | before | after |
|---|---|---|
| `"0644"`, `"0755"`, `"0000"`, every `0..=0o7777` | accepted | accepted |
| `"644"`, `"00644"` | accepted (masked) | **refused** |
| `"10644"` (the setuid form) | accepted → `0o644`, bit dropped | **refused** |
| `"+755"` | accepted → `0o755` | **refused** |
| `"37777777777"`, `"77777"`, `"07777"`, `"17777"` | accepted (masked) | **refused** |
| `"8"`, `""`, `"0o644"`, `"0x1a4"`, `" 644"`, `"0644 "`, `"-644"`, `"0644a"` | refused | refused |

**No row is old-refused/new-accepted: the change narrows only.** It is a real
behaviour change on the read path — a `tree.json` carrying one of the six
non-canonical spellings above used to load and now fails to load — and it is the
reading these documents already claimed. No in-tree fixture used a non-canonical
spelling (`sync/diff.rs` uses `"0644"`). The rule governs the JSON `tree.json`
wire ONLY: the far-side listing frame carries the raw `st_mode` in HEX
(`printf "%x"` ↔ `from_str_radix(.., 16)`), where masking the file-type bits IS
the semantics, and is untouched. No branch that was load-bearing (the guard, the
lock, the audit, the copy) was touched, and the source audits' pinned maps are
unchanged: no `libc` or `std::fs` symbol was added or removed.

## Rules for adding a constraint

- **State what it removes.** A constraint that removes no branch is decoration.
- **Prove the delta.** Before replacing a runtime check with a type, enumerate
  what the check refused and what the type refuses. They are rarely equal — the
  validated-path type is *stricter* than the check it replaces, and that
  difference is a behaviour change, not a refactor.
- **Flipping a test that encoded the looser rule is explicit**, with the reason
  recorded. A silent flip is a lost assertion.
- **A constraint that only the crate can construct is worth more than one the
  caller can build**: unforgeable is the difference between a rule and a
  convention.
