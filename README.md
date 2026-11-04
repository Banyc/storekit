# storekit

storekit is the storage layer under a few small deployment and checkpoint tools.
It gives them a folder they cannot accidentally escape through a symlink, file
replacement that is atomic and (on Linux) durable, advisory locks, checked names
and paths, a manifest that describes exactly what a tree contains, and a one-shot
copy of that tree to another machine over ssh.

It is not an application's data model. Callers are usually other tools, often
written by agents, and they will not read this implementation before calling it.
So the crate enforces its own rules and never relies on the caller to remember
them.

## Goals

- **One shared implementation** of the parts several projects kept rewriting: a
  rooted file store with atomic file replacement, a root that a symlink cannot
  escape on Unix, advisory locks, checked ids and paths, a manifest describing a
  tree, and a one-shot transport that copies that tree to another machine.
- **Safe when misused.** A wrong call fails to compile or returns an error; it
  does not quietly do the wrong thing.
- **Small and exact.** The manifest records names, entry kinds, modes, file
  contents and symlink targets, byte for byte. Nothing is normalised or guessed.
- **Callable without reading the source.** The rules live in the types, the
  errors and the tests, not in comments.
- **Linux and macOS.** Windows is compiled and type-checked, but unverified: the
  Windows replace is not atomic, and its path handling follows symlinks.

## Non-goals

- **Backups and checkpoints.** No history, versions, deduplication or retention.
  A restore does not carry owners, timestamps, ACLs, extended attributes or
  sparseness, and nothing reports that loss.
- **Live sync.** No file watching, no continuous replication, no merging. A sync
  is a single run, and it refuses overlapping source and destination roots.
- **Extra metadata.** Nothing beyond name, kind, mode, content and symlink
  target.
- **Unusual entries.** No hard links (on a Unix source), devices, sockets or
  FIFOs.
- **Remote execution.** The SSH transport moves a store; it does not run
  arbitrary commands.
- **An application's model.** Deployment slots, retention policy and the rest of
  a consumer's domain stay in that consumer.

## Design rules

1. **Enforce, don't trust.** If the API can make misuse impossible, do that.
   Otherwise check the input and refuse it, or detect the violation and fail. A
   rule that lives only in prose is not a rule.
2. **The safe path is the default.** A weaker guarantee is reachable only from an
   entry point whose name says so. A destructive choice is an enum, never a
   boolean. An operation that can take its own lock, does.
3. **Names and targets are stored exactly.** A manifest value is byte-exact:
   names are UTF-8 in NFC, with no CR, LF or TAB. A symlink target is stored as
   literal bytes; it may contain `/` and `..`, and a relative target is resolved
   against the directory containing the link. We refuse a target that is
   absolute, that walks above the root, or whose walk reaches a symlink (final or
   intermediate): the kernel follows that symlink, and we will not guess where it
   ends. A component counts as a symlink if its exact entry is one, or, when no
   exact entry exists, if it folds onto a symlink entry under our name fold: NFD,
   then the full Unicode case fold (so `ß` becomes `ss` and `ﬁ` becomes `fi`),
   then NFC, then strip trailing dots and spaces. This is deliberately not
   `str::to_lowercase`, which is a different mapping. A case-insensitive host
   folds the full way: on APFS, `STRASSE`, `strasse` and `straße` are one entry,
   and a lowercase-only fold would miss that. Nothing is normalised or
   substituted.
4. **A view is exact, or the check does not run.** No lossy decode, no trimming,
   no defaulted field, on any path that decides something. A listing carries the
   live kind of each entry beside its name.
5. **Decisions do not cross views.** A permission, an outcome, a kind and a listing
   are valid only for the moment they were observed. Anything that selects a
   mutation is read from the live tree.
6. **Delete only with permission for that path**, re-established against the
   live tree. A recursive removal never deletes a name it did not enumerate.
   Count a mutation before attempting it, and name the path if it fails.
7. **A run owns its destination.** Take the operation lock wherever it can be
   taken, hold it for the whole run, release it on every exit path. A write made
   outside that lock is detected over the paths the run reads, by re-checking the
   destination, and fails the run. The detection is not total: an ABA or a write
   after the last check is invisible.
8. **Fail closed.** Refuse rather than transform; error rather than guess; a
   check that cannot run is a failure. Never document a guarantee you do not
   implement.
9. **Frame or refuse at every boundary that reinterprets a value.** Shell
   operands are one quoted word each, parents are computed rather than derived by
   a shell, and `--` goes before a value that may start with `-`. Wire records
   are delimited by a byte a name cannot contain. The far-side listing frame
   carries the name last, NUL-delimited. The manifest tree record is
   tab-delimited with the path first; that is safe only because names containing
   NUL, LF, CR or TAB are refused everywhere, so no reader depends on field
   position.

## What is refused

The crate refuses, by rule:

- names that are not UTF-8 or not NFC, and names or targets containing CR, LF or
  TAB;
- absolute or escaping symlink targets, and a relative target whose walk passes
  through a symlink component (the kernel follows it, so a lexical collapse past
  that point is not its real resolution);
- hard links on a Unix source; the local Windows walk cannot see `nlink`, so it
  records both names as ordinary files;
- devices, sockets and FIFOs;
- collisions with names the crate reserves for itself;
- overlapping roots.

Overlap is decided per operation, not once. A `sync` requires its source and
destination to be disjoint. A strict ancestor/descendant nesting is refused when
the remote side is local; a far-side root, on either side of the run, cannot be
resolved from here, so the caller owns that disjointness. Equal roots are allowed
and the run is an idempotent no-op: the manifests are identical, so the diff is
empty. `root::OwnedRoot::parse` is stricter: it refuses two roots on one endpoint
when they are equal or one contains the other. The root-confined tree copy
(`atomic::copy_dir_recursive_fd`) refuses an overlapping source and destination,
equal included, by directory identity.

A destination root or entry reached through a symlink component is refused on
Unix: every parent component of an in-root mutation, and the final component of a
root open or an open/create-new. The Windows port is path-based, so a symlink in
a path component is followed there and `atomic::COMPONENT_CONFINED` is `false`.
There are two stated exceptions. The atomic replace cannot follow the final
entry, because it installs with `renameat`. A source spelling may resolve its own
intermediate symlink components, which the fd tree copy reads once. Where a
destination's lock cannot be taken, a component swapped between the check and the
operation is a stated residual, not a guarantee.

A destination member that is an absolute or escaping symlink, or a hard link, is
not refused. The destination manifest records it, the diff reports it as
extraneous, and `Extraneous::Delete` removes it. It is never transferred: when a
source entry at that path differs, the run refuses before any mutation; a `Same`
entry needs no refusal, because nothing is mutated, so it is skipped and
reported.

`Extraneous` is all-or-nothing, with no per-path policy. `Delete` removes every
destination-only entry the diff classified as extraneous, so it cannot prune
exactly one path (say snapshot 002 while 001 and 003 are kept). `Keep` removes
none. For partial retention, keep everything and remove the unwanted paths out of
band through the destination's own removal primitives, or make every path you
want kept part of the source.

A caller can ask whether a name is usable before it fails. The authority is
`storekit::is_unaddressable_name` for one segment and
`storekit::is_unaddressable_path` for a canonical manifest path; these are the
predicates `storekit::id::valid_name` itself consults. The implication runs one
way: `is_unaddressable_name(s)` being false does not mean `valid_name(s)`,
because the identifier rule also constrains the character set
(`[A-Za-z0-9-_.]`, so `Ünïcode` is addressable but not a valid id), the first
byte (no leading `-`), the length (`NAME_MAX`) and the `.`/`..` spellings. Read
them as "spellings the crate cannot address", not as a `valid_name` oracle.

They report the spellings the crate reserves for its own bookkeeping: the
`.sync-aside.` claim-aside prefix, the `.<name>.operation.lock` record, the
application lock record `operation.lock`, case aliases of any of those, crate temp
shapes, and the Win32 trailing-dot/space aliases of a lock record or a crate temp.
All of them are refused on every platform, so an id does not mean different
things on different hosts.

`is_reserved_name` and `is_reserved_path` are narrower: they are the byte-exact
reserved match, and they deliberately leave the application lock record and the
case/trailing-dot aliases alone. They are not on their own the answer to "may I
use this name". `is_reserved_name` is the primitive the broad authorities are
built on (`is_unaddressable_name`, `is_reserved_case_alias`); `is_reserved_path`
is public for a caller that needs exactly the byte-exact question, and this
crate's production code does not call it (a unit test does).

The sync strips with the broad authority: `is_unaddressable_path` for the source
view and `is_residue_path` for the destination view, at `sync::diff`'s
`strip_reserved` call sites. An unaddressable spelling is refused as an
identifier and is never transferred. A residue spelling — unaddressable, but not
a crate temp — is also never destroyed by `Extraneous::Delete`: it survives every
extraneous policy and is reported in `SyncReport::residue`. A crate temp shape is
the exception; it is the documented target of the recovery sweep, so
`Extraneous::Delete` does remove it and reports it in `Extraneous`.
"Unaddressable" therefore does not mean "undeletable".

## What is carried

Carried: name, kind, mode (including setuid, setgid and sticky), content, and
symlink target.

Not carried: ownership, extended attributes, POSIX ACLs, timestamps, file flags,
sparseness.

Hard links are refused rather than dropped, on a Unix source. The local Windows
walk cannot see `nlink`, so it records both names as ordinary files.

The loss is invisible to the differ: a dropped xattr or ACL leaves
`local_manifest == remote_manifest` true and the sync reporting no difference.
Only tooling on the destination that understands xattrs and ACLs can reveal it.
The authoritative statement is the `manifest` module documentation; the sync
entry points restate the scope.

Because the manifest carries no timestamps, "the newest N snapshots" must come
from the snapshot ID's lexical order, or from a timestamp record the caller keeps
itself. The crate cannot rank two snapshots by time, and two snapshots that
differ only in mtime compare `Same`.

Two consequences follow, and they are why this is not a backup format:

- A source containing a hard link (on a Unix source), an absolute symlink, or an
  escaping symlink cannot be snapshotted at all: the strict source manifest
  refuses the run. Normalize such a tree first, by copying the hard-linked
  content and making the symlink relative. A relative target that contains `/`
  or `..` but stays inside the root without walking through a symlink component
  (`dir/link -> ../other`) is lawful and is not one of these refusals. A target
  whose walk reaches a symlink component is refused even when it lands inside,
  because the kernel may follow that component out. Containment is decided
  against the result: a source link whose target walks through a component that
  only the destination supplies as a symlink (and that the source does not
  replace, kept under the default `Extraneous::Keep`) also fails the run, rather
  than installing an escaping link or silently removing the destination entry.
- A restore drops metadata with the differ blind: `diff(snapshot, live)` is empty
  while mtime, xattrs and sparseness differ. Ownership, `security.capability`,
  ACLs, timestamps, file flags and sparseness are not carried, and nothing
  reports their loss. Apply them out of band and verify with tooling that can see
  them.

## Durability and atomicity

A file a sync writes is published atomically and durably for every Unix
destination kind. "Durably" here means the Linux power-loss barrier; see the
first assumption below. A local destination — a pull, or a push whose transport
is `LocalTransport` — uses the crate's durable atomic replace: a unique temp file,
`fsync`, `rename`, then `fsync` of the parent directory. A remote destination uses
the same shape on the far side: temp, payload on stdin, mode, perl `fsync(2)`,
perl `rename(2)`, parent-directory `fsync(2)`. On every Unix destination, a write
that fails before the rename leaves the previous content in place and removes the
temp, so a failed push cannot destroy the snapshot it was replacing. The Windows
replace's pre-rename removal is the exception named below.

The Windows local port's replace is the one non-atomic case: there is no
directory fsync, and the target is removed before the rename. It is unverified.
The exact commit points are in the `manifest` module's "Durability and atomicity
of a written entry".

`EntryPolicy::AppendTail` costs O(total size) per append, because there is no
remote append primitive and the append is a compare-and-replace of the whole
file. One run that appends a few bytes to a log rewrites the whole log and
re-reads it several times: the destination manifest, the prefix test and the
compare-and-replace each read the destination whole; the source manifest, the
prefix test and the end-of-run source re-check each read the source whole; and
the two post-transfer verification passes each read the result whole, plus the
whole result written through the atomic temp and the lock record. Reads are
therefore a small multiple of the log, not of the appended bytes. Budget the
verification, not just the append rule. Batch small appends, or keep the log
outside the synced tree and ship it whole.

`AppendTail` is not a concurrency-safe append. It is a whole-file
compare-and-replace, and on a Windows destination that replace is additionally
non-atomic (the target is removed before the rename). Concurrent appends are not
preserved: two runs can read the same prefix, and the later publish discards the
other's line — a lost update on a remote destination, and a `renameat`-wide
window locally. A consumer that relies on one `O_APPEND` write of a complete line
landing under concurrent appenders must keep those writes outside the synced
tree; see the `EntryPolicy::AppendTail` warning for what to do instead.

To make a freshly pushed subtree durable, call `fsync_tree(child)` and
`fsync_parent(child)` on the transport rooted at the child's parent. A
`RootedRelativePath` is empty only for the crate-internal spelling that names an
owned root itself, so a transport rooted at the child cannot name its own root to
fsync the parent directory entry.

## What a snapshot costs

Three shape facts to budget for. No figures are quoted here: every number this
section used to carry was measured outside this repository, and a number a reader
cannot re-run from here is a claim that drifts.

Memory is O(largest entry), not O(changed bytes). `Remote::write` takes
`data: &[u8]`, and the read side materializes the whole entry, so peak memory
tracks the largest single file, for snapshot and restore alike. The SSH path
derives a size-aware deadline from the payload; the local path has neither a
deadline nor streaming. Keep the largest entry under the process's budget, or
move large blobs outside the synced tree and ship them with a tool that streams.
A streaming transport API would remove the bound; that is a deliberate future
direction and a transport-layer redesign this crate does not undertake.

A snapshot scans and hashes the whole tree: O(bytes scanned), not O(bytes
changed). Content addressing deduplicates the store, but there is no dirty
tracking and no reuse of a previously computed manifest. `canonicalize_tree`
reads every file to hash it, and `install_file` reads the whole source again to
write it. Reusing a previous manifest, or skipping the second read, would change
that; neither is built here.

A deep tree is worse than linear, and the shape depends on fresh versus
incremental. A local path-based destination re-verifies a path's ancestry before
each mutation: the probe count is linear in depth, but each probed prefix is
resolved component-wise, so a depth-D path costs O(D²) path operations and a
chain with one changed leaf has super-linear wall time in D — budget for worse
than quadratic. A fresh D-entry destination is worse still: each install pays its
own O(D²) ancestry probe and listing, so the run is cubic in depth, and a
checkpoint tool that recreates its destination per snapshot hits exactly that
shape. `canonicalize_tree` itself is not the cost; the engine's per-path
verification is. A descriptor-relative walk would lift this and is not built.

## Assumptions the logic rests on

Restrictions the crate does not enforce, because it cannot. Each buys a
simplification; removing one means adding back the logic it removes.

Linux and macOS are supported and exercised. The far side of a remote transfer
may be GNU or BSD userland: the far-side suites build the command string and run
it through the host's own userland, so a run exercises BSD on macOS and GNU on
Linux, and no single run exercises both
(`tests/ssh_farside_portability.rs` states the asymmetry). The Windows
implementation type-checks but is not exercised, and is described as unverified.
It is not only untested: some rules are Unix-only by construction, and each such
claim says so where it is made (`atomic::COMPONENT_CONFINED`, the local walk's
hard-link check).

- **Durability is a Linux claim.** `fsync` is the power-loss barrier on Linux.
  macOS's `fsync` does not flush the device write cache (this crate does not call
  `F_FULLFSYNC`), and the Windows port has no directory fsync, so on those
  targets durability means surviving a process crash, not a power loss.
  Atomicity holds on every Unix destination, macOS included. The Windows replace
  is the one non-atomic case, because Windows `rename` does not overwrite an
  existing target and the target is therefore removed first, so a reader can
  observe a transient absence. A caller needing power-loss recovery there owns
  that step. Wherever this crate calls a write durable or names power loss, this
  is the claim it means.
- **The name-mutation devices target Unix.** On Windows the crate's I/O is
  `windows_sys`, so a `libc` entry is inert only on a target that does not export
  the symbol. The gate's `cargo clippy --all-targets --target
  x86_64-pc-windows-msvc` exits 0 while reporting 87 `libc`/`std::os::unix`
  entries as "does not refer to a reachable function"; the entries that do
  resolve there are live denies too, and the crate simply calls none of them on
  that target. The Windows port is a compile target whose runtime this contract
  does not cover.
- **Every claim has one authoritative home.** A fact is stated where it is
  enforced and pointed at elsewhere. Where a second statement of a measured
  number is genuinely useful, the two must agree, and a drift between them is a
  defect in whichever is stale.
- **The destination changes only through this run.** One read of the destination
  is authoritative for the whole run. The crate takes the destination's
  operation lock wherever it can, making this true for cooperating writers. A
  write made outside that lock is still detected over the paths the run reads
  (rule 7 states the limit: it is not total) and fails the run.
- **The source does not change during the run.** One read of the source
  describes it for the whole run. Nothing in the crate can prevent a source
  write, so the caller owes this.
- **The destination filesystem's case and normalization behaviour is constant.**
  It is probed once and cached for the run, not re-probed per decision.
- **The destination filesystem renames atomically within a directory.** An entry
  can be moved aside and published by rename, so a replacement never passes
  through a state with neither the old nor the new content.
- **The far side provides a POSIX shell and `perl`.** One script per operation,
  with no binary agent to deploy or version on the far side.
- **The far side's userland is GNU or BSD.** Portability is a tested property of
  the scripts instead of a runtime negotiation.
- **The process's descriptor limit exceeds the tree's depth.** A walk holds one
  descriptor per directory level instead of pooling or segmenting.
- **A path-based operation addresses no more than the platform's path limit.**
  Path-shaped operations need no segmented traversal. The limit does apply to the
  manifest walk: `canonicalize_tree` (`crate::manifest`) uses `WalkDir` plus
  `symlink_metadata`/`read` on accumulated paths, so a tree deeper than the
  platform's path limit is refused with `ENAMETOOLONG` at the first path that
  overflows. With 1-byte components the path grows 2 bytes per level, so the
  bound is `floor((PATH_MAX - 1 - base_len)/2)`, where `base_len` is the length
  in bytes of the base path as it resolves on the filesystem; the kernel accepts
  at most `PATH_MAX - 1` bytes for a path, on both platforms. A
  descriptor-relative manifest walk would lift this and is not built. Removal
  walks descriptor-relative one level per level and is not limited by the path
  limit, so removal supports deeper trees than the walk that describes them;
  that advantage is itself bounded by the descriptor limit above, which is the
  real ceiling on removal depth.
- **Metadata beyond name, kind, mode, content and symlink target is outside the
  model.** A small manifest, and no extended-attribute, ACL, ownership, timestamp
  or sparseness machinery.

## What is guaranteed

This crate is the storage substrate its consumers build on: atomic replace, root
confinement, locks, checked ids and paths, manifest and wire format, transport,
sync. It takes responsibility for exactly that and no more.

**Enforced.** Three things, all checked by the compiler or by a test.

1. The mutation symbols the crate funnels — the small set of wrappers every file
   mutation goes through, including the `libc` symbols they call — are denied in
   every module that does not carry the allow, on each target that exports the
   symbol. No spelling, alias or module route reaches them. `clippy.toml` lists the symbols the funnel uses plus a
   reviewed set around them; the list is deliberately wider than the funnel, because
   naming a symbol the funnel never calls is how a route it could acquire later
   is refused in advance. The device is a Unix-target device: on Windows `libc`
   is not the crate's I/O library, so an entry is inert only where the target
   does not export the symbol; the entries that do resolve there are live denies
   as well.
2. Every production `libc` reference — inside a funnel module
   (`atomic/{mod,guard,unix,windows}.rs`) or anywhere else — is named in the
   audit's pin, by file, symbol and count. The map of references not on the pin
   is asserted empty, so an unreviewed `libc` reference is a failing test
   wherever it stands, in the spellings the scan reads. A call reached through an
   aliased `use` is neither named nor counted once that `use` itself has been
   reviewed; the residuals state the bound, as clause 3 does for its own. The pin
   records a review, not a proof: a pinned reference is one somebody looked at,
   and whether the mutation it performs is refused is the deny's business only if
   the deny names the symbol.
3. The funnel's own call counts are pinned per file and per symbol, so a changed
   or added call inside the funnel forces review. This covers every call the
   pin's derivation can resolve: a direct call, an inherent or builder method on
   a path-resolvable receiver, or a call held in an enclosing `let`. A call whose
   receiver arrives as a function parameter, a return, a struct field or a
   function pointer moves no pinned count, and inside a funnel module the deny is
   allowed, so nothing else refuses it either. A canonical path spelled inside a
   `macro_rules!` body is counted (a test pins that); an aliased or non-canonical
   spelling there is not. `src/atomic/guard.rs`'s audit names the shape it can
   resolve at the derivation, and the review of a funnel change has to cover the
   rest. A pin counts calls, not arguments: an argument change that adds no newly
   counted symbol is a review responsibility. On Unix an argument that adds a
   `libc::…` constant is noticed, because it moves the libc pin; on Windows the
   funnel's `custom_flags` sites spell `windows_sys` constants, which that pin
   does not count.

**Guaranteed as an API.** Root confinement on Unix: a relative symlink target
cannot leave the root, and neither can a mutation named by `(&RootDir,
&RootedRelativePath)`. The Windows port is path-based, so a symlink in a path
component is followed there (`atomic::COMPONENT_CONFINED` is `false`). Also
guaranteed: the atomic replace's commit points and its reported durability; lock
mutual exclusion, with a record never destroyed by adoption; validated ids and
paths; manifest fidelity and wire injectivity; the ownership binding; and the
typed error kinds.

**Provisioning.** A `PUSH` provisions its destination before reading the
destination manifest: the destination root and the caller's
`Layout::bootstrap_dirs` are created, so a fresh remote destination works without
the caller pre-creating it and `Layout::empty()` is enough. A `PULL` into a local
destination creates that root lazily, on the first mutation.

**Not promised.** Completeness of the symbol set: a mutating symbol nobody
listed, a raw `syscall(SYS_…)`, an `extern "C"` declaration, a `windows_sys`
creator, a proc-macro-generated call, third-party code. Keeping the funnel
complete is a review responsibility over the symbols the crate actually names.
`docs/CONSISTENCY.md` states the residuals the contract rests on, and
`docs/API-CONSTRAINTS.md` names the ones its constraints leave at each item; a
seam with a reach of its own states it where it lives.

The operative definition of the funnel is the set of
`#[allow(clippy::disallowed_methods)]` attributes in the source: module-level in
the funnel modules, or item-level on an individual reviewed function. A
module-level attribute is inherited by that module's children, so the effective
set is the annotated modules and their descendants. It is not enumerated here; it
is whatever `rg -n 'allow\(clippy::disallowed_methods\)' src` reports, plus the
modules those attributes cover.

## The gate

Read exit codes directly, never through a pipe. On either platform:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings   # NOT optional
cargo test
STOREKIT_FULL_TESTS=1 cargo test       # NOT optional; see below
cargo check --all-targets --target x86_64-pc-windows-msvc
cargo test --doc
RUSTDOCFLAGS="-D warnings -A rustdoc::private_intra_doc_links" cargo doc --no-deps
```

`cargo doc` is in the gate because a doc link that no longer resolves is a
citation that has drifted. The `-A` allows module docs to link private items,
which this crate does deliberately for its maintainers, while an unresolved link
— a renamed item, a misspelled path — stays a hard error.

`STOREKIT_FULL_TESTS=1` does not add test names; it widens the suite in place,
and two wrappers read the variable.

- `slow_tests_enabled()` is consulted by four tests. Three return early with a
  printed skip reason: the atomic replace's sweep over every pre-rename stage,
  the concurrent-controller ssh case, and the every-boundary swap case. One
  widens its own exhaustive sweep instead
  (`valid_name_agrees_with_the_restated_rule`). The default run covers sampled
  shapes; the widened run covers all of them. The search
  `rg -n slow_tests_enabled src` returns six hits: those four, the definition,
  and one `use`.
- `proptest_cases(..)` widens every property test that sizes its case count
  through it, so the widened run also covers more generated cases, not more test
  names.

Two clippy commands, and the second is not optional: `--all-targets` compiles the
host only, and `cargo check --target …` runs no lints, so a `#[cfg(windows)]`-only
module is invisible to both. `remote_lock`, which stands up a real `sshd` and is
`#![cfg(unix)]` rather than Linux-only, is part of `cargo test` wherever one is
available. The `ssh_farside_*` suites drive the far-side protocol through a `PATH`
shim, not a real `ssh`.

The Windows check compiles the whole target, tests included. A signature change
to a Windows code path, production or test, therefore fails this gate instead of
staying invisible until someone builds on Windows. The tests cannot execute on
this host, so the reproductions that need a Unix filesystem primitive
(`O_NOFOLLOW`, `flock`, `pipe`/`poll`, `symlink`, mode bits, `mkfifo`, raw fds)
are `#[cfg(unix)]`; the rest of the suite is compiled for Windows.

## Rules for changing this crate

- **One logical change per commit**, gated before it lands and on both platforms.
- **No assertion is weakened or deleted.** A test that encoded a looser rule may
  be flipped, but only explicitly, with the reason recorded at the test. A
  deletion is justified by the assertions that cover it — a per-test
  reconciliation — never by a preserved test count, and never by whether this
  crate's own production happens to use the name.
- **A claim is a measurement or it is a label.** Cite a command, a test or a
  table, or say in the sentence that it is unmeasured; cite items by name, never
  by line number. `docs/CONSISTENCY.md` carries the rules this crate earned; they
  are binding.
- **Port the coverage, do not widen the surface.** When a public name moves, the
  consumer must still compile: fix the consumer in the same round.
- **A weak path is reachable only through a name that states it.**
- **Evidence is about a revision.** This crate's own docs, its consumers' trees
  and any cited specification drift; re-read before relying on one.
- **A public item earns its place by a caller's need** — one that exists today,
  or one that is stated and planned; never by this crate's own use alone.
