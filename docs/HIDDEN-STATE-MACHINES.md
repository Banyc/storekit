# Hidden state machines

A state machine is a set of states plus the transitions between them. This
inventory lists the machines in `storekit` whose state is not one Rust state
type. Their state is carried by combinations of flags, counters, maps, sets,
thread-locals, filesystem entries, process state, or generated far-side scripts.
A machine can be tested and still be hidden in this sense: nothing in the type
system prevents an unlisted transition.

Each entry names the authority, the state carrier, the states and transitions,
and the point a reviewer must check. Symbols are cited by name; line numbers are
deliberately absent because they rot. Every statement is derived by reading the
named symbols; none is a test result. Test-only machines are listed last.

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
`Retired`, `Absent`, `Held`, `DestinationLive`, and `NoRecordLocation`.

### Atomic-replace commit machine

**Where.** `src/atomic/unix.rs::{write_atomic_replace, replace_core}`;
`src/atomic/windows.rs::write_atomic_replace`;
`src/atomic/mod.rs::{ReplaceOutcome, ReplaceStage}`.

**States.** Old target visible; temp created/written; temp private; temp
fsynced; (for compare-and-replace) first compare passed; second compare passed;
renamed (new content visible); parent fsynced (new content durable). A
pre-rename failure is an `Err` with the old content visible; a post-rename
parent fsync failure is `ReplaceOutcome::ReplacedDurabilityUnknown`.

**Carrier.** Stage-local code plus the temp and target entries. `ReplaceStage`
names the fault points, not the whole state.

**Transitions.** The Unix path-based and fd-based forms share the stage order;
the Windows form removes the target before the rename, so its rename stage has a
transient-absence state and no parent-directory commit point.

**Watch.** The two commit points are reported distinctly. Every pre-rename
failure path that has created a temp must discard it; the post-rename failure is
not a cleanup point.

### Compare-and-swap replace

**Where.** `src/atomic/unix.rs::{replace_core, CoreReplace, CompareReplace}`.

**States.** Live entry matches `expected` before the temp is created; it still
matches immediately before `renameat`; it does not match at either point. A
mismatch writes nothing and discards the temp.

**Carrier.** The expected bytes are compared at two separate moments; the
comparison result between them is not represented by a type.

**Watch.** The residual window is between the second comparison and
`renameat`. On a destination without a server-side compare, a writer inside that
window is lost and nothing signals it.

### Copy undo journal and two-phase copy

**Where.** `src/atomic/unix.rs::{CopyUndo, copy_dir_recursive_fd}`;
`src/atomic/unix.rs::copy_tree_verbatim`.

**States.** Undo journal armed; per-directory restore mode planned; walk
creates or widens directories; final modes applied deepest-first; journal
disarmed. A failed copy is supposed to restore every mode the journal recorded.

**Carrier.** `CopyUndo.armed: bool`, `CopyUndo.modes: Vec<(PathBuf, u32)>`, the
walk's `dirs` vector, and the heap frame stack.

**Transitions.** A created directory is journaled with the removable `0o700`; a
pre-existing directory is journaled with its original mode. Success applies the
exact modes and disarms.

**Watch.** `CopyUndo::disarm` runs before the source-shape recheck in
`copy_dir_recursive_fd`. A source-shape failure therefore returns `Err` with the
intended modes already applied and the journal disarmed. `copy_tree_verbatim`
has no undo journal at all; a failed verbatim copy leaves a partial destination
by design.

### Iterative removal walk

**Where.** `src/atomic/unix.rs::remove_dir_contents_fd`;
`src/sync/apply.rs::{Applier::remove_subtree, DirFrame}`;
`sync::apply::Removal`.

**States.** A directory frame is entered; its children are visited one at a
time; a child's outcome is folded into its parent; a directory is removed only
after its live listing is re-read as empty. A reserved child leaves the holding
directory in place.

**Carrier.** The heap stack, `DirFrame.next`, `DirFrame.outcome`, `pending`, and
`completed`. `pending` and `completed` are never both set by construction.

**Transitions.** A leaf is a single non-recursive unlink. A directory leaf is a
non-recursive `rmdir` after an emptiness re-list. `Removal` is `Removed`,
`ResidueLeft`, or `Blocked`.

**Watch.** The final directory removal is not recursive, so a child created
after the last enumeration makes the removal fail loudly instead of being
destroyed unnamed. The sync walk re-establishes its sanction against the live
listing before descending into a claimed or partial subtree.

### Durable directory-chain creation

**Where.** `atomic::unix::{ensure_private_dir_durable,
ensure_private_dir_durable_fd}`; the Windows twin in `atomic::windows`.

**States.** Chain absent; missing components collected deepest-first; components
created top-down at `0o700`; each created component's parent fsynced; caller's
parent fsynced; chain durable.

**Carrier.** The `missing` vector and the live filesystem. The function returns
whether it created anything.

**Watch.** A creation or chmod failure mid-chain returns without undoing the
components already created. The caller sees a partial chain, not a rollback.

### Owned-root registry

**Where.** `src/root.rs::{OWNED_ROOTS, OwnedRoot, OwnedRootRegistration}`.

**States.** Unregistered; registered under `(EndpointKey, canonical root)`;
refcounted by clones; released when the last clone drops.

**Carrier.** The process-global `Mutex<BTreeMap<...>>` plus the `Arc` registration
token.

**Transitions.** `OwnedRoot::parse` checks the path is a real directory, not a
symlink, not the filesystem root, then refuses an equal/ancestor/descendant root
on the same endpoint before inserting. `OwnedRootRegistration::drop` removes the
registration.

**Watch.** The registry is process-global and in-memory. It does not survive a
restart, and it does not coordinate with another process that owns the same
directory.

### Residue handle lifecycle

**Where.** `src/sync/residue.rs::{Residue, Residue::detect, Residue::recover_to,
Residue::discard}`.

**States.** Not a residue; missing; present but not a regular entry; genuine
residue. Recovery target absent or occupied. Recovered; discarded; nested
residue refused.

**Carrier.** A `Residue` value is a proof about a path observed at `detect`
time; the live filesystem is re-read at each operation.

**Transitions.** Recovery takes the destination operation lock, revalidates the
aside, refuses an occupied target, renames, fsyncs the parent, and reads the
result back. Discard takes the same lock, revalidates the spelling, and removes
a file/symlink or a directory strand, refusing a nested residue.

**Watch.** A `Residue` is not a lease on the path. `discard` is the explicit
destructive decision; the implicit removal primitives refuse a residue.

## Sync engine

### Destination-ownership token and preflight binding

**Where.** `src/sync/apply.rs::{DestinationOwnership, LockedDestination,
LockedRemoteDestination, InRootLock, Prepared}`;
`DestinationOwnership::{lock, lock_remote, lock_with_in_root_lock}`;
`Prepared::matches`; `sync`.

**States.** `Unowned`; `Locked`; `LockedWithInRoot`; `LockedRemote`. A token is
valid for one run binding, or refused as a replay. A far-side session is alive
or lost.

**Carrier.** The enum arm plus the private `Prepared` fields: direction, pinned
local root, destination root, destination localness, remote root spelling,
remote endpoint identity, remote localness, and source manifest.

**Transitions.** A minting path runs the preflight, prepares the transport
identity, reads the source manifest, records the transport binding, and takes
the lock(s). `sync` re-checks all five compared axes before use, runs, then
checks a far-side session's liveness before returning.

**Watch.** `Prepared::matches` compares direction, local root, remote root,
endpoint identity, and remote localness. The derived destination shape is not a
compared axis; it is implied by the other axes. A far-side lock is not a lease:
a lost connection releases it, and the post-run liveness check reports the run
as failed.

### Run phase pipeline

**Where.** `sync::{sync, run}`; `sync::apply::{Applier::run, run_steps,
settle, derive_report}`.

**States.** Preflight → lock → provision remote destination → destination
manifest → reserved strip and diff → unsupported-destination preflight →
containment check → `report_extraneous` → `transfer` → `finalize` →
`verify(false)` → `remove_extraneous` → `settle` (`restore` then
`verify(true)`) → `reconcile_residue` → `derive_report` → source re-read.

**Carrier.** The phase order and the `Applier` fields. There is no phase enum.

**Transitions.** A failure inside `run_steps` still runs `settle`, residue
reconciliation, and report derivation. Removal runs only after transfers and
the first verification pass. The source is re-read after the run.

**Watch.** The ordering is the safety property: transfer and verification
complete before sanctioned destruction, and a failed run still accounts for what
it mutated.

### Mode journal

**Where.** `sync::apply::{ModeEntry, ModeJournal}`; `widen_ancestors`;
`widen_dir_verified`; `finalize`; `restore`; `note_residue`.

**States.** Per live path identity: untracked; first touch with original mode
(recorded once); widened; final mode recorded; final mode applied; removed;
abandoned.

**Carrier.** `kind`, `original: Option<Mode>`, `final_mode: Option<Mode>`,
and four independent flags: `widened`, `final_applied`, `removed`, `abandoned`.

**Transitions.** `note_first_touch` records the original only once; a path
removed by this sync starts a fresh identity if recreated. `mark_widened`,
`mark_final_mode`, `mark_removed`, and `mark_abandoned` move the flags.
`restore_plan` is deepest-first; `mode_targets` derives verification targets;
`widened_paths` excludes removed and abandoned paths.

**Watch.** The flags can represent contradictory combinations (`removed` plus
`abandoned`, or `final_applied` plus `removed`). The derived functions filter
them, but the invariant is conventional rather than type-enforced.

### Pending-mutation accounting

**Where.** `sync::apply::{PendingMutations, MutationKind, Outcome,
begin_mutation, commit_mutation}`; `Applier::indeterminate`;
`note_indeterminate`.

**States.** Per path: idle; pending content; pending mode; both; reported
indeterminate.

**Carrier.** Two counters per path in a `BTreeMap`. `Outcome` separately records
`Skipped` or `Transferred`.

**Transitions.** `begin_mutation` counts the transfer and increments the counter
for the attempted kind; a content attempt also drops the ancestry memo at that
path. `commit_mutation` decrements only the same kind. A failed attempt is never
decremented. `note_indeterminate` inserts a permanent content count without an
attempt.

**Watch.** A mode restore clears only a mode attempt; it cannot resolve a failed
write, create, rename, or removal. A path remains indeterminate while any begun
attempt is uncommitted.

### Claim, rollback, and residue reconciliation

**Where.** `sync::apply::{Claim, UnconfirmedMove, RenamedEntryLocation}`;
`claim_aside`; `drop_claim`; `rollback_claim`; `discard_partial`; `rename_back`;
`locate_after_failed_rename`; `record_stranded_entry`;
`record_leftover_aside`; `note_residue`; `re_root_residue`;
`reconcile_residue`.

**States.** Live at the original path; claimed by rename to an aside; aside
dropped; aside rolled back; partial install discarded; rename landed at the
destination; rename still at the source; location unknown. Residue candidate;
confirmed residue; unconfirmed possibility.

**Carrier.** `Claim` (original path and aside path, no kind), `UnconfirmedMove`,
`dest_residue`, `claim_failures`, and live probes. `RenamedEntryLocation` names
only the read-back result.

**Transitions.** A failed claim or rollback rename is read back before anything
is named. A confirmed aside is residue; a confirmed-gone aside is not; an
unconfirmable probe records a candidate consumed by `reconcile_residue`. The
report-time reconciliation confirms candidates against the live destination and
routes an unconfirmable one to `indeterminate` plus a restore failure naming
both possible spellings.

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

**Where.** `sync::apply::{dest_case_insensitive, remove_probe,
refuse_unrepresentable_case_aliases, CaseMember}`;
`aliased_dest`; `refuse_address_folded_onto_another_name`.

**States.** Unprobed; probed insensitive; probed sensitive. Probe directory
absent, created, found by flipped spelling, removed, or left as residue. Source
entry aliased or not; conflict recorded or not.

**Carrier.** `dest_case_insensitive: Option<bool>`, `aliased_dest:
BTreeMap<String, String>`, and a reserved `.sync-aside.case-probe...` directory
on disk.

**Transitions.** Group source and destination names by fold; probe only when a
transfer is at stake and the destination root exists; create a mixed-case probe
directory, look it up with the case flipped, remove it, and cache the answer.
An aliased on-disk spelling is recorded so it and its subtree cannot be removed.

**Watch.** An absent destination root cannot be probed without creating it, so
the pair is conservatively refused without probing. The probe's own create and
remove are counted mutations and can leave residue.

### Verification passes

**Where.** `sync::apply::{verify, verify_names, verify_directory_listings,
verify_claimed_untouched}`; `Applier::{verified, verify_failures, listings}`.

**States.** Per path: unverified; content/kind verified; mode verified; name
verified; failed; applied; skipped. The run has a pre-settle pass and a
post-settle pass.

**Carrier.** `verify: Vec<VerifyItem>`, `verified: BTreeSet`,
`verify_failures: BTreeSet`, the mode journal's targets, and `touched_dirs`.

**Transitions.** `verify` clears the listing cache, checks kind and content,
checks mode targets, verifies names and ancestor names byte-exactly, verifies
touched-directory listings, and re-confirms claimed-untouched entries. A path
remains in `verified_now` only when every check in the pass passed; a later
check removes it on failure, and failures are removed from the cumulative
`verified` set.

**Watch.** `verified` must mean the latest pass. A path that passed the first
pass and failed the post-settle pass is not `applied`. A directory that cannot
be enumerated faithfully is a run-level error, not a per-path conflict.

### Report partition

**Where.** `sync::apply::{derive_report, apply_precedence}`.

**States.** Each path may appear in `indeterminate`, `conflicts`, `residue`,
`applied`, `skipped`, `extraneous`, or `verify_failures`. `transient_dirs` is
outside the partition and may share a path with `applied` or
`verify_failures`.

**Carrier.** The outcome map, mode journal, conflicts, residue set, extraneous
set, and verification sets.

**Transitions.** Apply the one precedence:

`indeterminate` > `conflicts` > `residue` > `applied` > `skipped` >
`extraneous` > `verify_failures`.

A path named by a higher list is removed from every lower list. The unsupported
destination annotation is filtered to paths the report already names.

**Watch.** A successful `Ok` run may still carry `verify_failures`. `Ok` means
the run completed, not that the destination is clean.

### Lazy local root

**Where.** `sync::apply::{LocalSide, LocalSide::open, LocalSide::manifest,
LocalSide::destination_manifest, LocalSide::root_dir,
LocalSide::root_for_mutation, LocalSide::confirm_pinned_root}`.

**States.** Root absent and unpinned (destination only); present and pinned;
present and unpinned then pinned; absent then adopted as empty or created;
appeared non-empty and refused; disappeared after pinning and refused.

**Carrier.** `LocalSide.root: OnceCell<RootDir>` plus `ensure: bool` and the live
filesystem.

**Transitions.** `open` pins an existing directory immediately. A destination
root is created or adopted only at the first mutation; an empty directory is
adopted, a non-empty one is refused. `confirm_pinned_root` compares the pinned
inode with the path after a manifest walk.

**Watch.** A missing source root is an error; an absent destination root is the
empty tree. The destination root's own mode is not journaled, and a read-only
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

**Where.** `sync::run`; `sync::apply::source_changed_error`.

**States.** Plan read before locking; source stable at the post-run re-read;
source added, removed, or changed; re-read failed.

**Carrier.** `source_plan: TreeMetadata` and the second `source.manifest()` call.

**Transitions.** After `Applier::run`, re-read the source and compare it to the
plan. A difference fails the run and names the changed paths; a failed re-read
also fails it.

**Watch.** Two samples can refute quiescence, never prove it. A source that
changes and changes back is indistinguishable from a stable one.

### Manifest walk, wire assembly, and containment

**Where.** `manifest::{canonicalize_tree_with, UnsupportedSink,
RefuseUnsupported, RecordUnsupported}`; `canonicalize_remote_entries_with`;
`require_complete_walk`; `SymlinkContainmentIndex`; `ComponentResolution`;
`SpelledWalkRefusal`; `KindIndex`; `ContainmentViews`;
`check_relative_symlink_target_two_views`.

**States.** Local walk collecting entries; per-entry path/kind/mode/content
validation; post-pass containment over the complete entry set; sort and digest.
Wire assembly parsing fields; parent-closure check; containment post-pass;
strict refusal or tolerated recording. Containment resolution: absent,
non-symlink, or symlink; refusal: escapes root, through symlink, or
plan-dependent.

**Carrier.** The growing entry vector, the `seen` set, the sink, and the
containment indexes. The walk itself has no state type.

**Transitions.** The strict sink fails on the first tolerated condition; the
recording sink stores the reason and keeps the entry under its live kind. The
wire assembler runs the same validators and the same fold as the local walk.
`require_complete_walk` refuses a listing whose producing walk did not exit
zero.

**Watch.** Containment is a property of the result, so the applier re-checks the
source manifest against the raw destination observation, not a precomputed
result view. The fold must not be narrower than the destination host's fold.

## Transport

### Create-new seven-stage install

**Where.** `src/transport/mod.rs::{CreateNewStep, CreateNewFault,
CreateNewVerdict, durable_create_new}`; `SshTransport::write_new_cmd`;
`SshTransport::try_write_new_sidecar_cmd`.

**States.** Temp create; write; final chmod; file fsync; no-replace publish;
temp unlink; parent fsync. The publish either creates the entry or reports
`EEXIST`, which leads to verification and `AlreadyPresent` or `Conflict`.

**Carrier.** Stage-local code, the temp and destination entries, and a one-shot
`AtomicBool` in `CreateNewFault`.

**Transitions.** A pre-install failure propagates; a conflict verifies the
existing entry; an identical retry converges and still fsyncs the parent; a
different winner is never replaced.

**Watch.** The local arm returns early after the temp exists at the `Write`,
`Chmod`, `FileFsync`, `Publish`, and `Unlink` stages without unlinking it. The
far-side scripts remove the temp on their pre-install failures. A local
`Publish` fault leaves the temp; a local `Unlink` fault can leave both the
published entry and its temp.

### Create-new verification ladder

**Where.** `transport::{verify_existing, OpenedExisting, VerifiedExisting}`;
`SshTransport::verify_open_script`; test-only `VerifySwap`.

**States.** Open with `O_NOFOLLOW`; fstat the opened descriptor; read through the
same descriptor. Outcomes: entry, not found, not regular, unreadable. For an
entry: type, exact mode, content equivalence; readability is decided by the
open/read step that produced the `Unreadable` outcome.

**Carrier.** The single opened descriptor's metadata and content; the decision
ladder evaluates them in order.

**Transitions.** The first applicable class wins: not-regular before
unreadable/mode/content; unreadable before mode; mode before content. `Ok` is
reachable only when every check held on the one inode.

**Watch.** A symlink is never followed. A test `VerifySwap` fires once at
`BeforeOpen`, `AfterOpen`, or `AfterFstat`; the descriptor-bound property is the
reason an after-open swap does not change the verdict.

### Sidecar mutex critical section

**Where.** `src/transport/mod.rs::{SIDECAR_DEPTH, with_operation_lock_sidecar,
SidecarHold, wait_for_sidecar_flock, ensure_operation_lock_sidecar_durable}`.

**States.** Re-entrancy depth zero or positive; sidecar record absent, created,
or existing; lock free, contended, or acquired; wait arm EWOULDBLOCK, EINTR,
deadline, or other error; hold or release.

**Carrier.** `SIDECAR_DEPTH` is a thread-local `Cell<usize>`; `SidecarHold` is the
RAII release; the record is a stable filesystem inode.

**Transitions.** The public entry runs the reserved-spelling gate before the
depth check. A nested call runs the closure directly. A non-nested call ensures
the record, opens it read-only, waits against a monotonic deadline, raises the
depth, and holds until drop. `Drop` restores the previous depth and unlocks.

**Watch.** The sidecar serializes mutations of the one record `Layout::lock`
names; it is not destination ownership and does not exclude a sync's sibling
record. The wait is bounded but a live holder is waited for, unlike
`FileLock::acquire`.

### Local compare-and-delete claim/restore

**Where.** `src/transport/mod.rs::LocalTransport::remove_file_if_inner`;
`RemoveIfVerdict`.

**States.** Sidecar-held branch: read, match, remove, or mismatch. Claim branch:
absent; claimed by rename; verified match; verified mismatch; removed;
restored; conflict; error.

**Carrier.** `SIDECAR_DEPTH` selects the branch. The claim branch's temp name and
its content are the intermediate state.

**Transitions.** Under the sidecar, the read-compare-remove is direct. Without
it, rename to a unique claim temp, verify, unlink on match, or restore
no-replace on mismatch. A concurrent winner is never replaced.

**Watch.** In the claim branch a failed read or a failed restore reports an
error after unlinking the claim temp. The moved record is the only copy lost in
that transition; the comment states this explicitly.

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

**Carrier.** The walk's heap frame stack and the `dirs` vector of
`(path, final_mode, depth)`. The default has no temp directory or final rename.

**Transitions.** `copy_tree_walk` recreates the tree through `list`/`read`/`write`;
the caller sorts `dirs` by depth and applies the final modes. The SSH override
runs a same-filesystem `cp -a` instead.

**Watch.** The default drops ownership, xattrs, ACLs, and timestamps; the SSH
override preserves more metadata. A failed default copy leaves a partial
destination tree.

### Far-side lock session

**Where.** `src/transport/ssh::{SshFarSideLock, spawn_far_side_lock,
perl_hold_lock, far_side_lock_refusal, reap_bounded}`.

**States.** Spawn; readiness reader; `LOCKOK`; holder live; released; exited
with status; lost. Refusal states: contention, unrecognized record, holder
error, missing perl, ssh-before-command, acquisition timeout.

**Carrier.** The local ssh child, the remote perl process, the first stdout line,
and the child exit status. `SshFarSideLock` carries `released: bool` and cached
`status`.

**Transitions.** The remote holder creates the record, takes a non-blocking
`flock`, refuses a foreign non-empty record, writes the header and op id, prints
`LOCKOK`, then blocks on stdin. Closing stdin makes it unlock and exit. Release
waits a bounded grace; if the client has not exited, it kills and reaps it.

**Watch.** The session is a lock, not a lease. The run checks liveness after it
finishes; a dead session makes the run fail even if every operation succeeded.

### Far-side script protocols

**Where.** `SshTransport::{upload_bytes, write_new_cmd,
try_write_new_sidecar_cmd, remove_file_if_sidecar_cmd, recover_sidecar_cmd}`;
`perl_hold_lock`.

**States.** Parent mkdir; temp allocation; payload on stdin; length verify;
chmod; file fsync; publish by `link` or `rename`; temp unlink; parent fsync.
Verdict frames `R`/`M`/`A`, `OK`/`MISMATCH`/`ABSENT`, and reserved exit codes.

**Carrier.** Shell/perl locals, temp entries, and exit codes. No Rust type sees
the intermediate filesystem state.

**Transitions.** Uploads report a post-rename parent-fsync failure with the
reserved post-rename exit; create-new reports a pre-install failure or a
confirmed conflict; sidecar remove/recover report their verdict frame.

**Watch.** The seven-step create-new order and the upload order are the remote
realizations of the local primitives. A pre-install failure must not publish;
a confirmed conflict must not replace the winner.

### Local process runner lifecycle

**Where.** `transport::runner::{OwnedChild, RunnerConfig, ChildRunner::exec}`;
`transport::runner::unix::exec`; `ProcessState`; `RunOutcome`; `RunError`;
`DrainState`.

**States.** Spawned; running; exited but unreaped (zombie); timeout TERM;
escalated KILL; owned-handle kill; foreground-only group check; reaped; drained.
Error states: spawn, wait, background leftover, kill, reap.

**Carrier.** `OwnedChild.reaped: bool`; the exec loop's `timed_out`,
`sent_kill`, `sent_owned`, and `kill_error` locals; the process group and the
zombie's allocated pid.

**Transitions.** The wait loop peeks with `waitid(WNOWAIT)` so the direct child
stays a zombie and holds its pgid. On timeout it TERMs and escalates to KILL;
after exit it enumerates live group members, terminates any leftover, then reaps
exactly once. `OwnedChild::drop` is the final backstop.

**Watch.** The foreground-only guarantee covers in-group descendants only. A
`setsid` escapee is detected only while it holds an inherited pipe; a fully
daemonized descendant is outside the contract. On Windows the group check and
group kill do not exist.

### SSH subprocess runner

**Where.** `transport::ssh::runner::{SshRunner, SpawnedChild, SshRunner::run,
RunError}`; `SshRunnerSeam`.

**States.** Spawned; wait thread running; output success or error; timeout with
the child already reaped (collect the real result) or still running (kill, join,
timeout); disconnected; leftover pipes.

**Carrier.** `SpawnedChild.reaped: Arc<AtomicBool>`, the result channel, and the
wait thread's join handle.

**Transitions.** `recv_timeout` selects the arm. A timeout with `reaped` set
does not kill and receives the wait closure's real result; a timeout with
`reaped` clear kills, joins, and reports a timeout. A disconnected channel kills
and joins.

**Watch.** The timeout branch can block on `rx.recv()` after `reaped` is set; the
wait closure's bounded drain is what makes that wait terminate. The deadline
does not bound the whole call: termination grace and drain are additive.

### Host-identity preparation and pin cache

**Where.** `transport::ssh::{SshTransport::prepare_identity,
SshTransport::pin_known_hosts}`; `transport::ssh::hostkey`.

**States.** Unprepared; mux directory absent or present. Identity source:
explicit `known_hosts`, configured fingerprint, or neither (refused at
construction). Pin cache absent; valid; stale/unreadable/mismatched; removed;
keyscan fetched; matched; pinned; create-new failed.

**Carrier.** `SshTransport.pinned_known_hosts: Mutex<Option<PathBuf>>`, the mux
directory, and the pin cache file.

**Transitions.** Every request self-prepares. Preparation creates/chmods the mux
directory and, for a fingerprint without an explicit known-hosts file, validates
or re-pins the cache. `pin_known_hosts` removes a stale cache, runs
`ssh-keyscan`, filters by fingerprint, and creates the pin with `create_new`.

**Watch.** A concurrent re-pin that wins the `create_new` makes the loser return
`EEXIST`; it does not re-validate and adopt the winner. The mux directory and
pin files are preparation residue that the crate never removes.

### Receiver identity and layout provisioning

**Where.** `transport::{ReceiverId, read_receiver_id_opt, provision_receiver_id,
Layout}`; `LocalTransport::provision_layout`; `SshTransport::provision_layout`.

**States.** Base absent or present; bootstrap directories absent or present;
receiver marker absent, valid, malformed, legacy-format, or concurrently
created. Provisioned.

**Carrier.** Directories on disk and the immutable marker `<id>\n`.

**Transitions.** Provisioning creates the root and bootstrap directories, then
reads or creates the receiver marker. A missing marker is created with
`try_write_new`; `Created` returns the new id; `AlreadyPresent` or `Conflict`
adopts the existing marker by reading it. A malformed or legacy marker fails
closed.

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
