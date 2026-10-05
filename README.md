# storekit

A store kit: a rooted, descriptor-confined store with atomic writes, an advisory
lock, and validated identifiers — plus the transport that moves a tree of it
between hosts. No application's domain model.

Callers are other projects, mostly agent-written, and will not read the
implementation before calling. So the crate enforces, and never relies on the
caller.

## The constitution

1. **The crate enforces; the caller is not trusted.** Make misuse unrepresentable
   at the API; otherwise refuse the input, or detect the violation and fail
   closed. A rule that lives only in prose is not a rule.
2. **The safe path is the default.** A weaker guarantee is reachable only through
   an entry point whose name states it. A destructive choice is an enum, never a
   boolean. An operation that can take its own lock takes it.
3. **Addresses are faithful and injective.** A manifest value is byte-exact:
   names UTF-8 in NFC, no CR, LF or TAB. A symlink target is link DATA, stored
   UTF-8-verbatim: it may contain `/` and `..`, and a relative target is
   resolved against the directory CONTAINING the link (POSIX). It is refused
   when it is ABSOLUTE, when a `..` walks above the root, or when the walk
   reaches a symlink component — final or intermediate — because the kernel
   FOLLOWS that component and a lexical collapse past it is not the kernel's
   answer; the crate refuses rather than guess where the follow ends. A
   component counts as a symlink component when its EXACT entry is one, or —
   when no exact entry exists — when it folds onto a symlink entry under the
   crate's containment name fold: NFD, then the FULL Unicode case fold (the `C`
   and `F` mappings, so `ß` folds to `ss` and the `ﬁ` ligature to `fi`), then
   NFC, with trailing `.` and spaces removed. This is deliberately NOT
   `str::to_lowercase`, which is a different mapping: a case-insensitive host
   folds the full way, so on APFS `STRASSE`, `strasse` and `straße` are ONE
   entry, and a lowercase-only fold would miss that. So a case- or
   normalization-folding spelling cannot be missed by one view and accepted by
   another. Nothing is normalized or substituted.
4. **A view is faithful, or the check does not run.** No lossy decode, no
   trimming, no defaulted field, on any path that decides something. A listing
   carries the live kind of each entry beside its name.
5. **No decision crosses views.** A sanction, an outcome, a kind and a listing
   are each valid only for the moment they were observed; anything that selects
   a mutation is read from the live tree.
6. **Destroy only under a sanction for that path**, re-established against the
   live tree. Never let a recursive removal delete an unenumerated name. Count a
   mutation before attempting it, and name it if it fails.
7. **A run owns its destination.** Take the operation lock wherever it can be
   taken, hold it for the whole run, release it on every exit path. A write made
   outside that lock is DETECTED over the paths the run reads, by re-checking
   the destination, and fails the run — the detection is not total, so an ABA or
   a write after the last verification is undetectable.
8. **Fail closed.** Refuse rather than transform; error rather than guess; a
   check that cannot run is a failure. Never document a guarantee that is not
   implemented.
9. **Frame or refuse at every boundary that re-interprets a value.** Shell
   operands are one quoted word each, parents computed rather than derived by a
   shell, `--` before a value that may start with `-`. Wire records are
   delimited by a byte a name cannot contain. The far-side LISTING frame carries
   the name LAST (NUL-delimited); the manifest's tree record is tab-delimited
   with the path FIRST, and that is safe only because every name containing NUL,
   LF, CR or TAB is refused everywhere, so no reader depends on field position.

## Refused, by rule

Names not UTF-8 or not NFC · names or targets containing CR, LF or TAB ·
absolute or escaping symlink targets · a relative target that walks through a
symlink component (the kernel follows it, so a lexical collapse past it is not
its resolution) · hard links · devices, sockets, FIFOs ·
reserved-name collisions · overlapping roots, decided per operation rather than
once: a `sync` requires its SOURCE and DESTINATION to be DISJOINT — a strict
ancestor/descendant nesting is refused WHEN THE DESTINATION'S ROOT IS LOCAL — a
far-side root is unresolvable from here, so the caller owns that disjointness — while
EQUAL roots are allowed and the
run is an idempotent no-op (the two manifests are identical, so the diff is
empty) — whereas `root::OwnedRoot::parse` refuses two roots on one endpoint
whenever they are EQUAL or one is an ancestor/descendant of the other, and the
root-confined tree copy (`atomic::copy_dir_recursive_fd`) refuses an
overlapping source and destination (equal included) by directory IDENTITY ·
a DESTINATION root or entry reached through a symlink component (every PARENT
component of an in-root mutation, and the FINAL component of a root open or an
open/create-new) — with two stated exceptions: the atomic REPLACE cannot follow
the final entry (it installs with `renameat`), and a SOURCE spelling may resolve
its own intermediate symlink components, which the fd tree copy reads once —
where a destination's lock cannot be taken, a component swapped between the
check and the operation is a stated residual, not a guarantee.

As a DESTINATION member, an absolute/escaping symlink or a hard link is not
refused: the destination manifest records it, the diff reports it extraneous,
and `Extraneous::Delete` removes it. It is never TRANSFERRED — a source entry
at the same path is refused before any mutation. `Extraneous` is all-or-nothing
(no per-path delete policy): `Delete` cannot PRUNE EXACTLY one destination-only
path (for example snapshot 002 while 001 and 003 are kept) — it removes every
destination-only entry the diff classified extraneous, and `Keep` removes none.
The sanctioned route for a partial retention is `Keep` everything and remove
the unwanted paths out of band through the destination's own removal
primitives (or make every path you want kept part of the source).

A consumer can ask whether a name may be used BEFORE it fails. The authority
is `storekit::is_unaddressable_name` (one path segment) and its path form
`storekit::is_unaddressable_path` (a canonical manifest path): these are the
predicate the identifier rule `storekit::id::valid_name` itself consults. The
implication runs ONE WAY: `valid_name(s)` false does NOT mean `s` is a single safe
segment, because the identifier rule ALSO constrains the charset
(`[A-Za-z0-9-_.]`, so `Ünïcode` is refused and is not unaddressable), the first byte
(no leading `-`), the length (`NAME_MAX`) and the `.`/`..` spellings — so read these
predicates as "spellings the crate cannot address", NOT as a `valid_name` oracle.
They report the spellings the crate reserves for its OWN bookkeeping — the
`.sync-aside.` claim-aside prefix, the `.<name>.operation.lock` record, the
application lock record `operation.lock`, case aliases of any of those, crate
temp shapes, and the Win32 trailing-dot/space aliases of a lock record OR a crate
temp (all refused on every platform, so an id does not mean different things on
different hosts).
`is_reserved_name` / `is_reserved_path` are NARROWER — the byte-exact reserved
MATCH. They deliberately leave the application lock record and the case/trailing-
dot aliases alone, so they are NOT on their own the answer to "may I use this
name": `is_reserved_name` is the primitive the broad authorities are BUILT on
(`is_unaddressable_name`, `is_reserved_case_alias`), and `is_reserved_path` is
public for a caller that needs exactly the byte-exact question — this crate's
PRODUCTION code does not call it (a unit test does). What the sync STRIPS with is
the BROAD authority:
`is_unaddressable_path` for the source view and `is_residue_path` for the
destination view, at `sync::diff`'s `strip_reserved` call sites. An unaddressable
spelling is refused as an identifier and is never transferred by a sync. A
RESIDUE spelling (an unaddressable one that is not a crate temp) is also never
destroyed by `Extraneous::Delete`: it survives every extraneous policy and is
reported in `SyncReport::residue`. A crate TEMP shape is the exception — it is
the documented target of the recovery sweep, so `Extraneous::Delete` DOES remove
it and reports it in `Extraneous` — so "unaddressable" must not be read as
"undeletable".

## Fidelity scope

Carried: name, kind, mode (including setuid, setgid, sticky), content, symlink
target. Not carried: ownership, extended attributes, POSIX ACLs, timestamps,
file flags, sparseness. Refused rather than dropped: hard links. The loss is
INVISIBLE TO THE DIFFER: a dropped xattr or ACL leaves `local_manifest ==
remote_manifest` true and the sync reporting no difference, so only
xattr/ACL-aware tooling on the destination can reveal it. Authoritative
statement: the `manifest` module documentation; the sync entry points restate
the scope.

Because the manifest carries NO TIMESTAMPS, "the newest N snapshots" must come
from the snapshot ID's LEXICAL order (or from a timestamp record the caller
keeps itself). The crate cannot rank two snapshots by time, and two snapshots
that differ only in mtime compare `Same`.

## This is not a backup or checkpoint format

`storekit` moves a tree faithfully WITHIN THE MANIFEST MODEL; it is **not a
backup or checkpoint format**, and it cannot stand in for one:

- A source containing a **hard link**, an **absolute symlink**, or an
  **escaping symlink** cannot be snapshotted AT ALL: the strict source manifest
  refuses the run, so such a tree must be normalized (copy the hard-linked
  content, make the symlink relative) before it can be pushed. A relative
  symlink target that contains `/` or `..` but stays inside the root WITHOUT
  walking through a symlink component (`dir/link -> ../other`) is lawful and is
  NOT one of these refusals; a target whose walk reaches a symlink component is
  refused even when it happens to land inside, because the kernel may follow
  that component out. Containment is decided against the RESULT: a source link
  whose target walks through a component that only the DESTINATION supplies as a
  symlink (and that the source does not replace, kept under the default
  `Extraneous::Keep`) also fails the run rather than installing an escaping link
  or silently removing the destination entry.
- A **restore drops metadata with the differ blind**: `diff(snapshot, live)` is
  EMPTY while `mtime`, xattrs and sparseness differ. Ownership,
  `security.capability`, ACLs, timestamps, file flags and sparseness are not
  carried, and nothing reports their loss. Apply them out of band and verify
  with tooling that can see them.

## Durability and atomicity

A file a sync writes is published ATOMICALLY and DURABLY for every Unix
reachable destination kind (durability means the LINUX power-loss barrier — see the
durability assumption below): a LOCAL destination (a pull, or a push whose
transport is `LocalTransport`) uses the crate's durable atomic replace (unique
temp + `fsync` + `rename` + parent-directory `fsync`), and a REMOTE destination
uses the same shape on the far side (temp, payload on stdin, mode, perl
`fsync(2)`, perl `rename(2)`, parent-directory `fsync(2)`). A write that fails
before the rename leaves the PREVIOUS content in place and removes the temp, so
a failed push can no longer destroy the snapshot it was replacing. The Windows
local port's replace is the ONE non-atomic case (no directory fsync; the target
is removed before the rename) and is unverified. The exact commit points are in
the `manifest` module's "Durability and atomicity of a written entry".

`EntryPolicy::AppendTail` costs O(TOTAL SIZE) per append, because there is no
remote append primitive and the append is a compare-and-replace of the whole
file. Measured on Linux release with `strace` byte accounting (kache
neutralised, load ~1), ONE run that appends 32 bytes to a 1 MiB log reads
**8,388,768 bytes** and writes **1,048,678 bytes**: three whole reads of the
1 MiB destination (the destination manifest, the prefix test, and the
compare-and-replace), three whole reads of the 1 MiB + 32 source (the source
manifest, the prefix test, and the end-of-run source re-check), and two whole
reads of the 1 MiB + 32 result for the TWO post-transfer verification passes —
plus the whole 1 MiB + 32 result written through the atomic temp and the
70-byte lock record. The old "~3.1 MB read" figure counted only the append
rule's three whole-file reads and omitted the verification reads. Budget the
verification, not just the append rule. Batch small appends, or keep the log
outside the synced tree and ship it whole.

**`AppendTail` is NOT a concurrency-safe append.** It is a whole-file
compare-and-replace: concurrent appends are NOT preserved (two runs can read
the same prefix and the later publish discards the other's line — a lost
update on a remote destination, and a `renameat`-wide window locally). A
consumer that relies on one `O_APPEND` write of a complete line landing under
concurrent appenders must keep those writes OUTSIDE the synced tree; see the
`EntryPolicy::AppendTail` warning for what to do instead.

To make a freshly pushed SUBTREE durable, call `fsync_tree(child)` AND
`fsync_parent(child)` on the transport rooted at the child's PARENT. A
`RootedRelativePath` cannot be empty, so a transport rooted at the child itself
cannot name its own root to fsync the parent directory entry.

## What a snapshot costs

Two costs a checkpoint tool must budget for, both measured on a 350 MB tree
unless stated otherwise.

**Memory is O(largest entry), not O(changed bytes).** `Remote::write` takes
`data: &[u8]` and the read side materializes the whole entry, so a single
350 MB file costs peak RSS 362,064 KB (macOS) / 362,860 KB (Linux) for
snapshot AND for restore — a 4 GB file needs roughly 4 GB of addressable
memory in the process doing the transfer. The two destination kinds are NOT
equally protected against a slow link: the SSH path derives a size-aware
deadline from the payload (`upload_deadline` / `transfer_deadline(bytes,
min_rate, command_deadline)`), while the LOCAL path has neither a deadline
nor streaming. Workaround: keep the largest entry under the process's memory
budget, or move large blobs outside the synced tree and ship them with a
tool that streams. A streaming transport API would remove the bound; it is a
deliberate future direction, not part of this change (adding it would
transport-layer-wide redesign under a change that is about residue).

**A snapshot still scans and hashes the WHOLE tree, so it is O(bytes
scanned), not O(bytes changed).** Content addressing makes the STORE
deduplicated — equal content is stored once — but there is no dirty tracking
and no reuse of the previous manifest: `canonicalize_tree`
(`crate::manifest`) reads every file to hash it, and `install_file`
(`crate::sync::apply`) reads the whole source again to write it. Measured on
the 350 MB tree, changing one 4-byte file: 1.327 s before -> 1.384 s after
(macOS); 2.739 s -> 2.690 s (Linux). A periodic checkpoint therefore pays
O(total bytes scanned) every run, which is a design cost of the
manifest-and-hash model, not a bug; an implementation that reused a
previously computed manifest or skipped the second read would change it, and
neither is built here.

**A deep tree is worse than the incremental measurement suggested, and the
shape depends on fresh vs incremental.** A LOCAL path-based destination
re-verifies a path's ancestry before mutating it, at O(depth) per probe. A
depth-D chain with ONE changed leaf is therefore SUPER-LINEAR in D, and the
measured exponent is PLATFORM-DEPENDENT: ≈2.0 on Linux (measured 99 / 373 /
1472 ms at D = 100 / 200 / 400; ratios 3.76 / 3.95) but ≈2.5 on macOS
(measured 0.64 / 3.44 / 21.2 s at the same depths; ratios 5.4 / 6.2; the
consumer's earlier 0.63 / 3.38 / 21.6 s agree). The single O(D^2) label was
wrong on macOS — budget for worse than quadratic. A FRESH destination installs
all D entries, and EACH install pays its own ancestry probe: one probe is O(D)
syscalls, each resolving up to D components, so one install is O(D^2) and a
fresh D-entry destination is O(D^3). The measured shape is AT LEAST cubic — the
consumer's 4.855 s / 41.39 s / 582.1 s give ratios 8.5 and 14.1 for two
doublings, where a purely cubic curve predicts 8. `canonicalize_tree` alone is
cheap (2.76 ms / 6.53 ms /
25.4 ms), so the engine's per-path verification is the cost, and a checkpoint
tool that recreates its destination per snapshot should budget cubic-OR-WORSE. The
Linux and macOS incremental figures above are re-measured under the audit
(release build, kache neutralised, load ≈0.3 Linux / ≈1.4 macOS); the
fresh-destination and `canonicalize_tree` figures are the reviewing consumer's,
not re-measured here.

## A fresh destination

A `PUSH` provisions its destination before reading the destination manifest: the
destination ROOT and the caller's `Layout::bootstrap_dirs` are created, so a
fresh remote destination works without the caller pre-creating it and
`Layout::empty()` is enough. A `PULL` into a local destination creates that root
lazily, on the first mutation.

## Platform

Linux and macOS are supported and exercised. The far side of a remote transfer
may be GNU or BSD userland; both are exercised. The Windows implementation
type-checks but is not exercised, and is described as unverified.

The Windows check compiles the WHOLE target, tests included:
`cargo check --all-targets --target x86_64-pc-windows-msvc`. A signature change
to a Windows code path — production or test — therefore fails this gate instead
of staying invisible until someone builds on Windows. The tests cannot EXECUTE
on this host, so the reproductions that need a Unix filesystem primitive
(`O_NOFOLLOW`, `flock`, `pipe`/`poll`, `symlink`, mode bits, `mkfifo`, raw fds)
are `#[cfg(unix)]`; the rest of the suite is compiled for Windows.

## Assumptions the logic rests on

Restrictions the crate does not enforce, because it cannot. Each buys a
simplification; removing one means adding back the logic it removes.

- **Durability is a LINUX claim.** *Buys:* one durability story instead of a
  per-platform one. `fsync` is the power-loss barrier on Linux; macOS's `fsync` does
  not flush the device write cache (this crate does not call `F_FULLFSYNC`) and the
  Windows port has no directory fsync, so on those targets the crate guarantees the
  replace's ATOMICITY and durability against a process CRASH, not against power loss.
  A caller needing power-loss recovery there owns that step. Wherever this crate calls a
  write DURABLE, this is the claim it means.
- **The name-mutation devices target UNIX.** *Buys:* one funnel, one deny list, one
  pin, and no per-target discussion. On Windows the crate's I/O is `windows_sys`, so a
  `libc` entry is inert only on a target that does not EXPORT the symbol: measured,
  most do not resolve there, and the ones that DO are live denies there too (the crate
  simply calls none of them on that target). The Windows port is a COMPILE target
  whose runtime this contract does not cover.
- **Every claim has one AUTHORITATIVE home.** *Buys:* a place to correct, and a rule for
  the copies. A fact is stated where it is enforced and pointed at elsewhere; where a
  second statement of a MEASURED number is genuinely useful (a README figure beside the
  code comment that measured it), the two must AGREE, and a drift between them is a
  defect in whichever is stale.

- **The destination changes only through this run.** *Buys:* one read of the
  destination is authoritative for the whole run, so work is never ordered
  against an unknown mutation. The crate takes the destination's operation lock
  wherever it can, making this true for cooperating writers; a write made
  outside that lock is still detected over the paths the run reads (rule 7
  states the limit — it is not total) and fails the run.
- **The source does not change during the run.** *Buys:* one read of the source
  describes it for the whole run. Nothing in the crate can prevent a source
  write, so the caller owes this.
- **The destination filesystem's case and normalization behaviour is constant.**
  *Buys:* it is probed once and cached for the run rather than re-probed per
  decision.
- **The destination filesystem renames atomically within a directory.** *Buys:*
  an entry can be moved aside and published by rename, so a replacement never
  passes through a state with neither the old nor the new content.
- **The far side provides a POSIX shell and `perl`.** *Buys:* one script per
  operation, with no binary agent to deploy or version on the far side.
- **The far side's userland is GNU or BSD.** *Buys:* portability is a tested
  property of the scripts instead of a runtime negotiation.
- **The process's descriptor limit exceeds the tree's depth.** *Buys:* a walk
  holds one descriptor per directory level instead of pooling or segmenting.
- **A path-based operation addresses no more than the platform's path limit.**
  *Buys:* path-shaped operations need no segmented traversal. This limit DOES
  apply to the manifest walk: `canonicalize_tree` (`crate::manifest`) uses
  `WalkDir` plus `symlink_metadata`/`read` on accumulated PATHS, so a tree
  deeper than the platform's path limit is refused with `ENAMETOOLONG` at the
  first path that overflows. With 1-byte components the path grows 2 bytes per
  level, so the bound is `floor((PATH_MAX - 1 - base_len)/2)`, where `base_len`
  is the length in BYTES of the base path AS IT RESOLVES on the filesystem: the
  kernel accepts at most `PATH_MAX - 1` bytes for a path, on both platforms.
  Worked examples, each measured by growing a 1-byte chain and asking
  `canonicalize_tree` after every level: on Linux (`PATH_MAX` 4096) a 34-byte
  resolved base admits depth 2030 — the path at that depth is 4094 bytes and
  the next reachable length, 4096, is refused; on macOS (`PATH_MAX` 1024) a
  72-byte resolved base admits 475, where 1022 bytes is accepted and 1024 is
  refused. A 1-byte base is the case `floor((PATH_MAX - 2)/2)`. A
  descriptor-relative manifest walk would lift this;
  it is not implemented, and this bullet is the statement of the real limit.
  The descriptor-relative
  REMOVAL walk (`atomic::unix::remove_dir_contents_fd`, private) holds one descriptor
  per level and is NOT limited by the path limit, so removal supports deeper
  trees than the walk that describes them — but that advantage is itself
  bounded by the descriptor limit, the assumption bullet above ("the process's
  descriptor limit exceeds the tree's depth"), which is the real ceiling on
  removal depth.
- **Metadata beyond name, kind, mode, content and symlink target is outside the
  model.** *Buys:* a small manifest, and no extended-attribute, ACL, ownership,
  timestamp or sparseness machinery.

## Design conflicts surfaced by the consumer audit

Three places where this crate's guarantees and a real consumer's design pulled apart.
All three are CLOSED, and the decisions are binding.

**(a) The sync lock is a SIBLING of the destination root, not the in-root layout
lock.** Two records guard one destination — the sibling `.<name>.operation.lock`
(outside the root) and the caller's in-root `Layout::lock` — so they are composed BY
NAME rather than chosen: `DestinationOwnership::lock_with_in_root_lock` takes the
sibling record first and then the in-root one (both NON-BLOCKING, so the canonical
order cannot deadlock), and it requires the destination root to PRE-EXIST. The in-root
record is destination RESIDUE (`reserved::is_residue_path`), so the run strips it from
the view it judges, never transfers it and never destroys it; it is reported in
`SyncReport::residue`. A documented objection to composing them — "the record would
create the destination root and enter the manifest the run is judging" — was false in
its second half, which is why it was revisited.

**(b) Ownership of a REMOTE destination is a persistent FAR-SIDE session.**
`DestinationOwnership::lock_remote` + `Remote::lock_far_side`: a long-lived local
`ssh` client whose remote process holds `flock(LOCK_EX|LOCK_NB)`, released when stdin
reaches EOF, i.e. released on every exit path including a panic. It is NOT a lease —
it cannot outlive its client. The trait's default REFUSES, so a transport that does
not override it cannot own a remote destination, and the explicitly named weaker path
is `DestinationOwnership::Unowned`. `SshTransport` overrides it (perl, because BSD has
no `flock(1)`), and a contended acquisition returns the typed `LockContended`. A
token is bound to the transport that minted it: direction, pinned local root, remote
root spelling, endpoint identity, remote localness — each compared, each pinned by a
test per direction.

**(c) The fd-confined tree helpers are public.** `deploy` CALLS
`copy_dir_recursive_fd` and `fsync_tree_recursive_fd` from its own store module and
drives a staged publish through `with_operation_lock_sidecar`; `copy_tree_verbatim` is
public for a live-base clone the migration needs but no consumer calls yet (measured:
zero occurrences in `deploy`). A public name is justified by a CONSUMER's need — a
current one or a stated, planned one — and never by this crate's own production; where
the need is planned rather than present, the docs say which it is.

## The contract

This crate is the store SUBSTRATE its consumers build stores on: atomic replace, root
confinement, locks, validated ids and paths, manifest and wire, transport, sync. It
takes responsibility for exactly this, and no more.

* **Enforced.** (a) The mutation symbols THIS crate funnels — including the `libc`
  symbols its wrappers call — are denied by the compiler in every module that does not
  carry the allow, on each target that EXPORTS the symbol (the devices target UNIX —
  see the assumptions), so no spelling, alias or module route reaches
  them (`clippy.toml`: the symbols the funnel uses, plus a reviewed set around
  them — the list is deliberately WIDER than the funnel, and naming a symbol the
  funnel never calls is how a route it could acquire later is refused in
  advance). The device is a UNIX-target device (see the assumptions): on Windows `libc`
  is not the crate's I/O library, so an entry is inert only where the target does not
  export the symbol — the entries that DO resolve there are live denies as well. (b) Every production
  `libc` reference — inside a funnel module (`atomic/{mod,guard,unix,windows}.rs`) or
  anywhere else — is NAMED in the audit's pin, by file, symbol and count; the map of
  references NOT on the pin is asserted EMPTY, so an unreviewed `libc` reference is a
  failing test wherever it stands. The
  pin records a REVIEW, not a proof: a pinned reference is one somebody looked at, and
  whether the mutation it performs is refused is the deny's business only if the deny
  names the symbol. (c) The funnel's OWN call counts are pinned per file and per
  symbol, so a changed or added call inside the funnel forces review — for every
  call the pin's derivation can RESOLVE: a direct call, an inherent or builder
  method on a path-resolvable receiver, or a call held in an enclosing `let`. A
  call whose receiver arrives as a FUNCTION PARAMETER, a RETURN, a STRUCT FIELD
  or a function pointer moves no pinned count — and inside a funnel module the
  deny is allowed, so nothing else refuses it either. A canonical path spelled
  inside a `macro_rules!` body IS counted (a test pins that); an aliased or
  non-canonical spelling there is not. `src/atomic/guard.rs`'s audit names the
  shape it can resolve at the derivation, and the review of a funnel change has
  to cover the rest. A pin counts CALLS, not ARGUMENTS: an argument change that adds
  no newly counted symbol is a review responsibility (on UNIX an argument that adds a
  `libc::…` constant IS noticed — it moves the libc pin — while on Windows the funnel's
  `custom_flags` sites spell `windows_sys` constants, which that pin does not count).
* **Guaranteed as an API.** Root confinement (a relative symlink target cannot leave
  the root, and neither can a mutation named by `(&RootDir, &RootedRelativePath)`), the
  atomic replace's commit points and its reported durability, lock mutual exclusion
  with a record never destroyed by adoption, validated ids and paths, manifest fidelity
  and wire injectivity, the ownership binding, and the typed error kinds.
* **NOT promised.** Completeness of the SYMBOL SET — a mutating symbol nobody listed, a
  raw `syscall(SYS_…)`, an `extern "C"` declaration, a `windows_sys` creator, a
  proc-macro-generated call, third-party code. Keeping the funnel complete is a REVIEW
  responsibility over the symbols the crate actually names. `docs/CONSISTENCY.md`
  states the residuals the CONTRACT rests on, and `docs/API-CONSTRAINTS.md` names the
  ones its constraints leave at each item; a seam with a reach of its own states it
  where it lives.

The operative definition of the funnel is the set of
`#[allow(clippy::disallowed_methods)]` ATTRIBUTES in the source — module-level in the
funnel modules, or item-level on an individual reviewed function. A module-level
attribute is INHERITED by that module's children, so the effective set is the annotated
modules AND their descendants, and it is not enumerated here: it is whatever
`rg -n 'allow\(clippy::disallowed_methods\)' src` reports plus the modules those
attributes cover.

## The gate

Read exit codes DIRECTLY, never through a pipe. On either platform:

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

`cargo doc` is in the gate because a doc link that no longer resolves is a citation
that has drifted: the `-A` allows module docs to link PRIVATE items, which this crate
does deliberately for its maintainers, while an UNRESOLVED link — a renamed item, a
misspelled path — stays a hard error.

`STOREKIT_FULL_TESTS=1` does not add test names: it WIDENS the suite in place, and
two wrappers read the variable.

* `slow_tests_enabled()` — FOUR tests consult it. Three return early with a
  printed skip reason: the atomic replace's sweep over EVERY pre-rename stage, the
  concurrent-controller ssh case, and the every-boundary swap case. One widens its
  own exhaustive sweep instead
  (`valid_name_agrees_with_the_restated_rule`). The default run
  covers sampled shapes; the widened run covers all of them. (The search
  `rg -n slow_tests_enabled src` returns SIX hits: those four, the definition, and
  one `use`.)
* `proptest_cases(..)` — every property test that sizes its case count through it
  widens under the same variable, so the widened run also covers more generated
  cases, not more test names.

TWO clippy commands, and the second is not optional: `--all-targets` compiles the HOST
only, and `cargo check --target …` runs no lints, so a `#[cfg(windows)]`-only module is
invisible to both. `remote_lock` — which stands up a REAL `sshd`, and is
`#![cfg(unix)]` rather than Linux-only — is part of `cargo test` wherever one is
available, while the `ssh_farside_*` suites drive the far-side protocol through a
`PATH` shim, not a real `ssh`.

## Rules for changing this crate

* **One logical change per commit**, gated before it lands and on both platforms.
* **No assertion is weakened or deleted.** A test that encoded a LOOSER rule may be
  flipped, but only explicitly, with the reason recorded at the test. A deletion is
  justified by the ASSERTIONS that cover it — a per-test reconciliation — never by a
  preserved test count, and never by whether this crate's own production happens to
  use the name.
* **A claim is a measurement or it is a label.** Cite a command, a test or a table, or
  say in the sentence that it is unmeasured; cite items by name, never by line number.
  `docs/CONSISTENCY.md` carries the rules this crate earned — they are binding.
* **Port the coverage, do not widen the surface.** When a public name moves, the
  consumer must still compile: fix the consumer in the same round.
* **A weak path is reachable only through a name that states it.**
* **Evidence is about a REVISION.** This crate's own docs, its consumers' trees and any
  cited specification drift; re-read before relying on one.
