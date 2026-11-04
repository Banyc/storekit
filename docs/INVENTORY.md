# Inventory: sync selection and tolerant staging

A working document, not a contract. Each item closes only when its carrier exists,
every acceptance test is present, and the gate is green; then this file is deleted.

| Item | Status |
|---|---|
| `sync-exclusions` | OPEN |
| `tolerant-staging-docs` | OPEN |

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
- `copy_tree_verbatim` is not a substitute for an exclusion: it turns hard links into
  independent files but reproduces absolute symlink targets verbatim.

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

**Goal.** State the recipe for a source the strict walk refuses: stage a verbatim copy
and sync the staged tree. The docs must say exactly what that copy fixes and what it
does not.

**Carrier.** `src/sync/mod.rs` module docs, the `copy_tree_verbatim` docs in
`src/atomic`, and the README's "What is carried" and non-goals (which say "normalize
first" without naming the tool or its limit).

**Content.**

- Hard links: `copy_tree_verbatim` copies each name's content separately, so the staged
  tree has no hard links and the strict walk accepts it.
- Absolute and escaping symlink targets: the copy reproduces them verbatim, so the
  staged tree is still refused. Rewrite the targets, or replace the link with its
  target's content, before syncing.
- Non-regular entries: the copy refuses FIFOs, sockets and devices, so remove them
  first.
- The staged destination is an ordinary directory, not a store root; never hand it to
  the recovery sweep.

**Watch.** The docs must not claim the copy normalizes symlinks. A reader following them
with a venv must hit the rewrite step, not a second unexplained failure.

**Acceptance.**

- An integration test: a source with a hard link refuses `sync`; after
  `copy_tree_verbatim` into a staging root, the same sync succeeds.
- A second test: a source with an absolute symlink is still refused after the verbatim
  copy, which is the documented limit.
- Both tests are cited by name from the docs they back.
