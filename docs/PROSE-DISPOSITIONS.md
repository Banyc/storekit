# Prose dispositions

The opt-in ledger for the prose sweep declared in
[`HIDDEN-STATE-MACHINES.md`](HIDDEN-STATE-MACHINES.md) ("Prose sweep
(opt-in)"). One row per covering section: a claim is kept only when it was
**re-derived** from the symbols it names in the round that landed the slice;
prose no one re-derived is **deleted**. `symbols` names what the decision was
derived from, so a later reader can re-check it.

| Slice | File | Section | Disposition | Symbols |
|---|---|---|---|---|

## Slice 1 — `docs/HIDDEN-STATE-MACHINES.md` (change `lvqpxqmm`)

| File | Section | Disposition | Symbols re-derived from |
|---|---|---|---|
| docs/HIDDEN-STATE-MACHINES.md | (intro) | not-a-claim | — |
| docs/HIDDEN-STATE-MACHINES.md | Inventory status | not-a-claim | — |
| docs/HIDDEN-STATE-MACHINES.md | Prose sweep (opt-in) | kept-corrected | `prepare`, `prepare_identity`, `destination_lock_record`, `lock_with_in_root_lock`, `delete_verdict`, `ConflictReason::ResidueBelow` |
| docs/HIDDEN-STATE-MACHINES.md | Store substrate | not-a-claim | — |
| docs/HIDDEN-STATE-MACHINES.md | Lock-record lifecycle | kept-corrected | `FileLock::acquire`, `open_lock_file`, `RECORD_HEADER`, `LockAttempt`, `AdministrativeRecoveryGuard`, `RetireOutcome`, `retire_destination_lock` |
| docs/HIDDEN-STATE-MACHINES.md | Atomic-replace commit machine | kept-corrected | `PathTemp`, `Temp`, `WindowsTemp`, `Published`, `ReplaceStage`, `ReplaceOutcome`, `write_atomic_cas_fd`, `replace_core` |
| docs/HIDDEN-STATE-MACHINES.md | Compare-and-swap replace | kept-corrected | `replace_core`, `CoreReplace`, `CompareReplace`, `Temp::{create,write,make_private,sync,publish}` |
| docs/HIDDEN-STATE-MACHINES.md | Copy mode journal and two-phase copy | kept-corrected | `CopyModes`, `PlannedMode`, `CopyModes::commit`, `CopyModes::Drop`, `copy_dir_recursive_fd`, `copy_tree_verbatim` |
| docs/HIDDEN-STATE-MACHINES.md | Iterative removal walk | kept-corrected | `remove_dir_contents_fd`, `Applier::remove_subtree`, `DirFrame`, `Work`, `Removal::join`, `Side::remove_dir` |
| docs/HIDDEN-STATE-MACHINES.md | Durable directory-chain creation | kept-corrected | `DirChain`, `CreatedChain`, `ChainCommit`, `CreatedProof`, `PathChain`, `UnixChain`, `WindowsChain`, `ensure_private_dir_durable`, `ensure_private_dir_durable_fd`, `symlink_fd` |
| docs/HIDDEN-STATE-MACHINES.md | Owned-root registry | kept-corrected | `OwnedRoot::parse`, `OwnedRootRegistration`, `OWNED_ROOTS`, `EndpointKey`, `roots_overlap` |
| docs/HIDDEN-STATE-MACHINES.md | Residue handle lifecycle | kept-corrected | `Residue`, `ResidueAside::parse`, `Residue::{detect,operation_lock,recover_to,discard}` |
| docs/HIDDEN-STATE-MACHINES.md | Sync engine | not-a-claim | — |
| docs/HIDDEN-STATE-MACHINES.md | Destination-ownership token and preflight binding | kept-corrected | `DestinationOwnership::{lock,lock_remote,lock_with_in_root_lock,binding}`, `Prepared`, `Prepared::matches` |
| docs/HIDDEN-STATE-MACHINES.md | Run phase pipeline | kept-corrected | `Applier::run`, `run_steps`, `settle`, `StepsRan`, `Settled`, `Reconciled`, `VerifyPass`, `PreSettleVerified`, `derive_report`, `SourcePlan::confirm`, `fail_run` |
| docs/HIDDEN-STATE-MACHINES.md | Mode journal | kept-corrected | `ModeRecord`, `OriginalMode`, `ModeJournal`, `note_first_touch`, `mark_widened`, `take_pending_finals`, `mark_final_mode`, `mark_removed`, `restore_plan`, `mode_targets`, `widened_paths` |
| docs/HIDDEN-STATE-MACHINES.md | Pending-mutation accounting | kept-corrected | `PendingMutations`, `MutationKind`, `MutationAttempt`, `begin_mutation`, `commit_mutation`, `note_indeterminate`, `Outcome`, `location_unknown` |
| docs/HIDDEN-STATE-MACHINES.md | Claim, rollback, and residue reconciliation | kept-corrected | `Claim`, `ClaimDisposition`, `UnconfirmedMove`, `RenamedEntryLocation`, `claim_aside`, `drop_claim`, `rollback_claim`, `discard_partial`, `rename_back`, `locate_after_failed_rename`, `account_failed_move`, `reconcile_residue` |
| docs/HIDDEN-STATE-MACHINES.md | Deletion-sanction state | kept-corrected | `Sanction`, `may_delete`, `is_prohibited`, `is_directly_prohibited`, `live_entry_manifest_spelling`, `live_entry_is_addressed`, `residue_guards`, `ConflictReason::forbids_destruction`, `delete_verdict` |
| docs/HIDDEN-STATE-MACHINES.md | AppendTail compare-and-append loop | kept-corrected | `append_tail`, `append_attempt`, `append_settle_mode`, `append_write`, `AppendAttempt`, `CheckedWrite` |
| docs/HIDDEN-STATE-MACHINES.md | Case-sensitivity probe and fold gate | kept-corrected | `case_sensitivity`, `refuse_unrepresentable_case_aliases`, `CaseSensitivity`, `CaseProbe`, `CaseMember`, `aliased_dest`, `root_present` |
| docs/HIDDEN-STATE-MACHINES.md | Verification passes | kept-corrected | `verify`, `verify_names`, `verify_directory_listings`, `verify_claimed_untouched`, `VerifyPass`, `PreSettleVerified`, `PassReport`, `clear_listings`, `verified` |
| docs/HIDDEN-STATE-MACHINES.md | Report partition | kept-corrected | `derive_report`, `ReportClass`, `ReportClaims`, `ReportPartition`, `transient_dirs` |
| docs/HIDDEN-STATE-MACHINES.md | Lazy local root | kept-corrected | `LocalSide`, `LocalRole`, `LocalSide::{open,manifest,destination_manifest,root_dir,root_for_mutation,confirm_pinned_root}` |
| docs/HIDDEN-STATE-MACHINES.md | Run-scoped caches | kept-corrected | `listings`, `CachedListing`, `ancestry_dirs`, `touched_dirs`, `removed`, `clear_listings`, `is_already_gone` |
| docs/HIDDEN-STATE-MACHINES.md | Source-quiescence two-sample check | kept-corrected | `run`, `SourcePlan`, `SourcePlan::confirm`, `source_changed_error`, `fail_run` |
| docs/HIDDEN-STATE-MACHINES.md | Manifest walk, wire assembly, and containment | kept-corrected | `canonicalize_tree_with`, `UnsupportedSink`, `RefuseUnsupported`, `RecordUnsupported`, `EntryAssembler`, `ShownPath`, `CompleteListing`, `Classified`, `SymlinkContainmentIndex`, `check_relative_symlink_target_two_views`, `require_parent_closed` |
| docs/HIDDEN-STATE-MACHINES.md | Transport | not-a-claim | — |
| docs/HIDDEN-STATE-MACHINES.md | Create-new seven-stage install | kept-corrected | `CreateNewStep`, `CreateNewFault`, `CreateNewVerdict`, `CreateNewOptions`, `CreateNewTemp`, `Publish`, `durable_create_new`, `write_new_cmd`, `try_write_new_sidecar_cmd` |
| docs/HIDDEN-STATE-MACHINES.md | Create-new verification ladder | kept-corrected | `verify_existing`, `OpenedExisting`, `OpenedRegular`, `VerifiedExisting`, `verified_to_verdict`, `open_verify_local`, `verify_open_script`, `VerifySwap` |
| docs/HIDDEN-STATE-MACHINES.md | Sidecar mutex critical section | kept-corrected | `SIDECAR_HELD`, `SidecarScope`, `SidecarHold`, `acquire_sidecar`, `with_operation_lock_sidecar`, `wait_for_sidecar_flock` |
| docs/HIDDEN-STATE-MACHINES.md | Local compare-and-delete claim/restore | kept-corrected | `LocalTransport::remove_file_if`, `RemoveIfRoute`, `Claimed`, `RemoveIfVerdict` |
| docs/HIDDEN-STATE-MACHINES.md | Remote default compare-and-delete | kept-corrected | `Remote::remove_file_if`, `LocalTransport::remove_file_if`, `SshTransport::remove_file_if`, `metadata_opt` |
| docs/HIDDEN-STATE-MACHINES.md | Remote default tree copy | kept-corrected | `Remote::copy_tree`, `copy_tree_walk`, `PendingFinalModes`, `SshTransport::copy_tree` |
| docs/HIDDEN-STATE-MACHINES.md | Far-side lock session | kept-corrected | `SshFarSideLock`, `SessionState`, `spawn_far_side_lock`, `perl_hold_lock`, `far_side_lock_refusal`, `reap_bounded`, `holder::READY` |
| docs/HIDDEN-STATE-MACHINES.md | Far-side script protocols | kept-corrected | `upload_bytes`, `write_new_cmd`, `try_write_new_sidecar_cmd`, `remove_file_if_sidecar_cmd`, `recover_sidecar_cmd`, `RemotePublish` |
| docs/HIDDEN-STATE-MACHINES.md | Local process runner lifecycle | kept-corrected | `OwnedChild`, `ChildLifecycle`, `RunnerConfig`, `Termination`, `Escalating`, `KillFailure`, `RunOutcome`, `RunError`, `DrainState` |
| docs/HIDDEN-STATE-MACHINES.md | SSH subprocess runner | kept-corrected | `SshRunner`, `SpawnedChild`, `SshRunnerSeam`, `WaitStatus`, `WaitStatusCell`, `WaitResult` |
| docs/HIDDEN-STATE-MACHINES.md | Host-identity preparation and pin cache | kept-corrected | `HostIdentity`, `prepare_identity`, `pin_known_hosts`, `prepare_for_request`, `ssh_args`, `mux_socket_dir` |
| docs/HIDDEN-STATE-MACHINES.md | Receiver identity and layout provisioning | kept-corrected | `ReceiverId`, `MarkerState`, `MarkerState::probe`, `read_receiver_id`, `provision_receiver_id`, `Layout`, `provision_layout` |
| docs/HIDDEN-STATE-MACHINES.md | Test-only state machines | not-a-claim | — |
| docs/HIDDEN-STATE-MACHINES.md | Fault and swap latches | kept-corrected | `CreateNewFault`, `VerifySwap` |
| docs/HIDDEN-STATE-MACHINES.md | Scripted transports | kept-corrected | `RecordingRemote`, `PathRemote`, `ScriptedExec` |
| docs/HIDDEN-STATE-MACHINES.md | `IdentityProbe` | kept-corrected | `sync::diff::tests::IdentityProbe`, `prepared` |
| docs/HIDDEN-STATE-MACHINES.md | Source audit traversal | kept-corrected | `atomic::guard::tests`, `FsVisitor`, `aliases`, `in_funnel`, `locals` |
| docs/HIDDEN-STATE-MACHINES.md | Child-process orchestration | kept-corrected | `fifo_regression`, `deep_tree_regression`, `ChildOutcome`, `RLIMIT_NOFILE` |
| deferred | src/sync/apply.rs | `prepare` doc | pending: "so a refusal leaves no residue" is false on the SSH path | `prepare`, `prepare_identity` |
| deferred | src/sync/apply.rs | `lock_with_in_root_lock` doc | pending: the parent dir is refused as a `ResidueBelow` conflict, not "reported as residue" | `delete_verdict`, `ConflictReason::ResidueBelow` |

## Slice 3 — `README.md`, `EXTRACTION.md`, `MIGRATION.md` (change `nlzqzopu`)

| File | Section | Disposition | Symbols re-derived from |
|---|---|---|---|
| README.md | Title + intro | kept | `src/lib.rs` |
| README.md | The constitution (rules 1–9) | kept | `error.rs`, `lib.rs` deny, `DestinationOwnership`, `Extraneous`, `FileLock`, `fold_component`, `casefold::case_fold`, `reserved::is_unaddressable_name`, `id::valid_name`, `canonicalize_tree_with`, `TreeEntry`, `MaterializationKind`, `verify*`, `GuardedRel`, `Sanction`, `begin_mutation`/`commit_mutation`, `ReplaceOutcome` |
| README.md | Refused, by rule | kept | `reserved::{is_unaddressable_*,is_reserved_*,is_residue_path,is_lock_record_name}`, `id::valid_name`, `sync::diff::strip_reserved`, `canonicalize_tree`, `roots_overlap`, `refuse_overlapping_copy` |
| README.md | Fidelity scope | kept | `manifest` fidelity docs, `TreeEntry`, `#[cfg(unix)]` nlink |
| README.md | This is not a backup or checkpoint format | kept | `check_relative_symlink_target_two_views`, `ContainmentViews`, `write_atomic_replace_fd` |
| README.md | Durability and atomicity | kept | `write_atomic_replace_fd`, `ReplaceOutcome`, `EntryPolicy::AppendTail`, `Remote::{write,fsync_tree,fsync_parent}` |
| README.md | What a snapshot costs | corrected (probe linear / work quadratic / fresh destination cubic) | `sync` cost docs, `install_file`, `canonicalize_tree`, `upload_deadline` |
| README.md | Destination provisioning | kept | `provision_layout`, `Layout` |
| README.md | Platform | corrected ("both are exercised" scoped to one userland per run) | `ssh_farside_portability`, `remote_lock`, `COMPONENT_CONFINED` |
| README.md | Assumptions the logic rests on | kept | manifest path-limit docs, `deep_tree_regression`, `dirchain`, `lock` fsync docs, `casefold` |
| README.md | Design conflicts (a) sibling vs in-root lock | corrected (residue is spelling-scoped) | `DestinationOwnership`, `destination_lock_path`, `is_residue_path`, `SyncReport::residue` |
| README.md | Design conflicts (b) remote ownership | kept | `lock_remote`, `Remote::lock_far_side`, `RECORD_HEADER`, `Error::LockContended`, `Prepared` |
| README.md | Design conflicts (c) fd-confined helpers | kept | `copy_dir_recursive_fd`, `copy_tree_verbatim`, `fsync_tree_recursive_fd`, `with_operation_lock_sidecar` |
| README.md | The contract (Enforced/Guaranteed/NOT promised) | kept (87-entry count re-measured) | `clippy.toml`, `every_production_libc_reference_is_pinned`, windows clippy |
| README.md | The contract — operative funnel definition | kept | `rg allow(clippy::disallowed_methods)` (61), `atomic` module allow |
| README.md | The gate | kept (counts re-measured) | `test_support::{slow_tests_enabled,proptest_cases}`, the four widened tests, `remote_lock` |
| README.md | Rules for changing this crate | kept | `tests/consumer_fit.rs`, `docs/CONSISTENCY.md` |
| EXTRACTION.md | Header + intro | not-a-claim (historical record) | — |
| EXTRACTION.md | Adaptations 1, 3–7 | kept | `error.rs`, `test_support`, `src` module list |
| EXTRACTION.md | Adaptations 2 (Visibility) | corrected (stale `ckpt` dependency sentence deleted) | `tests/consumer_fit.rs`, `relpath::from_validated` |
| EXTRACTION.md | Adaptations 8 (Windows) | corrected ("no Windows host has executed it" deleted as unverifiable) | `cargo check --target x86_64-pc-windows-msvc`, `#[cfg(unix)]` gates |
| EXTRACTION.md | Slices table + closing serde note | kept | `id_newtype!`, `atomic`, `OwnedRoot`, `FileLock`, `Remote`, `manifest`, `sync`, `__serde` |
| MIGRATION.md | Intro + Status | not-a-claim (declared record) | — |
| MIGRATION.md | What the migration is not | kept | nlink cfg split, manifest fidelity scope |
| MIGRATION.md | Order | not-a-claim | — |
| MIGRATION.md | Module map | corrected | `sync_parent_dir`, `ensure_private_dir_durable`, `write_atomic_replace`, `EndpointKey`, `from_validated`, `LocalTransport::new`, `canonicalize_tree_destination`, `copy_tree_verbatim`, `with_operation_lock_sidecar`, `SIDECAR_*` |
| MIGRATION.md | Ownership (a) and (b) | kept | `lock`, `lock_with_in_root_lock`, `is_residue_path`, `lock_remote`, `lock_far_side`, `RECORD_HEADER`, `LockContended`, `Unowned` |
| MIGRATION.md | The lock record is self-describing | kept | `FileLock::acquire`, `RECORD_HEADER`, `LockRecordNotRecognized`, `perl_hold_lock`, `far_side_lock_refusal`, `remote_lock` test |
| MIGRATION.md | The one real data migration | kept | `RECEIVER_ID_LEN`, `ReceiverId::parse`, `MarkerState`, `TransportKind::ReceiverIdMarker*` |
| MIGRATION.md | The `manifest`/`canonical` split | kept | `canonicalize_remote_entries_checked`, `require_parent_closed`, `validate_entry_path`, wire assembler |
| MIGRATION.md | How each step was validated / Riskiest step | not-a-claim | — |
| MIGRATION.md | Out of scope | corrected ("never executed" → the crate's stated unverified status) | windows gate command |
| MIGRATION.md | `ckpt` intro | not-a-claim | — |
| MIGRATION.md | `ckpt` adopts table | corrected (hard-link sentence scoped to the adopted primitives) | `atomic::{path_kind_fd,read_fd,write_atomic_cas_fd,...}`, `ensure_private_dir_durable_fd`, `RootedRelativePath` |
| MIGRATION.md | `ckpt` does not adopt / Validation | kept / not-a-claim | `EntryPolicy::AppendTail`, `id_newtype!`, `LockFileEx`/`flock` |
