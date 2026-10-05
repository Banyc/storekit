# Migrating `~/code/deploy` onto `storekit`

The crate was extracted from `deploy`, so the migration is mostly deletion: swap
`deploy`'s copies of the substrate for the crate's, keep everything that is
domain. This file is the ordered checklist and the record of what does NOT map.

**Status: the deploy-side migration is DONE, and this file is its record.**
`deploy`'s substrate now comes from the crate (the last slice was the `id`
newtype machinery). The checklist below is therefore a record of what was done
and why, not a plan — the same convention as `EXTRACTION.md`. The two questions
it raised are both RESOLVED and marked where they appear: whether `sync` should
replace `deploy`'s own transfer protocol (answered NO for `materialize`, which is
domain), and the receiver-marker adoption (done).

## What the migration is not

- **Not a drop-in for `deploy`'s tree transfer.** `deploy`'s push is a domain
  protocol — generations, transactions, the `current` symlink, owner markers —
  and stays in `deploy` (`remote/helper/**`). The crate supplies the transport
  and, where the transfer is a plain tree mirror, the `sync` engine.
- **Not tolerant of what `deploy` tolerates.** The crate's fidelity scope is
  strict and documented: no hard links, no absolute or escaping symlink
  targets, NFC-only names, no CR/LF/TAB. A `deploy` directory holding any of
  those is refused, loudly, rather than carried.

## Order

1. **Freeze the crate.** The API is the target of the migration; land
   `docs/API-CONSTRAINTS.md`'s work first.
2. **Data migration before any push** (see below) — the only step that touches
   existing on-disk state, and the one that fails closed if skipped. **DONE.**
3. **Swap the substrate in dependency order**: `digest`/`platform`/`trace` →
   `atomic` → `lock` → `root`/`owned_root` → `relpath` → `transport` (+ runner,
   ssh) → `manifest`/`canonical`. **DONE** (the final split is recorded below).
4. **Adopt `sync`** only where a transfer is a plain mirror of one tree into
   another. **DONE** for the manifest slice: it is NOT adopted by
   `deploy/remote/canonical/materialize.rs` (see below).

## Module map

| `deploy` | `storekit` | Adaptation |
|---|---|---|
| `store/atomic/{mod,unix,windows}.rs` | `atomic::*` | Every root-relative mutation takes `&RootedRelativePath`, parsed once at the boundary. The path-based helpers `set_private`, `sync_parent_dir`, `ensure_private_dir_durable` and `remove_dir_all_path` are gone from the PUBLIC surface: the first three remain `pub(crate)` for the crate's own paths, `remove_dir_all_path` is `#[cfg(test)]` with no production caller, and `ensure_private_dir` exists on the WINDOWS port only — a consumer uses the `_fd` twins. `write_atomic_replace(&Path)` is public for the unconfined, absolute-path case. |
| `deploy/lock/{mod,unix,windows}.rs` | `lock::FileLock` | 1:1. |
| `store/local/owned_root.rs` | `root::OwnedRoot` + `EndpointKey` | Domain cut: the crate owns `EndpointKey`/`LOCAL_ENDPOINT_MARKER`. |
| `identity::*` (`id_newtype!`, `valid_name`, `valid_hex_digest`) | `id::*` | 1:1. The macro names `serde` through the crate, so the call site needs no `serde` dependency. |
| `remote/transport/rooted.rs` | `relpath::RootedRelativePath` | `from_validated` is **crate-private**; use the fallible `parse` at the boundary (a `LazyLock` for the static spellings). |
| `remote/transport/mod.rs`, `runner/**`, `ssh/**` | `transport::*` | `Layout` is a constructor argument; `LocalTransport::new(env, base, layout)`. `Remote::exists` is a default method; `metadata_opt` is the typed probe. |
| `remote/canonical/mod.rs` (tree half) | `manifest::*` | Pick by ROLE: `canonicalize_tree` for a source (strict), `canonicalize_tree_destination` for a destination (tolerant, `UnsupportedEntry { path, kind, reason }`). The `_checked` assemblers carry the completeness fact. **DONE**: `deploy`'s half is deleted and the substrate names re-exported; `deploy` has no tolerant destination path, so it uses the strict source walk for destinations and compares digests. |
| `remote/canonical/materialize.rs` | — | **STAYS in `deploy`.** It is the config-driven mapping/template materializer (`materialize_variant`, `TemplateVars`, `render_template`), not a tree-transfer protocol and not a plain mirror, so `sync` does not replace it. |
| *(new)* | `atomic::copy_dir_recursive_fd` + `fsync_tree_recursive_fd` | `deploy`'s shape: an arbitrary, possibly out-of-root `&Path` source into a root-confined `RootedRelativePath` staging destination, then canonicalize + digest + rename. |
| *(new)* | `atomic::copy_tree_verbatim(src, dst)` | The TOLERANT clone: copies a tree (reserved spellings, the `operation.lock` record, and crate-temp shapes included) between two ordinary absolute paths. Intended for a live-base clone such as `retention`'s — searched, that checkpoint currently clones with its own test-only `std::fs::copy` and does not call this, so the name is held for the planned migration rather than attested by a call site. The destination is NOT a store root and must never be handed to the recovery sweep. |
| *(new)* | `transport::{with_operation_lock_sidecar, SIDECAR_WAIT_TIMEOUT, SIDECAR_RETRY_INTERVAL}` | The operation-scoped sidecar critical section (blocking-with-deadline, re-entrant, same record/inode as the far-side lock path). Replaces `deploy`'s own platform flock triple in `deploy/lock/{unix,windows}.rs` and its `remote::transport::{with_operation_lock_sidecar, wait_for_sidecar_flock, ensure_operation_lock_sidecar_durable}`. |
| *(new)* | `sync::sync(direction, .., ownership)` | One entry point. See the ownership note below. |
| `remote/helper/**` (push engine) | — | **Stays in `deploy`.** |
| `store/local/**`, `retention/**`, `kernel/**`, `ledger/**`, `verify/**`, `config/`, `init.rs` | — | **Stays in `deploy`.** |

## Ownership: the two design conflicts

- **(a) The sibling record vs `deploy`'s in-root lock — CLOSED.** The crate's
  plain `DestinationOwnership::lock` takes ONLY a sibling record
  (`<parent>/.<name>.operation.lock`), which does not exclude `deploy`'s in-root
  `state/operation.lock`. `DestinationOwnership::lock_with_in_root_lock(..,
  in_root_lock)` holds BOTH, in a fixed order (sibling first), with both
  acquisitions non-blocking so the order cannot deadlock. A `deploy` destination
  that has its own in-root record should take this form, and it must already
  exist (the composed form refuses a missing root without creating anything).
- **(b) A remote destination can now be locked — CLOSED.**
  `DestinationOwnership::lock_remote(direction, local_root, remote)` holds the
  destination's operation lock ON THE FAR SIDE for the whole run through a
  persistent far-side lock session (a long-lived `ssh` client whose remote
  `perl` takes a non-blocking `flock` on the SAME sibling record the local case
  uses, under the SAME record rule — it adopts an empty or header-prefixed entry
  and refuses any other non-empty one untouched, so neither arm destroys the
  other's record; see "The lock record is self-describing" below). A `deploy`
  push into a remote destination should prefer this over
  `DestinationOwnership::Unowned`: it serialises concurrent cooperating runs and
  the record is released on every exit path. It is NOT a lease — a far-side lock
  cannot outlive its client — so `deploy`'s own far-side serialisation need not
  be DELETED for correctness while the run still depends on it; `lock_remote` is
  the crate-level exclusion, and `deploy`'s own protocol can be retired once the
  run no longer needs it. A NON-COOPERATING far-side writer is still outside the
  crate's exclusion. The pre-existing refusal is unchanged: `sync` still refuses
  a remote destination the caller does not own, and `Unowned` still names the
  weaker path.

## The lock record is self-describing (change `vylurynk`, commit `34ab81ee`)

`lock::FileLock::acquire` used to truncate and rewrite whatever non-empty entry it
found at the record path. It now adopts an entry only when it is EMPTY or already
carries the record header (`storekit lock record v1`), and refuses anything else with
the typed `PreflightKind::LockRecordNotRecognized`, leaving the entry byte-for-byte
and mode-for-mode untouched.

Both arms carry this rule. The far-side holder (change `rkqqztok`) reads the entry
AFTER its flock and, if it is non-empty and does not begin with the header, refuses
it with the same typed `PreflightKind::LockRecordNotRecognized` and the same
byte-for-byte and mode-for-mode guarantee, before any `chmod`; otherwise it writes
the header with its identity and tightens the mode to `0600`. So a record either arm
wrote is adopted by the other — measured across a real `sshd` by
`remote_lock`'s `a_crate_written_record_is_adopted_across_the_far_side_and_local_arms`.

**Consequence for an existing store**: a record written by an earlier revision holds
a bare op id and no header, so the first acquisition after the upgrade is REFUSED —
fail-closed and typed, not silent. The refusal names the remedy, including the
condition under which the remedy is safe: move the entry aside (or remove it) only
when NO RUN IS HOLDING IT, because unlinking a record a live holder has flocked lets
the next acquisition lock a different inode. A record holds nothing but the holder's
op id, so nothing is lost by moving it.

This is not a data migration: no tool has to rewrite anything, and a `deploy` record
created through its own `FileLock::acquire` call sites carries the header from the
first acquisition after the upgrade. It is listed here because it is the one change
in this migration that turns a previously-accepted on-disk state into a refusal.

## The one real data migration: the receiver marker

`deploy` stores `recv-<uuid-v7>` at `<deploy_dir>/receiver-uuid`. The crate
requires **40 lowercase hex** and fails closed on anything else, with no adoption
path — so without this step every existing deployment directory reads as
malformed, and the failure is silent to a test suite that builds fresh fixtures.

### Done (`deploy` `dev`, commit `47e0e092`)

Adopt-on-read is implemented in `deploy` (`remote::transport::receiver_marker`):

- **Derivation.** `sha256("deploy/receiver-id/v1\0" || legacy)[..20]`, hex, where
  `legacy` is the canonical trimmed `recv-<uuid-v7>` string. Deterministic from
  the canonical string (file whitespace cannot change it), and domain-separated
  so it cannot collide with another hash of the same string. 160 bits, the same
  budget the crate's own receiver ids use.
- **Where.** The adopting read is `read_receiver_uuid_opt`; a read-only
  `peek_receiver_uuid_opt` is used when a preflight is a `--dry-run`, because
  adopt-on-read would otherwise MUTATE on a dry run. Fresh directories adopt on
  provision.
- **Wire form.** `<40 lowercase hex>\n` at `<deploy_dir>/receiver-id`, beside the
  legacy file. `receiver-uuid` is never opened for write and its bytes are
  unchanged; the module doc states the two conditions that would make it
  removable.
- **Fail closed.** An empty marker, `recv-`, a truncated uuid, or a 39/41-hex
  string is refused by `deploy`'s read AND by the crate, with no adoption.
- **Idempotent.** A second read leaves the inode, mtime and bytes identical.

The alternative (re-provisioning every directory) was not taken: it would discard
a stored identity for no benefit.

The crate will not adopt a foreign format silently, and should not: silently
adopting would misidentify a deployment directory.

## The `manifest`/`canonical` split (DONE)

`deploy/remote/canonical/mod.rs`'s tree half is the substrate's
`manifest::*`; `deploy`'s copy is deleted and the substrate names re-exported.
The swap replaces `deploy`'s LEXICAL, tree-ROOT-relative symlink containment
with the substrate's PHYSICAL walk from the link's containing directory: a
target that traverses a symlink component (intermediate or final) is now
refused, and a POSIX in-root target such as `dir/link -> ../other` is now
accepted (an over-refusal `deploy` could never have stored is lifted). Names
must already be NFC/UTF-8, targets must be UTF-8, the wire assembler takes the
walk's exit status (`canonicalize_remote_entries_checked`), splits on LF alone,
requires six fields and a parent-closed manifest, and enforces `NAME_MAX`. The
byte format is unchanged for a tree both walks accept, so no stored `tree.json`
or digest changes.

`deploy/remote/canonical/materialize.rs` STAYS. It is a mapping-set → staging
tree materializer (multiple sources placed into one content-addressed tree,
`{{var}}` rendering, per-mapping mode overrides, symlink/special sources
refused), not a plain mirror of one tree into another; the substrate's `sync`
engine cannot express its placement or rendering, so the answer to the step-4
question for this file is **do not adopt `sync`**.

## How each step was validated

- the crate's gate on BOTH platforms, plus `tests/consumer_fit.rs`, which fails
  to COMPILE if a consumer-required name is removed;
- `deploy`'s own test suite on the touched modules;
- for the marker step: a fixture directory carrying a legacy marker pushes
  successfully after adoption, and a corrupt marker still fails closed.

## Riskiest step

The receiver-marker migration — DONE (above). It was the only step that fails
closed against existing production state, and neither side's tests covered it
beforehand; the adoption test now fails on the pre-change tree and passes after.

## Out of scope

- the Windows runtime (`deploy`'s Windows port is type-checked only, and the
  crate's Windows test target compiles but has never executed).
