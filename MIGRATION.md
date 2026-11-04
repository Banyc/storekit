# Migrating the consumers onto `storekit`

Two consumers have moved onto the crate, and both are done: `deploy` first, `ckpt`
second. Each migration's acceptance bar was the same: the consumer's own tests pass,
unedited.

The crate was extracted from `deploy`, so that migration was mostly deletion — swap
`deploy`'s copies of the substrate for the crate's, keep the domain. This file is the
record of what was done and what does not map, not a plan, in the same spirit as
`EXTRACTION.md`.

**Status.** Both migrations are complete. `deploy`'s substrate now comes from the crate;
the last slice was the `id` newtype machinery. The two questions the migration raised
are both resolved: `sync` does not replace `deploy`'s transfer protocol (because
`materialize` is domain), and the receiver marker was adopted.

## What the migration is not

- **Not a drop-in for `deploy`'s tree transfer.** `deploy`'s push is a domain
  protocol — generations, transactions, the `current` symlink, owner markers — and
  stays in `deploy` (`remote/helper/**`). The crate supplies the transport and, where
  the transfer is a plain tree mirror, the `sync` engine.
- **Not tolerant of what `deploy` tolerates.** The crate's fidelity scope is strict:
  no hard links (on a Unix source; the local Windows walk cannot see `nlink`), no
  absolute or escaping symlink targets, NFC-only names, no CR/LF/TAB. A `deploy`
  directory holding any of those is refused, loudly, rather than carried.

## `deploy`

**Order.** Four steps, all done. Freeze the crate first, since its API is the migration
target. Do the data migration before any push, because it is the only step that touches
existing on-disk state and it fails closed if skipped. Then swap the substrate in
dependency order: `digest`/`platform`/`trace`, `atomic`, `lock`, `root`/`owned_root`,
`relpath`, `transport` (with runner and ssh), `manifest`/`canonical`. Adopt `sync` only
where a transfer is a plain mirror of one tree into another; the manifest slice did not,
as the module map shows.

**Module map.**

| `deploy` | `storekit` | Adaptation |
|---|---|---|
| `store/atomic/{mod,unix,windows}.rs` | `atomic::*` | Every root-relative mutation takes `&RootedRelativePath`, parsed once at the boundary. The path-based helpers `sync_parent_dir`, `ensure_private_dir_durable` and `remove_dir_all_path` are gone from the public surface: the first two remain `pub(crate)` for the crate's own paths, `remove_dir_all_path` is `#[cfg(test)]` with no production caller, and `ensure_private_dir` exists on the Windows port only — a consumer uses the `_fd` twins. The Unix path-based `set_private` was removed; the replace's temp chmod now runs on the temp's create handle, and the Windows port keeps its documented no-op. `write_atomic_replace(&Path)` is public for the unconfined, absolute-path case. |
| `deploy/lock/{mod,unix,windows}.rs` | `lock::FileLock` | One to one. |
| `store/local/owned_root.rs` | `root::OwnedRoot` + `EndpointKey` | Domain cut: the crate owns `EndpointKey`/`LOCAL_ENDPOINT_MARKER`. |
| `identity::*` (`id_newtype!`, `valid_name`, `valid_hex_digest`) | `id::*` | One to one. The macro names `serde` through the crate, so the call site needs no `serde` dependency. |
| `remote/transport/rooted.rs` | `relpath::RootedRelativePath` | `from_validated` is crate-private; use the fallible `parse` at the boundary (a `LazyLock` for static spellings). |
| `remote/transport/mod.rs`, `runner/**`, `ssh/**` | `transport::*` | `Layout` is a constructor argument: `LocalTransport::new(env, base, layout)`. `Remote::exists` is a default method; `metadata_opt` is the typed probe. |
| `remote/canonical/mod.rs` (tree half) | `manifest::*` | Pick by role: `canonicalize_tree` for a source (strict), `canonicalize_tree_destination` for a destination (tolerant, `UnsupportedEntry { path, kind, reason }`). The `_checked` assemblers carry the completeness fact. Done: `deploy`'s half is deleted and the substrate names re-exported. `deploy` has no tolerant destination path, so it uses the strict source walk for destinations and compares digests. |
| `remote/canonical/materialize.rs` | — | **Stays in `deploy`.** It is the config-driven mapping/template materializer (`materialize_variant`, `TemplateVars`, `render_template`), not a tree-transfer protocol and not a plain mirror, so `sync` does not replace it. |
| *(new)* | `atomic::copy_dir_recursive_fd` + `fsync_tree_recursive_fd` | `deploy`'s shape: an arbitrary, possibly out-of-root `&Path` source into a root-confined `RootedRelativePath` staging destination, then canonicalize, digest and rename. |
| *(new)* | `atomic::copy_tree_verbatim(src, dst)` | The tolerant clone: copies a tree (reserved spellings, the `operation.lock` record and crate-temp shapes included) between two ordinary absolute paths. Intended for a live-base clone; no consumer calls it yet — `tests/consumer_fit.rs` records that measurement at the consumer revision it names — so the name is held for the planned migration rather than attested by a call site. The destination is not a store root and must never be handed to the recovery sweep. |
| *(new)* | `transport::{with_operation_lock_sidecar, SIDECAR_WAIT_TIMEOUT, SIDECAR_RETRY_INTERVAL}` | The operation-scoped sidecar critical section (blocking with a deadline, re-entrant). It takes the same record and inode the transport's own lock-record mutations take, so a caller's critical section and a transport lock-record mutation contend on one file. Replaces `deploy`'s own platform flock triple in `deploy/lock/{unix,windows}.rs` and its `remote::transport::{with_operation_lock_sidecar, wait_for_sidecar_flock, ensure_operation_lock_sidecar_durable}`. |
| *(new)* | `sync::sync(direction, .., ownership)` | One entry point; see the ownership note below. |
| `remote/helper/**` (push engine) | — | **Stays in `deploy`.** |
| `store/local/**`, `retention/**`, `kernel/**`, `ledger/**`, `verify/**`, `config/`, `init.rs` | — | **Stays in `deploy`.** |

**Ownership, and the two design conflicts it closed.** The plain
`DestinationOwnership::lock` takes only a sibling record
(`<parent>/.<name>.operation.lock`), which does not exclude `deploy`'s in-root
`state/operation.lock`. `DestinationOwnership::lock_with_in_root_lock(.., in_root_lock)`
holds both, in a fixed order (sibling first), with both acquisitions non-blocking so
the order cannot deadlock. A `deploy` destination that has its own in-root record
should take this form, and the root must already exist: the composed form refuses a
missing root without creating anything.

A remote destination can now be locked too. `DestinationOwnership::lock_remote(direction,
local_root, remote)` holds the destination's operation lock on the far side for the whole
run through a persistent far-side lock session: a long-lived `ssh` client whose remote
`perl` takes a non-blocking `flock` on the same sibling record the local case uses,
under the same record rule. It adopts an empty or header-prefixed entry and refuses any
other non-empty one untouched, so neither arm destroys the other's record. A `deploy`
push into a remote destination should prefer this over `DestinationOwnership::Unowned`:
it serialises concurrent cooperating runs, and the record is released on every exit
path. It is not a lease — a far-side lock cannot outlive its client — so `deploy`'s own
far-side serialisation need not be deleted for correctness while the run still depends
on it; `lock_remote` is the crate-level exclusion, and `deploy`'s protocol can be
retired once the run no longer needs it. A non-cooperating far-side writer is still
outside the crate's exclusion. The pre-existing refusal is unchanged: `sync` still
refuses a remote destination the caller does not own, and `Unowned` still names the
weaker path.

**The lock record describes itself** (change `vylurynk`, commit `34ab81ee`).
`lock::FileLock::acquire` used to truncate and rewrite whatever non-empty entry it found
at the record path. It now adopts an entry only when it is empty or already carries the
record header (`storekit lock record v1`), and refuses anything else with the typed
`PreflightKind::LockRecordNotRecognized`, leaving the entry byte-for-byte and
mode-for-mode untouched.

Both arms carry this rule. The far-side holder (change `rkqqztok`) reads the entry after
its flock and, if it is non-empty and does not begin with the header, refuses it with
the same typed kind and the same byte-for-byte and mode-for-mode guarantee, before any
`chmod`; otherwise it writes the header with its identity and tightens the mode to
`0600`. A record either arm wrote is adopted by the other, measured across a real
`sshd` by `remote_lock`'s
`a_crate_written_record_is_adopted_across_the_far_side_and_local_arms`.

This has a consequence for an existing store: a record written by an earlier revision
holds a bare op id and no header, so the first acquisition after the upgrade is refused
— fail-closed and typed, not silent. The refusal names the remedy and the condition
under which it is safe: move the entry aside (or remove it) only when no run is holding
it, because unlinking a record a live holder has flocked lets the next acquisition lock
a different inode. A record holds nothing but the holder's op id, so nothing is lost by
moving it. This is not a data migration: no tool rewrites anything, and a `deploy`
record created through its own `FileLock::acquire` call sites carries the header from
the first acquisition after the upgrade. It is listed here because it is the one change
in this migration that turns a previously accepted on-disk state into a refusal.

**The one real data migration: the receiver marker.** `deploy` stores
`recv-<uuid-v7>` at `<deploy_dir>/receiver-uuid`. The crate requires 40 lowercase hex
and fails closed on anything else, with no adoption path, so without this step every
existing deployment directory reads as malformed — and the failure is silent to a test
suite that builds fresh fixtures.

Done in `deploy` `dev`, commit `47e0e092`. The deriving read is
`sha256("deploy/receiver-id/v1\0" || legacy)[..20]`, hex, where `legacy` is the
canonical trimmed `recv-<uuid-v7>` string. It is deterministic from the canonical
string (file whitespace cannot change it) and domain-separated, so it cannot collide
with another hash of the same string; 160 bits is the same budget the crate's own
receiver ids use. The adopting read is `read_receiver_uuid_opt`; a read-only
`peek_receiver_uuid_opt` is used when a preflight is a `--dry-run`, because adopt-on-read
would otherwise mutate on a dry run. Fresh directories adopt on provision. The wire
form is `<40 lowercase hex>\n` at `<deploy_dir>/receiver-id`, beside the legacy file;
`receiver-uuid` is never opened for write and its bytes are unchanged, and the module
doc states the two conditions that would make it removable. An empty marker, `recv-`,
a truncated uuid, or a 39/41-hex string is refused by `deploy`'s read and by the crate,
with no adoption. A second read leaves the inode, mtime and bytes identical.
Re-provisioning every directory was not taken: it would discard a stored identity for
no benefit. The crate will not adopt a foreign format silently, and should not: that
would misidentify a deployment directory.

**The `manifest`/`canonical` split.** `deploy/remote/canonical/mod.rs`'s tree half is
the substrate's `manifest::*`; `deploy`'s copy is deleted and the substrate names
re-exported. The swap replaces `deploy`'s lexical, tree-root-relative symlink
containment with the substrate's physical walk from the link's containing directory: a
target that traverses a symlink component (intermediate or final) is now refused, and a
POSIX in-root target such as `dir/link -> ../other` is now accepted, lifting an
over-refusal `deploy` could never have stored. Names must already be NFC/UTF-8, targets
must be UTF-8, the wire assembler takes the walk's exit status
(`canonicalize_remote_entries_checked`), splits on LF alone, requires six fields and a
parent-closed manifest, and enforces `NAME_MAX`. The byte format is unchanged for a
tree both walks accept, so no stored `tree.json` or digest changes.

`deploy/remote/canonical/materialize.rs` stays. It is a mapping-set to staging tree
materializer (multiple sources placed into one content-addressed tree, `{{var}}`
rendering, per-mapping mode overrides, symlink and special sources refused), not a
plain mirror of one tree into another. The substrate's `sync` engine cannot express its
placement or rendering, so the answer for this file is: do not adopt `sync`.

**Validation.** The crate's gate on both platforms, plus `tests/consumer_fit.rs`, which
fails to compile if a consumer-required name is removed; `deploy`'s own test suite on
the touched modules; and, for the marker step, a fixture directory carrying a legacy
marker pushes successfully after adoption while a corrupt marker still fails closed. The
receiver-marker step was the riskiest, and it is done: it was the only step that fails
closed against existing production state, and neither side's tests covered it
beforehand; the adoption test now fails on the pre-change tree and passes after.

**Out of scope.** The Windows runtime: `deploy`'s Windows port is type-checked only, and
the crate's Windows test target is compiled but not executed here, so the crate states
its Windows runtime as unverified.

## `ckpt`

`ckpt` is a record book — sessions holding ctf flags holding verification hits — laid out
as `<root>/sessions/<a>/<b>/meta.json`, `.../flags/<a>/<b>/{meta.json,hits.log}` and
`<root>/by-flag/<a>/<b>`, with every id a 40-character lowercase hex object name. It has
no locking and a true `O_APPEND` log, and both are contracts rather than omissions. It
had no `storekit` dependency before this migration.

**What it adopts, and what each adoption cost.**

| `ckpt` site | `storekit` item | the difference the migration had to preserve |
|---|---|---|
| `Store::read_record`, `sparse::read_owned_file` | `atomic::path_kind_fd` + `atomic::read_fd` | A missing file or ancestor must stay an absence (`Ok(None)`), so the absence check happens before the read; a file, symlink or foreign entry in its place must stay a `Corrupt` naming that path. `ckpt`'s own `nlink > 1` check stays: none of the read/write primitives it adopts checks hard links. |
| `sparse::write_new_file` | `atomic::path_kind_fd` + `atomic::write_atomic_cas_fd` | The substrate treats an identical re-write as an idempotent success; `ckpt` must report a name that already holds a file, so its pre-check runs first. Files are now written mode `0o600` and fsynced (temp, file, parent), where `std::fs::write` used the umask with no fsync — a recorded tightening. |
| `sparse::ensure_directory`, `create_record_dir`, `require_directory` | `atomic::path_kind_fd` + `atomic::create_dir_fd` | Not `ensure_private_dir_durable_fd`: that chmods an existing directory to `0o700`, and `ckpt` leaves modes alone. |
| `sparse::entries` | `atomic::path_kind_fd` + `atomic::read_dir_fd` | The two-character/38-character shard split, the dotfile skip and the `Missing` policy stay in `ckpt`; the substrate knows nothing about the layout. |
| `store::take_back_mapping` | `atomic::remove_file_fd` | A missing entry is still a successful take-back. |
| the owned root | `atomic::RootDir`, `RootedRelativePath` | The root is canonicalized first: `RootDir::open` refuses a symlinked root while `ckpt` permits a root that is "a link the caller chose". |

Adopting the `_fd` primitives also retires a stated `ckpt` limitation: its old checks
were made "before a path is used rather than while it is open", so a symlink swapped in
mid-operation was outside them. The substrate acts through one descriptor.

**What it deliberately does not adopt.**

- **No lock.** A hit appended by one process must be visible to the next read of another,
  and a lock would also drop a non-dot record that `ckpt`'s own shard walk reports as a
  foreign entry.
- **No `AppendTail`.** It is a whole-file compare-and-replace with a stated lost-update
  hazard; `hits.log` is a true `O_APPEND` whose point is that concurrent verifications
  both land. The two concurrency tests are the guards.
- **No `sync`/`manifest`/`transport`/tree copy.** Nothing is cached, and the files are the
  index.
- **No `id_newtype!`.** Its construction stores the input verbatim, while `ckpt`
  canonicalizes to lowercase and accepts an uppercase spelling; `ckpt`'s rule is also
  strictly narrower (40 lowercase hex) than the substrate's. `ckpt` keeps its own `Id`.

**Validation.** `ckpt`'s `tests/record_book.rs` (108) and `tests/cli.rs` (17) are
byte-identical to the pre-migration revision and all pass; the in-crate unit tests are
unchanged. `tests/substrate_fit.rs` cites every adopted item by name as a function
value, so a signature change breaks the build and the coupling is compile-enforced
rather than implied. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test` and `cargo check --all-targets --target x86_64-pc-windows-msvc` are green
after every slice.
