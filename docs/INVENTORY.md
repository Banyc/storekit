# Inventory: sync selection and tolerant staging

A working document, not a contract. Each item closes only when its carrier exists,
every acceptance test is present, and the gate is green; then this file is deleted.

| Item | Status |
|---|---|
| `sync-exclusions` | DONE — change `vustlsuzoxtl` (`16e46de1`) |
| `tolerant-staging-docs` | DONE — change `sqztzvzo` (`dfdafa51`) |

**Out of scope, recorded so they are not re-proposed.** Transactional apply (an
all-or-nothing outcome across entries) and incremental manifests (dirty tracking, reuse
of a previous manifest). Both are redesigns of the engine, not additions to it.

## sync-exclusions

**Goal.** `sync` takes a set of exclusions so named subtrees never enter either view:
not walked, not transferred, not deleted, not reported as extraneous. A `.git`,
`node_modules` or venv directory must be able to sit inside the synced root without
failing the run on an absolute symlink or a hard link, and without being removed by
`Extraneous::Delete`.

**Carrier.** A new validated type `Exclusions` in `src/sync`, built only by parsing each
spelling as a `RootedRelativePath`. `sync` takes it together with `Extraneous` in one
`SyncOptions` value; `diff` and the applier receive the same type, so there is one
representation and no second filter.

**Semantics.** Each of these is a test.

1. An exclusion is a rooted relative path naming a file or a subtree. The empty path
   (the root itself) is refused. Absolute paths, `..`, CR/LF/TAB and unaddressable
   names are refused by the existing parse.
2. A reserved spelling — the lock record, `.sync-aside.`, a crate temp shape, and their
   case/trailing-dot aliases — is refused. Excluding the crate's bookkeeping would
   bypass the residue and recovery rules.
3. Matching is at component boundaries: `node_modules` excludes `node_modules/x`, not
   `node_modules.bak`.
4. Matching folds components with the same fold the containment check uses
   (`fold_component`, shared via a `pub(crate)` helper), so a folding host cannot exclude a spelling in one view and miss
   it in another.
5. The source walk does not descend into an excluded directory, so an absolute symlink
   or hard link inside it is never seen and never refused.
6. The destination view is filtered before the diff, so an excluded destination entry is
   neither extraneous nor a deletion target, under every `Extraneous` policy.
7. The far-side listing is filtered before assembly, so an excluded entry never becomes
   part of the manifest and cannot trip the containment or kind refusals.
8. `SyncReport` names the exclusions that matched at least one entry on either side. An
   exclusion that matched nothing is not silently reported as applied, so a typo is
   visible.
9. Ownership, locking and every other report count are unchanged.

**Watch.**

- Filtering happens in three places (source walk, destination view, near-side assembly).
  It must be idempotent, and every view must see the same set.
- An in-root `Layout::lock` record can be named by an exclusion and must be refused
  (rule 2); the sibling record is outside the root and cannot be named.
- The run's overlap refusal is unaffected: exclusions do not make a nested source and
  destination disjoint.
- `copy_tree_verbatim` is not a substitute for an exclusion: on Unix it REFUSES a
  hard link (`CopyHardLink`), reproduces absolute symlink targets verbatim, and refuses
  special files. It cannot turn a source the strict walk refuses into one it accepts.

**Acceptance.**

- A tree containing `.git` with an absolute symlink and a hard link syncs with `.git`
  excluded; the same tree without the exclusion refuses. Both directions (push and pull)
  are covered.
- `Extraneous::Delete` leaves an excluded destination subtree byte-identical.
- `node_modules` does not exclude `node_modules.bak`; `node_modules/x` does exclude the
  child.
- A reserved exclusion and the empty exclusion are refused with the typed kind.
- The report names matched exclusions and omits unmatched ones.
- A pull from a remote source with an excluded subtree neither transfers nor refuses on
  it.

## tolerant-staging-docs

**Goal.** State the real boundary. The strict source walk refuses a hard link (Unix),
an absolute or escaping symlink, and a special file. The crate's own copy cannot
normalize any of them: `copy_tree_verbatim` refuses a hard link on Unix (the Windows
port duplicates it), reproduces an absolute symlink target verbatim, and refuses FIFOs,
sockets and devices. So a hard-link or symlink-heavy source must either be normalized
OUTSIDE the crate into a staging root before it is synced, or kept out of the synced
root by `sync-exclusions`.

**Carrier.** `src/sync/mod.rs` module docs, the `copy_tree_verbatim` docs in
`src/atomic`, and the README's "What is carried" and non-goals (which say "normalize
first" without naming the tool, the limit, or the alternative).

**Content.**

- Name what the strict walk refuses: a hard link on Unix, an absolute or escaping
  symlink, a FIFO/socket/device.
- `copy_tree_verbatim` is NOT a normalizer: it refuses a hard link on Unix with
  `StoreKind::CopyHardLink`, reproduces an absolute or escaping symlink target
  verbatim, and refuses special files. A tree it copies is still refused by the strict
  walk when it contains either link form.
- The external quiet copy: materialise symlink targets and split hard links before the
  sync, for example `rsync -a --copy-links` WITHOUT `-H` (GNU tar's
  `--dereference --hard-dereference` is the equivalent). Either changes the tree (a link
  becomes a copy), so the choice is the caller's.
- `sync-exclusions` is the in-crate alternative for known-heavy subtrees (`.git`,
  `node_modules`, a venv): they never enter the walk at all.
- The staged tree is an ordinary directory, not a store root; never hand it to the
  recovery sweep.

**Watch.** Do not claim that any crate primitive normalizes symlinks or hard links, and
  do not present `copy_tree_verbatim` as the staging step. A reader following the docs
  with a venv must reach the external normalization step, not a second unexplained
  refusal.

**Acceptance.**

- A test pins `copy_tree_verbatim` refusing a hard-linked source with
  `StoreKind::CopyHardLink` on Unix (the Windows claim is the documented duplication).
- A test pins an absolute symlink surviving `copy_tree_verbatim` and still being
  refused by `sync`, which is the documented limit.
- The docs name the external normalization step and cite both tests by name.
