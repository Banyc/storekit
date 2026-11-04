# Consistency

A defect here is two things that must agree, disagreeing. That makes it
deterministic: a sweep can find it, two definitions decide it, and the fix does not
require enumerating the space around it.

Races, interleavings and time-of-check/time-of-use windows are out of scope by
decision. Where one is known, it is recorded as a residual with its window named,
not pursued.

## What must agree

| # | pair |
|---|---|
| A | a doc claim and the code |
| B | `foo` and `foo_fd` (one spelling guarded, the other not) |
| C | the local view, the wire view and the copy view |
| D | a predicate's name and the question it answers |
| E | what `parse` accepts and what every operation does with it |
| F | a constant, its derivation and the resource's limit |
| G | an error class and the condition it reports |
| H | a test's name and the failure it can express |
| I | an audit pin and the actual count (a pinned number is a reviewed constant a change must update) |
| J | the `unix` and `windows` twins |
| K | the revision you are reading and the revision you believe you are reading |
| L | the platform you compile and the platform you claim |
| M | the evidence cited for a change and the population it covers |

## Rules

These are binding.

- **A claim is a measurement or a label.** A behavioural or countable claim either
  names the command, test or table that produced it, or says in the sentence that it
  is unmeasured. Cite items by name, never by line number. A bare count with no
  counting rule is a label.
- **A guarantee belongs at one authority every path passes through.** A caller
  supplies kinds, never a resolution function: one funnel (`crate::atomic`), one
  mode authority (`crate::platform::chmod`), one conversion from a wire path to a
  host path (`crate::relpath::RootedRelativePath::from_manifest`).
- **Folding is a denial tool, never a permission tool.** Over-refusal is the safe
  direction. Where the result of a run would decide a safety property, refuse rather
  than model the plan: state the cost, with its number, instead of guessing.
- **A guarantee is tested by removing it.** Every comparison that can refuse must
  have a test that fails when that comparison alone is deleted — per direction, per
  arm, per platform. A check that cannot fail is a defect, not protection.
- **A device's derivation must not come from the thing it certifies.** An oracle
  that reads the same file it validates is a consistency check; say so, or anchor it
  outside.
- **A deletion is justified by its consumers, not by this crate's own production** —
  and by the assertions that cover it, not by a preserved test count. Port the
  coverage; never widen the surface for a test's convenience.
- **A constraint that removes no branch is decoration.** Prove the delta before
  replacing a runtime check with a type. Flipping an assertion that encoded a looser
  rule is explicit, with the reason recorded.
- **Prefer the unforgeable value.** A type only the crate can construct beats a
  documented convention; a typed error kind beats a message a caller must match.
- **State every residual with its reach**, and every cost with its number. A weak
  path is reachable only through a name that states the weakness.
- **Every file a fix touches, and every copy of the claim it corrects.** Docs and
  code are one artifact; a claim narrowed in one place and left standing in another
  is a defect in whichever is wrong.

## Known residuals

**The contract is prose, and prose drifts from the code.** Every claim in this
repository — the README's contract, this file's pairs, doc comments — is a sentence
someone has to keep true. No test checks an arbitrary sentence against the code, so
prose as prose is not machine-checked. The compiler checks types (the
`compile_fail` doctests are the type-level claims being checked), the gate checks
tests, and the few claims tests do pin are the derived lists and the counted
figures. `pair_less_mutation_enumeration_is_the_derived_public_surface` derives the
pair-less mutation list; `std_fs_name_mutation_counts_are_pinned` and
`every_production_libc_reference_is_pinned` pin the audit's counted `std::fs` and
`libc` figures. Keeping the rest true is a review responsibility, and a claim
narrowed in one place while a sibling copy keeps the old wording is the defect
class that responsibility covers.

Named, scoped, not pursued:

- **Funnel completeness.** No mechanism certifies that every name mutation anywhere
  goes through the guarded funnel; that claim quantifies over the whole language
  (spellings, aliases, macros, builders, module routes, `extern "C"`, raw syscall
  numbers, third-party code). What the crate enforces is bounded. The symbols it
  funnels — including the `libc` symbols its wrappers call, which is what closes the
  cross-module alias route — are denied by the compiler in every module without the
  allow, on each target that exports the symbol. The six denied `libc::…at` symbols
  the Unix wrappers call do not resolve on Windows, so those entries are inert
  there; see the resolution residual below. Every production `libc` reference, in a
  funnel module or not, is named in the audit's pin, with the unpinned map asserted
  empty, and the funnel's own call counts are pinned. The pin records a review, not
  a proof. Completeness of the symbol set is a review responsibility.
- **The far-side root of an `SshTransport` is unresolvable from here, on either
  side.** `refuse_overlapping_roots` returns early when the remote is not local; that
  is the condition, not "the destination is remote". So a `sync` whose two roots are
  not both visible locally computes no disjointness check, whether the far-side root
  is the destination (a push into a root that nests the source) or the source (a pull
  into a local destination nested inside the far-side source). A bind mount, a shared
  mount, or an `ssh` target that is this host is exactly the case where the two
  really do overlap, and nothing this host can see establishes otherwise.
  `a_far_side_source_overlap_is_not_refused_and_the_source_check_is_what_catches_it`
  measures the pull direction: no `RootsOverlap` is raised, the run writes into its
  source, and the end-of-run source re-check is what fails it — after the writes. A
  caller that co-locates them owns the check. Stated in `src/sync/apply.rs`'s module
  docs.
- **The count pins pin call counts, not arguments.** An argument change at a call
  site inside a reviewed allow region moves no `std::fs` count, and the deny is
  allowed there. An argument that introduces a new counted symbol is noticed by the
  same token scan: adding `libc::O_CREAT` to a funnel's `custom_flags` adds a
  reference the pin does not expect. An argument that introduces none (a constant, a
  length, a bit already spelled) is a review responsibility. On Windows an added flag
  is invisible to that pin, because the funnel's `custom_flags` sites spell
  `windows_sys` constants there.
- **The deny list names resolved `std`/`libc` symbols**, so a Windows named-pipe
  creator (`CreateNamedPipeW`) or a raw `CreateFileW`/`NtCreateFile` reached through
  `windows_sys` is outside the clippy deny, and the `libc` pin is libc-specific.
  Rust's stable `std` exposes no named-pipe creator, and this crate's own
  `windows_sys` uses are non-adopting (`GetFileInformationByHandle`,
  `LockFileEx`/`UnlockFileEx`), so the reach is a raw Win32 creator the crate would
  have to add. It is stated at `src/atomic/guard.rs` and in `clippy.toml`.
- **The funnel-side pin is a syntactic derivation**, so its guarantee has a shape
  boundary. It resolves a direct call, an inherent or builder method on a
  path-resolvable receiver, and a call held in an enclosing `let`. A name-adopting
  call whose receiver arrives as a function parameter, a return, a struct field or a
  function pointer moves no pinned count, and inside a funnel module the deny is
  allowed there, so nothing else refuses it either. A canonical path spelled inside
  a `macro_rules!` body is counted (a test pins that); an aliased or non-canonical
  spelling there is not. That shape is outside the pin's guarantee rather than a hole
  in a promise, and the contract's clause 3 says so.
- **The `libc` pin is a token scan, not a call resolver.** Its miss-set is therefore
  not the `std::fs` pin's. Inside a funnel module, a `c::openat(…)` reached through
  an existing `use libc as c;` is neither named nor counted by the pin: the `use` was
  pinned once, as `libc`, when it was added, and adding one is a new unpinned
  reference that fails the test. The completeness test still refuses a symbol the
  deny does not list, because it resolves the alias. What slips past all three
  devices is an additional alias-spelled call of a symbol the deny already names.
  The `std::fs` half is strict here: an aliased `std::fs` call is a `ModuleAlias`
  violation.
- **The Windows compare-and-replace does not refuse a symlink, and the Unix arm
  does.** `atomic::windows::write_atomic_if_match_fd` is path-based: its comparison
  reads through `std::fs::read`, which follows a link, and its replace removes the
  link and installs a regular file. `atomic::unix`'s compare opens the live entry
  `O_NOFOLLOW` and refuses a symlink outright. Only the Windows arm is affected, its
  runtime is out of scope, and the divergence is stated at the function rather than
  left for a reader.
- **The far-side lock holder has no twin for the local arm's parent checks.** The
  local `FileLock` refuses a record whose final component or whose parent is a
  symlink, each with its own typed kind. The far-side holder's `perl` tests the
  directory with `-d`, which follows a symlinked parent, and a symlinked record
  surfaces as a generic far-side script error. Mutual exclusion and the record-content
  rule still hold on both arms; the difference is the typed refusal.
- **`FileLock::acquire` checks the record's immediate parent only.** It `lstat`s that
  parent's final component, so a symlink there is refused; a symlink in any higher
  ancestor (grandparent and above) is still followed by its path-based helpers, and
  the refused-by-rule set covers a swapped component in-root, not this.
- **No deny entry is checked for resolution.** `clippy.toml` is a list of resolved
  symbols, and nothing verifies that an entry resolves on the Unix target: a typo
  produces a non-fatal config-time "does not refer to a reachable function"
  diagnostic that the gate's `-D warnings` does not cover, so it can rot silently. A
  symbol the funnel does use is caught by the closure test, which checks the list,
  not the resolution. Deleting an entry in the `libc` wider set is noticed by nothing:
  the funnel-used subset is closure-checked, and the `std::fs`/`std::os` halves carry
  a pinned expected table (removing `UnixDatagram::bind`'s entry fails
  `pathname_socket_binds_are_denied_and_counted`). Four unreferenced entries are
  named by hand and fail when their entry leaves the deny: `libc::bind` (in
  `pathname_socket_binds_are_denied_and_counted`) and `libc::chmod`,
  `libc::renameat2`, `libc::fopen` (in
  `libc_alias_routes_are_seen_by_the_scanner_or_the_deny`). For every other `libc`
  entry naming a symbol no code path references there is no witness at all:
  deleting it leaves the suite green. Those entries are kept as the reviewed record
  rather than pruned.
- **The wire reader validates shapes, not cross-field consistency.** The reader
  refuses an unknown kind, an invalid mode, a malformed path, and a malformed schema
  version, algorithm or tree digest, so a consumer reading `tree.json` with bare
  serde still gets those. It does not check `content_sha256`/`symlink_target` (the
  target's containment rule is relative to the entry's own path, so it cannot be a
  per-field check), nor the digest recomputation and the duplicate/ordering rules:
  those live in `verify_tree_metadata`, which a caller may skip.
- **The `std::fs` audit parses the crate's sources.** A value carried across a
  variable, `dyn` dispatch, an `extern "C"` declaration, or a proc-macro expansion is
  not seen.
- **Identity injectivity on folding hosts.** The reserved-spelling bookkeeping folds
  case and strips a trailing `.` or space, so it is protected everywhere; two
  ordinary valid names can still collide on a case-insensitive or
  trailing-dot-folding host, which no per-name rule can decide. A caller must not use
  ids that fold together. The trailing-dot half could be closed by refusing the
  spelling, at the cost of rejecting a legitimate absolute-FQDN host name.
- **Races, interleavings and TOCTOU windows**, per the scope note above; where a
  window is known it is stated at the item that has it.
- **The Windows port compiles and lints, but has never been executed.** Its weaker
  guarantees are stated on each primitive and in `atomic::COMPONENT_CONFINED`.
- **Unforgeability is type-level, not cryptographic.** A caller inside the crate can
  construct anything; the fences are private fields, absent `Default`/`Clone`/`From`,
  and `compile_fail` doctests that pin the error codes.
