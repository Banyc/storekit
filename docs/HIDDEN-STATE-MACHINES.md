# Hidden state machines

A state machine is a set of states plus the transitions between them. This
inventory covers the machines in `storekit` whose state crosses a boundary the
type system does not close by itself: filesystem entries, an advisory lock held
by the kernel, a child process's lifecycle, a far-side script's step order, or a
run-scoped combination of sets and counters.

Most entries now name an explicit CARRIER — a linear temp, a journal record, a
sealed proof, or an enum that replaced a flag pair. Where that is so, the entry
states what the type makes unrepresentable. Where it is not, the entry names the
authority that still holds the state (kernel, process, far-side script,
filesystem) and its residual. No entry claims a type enforces something it does
not.

Each entry names the authority, the carrier, the states and transitions, and the
point a reviewer must check. Symbols are cited by name; line numbers are
deliberately absent because they rot. Every statement is derived by reading the
named symbols; none is a test result. Test-only machines are listed last.

## Inventory status

One row per entry. `TYPED` is a carrier that is a type or a linear value;
`EXTERNAL` is state the kernel, the filesystem, or a far-side process holds, with
no in-crate carrier left to build — its reach is the entry's Watch; `OPEN` is an
entry whose carrier is still implicit and is an item of this inventory;
`TEST-ONLY` is test scaffolding. An OPEN row is marked TYPED, with the prose of
its entry corrected, in the round that lands it.

| Entry | Status |
|---|---|
| Lock-record lifecycle | EXTERNAL — the kernel's advisory `flock` bit; the typed observations are `LockAttempt` and `RetireOutcome::Held` |
| Atomic-replace commit machine | TYPED |
| Compare-and-swap replace | EXTERNAL — the compare-to-`renameat` window; races are out of scope by `CONSISTENCY.md` |
| Copy mode journal and two-phase copy | TYPED |
| Iterative removal walk | TYPED |
| Durable directory-chain creation | TYPED |
| Owned-root registry | TYPED |
| Residue handle lifecycle | TYPED |
| Destination-ownership token and preflight binding | TYPED |
| Run phase pipeline | TYPED — `StepsRan` → `Settled` → `Reconciled`; `Applier::run` is the chain |
| Mode journal | TYPED |
| Pending-mutation accounting | TYPED |
| Claim, rollback, and residue reconciliation | TYPED |
| Deletion-sanction state | TYPED |
| AppendTail compare-and-append loop | TYPED — the attempt's verdict is `AppendAttempt::{Done, Changed}`; the retry bound is policy and the live destination is the filesystem's |
| Case-sensitivity probe and fold gate | OPEN — `case-sensitivity` |
| Verification passes | TYPED |
| Report partition | TYPED |
| Lazy local root | TYPED |
| Run-scoped caches | TYPED |
| Source-quiescence two-sample check | TYPED |
| Manifest walk, wire assembly, and containment | TYPED — `Classified` is the per-entry carrier; the walk holds no cross-entry state type |
| Create-new seven-stage install | TYPED |
| Create-new verification ladder | TYPED |
| Sidecar mutex critical section | TYPED |
| Local compare-and-delete claim/restore | TYPED |
| Remote default compare-and-delete | EXTERNAL — the documented non-atomic fallback; the live entry is the filesystem's and the claim step is deliberately absent |
| Remote default tree copy | TYPED |
| Far-side lock session | TYPED |
| Far-side script protocols | EXTERNAL — the step order lives in the shell/perl; the Rust boundary reads one status |
| Local process runner lifecycle | TYPED — `Termination` is the timeout/escalation ladder; `ChildLifecycle` and `DrainState` are typed |
| SSH subprocess runner | TYPED |
| Host-identity preparation and pin cache | TYPED |
| Receiver identity and layout provisioning | TYPED |
| Fault and swap latches | TEST-ONLY |
| Scripted transports | TEST-ONLY |
| `IdentityProbe` | TEST-ONLY |
| Source audit traversal | TEST-ONLY |
| Child-process orchestration | TEST-ONLY |
| Prose sweep: stale comments and docs (opt-in) | OPEN — `prose-sweep`: every claim in the declared slice is re-derived or deleted |

## Prose sweep (opt-in)

Not a state machine: a cleanup program over this inventory's prose and the rest
of the crate's comments and docs.

**Rule.** A claim survives only when it was RE-DERIVED from the symbols it
names in the same round and corrected there. Prose nobody re-derived is
DELETED, not kept by default. Every keep, correction, and deletion is recorded
in `docs/PROSE-DISPOSITIONS.md` (one row per covering section), together with
the symbols it was derived from, so a later reader can see what was opted in
and what was removed rather than assumed.

**Slices** — each is a declared, committed scope; a slice touches only its own
files and appends its own ledger rows.

1. `docs/HIDDEN-STATE-MACHINES.md`.
2. `docs/CONSISTENCY.md`, `docs/API-CONSTRAINTS.md`.
3. `README.md`, `EXTRACTION.md`, `MIGRATION.md`.
4. Module docs (`//!`) of `src/**`, sub-sliced per module.
5. Item docs (`///`) of public items in `src/**`, sub-sliced per module.
6. Inline comments (`//`, `/* */`) in `src/**`, sub-sliced per file.

A slice is done when every comment/doc site in its files is dispositioned and
the diff matches the ledger. Reviewers check the ledger and the re-derivation,
not the prose's style.

**Pending dispositions.** `src/sync/apply.rs`, `prepare`'s doc paragraph: "so a
refusal leaves no residue" is false on the SSH path (`prepare_identity` creates
the mux directory and possibly the known-hosts pin before the strict source
read). `lock_with_in_root_lock`'s doc and its module-docs twin: a destination
parent directory that holds the in-root record is refused by `delete_verdict`
as a `ResidueBelow` CONFLICT, not "reported as residue". Both are corrected or
deleted by the slice that covers that file; neither is re-opened here.

## Store substrate

### Lock-record lifecycle

**Where.** `src/lock/mod.rs::FileLock`; `lock::unix::open_lock_file`;
`lock::windows`.

**States.** Absent; empty; recognized record (`RECORD_HEADER`-prefixed); foreign
non-empty entry; record symlink; parent symlink; held by a live holder;
held by this process; released. A released record persists with its header and
last operation id.

**Carrier.** The record's bytes and inode plus the advisory `flock`. `LockAttempt`
names only the result of one attempt.

**Transitions.** `FileLock::acquire` creates or opens the record without
following a symlink, attempts the non-blocking lock, refuses a foreign
non-empty entry before chmod/truncate/write, then writes the header and
operation id. `Drop` unlocks and closes; it never unlinks.

**Watch.** The stable-inode guarantee exists only while the record is not
removed, replaced, or renamed. `AdministrativeRecoveryGuard` is the separate
capability state for breaking the record: it can be constructed only by taking
the local operation lock, and its lifetime is that lock's lifetime.
`retire_destination_lock` is the sanctioned removal path; its outcomes are
`Retired`, `Absent`, `Held`, `DestinationLive`, and `NoRecordLocation`. The one
state that stays EXTERNAL is `held by a live holder`: it is the KERNEL's
advisory `flock` bit, and the typed observations of it are `LockAttempt` (one
acquisition attempt's answer), the `Error::LockContended` refusal it maps to, and
`RetireOutcome::Held` (the same bit read from the retirement side). No Rust value
records that a holder EXISTS, and a
non-cooperating process that never opens this inode is invisible — that is the
residual, and it is why `Drop` never unlinks the record.

### Atomic-replace commit machine

**Where.** `src/atomic/unix.rs::{write_atomic_replace, PathTemp, replace_core,
Temp, write_atomic_cas_fd}`;
`src/atomic/windows.rs::{write_atomic_replace, WindowsTemp}`;
`src/atomic/mod.rs::{ReplaceOutcome, ReplaceStage}`.

**States.** Old target visible; temp created/written; temp private; temp
fsynced; (for compare-and-replace) first compare passed; second compare passed;
renamed (new content visible); parent fsynced (new content durable). A
pre-rename failure is an `Err` with the old content visible; a post-rename
parent fsync failure is `ReplaceOutcome::ReplacedDurabilityUnknown`.

**Carrier.** The LINEAR temp value: `PathTemp` (path-based Unix), `Temp`
(descriptor-relative Unix), `WindowsTemp` (Windows). Each holds its temp name
as an `Option` and its create-time `File` handle; `Published` is what a
successful publish returns, so a post-publish temp unlink is unrepresentable.
`ReplaceStage` names the fault points, not the whole state.

**Transitions.** The stage methods consume and return the temp: on Unix
`create` → `write` → `make_private` → `sync` →
`publish`/`publish_no_replace`; the Windows form has no private-mode stage
(`create` → `write` → `sync` → `publish`). The Unix
write, chmod, and fsync go through the create-time handle, never a descriptor
reopened by name; `publish` takes the name out first, so `Drop` can only unlink
a live pre-publish temp. Every pre-publish exit removes the temp: an explicit
`discard` (carrying both the original failure and a cleanup failure) or the
`Drop` backstop for a `?` return. The Windows form removes the target before
the rename, so its rename stage has a transient-absence state and no
parent-directory commit point.

**Watch.** The two commit points are reported distinctly. Every pre-rename
failure path that has created a temp must discard it; the post-rename failure
is not a cleanup point. The Unix CAS (`write_atomic_cas_fd`) installs with
`linkat` without replacement, chmods the installed entry, then fsyncs the
parent; its temp handle is closed before the link, matching the pre-typestate
descriptor lifetime. The pre-rename stages' chmod/fsync receivers are pinned
by the source audit in `atomic::guard`'s tests. What no value carries is the
FILESYSTEM's answer to the rename and the parent fsync:
`ReplaceOutcome::ReplacedDurable` versus
`ReplaceOutcome::ReplacedDurabilityUnknown` is the crate's only record of it,
and a crash inside that window is the kernel's and the filesystem's state.

### Compare-and-swap replace

**Where.** `src/atomic/unix.rs::{replace_core, CoreReplace, CompareReplace}`.

**States.** Live entry matches `expected` before the temp is created; it still
matches immediately before `renameat`; it does not match at either point. A
mismatch writes nothing and discards the temp.

**Carrier.** The expected bytes are compared at two separate moments; the
comparison result between them is not represented by a type.

**Watch.** The residual window is between the second comparison and
`renameat`. On a destination without a server-side compare, a writer inside that
window is lost and nothing signals it. The temp itself is the linear `Temp`, so
a mismatch — and an observation failure at the second compare — drops the name
structurally; what stays untyped is the window and the live filesystem.

### Copy mode journal and two-phase copy

**Where.** `src/atomic/unix.rs::{CopyModes, PlannedMode,
copy_dir_recursive_fd}`; `src/atomic/unix.rs::copy_tree_verbatim`.

**States.** Journal collecting `PlannedMode { restore, final_mode }` entries;
walk creates or widens directories; `commit` applies the final modes
deepest-first (the copy root last) and clears the journal; a failure in the walk
or in `commit` leaves the entries in place, so the journal's `Drop` restores
every mode deepest-first. A successful `commit` has consumed the journal; a
failed copy is supposed to restore every mode the journal recorded.

**Carrier.** `CopyModes.root`, `CopyModes.modes: Vec<PlannedMode>` (one record
per destination directory, with the FIRST record for a path winning), and the
walk's heap frame stack. There is no `armed` flag and no parallel `dirs`
vector: the consuming `commit` IS the disarm, and the final mode lives in the
same `PlannedMode` as the restore mode.

**Transitions.** A created directory is planned with the removable `0o700`
restore mode and the source's final mode; a pre-existing directory is planned
with its original mode. Success applies the exact modes and clears the
obligations.

**Watch.** `commit` runs before the source-shape recheck in
`copy_dir_recursive_fd`; a source-shape failure therefore returns `Err` with the
intended modes already applied and the journal consumed. The root-final
ordering is part of `commit` (the deepest-first sort leaves the shallowest copy
root last) and is pinned by the root-mode assertion and the mid-commit
failpoint test. `copy_tree_verbatim` has no journal at all; a failed verbatim
copy leaves a partial destination by design.

### Iterative removal walk

**Where.** `src/atomic/unix.rs::remove_dir_contents_fd`;
`src/sync/apply.rs::{Applier::remove_subtree, DirFrame}`;
`sync::apply::Removal`.

**States.** A directory frame is entered; its children are visited one at a
time; a child's outcome is folded into its parent; a directory is removed only
after its live listing is re-read as empty. A reserved child, or a child the
sanction does not address, leaves the holding directory in place.

**Carrier.** The heap stack of `DirFrame { path, rel, children, next, outcome }`
and the walk's one-unit phase `Work::{Enter, Fold}`. The phase is the VARIANT,
so "attempting an entry" and "folding a finished subtree" (the old
`pending`/`completed` pair) cannot both hold.

**Transitions.** A leaf is a single non-recursive unlink. A directory leaf is a
non-recursive `rmdir` after an emptiness re-list. `Removal` is `Removed`,
`ResidueLeft`, or `Blocked`, and `Removal::join` is the fold: `ResidueLeft`
absorbs, `Blocked` upgrades only a `Removed` accumulator, and `Removed` is the
unit — so a directory holding residue is never removed because its siblings
went.

**Watch.** The final directory removal is not recursive, so a child created
after the last enumeration makes the removal fail loudly instead of being
destroyed unnamed. The sync walk re-establishes its sanction against the live
listing before descending into a claimed or partial subtree.

### Durable directory-chain creation

**Where.** `atomic::dirchain::{DirChain, CreatedChain, ChainCommit,
CreatedProof}`; `atomic::unix::{ensure_private_dir_durable,
ensure_private_dir_durable_fd, PathChain, UnixChain}`; the Windows twin in
`atomic::windows`.

**States.** Chain absent; missing components collected deepest-first;
components created top-down at `0o700`; a `CreatedChain` assembled from the
port's state and the indices it created; its `commit` fsyncs the created
entries deepest-first and then the parent-of-parent entry, and only then
returns `DirChain::Created`; `DirChain::AlreadyPresent` when nothing was
created.

**Carrier.** `CreatedChain.chain` (a SEALED `ChainCommit`: `PathChain`,
`UnixChain`, or `WindowsChain`), `CreatedChain.created`, and the live
filesystem. `CreatedProof` is private to `dirchain` and `ChainCommit`'s
`Sealed` supertrait is nameable only there, so an in-crate module cannot hand
`commit` a no-op chain; the only `Created` producer is a port's commit. The
Windows commit is the documented no-op, so its `Created` verdict carries no
durability claim (the proof means "commit ran and returned `Ok`").

**Transitions.** Both `ensure_private_dir_durable` spellings return `bool` at
the boundary as `DirChain::is_created()`; the internal `_chain` fns return the
`DirChain` itself, and `write_atomic_replace`, `replace_core`, and `symlink_fd`
call the chain body directly (running its commit) without consuming the
verdict. `FileLock::acquire` and the public `_fd` entry point use the bool.

**Watch.** A creation or chmod failure mid-chain returns without undoing the
components already created. The caller sees a partial chain, not a rollback.
The created-entry order is deepest-first and the parent-of-parent fsync is a
separate commit step; the `commit-dir-entry*` probes pin both.

### Owned-root registry

**Where.** `src/root.rs::{OWNED_ROOTS, OwnedRoot, OwnedRootRegistration}`.

**States.** Unregistered; registered under `(EndpointKey, canonical root)`;
refcounted by clones; released when the last clone drops.

**Carrier.** The process-global `Mutex<BTreeMap<...>>` plus ONE
`Arc<OwnedRootRegistration>` per root. The registration is the single owner of
that root's `(endpoint, canonical)` pair: `OwnedRoot` holds nothing else, every
clone shares the one `Arc`, and the accessors read the registration's copy.

**Transitions.** `OwnedRoot::parse` checks the path is a real directory, not a
symlink, not the filesystem root, then refuses an equal/ancestor/descendant root
on the same endpoint before inserting the checked pair into the registry and
building the token from it. `OwnedRootRegistration::drop` removes the
registration, which happens on the LAST clone's drop.

**Watch.** The registry is process-global and in-memory. It does not survive a
restart, and it does not coordinate with another process that owns the same
directory.

### Residue handle lifecycle

**Where.** `src/sync/residue.rs::{Residue, ResidueAside, Residue::detect,
Residue::operation_lock, Residue::recover_to, Residue::discard}`.

**States.** Not a residue; missing; present but not a regular entry; genuine
residue. Recovery target absent or occupied. Recovered; discarded; nested
residue refused.

**Carrier.** A private `ResidueAside` — the spelling proof, whose only
constructor is `ResidueAside::parse` and whose one refusal is `NotResidue` —
plus the root it is spelled under. The live filesystem is re-read at each
operation.

**Transitions.** `Residue::detect` parses the aside's spelling, proves it is a
residue, opens the root, and probes the live kind. Both operations acquire the
destination operation lock through `Residue::operation_lock` (one message
spelling, one `storekit residue <verb> at <root> (pid <pid>)` op id), then
re-read existence and kind: recovery refuses an occupied target, renames,
fsyncs the parent, and reads the result back; discard removes a file/symlink or
a directory strand, refusing a nested residue. The substrate's lock-record guard
refuses a lock-record spelling.

**Watch.** A `Residue` is not a lease on the path. The spelling is proven once,
by construction, so no operation re-derives it; only existence and kind are
re-established. `discard` is the explicit destructive decision; the implicit
removal primitives refuse a residue.

## Sync engine

### Destination-ownership token and preflight binding

**Where.** `src/sync/apply.rs::{DestinationOwnership, LockedDestination,
LockedRemoteDestination, InRootLock, Prepared}`;
`DestinationOwnership::{lock, lock_remote, lock_with_in_root_lock}`;
`Prepared::matches`; `sync`.

**States.** `Unowned`; `Locked`; `LockedWithInRoot`; `LockedRemote`. A token is
valid for one run binding, or refused as a replay. A far-side session is alive
or lost.

**Carrier.** The enum arm plus the private `Prepared` fields: `direction`, the
pinned `LocalSide` (its `root_path` plus the lazily pinned `RootDir`),
`remote_root`, `remote_identity`, `remote_is_local`, and `source_meta`. The
DESTINATION shape is not a field: `Prepared::dest_root()` and
`Prepared::dest_is_local()` DERIVE it from the direction and those axes, so a
recorded destination could never disagree with the binding that was checked.

**Transitions.** A minting path (`lock`, `lock_with_in_root_lock`, `lock_remote`)
runs `prepare` — pin the local root, refuse overlapping roots, refuse an
unlockable destination, prepare the transport identity, read the source
manifest — then `require_endpoint_identity`, then takes the lock(s). `sync`
reads the binding through `DestinationOwnership::binding()` (the ONE place it is
read, `None` for `Unowned`), calls `Prepared::matches` before the token is
destructured, runs, then checks a far-side session's `is_alive()` before
returning.

**Watch.** `Prepared::matches` compares the direction AND the pinned local root
(one check), the remote root spelling, the endpoint identity, and the remote
localness — the last because two transports that both state `None` are
otherwise indistinguishable. The destination shape is derived, not compared:
`dest_root()`/`dest_is_local()` are functions of the compared axes, so a token
cannot be replayed against a different tree. A far-side lock is not a lease:
a lost connection releases it, and the post-run liveness check reports the run
as failed.

### Run phase pipeline

**Where.** `sync::{sync, run}`; `sync::apply::{Applier::run, run_steps,
settle, VerifyPass, PreSettleVerified, derive_report}`.

**States.** Preflight → lock → provision remote destination → destination
manifest → reserved strip and diff → unsupported-destination preflight →
containment check → `report_extraneous` → `transfer` → `finalize` →
`verify(VerifyPass::PreSettle)` → `remove_extraneous` → `settle` (`restore`
then `verify(VerifyPass::PostSettle)`) → `reconcile_residue` →
`derive_report` → source re-read.

**Carrier.** The phase chain: three private values in `sync::apply` that each
consume the one before them. `StepsRan` carries the `Result` of `run_steps` —
it is constructed from BOTH arms, so a failed run has a value to travel in —
`Settled` carries that result plus `settle`'s restore failures and post-settle
verify error, and `Reconciled` carries the same after `reconcile_residue`.
`Applier::run` is that chain, and the `SyncResult` it returns — report
included — is assembled only in `Reconciled::report`, so the APPLIER's
post-transfer result — the mutating half of a run — cannot be produced by a
run of it that skipped `settle` or `reconcile_residue`, and a failed
`run_steps` cannot return early. `derive_report` is still a private method
that derives a `SyncReport` from whatever `Applier` state it is given — the
tests call it directly on an applier that never ran a phase — so what the
token gates is the applier chain's own result, not every `SyncReport`. The
`Applier` fields are the run's record; the chain is its control flow.
Separately typed: the two verification moments
(`VerifyPass::{PreSettle, PostSettle}`) and the removal gate — `verify`
returns `PreSettleVerified`, and `remove_extraneous` takes it, so no
destruction can precede the pass that authorises it.

Results reach the caller WITHOUT travelling that chain, all raised in `sync`
(the entry point) and the free `run` function it drives. The PREFLIGHT can
return `Err(SyncError::from(..))` BEFORE an `Applier` exists, and in that case
the engine transfers or removes no entry. That bounds the ENGINE, not the
preparation, locking, and provisioning that surround it: those are stated
where they live in `src/sync/apply.rs`'s module docs, and this entry does not
restate them. And after `applier.run()` returns,
`SourcePlan::confirm` re-reads the source and
`fail_run` attaches the after-the-fact source-quiescence condition to the
applier's result — keeping its report and, on failure, its own error —
without re-entering the chain. Last, `sync` checks the far-side lock session
for liveness and, when the connection died mid-run, returns (or replaces the
result with) a typed transport failure rather than a clean success.

**Transitions.** A failure inside `run_steps` still runs `settle`, residue
reconciliation, and report derivation. Removal runs only after transfers and
the pre-settle pass, and only by presenting that pass's token; `settle` runs
`verify(VerifyPass::PostSettle)` and discards its token because nothing is
removed after it. The source is re-read after the run, by `SourcePlan::confirm`.

**Watch.** The ordering is the safety property: transfer and verification
complete before sanctioned destruction — now a requirement of the removal
call's type rather than of two statements' order — and a failed run still
accounts for what it mutated. The other two obligations are likewise typed,
about the APPLIER's post-transfer result rather than about every report:
`Reconciled::report` is the only place the APPLIER's post-transfer result —
report included — is assembled, and it consumes a `Reconciled`, which only
`Settled::reconcile_residue` produces, which only `StepsRan::settle` produces,
so a run of the applier cannot return its own result before settling and
reconciling, and a failed `run_steps` cannot return
early — its `Err` travels through both phases. Outside the chain, and named
so no reader infers it: the preflight of `sync` and its free `run` returns
`Err` before an `Applier` exists and before any entry is transferred or
removed; `SourcePlan::confirm` / `fail_run` may attach the after-the-fact
source-quiescence condition to the
applier's result without re-entering the chain; and `sync`'s far-side liveness
check may replace a clean result with a transport failure. What the chain does
not gate
is the report type itself: `derive_report` derives one from whatever
`Applier` state it is handed, which is how the tests call it.

### Mode journal

**Where.** `sync::apply::{ModeRecord, ModeJournal, OriginalMode}`;
`widen_ancestors`; `widen_dir_verified`; `finalize`; `restore`; `note_residue`.

**States.** Per live path identity, exactly ONE `ModeRecord` variant: `Seen`
(first touch), `FirstTouchPending` (a final mode is intended and nothing has read
this path's mode yet), `FinalPending`, `Widened`, `WidenedFinalPending`,
`WidenedFinal`, `Final`, `FinalUnlanded` (the intended mode did not land),
`Removed`. `OriginalMode` is `Created` (this sync created the path, so it has no
mode of its own to restore) or `Observed(mode)`, recorded at the first touch.

**Carrier.** `ModeJournal { entries: BTreeMap<String, ModeRecord> }`. The final
mode a directory still owes `finalize` lives in that directory's OWN record, so
there is no second map; the old `final_mode`, `widened`, `final_applied`,
`removed`, and `abandoned` fields are gone.

**Transitions.** `note_first_touch` records the original only once, and a pending
final whose mode nothing had read takes that read as its identity's first touch;
a path removed by this sync starts a fresh identity if recreated. `mark_widened`
records the pre-widen mode as the first touch when nothing did (so a widen can
never run ahead of the mode it restores to), `note_final_pending` records the
intent, `take_pending_finals` drains it exactly once (so a second `finalize` is a
no-op), `mark_final_mode` decides `Final`/`WidenedFinal`/`FinalUnlanded`, and
`mark_removed` ends the identity. `restore_plan` is deepest-first; `mode_targets`
derives verification targets; `widened_paths` excludes only removed paths.

**Watch.** `ModeRecord::Removed` is NOT `Applier.removed`: `discard_partial`
takes a path back OUT of the removal ledger when the original is renamed over the
partial this sync made, while the journal keeps `Removed`, so a later creation
starts a fresh mode identity. A path LEFT IN PLACE as residue has no journal
state of its own (there is no `abandoned` flag): it keeps its widen, and the
report keeps it out of `transient_dirs` because `note_residue` names it in
`dest_residue` (or, once `reconcile_residue` moves it, in `indeterminate`).

### Pending-mutation accounting

**Where.** `sync::apply::{PendingMutations, MutationKind, MutationAttempt,
begin_mutation, commit_mutation}`; `Applier::indeterminate`;
`Applier::location_unknown`; `note_indeterminate`.

**States.** Per path: idle; pending content; pending mode; both; reported
indeterminate. Separately, a path with no begun attempt whose LOCATION could not
be confirmed.

**Carrier.** `PendingMutations { content, mode }` per path in
`Applier::indeterminate`, plus the `location_unknown: BTreeSet<String>` channel.
`MutationAttempt<'p>` — carrying the path spelling and its `MutationKind` — is
the `#[must_use]` token `begin_mutation` mints and `commit_mutation` consumes.
`Outcome` separately records `Skipped` or `Transferred`.

**Transitions.** `begin_mutation` counts the transfer and increments the counter
for the attempted kind; a content attempt also drops the ancestry memo at that
path. `commit_mutation` decrements only the SAME kind, and is reachable only
with the token, so no counter can be decremented that no attempt incremented. A
failed attempt is never decremented. `note_indeterminate` inserts into
`location_unknown` instead: it adds no counter and no transfer, so no commit
can erase it.

**Watch.** A mode restore clears only a mode attempt; it cannot resolve a failed
write, create, rename, or removal. A path remains indeterminate while any begun
attempt is uncommitted. The two channels are unioned only when the report is
derived: an uncommitted attempt may or may not have landed, while an unknown
location asserts neither spelling.

### Claim, rollback, and residue reconciliation

**Where.** `sync::apply::{Claim, ClaimDisposition, UnconfirmedMove,
RenamedEntryLocation}`; `claim_aside`; `drop_claim`; `rollback_claim`;
`discard_partial`; `rename_back`; `locate_after_failed_rename`;
`account_failed_move`; `record_stranded_entry`; `record_leftover_aside`;
`note_residue`; `re_root_residue`; `reconcile_residue`.

**States.** Live at the original path; claimed by rename to an aside; aside
dropped; aside rolled back; partial install discarded; rename landed at the
destination; rename still at the source; location unknown. Residue candidate;
confirmed residue; unconfirmed possibility.

**Carrier.** `Claim` (original path and aside path, no kind),
`ClaimDisposition::{Dropped, LeftBehind { claim, cause }}` (the linear claim's
only hand-back, and the only way a leftover aside can be recorded),
`UnconfirmedMove`, `dest_residue`, `claim_failures`, and live probes.
`RenamedEntryLocation` names only the read-back result.

**Transitions.** A failed claim or rollback rename is read back before anything
is named: `account_failed_move` is the ONE accounting for both renames —
`Destination` re-roots the recorded residue, `Source` leaves it, `Unknown`
pushes an `UnconfirmedMove` and asserts neither spelling — and the aside itself
is named only when the read-back confirms it still holds the entry
(`record_stranded_entry`). A confirmed aside is residue; a confirmed-gone aside
is not; an unconfirmable probe records a candidate consumed by
`reconcile_residue`. The report-time reconciliation confirms candidates against
the live destination and routes an unconfirmable one to `indeterminate` plus a
restore failure naming both possible spellings.

**Watch.** The report's lists assert existence and contain only confirmed
paths. The possibility channel asserts no location. A claim carries no kind, so
the removal primitive reads the live kind at its own entry.

### Deletion-sanction state

**Where.** `sync::apply::{Sanction, may_delete, is_prohibited,
is_directly_prohibited}`; `live_entry_manifest_spelling`;
`live_entry_is_addressed`; `residue_guards`; `ConflictReason::forbids_destruction`.

**States.** Unprohibited; conflict-prohibited; residue-guarded; aliased; a
destination-only `Extraneous` entry; the sync's own claim; the sync's own
partial creation.

**Carrier.** The `conflicts` map, `dest_residue`, `aliased_dest`, and the live
listing. There is no separate `blocked` set.

**Transitions.** `ConflictReason::forbids_destruction` is an exhaustive match, so
a conflict reason must decide its prohibition at compile time. `may_delete`
accepts the caller's proof (`ExtraneousFlag`, `OwnClaim`, `OwnPartial`);
`remove_subtree` re-establishes an `OwnClaim`/`OwnPartial` sanction against the
live listing before destroying a child.

**Watch.** A live entry no manifest spelling addresses is preserved and named.
The proof for own bookkeeping is not a licence to destroy a writer-created child.

### AppendTail compare-and-append loop

**Where.** `sync::apply::{append_tail, append_attempt, append_settle_mode,
append_write, AppendAttempt, CheckedWrite}`.

**States.** Attempt `0..=8`; `Done` or `Changed`; destination absent, regular
file, or non-file; destination a prefix, source a prefix, equal bytes, or
diverged; checked write written or mismatched.

**Carrier.** The loop index and the live destination read at each attempt.

**Transitions.** Read the live kind and bytes; apply the prefix rule; perform a
checked write against the bytes just read; a mismatch re-reads and re-decides.
An exhausted loop reports `ConflictReason::Diverged`; it never silently writes.

**Watch.** There is no atomic append. A remote destination has no server-side
compare, and the local compare-to-`renameat` window remains.

### Case-sensitivity probe and fold gate

**Where.** `sync::apply::{case_sensitivity, refuse_unrepresentable_case_aliases,
CaseSensitivity, CaseProbe, CaseMember}`;
`aliased_dest`; `refuse_address_folded_onto_another_name`.

**States.** Unprobed; probed insensitive; probed sensitive. Probe directory
absent, created, found by flipped spelling, removed, or left as residue. Source
entry aliased or not; conflict recorded or not.

**Carrier.** `CaseSensitivity { Unprobed, Sensitive, Insensitive }`
(`Applier::dest_case_sensitivity`), `aliased_dest:
BTreeMap<String, String>`, and a reserved `.sync-aside.case-probe...` directory
on disk carried by a created `CaseProbe { rel, flipped }` — both spellings
parsed once — until it is removed.

**Transitions.** Group source and destination names by fold; probe only when a
transfer is at stake and the destination root exists; create a mixed-case probe
directory, look it up with the case flipped, remove it, and cache the answer.
An aliased on-disk spelling is recorded so it and its subtree cannot be removed.

**Watch.** An absent destination root cannot be probed without creating it, so
the pair is conservatively refused without probing; that is the CALLER's
condition (`root_present()`), not a probe state, so no state can claim a probe
that never ran. `Unprobed` is the only askable state, so an answered probe
cannot be asked again, and `CaseProbe` is created before it can be answered or
removed and consumed by removal, so neither can happen twice. The probe's own
create and remove are counted mutations and can leave residue.

### Verification passes

**Where.** `sync::apply::{verify, verify_names, verify_directory_listings,
verify_claimed_untouched, VerifyPass, PreSettleVerified, PassReport}`;
`Applier::{verified, verify_failures, listings}`.

**States.** Per path: unverified; content/kind verified; mode verified; name
verified; failed; applied; skipped. The run has TWO passes, `VerifyPass::PreSettle`
and `VerifyPass::PostSettle`.

**Carrier.** `verify: Vec<VerifyItem>`, `verified: BTreeSet`,
`verify_failures: BTreeSet`, one `PassReport { failed, first_error }` per pass,
the mode journal's targets (`mode_targets(include_restored)`), and
`touched_dirs`. A pass that raises no new ERROR returns `PreSettleVerified`, the
removal step's required token.

**Transitions.** `verify` clears the listing cache, checks kind and then content
(hash or symlink target), checks the mode targets — including the modes a
restore wrote back only on `PostSettle` — verifies names and ancestor names
byte-exactly, verifies touched-directory listings, and re-confirms
claimed-untouched entries. A path remains in `verified_now` only when every
check in the pass passed; a later check removes it on failure, and failures are
removed from the cumulative `verified` set.

**Watch.** `verified` must mean the latest pass. A path that passed the first
pass and failed the post-settle pass is not `applied`. A directory that cannot
be enumerated faithfully is a run-level error, not a per-path conflict. The
token means the pass RAN: a path failed through an already-recorded conflict
(`PassReport::fail_without_error`) raises no new error and still yields one — the
conflict record, not the token, is what prohibits that path's removal. A
`PostSettle` pass returns the same token type only because the signature is
shared: `settle` discards it and no removal follows.

### Report partition

**Where.** `sync::apply::{derive_report, ReportClass, ReportClaims,
ReportPartition}`.

**States.** Each path may appear in `indeterminate`, `conflicts`, `residue`,
`applied`, `skipped`, `extraneous`, or `verify_failures`. `transient_dirs` is
outside the partition and may share a path with `applied` or
`verify_failures`.

**Carrier.** The outcome map, mode journal, conflicts, residue set, extraneous
set, and verification sets. `ReportClaims` holds one candidate set per class,
`ReportClass`'s declaration order IS the precedence, and `ReportPartition` is
the seven filed lists.

**Transitions.** Apply the one precedence:

`indeterminate` > `conflicts` > `residue` > `verify_failures` > `applied` >
`skipped` > `extraneous`.

`ReportClaims::classes` takes the MIN class per path over `ReportClass`'s `Ord`,
so the declaration is the single authority and a path claimed by a higher list
is absent from every lower one. `transient_dirs` is disjoint from every class
except `applied` and `verify_failures`. The unsupported destination annotation
is filtered to paths the report already names.

**Watch.** A successful `Ok` run may still carry `verify_failures`. `Ok` means
the run completed, not that the destination is clean.

### Lazy local root

**Where.** `sync::apply::{LocalSide, LocalRole, LocalSide::open,
LocalSide::manifest, LocalSide::destination_manifest, LocalSide::root_dir,
LocalSide::root_for_mutation, LocalSide::confirm_pinned_root}`.

**States.** Root absent and unpinned (destination only); present and pinned;
present and unpinned then pinned; absent then adopted as empty or created;
appeared non-empty and refused; disappeared after pinning and refused.

**Carrier.** `LocalSide { root_path, role: LocalRole, root: OnceCell<RootDir> }`
and the live filesystem. `LocalRole::{Source, Destination}` is the ONE authority
for "may create" and for "an absent root is the empty tree"; `ensure: bool` is
gone because it named the mechanism, not the side.

**Transitions.** `open` pins an existing directory immediately and refuses a
non-directory. Only a DESTINATION root may stay unpinned: it is created or
adopted at the first mutation, an empty directory is adopted, a non-empty one is
refused, and `manifest` reads a missing one as the empty tree. A missing SOURCE
root is an error. `confirm_pinned_root` compares the pinned inode with the path
after a manifest walk.

**Watch.** The destination root's own mode is not journaled, and a read-only
root fails loudly rather than being widened.

### Run-scoped caches

**Where.** `sync::apply::{listings, CachedListing, ancestry_dirs, touched_dirs,
removed, clear_listings}`.

**States.** Cache empty; populated; cleared at each verify pass. Ancestry memo
empty or populated; an entry dropped on a content mutation at that path.
`touched_dirs` accumulates; `removed` records paths this run removed, and
`is_already_gone` checks their ancestors.

**Carrier.** `RefCell<BTreeMap<String, CachedListing>>`,
`RefCell<BTreeSet<String>>`, `BTreeSet<String>`.

**Transitions.** `verify` calls `clear_listings` first. `begin_mutation` with
`MutationKind::Content` drops the ancestry memo at the mutated path.
`remove_subtree` and `drop_claim` insert into `removed`.

**Watch.** The ancestry memo is used only for a confined local destination; a
path-based destination probes live every time. Transfer-time and removal-time
listings may predate later sibling mutations and may be consulted only for the
one path's own name and kind.

### Source-quiescence two-sample check

**Where.** `sync::run`; `sync::apply::{SourcePlan, SourcePlan::confirm,
source_changed_error, fail_run}`.

**States.** Plan read before locking; source stable at the post-run re-read;
source added, removed, or changed; re-read failed. Separately: a run-level
condition discovered AFTER the engine returned, which `fail_run` attaches.

**Carrier.** `SourcePlan(TreeMetadata)` — the manifest the ENTRY POINT read
BEFORE the destination lock and before provisioning — and the second
`source.manifest()` call, which `confirm` consumes the plan against. `fail_run`
is the ONE place an after-the-fact condition is attached: an `Ok` report becomes
an `Err` carrying it, and an existing `SyncError` keeps its own error and gains
a `restore_failures` entry.

**Transitions.** After `Applier::run`, `SourcePlan::confirm(source.manifest())`
compares the two reads. A difference fails the run and names the paths that
moved; a failed re-read also fails it, as an unconfirmed quiescence.

**Watch.** Two samples can refute quiescence, never prove it. The crate cannot
lock a source, so the obligation stays the caller's and the type only states
that it was CHECKED; a source that changes and changes back is
indistinguishable from a stable one.

### Manifest walk, wire assembly, and containment

**Where.** `manifest::{canonicalize_tree_with, UnsupportedSink,
RefuseUnsupported, RecordUnsupported, EntryAssembler, ShownPath,
CompleteListing}`; `canonicalize_remote_entries_with`; `SymlinkContainmentIndex`;
`ComponentResolution`; `SpelledWalkRefusal`; `KindIndex`; `ContainmentViews`;
`check_relative_symlink_target_two_views`.

**States.** Local walk collecting entries; per-entry path/kind/mode/content
validation; post-pass containment over the complete entry set; sort and digest.
Wire assembly parsing fields; parent-closure check; containment post-pass;
strict refusal or tolerated recording. Containment resolution: absent,
non-symlink, or symlink; refusal: escapes root, through symlink, or
plan-dependent.

**Carrier.** `Classified`: ONE per-entry classification — `Dir { mode }`,
`File { mode, content_sha256 }`, `Symlink { content_sha256, target }` — whose
`push_into` is the only conversion to the `EntryAssembler::push` call, so the
kind and the fields an entry carries cannot disagree and the symlink's fixed
`0o777` exists in one place (one variant per kind, not a kind plus a field set
computed beside it). BOTH walks build this value and push it. `EntryAssembler`:
the growing entry vector and the `seen` set, finished with the `ShownPath` the
refusal names the link by. `CompleteListing` carries the listing text and the
root it describes TOGETHER with the producing walk's exit status. The walk has
no state type ACROSS entries: each classification is built, pushed and dropped
inside one loop iteration.

**Transitions.** `EntryAssembler::push` validates the spelling, refuses a
duplicate, and appends; `EntryAssembler::finish` CONSUMES the assembler, runs
the one containment post-pass (skipped when no relative symlink exists) and then
`sink.finish`, so no caller can write an entry after the verdict. The wire arm
runs `require_parent_closed` before `finish` (the local walk is parent-closed by
construction, `WalkDir` yielding a directory before its contents). The strict
sink fails on the first tolerated condition; the recording sink stores the
reason and keeps the entry under its live kind. The wire assembler runs the same
validators and the same fold as the local walk. `CompleteListing::new` is the
only constructor of the completeness evidence and refuses a listing whose
producing walk did not exit zero, so no assembler can be called without it.

**Watch.** Containment is a property of the result, so the applier re-checks the
source manifest against the raw destination observation, not a precomputed
result view. The fold must not be narrower than the destination host's fold. The
post-pass and the duplicate rule exist ONCE, in `EntryAssembler`, so the two
views cannot diverge; only the link's display differs (`ShownPath`: the local
walk's `root.join(rel)` versus the wire's manifest spelling). The
kind-and-its-fields pairing exists ONCE, in `Classified`, so neither view can
attribute one kind's field to another (a `Dir` with a hash, a `File` with a
target).

## Transport

### Create-new seven-stage install

**Where.** `src/transport/mod.rs::{CreateNewStep, CreateNewFault,
CreateNewVerdict, CreateNewOptions, CreateNewTemp, Publish,
durable_create_new}`; `SshTransport::write_new_cmd`;
`SshTransport::try_write_new_sidecar_cmd`.

**States.** Temp create; write; final chmod; file fsync; no-replace publish
(`Publish::Linked` or `Publish::NameTaken`, i.e. `EEXIST`); temp unlink; parent
fsync. The publish either installs the entry or finds a winner, which leads to
verification and `AlreadyPresent` or `Conflict`.

**Carrier.** The local arm's LINEAR `CreateNewTemp { path: Option<PathBuf>,
file: Option<File> }` plus the commit-point verdict `Publish::{Linked,
NameTaken}`; `CreateNewStep` names the stages, `CreateNewFault` is a one-shot
`AtomicBool` per fixture, and `CreateNewVerdict` (with `VerifiedExisting`) is the
outcome.

**Transitions.** The stage methods consume and return `CreateNewTemp`, so
reaching a later stage proves every earlier one ran. The commit TAKES THE NAME
OUT before the `link(2)`, which disarms `Drop`; `publish` then releases the temp
name best-effort in every outcome, so no caller arm owns a cleanup. A pre-install
far-side failure propagates; a conflict verifies the existing entry; an identical
retry converges and still fsyncs the parent; a different winner is never
replaced.

**Watch.** The LOCAL cleanup obligation is structural: every pre-publish exit —
each `?` and each injected `CreateNewStep` fault, `Publish` included — drops the
temp and its `Drop` unlinks the name, so no local stage leaves a temp behind. The
`Unlink` fault still fires after the commit and still reports an `Err` over a
committed destination, but the temp it used to leave is already gone. The
FAR-SIDE scripts are the external half: they remove their own temp on a
pre-install failure, and no Rust type sees their intermediate state.

### Create-new verification ladder

**Where.** `transport::{verify_existing, OpenedExisting, VerifiedExisting}`;
`SshTransport::verify_open_script`; test-only `VerifySwap`.

**States.** Open with `O_NOFOLLOW`; fstat the opened descriptor; read through the
same descriptor. The producer's typed observation is
`OpenedExisting::{Regular, NotFound, NotRegular, Unreadable}`: `Regular` is
constructed only after the opened inode is proven regular. For `Regular`: exact
mode, content equivalence; readability is decided by the open/read step that
produced `Unreadable`.

**Carrier.** `OpenedExisting`'s one armed observation, produced by the ONE
open→fstat→read closure (`open_verify_local`, or the ssh framed helper):
`Regular(OpenedRegular)` carries that inode's raw `mode` (type bits included;
the comparison masks `0o7777`) and its fd-read `content`, so `verify_existing`
evaluates one consistent observation in order and never re-derives a kind.
`VerifiedExisting` is the ladder's verdict, and `verified_to_verdict` is its only
route to `CreateNewVerdict`.

**Transitions.** The first applicable class wins: not-regular before
unreadable/mode/content; unreadable before mode; mode before content. `Ok` is
reachable only when every check held on the one inode.

**Watch.** A symlink is never followed. A test `VerifySwap` fires once at
`BeforeOpen`, `AfterOpen`, or `AfterFstat`; the descriptor-bound property is the
reason an after-open swap does not change the verdict.

### Sidecar mutex critical section

**Where.** `src/transport/mod.rs::{SIDECAR_HELD, SidecarScope, SidecarHold,
acquire_sidecar, with_operation_lock_sidecar, wait_for_sidecar_flock,
ensure_operation_lock_sidecar_durable}`.

**States.** This thread holds the sidecar or it does not; sidecar record absent,
created, or existing; lock free, contended, or acquired; wait arm EWOULDBLOCK,
EINTR, deadline, or other error; scope held or released.

**Carrier.** `SidecarScope::{Owned(SidecarHold), Nested}` is the acquisition's
answer as a VALUE; `SIDECAR_HELD` is a thread-local `Cell<bool>` re-entrancy bit
raised only once the `flock` is held; `SidecarHold` owns the read-only
descriptor and is the RAII release. The record is a stable filesystem inode.

**Transitions.** `acquire_sidecar` runs the reserved-spelling gate FIRST
(`refuse_reserved_mutation` with `Sanction::None`), before the re-entrancy answer
and before creating anything; an already-holding thread gets `Nested` and runs
its closure inside the outer section. Otherwise it ensures the record durably,
opens it read-only, waits against a monotonic deadline, raises the bit, and
returns `Owned`; `Drop` clears the bit and unlocks on every exit path, a panic
included.

**Watch.** A mutation declares that it runs under this scope by carrying it
(`RemoveIfRoute::Serialized`), rather than re-reading an ambient depth. The
sidecar serializes mutations of the one record `Layout::lock` names; it is not
destination ownership and does not exclude a sync's sibling record. The wait is
bounded but a live holder is waited for, unlike `FileLock::acquire`, and it is
ADVISORY: a non-cooperating writer that never opens the record is not excluded.

### Local compare-and-delete claim/restore

**Where.** `src/transport/mod.rs::{LocalTransport::remove_file_if,
RemoveIfRoute, Claimed}`; `RemoveIfVerdict`.

**States.** Serialized branch: read, match (remove), or mismatch (the entry,
inode included, is left untouched). Claim branch: absent; claimed by rename to an
aside; verified match; verified mismatch; removed; restored; conflict; error.

**Carrier.** `RemoveIfRoute::{Serialized(SidecarScope), Claim}` — chosen ONCE by
the caller from the mutation's OWN ownership (the identity-checked
`GuardedRel`) and the thread's held bit. The claim branch's intermediate state is
`Claimed { aside, content }`, a linear value whose `Drop` discards the aside
exactly once.

**Transitions.** Under the sidecar the read-compare-unlink is direct and the
scope stays bound for the whole arm. Without it, `Claimed::claim` renames to a
unique sibling and reads the claimed inode (a `NotFound` rename is a genuine
`Absent`), then `delete` on a match or `restore` no-replace on a mismatch. A
concurrent winner is never replaced.

**Watch.** In the claim branch a failed read discards the claim and propagates.
A failed no-replace restore propagates EXPLICITLY, and the moved claim —
discarded by the same `Drop` on the way out — was the record's only copy; the
slot is not blocked, the lease is the backstop. That deliberate destructive edge
is the residual the comment on `Claimed::restore` states.

### Remote default compare-and-delete

**Where.** `transport::Remote::remove_file_if` default;
`LocalTransport::remove_file_if` and `SshTransport::remove_file_if` overrides.

**States.** Absent; present and read; content equal to `expected`; content
different; removed; mismatch. The default is a metadata probe, a read, a
compare, and a removal; the production overrides add the claim/restore or
sidecar route.

**Carrier.** The live entry and the `RemoveIfVerdict` result. The default has no
claim temp, so its comparison and removal are separate states.

**Transitions.** A confirmed absent entry returns `Absent`; a read failure
propagates; equal content is removed; different content is `Mismatch` and left
in place.

**Watch.** The default is non-atomic. A production transport that does not
override it has no claim step, so two contenders can both observe a match. The
crate's production path bounds the primitive to the one lock record the layout
owns.

### Remote default tree copy

**Where.** `transport::Remote::copy_tree` default;
`transport::copy_tree_walk`; `SshTransport::copy_tree`.

**States.** Destination parent created; source walked; each entry recreated as
directory (owner-writable during the walk), file, or symlink; directory final
modes applied deepest-first.

**Carrier.** The walk's heap frame stack and `PendingFinalModes`, which records
each directory's final mode in PRE-ORDER and applies them deepest-first. The
default has no temp directory or final rename.

**Transitions.** `copy_tree_walk` recreates the tree through `list`/`read`/`write`;
`PendingFinalModes::finalize` consumes the pre-order and applies it in reverse,
so every child's mode precedes its parent's. The SSH override runs a
same-filesystem `cp -a` instead.

**Watch.** The default drops ownership, xattrs, ACLs, and timestamps; the SSH
override preserves more metadata. A failed default copy leaves a partial
destination tree. The finalize contract is the deepest-first parent-before-child
property, not the order among siblings.

### Far-side lock session

**Where.** `src/transport/ssh::{SshFarSideLock, SessionState,
spawn_far_side_lock, perl_hold_lock, far_side_lock_refusal, reap_bounded}`.

**States.** Spawn; readiness reader; `LOCKOK`; holder live; released; exited with
status; unanswerable. Refusal states: contention, unrecognized record, holder
error, missing perl, ssh-before-command, acquisition timeout.

**Carrier.** `SshFarSideLock { state: SessionState, stderr, record, local_pid }`
where `SessionState::{Alive { child }, Released, Lost { status }, Unanswerable }`
— exactly one holds, so released, lost, and unanswerable cannot be mixed and a
live client cannot also carry an exit status. `stderr` is a DIAGNOSTIC channel,
not lifecycle state; `holder::*` names the reserved exit codes and markers.

**Transitions.** The remote holder creates the record, takes a non-blocking
`flock`, refuses a foreign non-empty record, writes the header and op id, prints
`LOCKOK`, then blocks on stdin. Closing stdin makes it unlock and exit; `release`
waits a bounded grace, then kills and reaps. Every transition out of `Alive` is
one-way: `is_alive` moves to `Lost` on an observed exit or to `Unanswerable` when
the wait itself fails (fail closed), and `release` moves to `Released`; `Drop`
releases.

**Watch.** The session is a lock, not a lease. The run checks liveness after it
finishes; a dead session makes the run fail even if every operation succeeded.
Whether the holder actually holds the kernel `flock` is EXTERNAL: the readiness
line is the only evidence, and the kernel releases the record when the client
dies.

### Far-side script protocols

**Where.** `SshTransport::{upload_bytes, write_new_cmd,
try_write_new_sidecar_cmd, remove_file_if_sidecar_cmd, recover_sidecar_cmd}`;
`perl_hold_lock`; `transport::ssh::holder`; `RemotePublish`.

**States.** Parent mkdir; temp allocation; payload on stdin; length verify;
chmod; file fsync; publish by `link` or `rename`; temp unlink; parent fsync.
Verdict frames `R`/`M`/`A`, `OK`/`MISMATCH`/`ABSENT`, and reserved exit codes.

**Carrier.** Shell/perl locals, temp entries, and exit codes. No Rust type sees
the intermediate filesystem state; the Rust boundary only CLASSIFIES a status:
`RemotePublish::{InstalledDurably, Existed, PreInstall}` for create-new, the
sidecar verbs' verdict frames, and `holder::*` for the lock holder's reserved
codes and readiness markers.

**Transitions.** Uploads report a post-rename parent-fsync failure with the
reserved post-rename exit; create-new reports a pre-install failure or a
confirmed conflict; sidecar remove/recover report their verdict frame.

**Watch.** The seven-step create-new order and the upload order are the remote
realizations of the local primitives, and their STEP ORDER stays EXTERNAL — the
crate reads one status, never a far-side state. A pre-install failure must not
publish; a confirmed conflict must not replace the winner.

### Local process runner lifecycle

**Where.** `transport::runner::{OwnedChild, ChildLifecycle, RunnerConfig,
ChildRunner::exec, Termination, Escalating, KillFailure}`;
`transport::runner::unix::exec`; `ProcessState`; `RunOutcome`; `RunError`;
`DrainState`.

**States.** Spawned; running; exited but unreaped (zombie); timeout TERM;
escalated KILL; owned-handle kill; foreground-only group check; reaped; drained.
Error states: spawn, wait, background leftover, kill, reap.

**Carrier.** `OwnedChild { child, kill, lifecycle }` where
`ChildLifecycle::{Live, Reaped(ExitStatus)}` is the reap state as a VALUE: the
handle is reachable only through `live_child()`, `None` once reaped, so a signal
after the reap is unrepresentable and the status is carried instead of
discarded. The timeout and its escalation are `Termination::Armed { deadline }`
→ `Termed` → `GroupKilled` → `ChildKilled` on Unix (`Armed` → `ChildKilled` on
Windows, which has no process groups): ONE ascending value whose transitions are
the SOLE authority for that order. Each transition method (`termed`,
`group_killed`, `child_killed`) matches its required PREDECESSOR into the next
rung and dispatches the rung's signal only in that arm, so a rung advances only
from its predecessor and a transition attempted from any OTHER rung is a no-op
that signals nothing and returns the ladder unchanged. That order is a runtime
rule each transition's own `match` enforces — not a typing fact, and not a guard
repeated at the call sites — so the callers in `unix::exec` and `windows::exec`
supply only the DUE test (the bound that decides whether a rung is attempted) and
the dispatch, and the same rung cannot repeat and KILL cannot precede TERM. Each
rung past `Armed` carries `Escalating { escalation_start, failure }`, so the
instant the bounds are measured from is a field rather than a parallel local,
and the `KillFailure` accumulator's only writer is the transition that
dispatched the signal (the LAST failure is the reported one). The process group
and the zombie's allocated pid are kernel state.

**Transitions.** The wait loop peeks with `waitid(WNOWAIT)` so the direct child
stays a zombie and holds its pgid. On timeout it TERMs and escalates to KILL;
after exit it enumerates live group members, terminates any leftover, then reaps
exactly once, moving `ChildLifecycle` to `Reaped`. `OwnedChild::drop` is the
final backstop and returns early once reaped.

**Watch.** The foreground-only guarantee covers in-group descendants only. A
`setsid` escapee is detected only while it holds an inherited pipe; a fully
daemonized descendant is outside the contract. On Windows the group check and
group kill do not exist. The process group's membership, the zombie's pid, and
whether a descendant still lives are the KERNEL's state; `ChildLifecycle` and
`DrainState` are the only typed observations of them. `Termination`'s rungs
record that a signal was DISPATCHED, never that it landed: non-delivery is the
`KillFailure` the rung carries, and effectiveness is decided by the reap bound
and, on Unix, the liveness enumeration — `GroupKilled` must not be read as "the
group is dead". A rung refused by the ladder says nothing about the child: the
refusal is the ladder's order rule, not a kernel answer.

### SSH subprocess runner

**Where.** `transport::ssh::runner::{SshRunner, SpawnedChild, SshRunner::run,
RunError}`; `SshRunnerSeam`.

**States.** Spawned; wait thread running; output success or error; timeout with
the child already reaped (collect the real result) or still running (kill, join,
timeout); disconnected; leftover pipes.

**Carrier.** `SpawnedChild.status: Arc<WaitStatusCell>` holding
`WaitStatus::{Running, Reaped}`, the kill closure's EXCLUSIVE handle to the owned
child, the result channel, and the wait thread's join handle. The status cell is
only the deadline HINT; the channel carries the authority (`WaitResult`).

**Transitions.** `recv_timeout` selects the arm. A timeout with
`WaitStatus::Reaped` does not kill and receives the wait closure's real result; a
timeout with `Running` kills, joins, and reports a timeout. A disconnected
channel kills and joins.

**Watch.** The timeout branch can block on `rx.recv()` after the status reads
`Reaped`; the wait closure's bounded drain is what makes that wait terminate.
The deadline does not bound the whole call: termination grace and drain are
additive. Whether the child has actually been reaped is the KERNEL's state; the
`WaitStatus` cell is the one typed mirror of it, and it is a hint, not the
result.

### Host-identity preparation and pin cache

**Where.** `transport::ssh::{SshTransport::prepare_identity,
SshTransport::pin_known_hosts, SshTransport::prepare_for_request, HostIdentity}`;
`transport::ssh::hostkey`.

**States.** Identity classified ONCE at construction: `HostIdentity::KnownHosts`
or `HostIdentity::Fingerprint { expected, pinned }` (the constructor refuses the
both-set and the neither-set pair). Unprepared; mux directory absent or present.
Pin absent; valid; stale/unreadable/mismatched; removed; keyscan fetched;
matched; pinned; create-new failed.

**Carrier.** `SshTransport { identity: HostIdentity, known_hosts_cache_dir,
... }`: the identity SOURCE is a value, so no operation re-checks the pair, and
the pin's RESOLUTION is `HostIdentity::Fingerprint`'s
`pinned: Mutex<Option<PathBuf>>`, set only by `prepare_identity`. The mux
directory and the pin cache file are filesystem state.

**Transitions.** Every request self-prepares (`prepare_for_request` →
`prepare_identity`). Preparation creates/chmods the mux directory and calls
`pin_known_hosts`, which validates or re-pins the cache; `ssh_args` refuses an
unresolved pin rather than falling back to trust-on-first-use. `pin_known_hosts`
removes a stale cache, runs `ssh-keyscan`, filters by fingerprint, and creates
the pin with `create_new`.

**Watch.** A concurrent re-pin that wins the `create_new` makes the loser return
`EEXIST`; it does not re-validate and adopt the winner. The mux directory and
pin files are preparation residue that the crate never removes. What stays
external is the HOST's key: the crate can only compare what `ssh-keyscan`
reported against the configured fingerprint inside one run.

### Receiver identity and layout provisioning

**Where.** `transport::{ReceiverId, MarkerState, read_receiver_id,
provision_receiver_id, Layout}`; `LocalTransport::provision_layout`;
`SshTransport::provision_layout`.

**States.** Base absent or present; bootstrap directories absent or present; the
marker probe's outcome — `MarkerState::Present(id)`, `MarkerState::Absent`, or a
typed error (non-UTF-8, legacy format, malformed, read/metadata failure).
Provisioned.

**Carrier.** Directories on disk and the immutable marker `<id>\n`. `MarkerState`
is the one probe's outcome; only a CONFIRMED absence is `Absent`, so an
unanswerable probe is never absence.

**Transitions.** Provisioning creates the root and bootstrap directories, then
probes the marker: `Present` adopts the identity; `Absent` creates a fresh id
with `try_write_new`, whose `Created` returns it and whose `AlreadyPresent` or
`Conflict(_)` re-probes and requires the existing marker. A malformed or legacy
marker fails closed; `require` turns a confirmed absence into the typed
absent-marker refusal.

**Watch.** The marker is created once and never replaced. A concurrent
provisioner's existing marker is the truth. The default `Remote::provision_layout`
is a no-op; a production transport that does not override it provisions nothing.

## Test-only state machines

### Fault and swap latches

`transport::{CreateNewFault, VerifySwap}` arm an `AtomicBool` for exactly one
stage or boundary. The state is armed → fired; the stage/boundary is chosen at
construction. Production never arms one.

### Scripted transports

`sync::apply::tests::{RecordingRemote, PathRemote}` and
`transport::scripted::ScriptedExec` use operation-index counters, `fail_nth_*`
fields, `after_write`/`after_rename`/`after_removal` hooks, source and
destination writer hooks, and one-shot flags such as `first_op_fired`. The
scripted object's field combination is the state.

### `IdentityProbe`

`sync::diff::tests::IdentityProbe` gates the remote `exec` path on a
`prepared: AtomicBool` set by `prepare_identity`. It is a two-state contract
test for the transport preparation precondition.

### Source audit traversal

`atomic::guard::tests` carries module path, alias table, `in_funnel`, and a stack
of `let` locals through the parsed AST. It is a state machine over source
syntax, not over runtime filesystem state.

### Child-process orchestration

`fifo_regression` and `deep_tree_regression` drive child modes, resource limits,
stack sizes, and `ChildOutcome` values. The child process's phase and the
parent's probe are the state.
