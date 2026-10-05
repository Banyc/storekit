# Consistency

A defect here is **two things that must agree, disagreeing** — deterministic, so it
can be found by a sweep, decided by reading two definitions, and fixed without
enumerating the space around it.

**Out of scope by decision:** races, interleavings and TOCTOU windows. Where one is
known it is recorded as a residual with its window named, not pursued.

## The axes

| # | two things that must agree | what it has caught |
|---|---|---|
| A | a doc claim ↔ the code | a path-limit formula; a paragraph titled "the delta, measured" that was never measured; stale line citations; counts written from memory; a claim that a fix existed |
| B | `foo` ↔ `foo_fd` (one spelling guarded, the other not) | the lock-record guard; the residue guard; a path guard re-derived at every `_fd` primitive |
| C | local view ↔ wire view ↔ copy view | three accepted out-of-root escapes; a view prepared for one purpose (the diff) reused to decide another (what survives the run) |
| D | a predicate's name ↔ the question it answers | `is_reserved_name` offered as a "may I use this name" oracle; a token whose check answered "same path spelling" while claiming "same destination" |
| E | what `parse` accepts ↔ what every operation does with it | totality gaps; a manifest path the applier addressed with the host's path model |
| F | a constant ↔ its derivation ↔ the resource's limit | `NAME_MAX` temp overflow; the `sun_path` reserve; path-limit parity between ports |
| G | an error class ↔ the condition it reports | a legacy marker read as corruption; a removal-worded message on a create |
| H | a test's name ↔ the failure it can express | tautological assertions; a count-based bound test blind to quadratic behaviour; a guard that cannot fail; a check whose derivation came from the value it certified |
| I | an audit pin ↔ the actual count | the funnel `openat` count (moved deliberately, twice; the number is a reviewed constant that a change must update, which is what forces the review) |
| J | the `unix` ↔ `windows` twin surface | a public function present on one platform only |
| K | the revision you are READING ↔ the revision you BELIEVE you are reading | a surface count and a whole gate read from a checkout parented to the previous tip |
| L | the platform you COMPILE ↔ the platform you claim | a call site inside `#[cfg(target_os = "linux")]` that a macOS gate never compiles; a lint that never compiles `#[cfg(windows)]` code, so the device was absent on that target |
| M | the evidence cited for a change ↔ the population it covers | "our production never did" used to DELETE a name a consumer's interface declares |

## Rules this crate earned

Each of these was learned by finding the opposite in the code. They are binding.

- **A claim is a measurement or it is a label.** A behavioural or countable claim
  either names the command, test or table that produced it, or says in the sentence
  that it is unmeasured. Cite items by NAME, never by line number. A bare count with
  no counting rule is a label.
- **A guarantee belongs at ONE authority every path passes through.** A caller
  supplies kinds, never a resolution function; one funnel, one mode authority, one
  conversion from a wire path to a host path.
- **A fold is a DENIAL tool, never a PERMISSION tool.** Over-refusal is the safe
  direction. Where the result of a run would decide a safety property, refuse rather
  than model the plan: state the cost, with its number, instead of guessing.
- **A guarantee is tested by removing it.** Every comparison that can refuse must have
  a test that fails when that comparison alone is deleted — per direction, per arm,
  per platform. A check that cannot fail is a defect, not protection.
- **A device's derivation must not come from the thing it certifies.** An oracle that
  reads the same file it validates is a consistency check; say so, or anchor it
  outside.
- **A deletion is justified by its consumers, not by this crate's own production** —
  and by the assertions that cover it, not by a preserved test count. Port the
  coverage; never widen the surface for a test's convenience.
- **A constraint that removes no branch is decoration.** Prove the delta before
  replacing a runtime check with a type. Flipping an assertion that encoded a looser
  rule is explicit, with the reason recorded.
- **Prefer the unforgeable value.** A type only the crate can construct beats a
  documented convention; a typed error kind beats a message a caller must match.
- **State every residual with its reach**, and every cost with its number. A weak path
  is reachable only through a name that states the weakness.
- **Every file a fix touches, and every copy of the claim it corrects.** Docs and code
  are one artifact; a claim narrowed in one place and left standing in another is a
  defect in whichever is wrong.

## Stated residuals

Named, scoped, not pursued:

- **Funnel completeness.** No mechanism certifies that *every* name mutation anywhere
  goes through the guarded funnel — that claim quantifies over the whole language
  (spellings, aliases, macros, builders, module routes, `extern "C"`, raw syscall
  numbers, third-party code). What the crate enforces is bounded: the symbols it
  funnels — including the `libc` symbols its wrappers call, which is what closes the
  cross-module alias route — are denied by the compiler in every module without the
  allow, on each target that EXPORTS the symbol (the six `libc::…at` symbols the Unix
  wrappers call do not resolve on Windows, so those entries are inert there; see the
  resolution residual below); every production `libc` reference, in a funnel module or not,
  is NAMED in the audit's pin, with the unpinned map asserted empty; and the funnel's
  own call counts are pinned. The pin records a review, not a proof. Completeness of
  the SYMBOL SET is a review responsibility.
- **The far-side root of an `SshTransport` is unresolvable from here, on EITHER side.**
  `refuse_overlapping_roots` returns early when the REMOTE is not local — that is the
  condition, not "the destination is remote". So a `sync` whose two roots are not both
  visible locally computes NO disjointness check, whether the far-side root is the
  DESTINATION (a push into a root that nests the source) or the SOURCE (a pull into a
  local destination nested inside the far-side source). A bind mount, a shared mount,
  or an `ssh` target that IS this host is exactly the case where the two really do
  overlap, and nothing this host can see establishes otherwise.
  `a_far_side_source_overlap_is_not_refused_and_the_source_check_is_what_catches_it`
  measures the pull direction: no `RootsOverlap` is raised, the run WRITES INTO ITS
  SOURCE, and the end-of-run source re-check is what fails it — after the writes. The caller that
  co-locates them owns the check. Stated in `src/sync/apply.rs`'s module docs.
- **The count pins pin CALL COUNTS, not arguments.** An argument change at a call site
  inside a reviewed allow region moves no `std::fs` count, and the deny is allowed
  there. An argument that introduces a NEW counted symbol IS noticed, by the same token
  scan: adding `libc::O_CREAT` to a funnel's `custom_flags` adds a reference the pin
  does not expect (no test ships for it — it is a property of the scan, not a case the
  suite exercises). An argument that introduces none (a constant, a length, a bit
  already spelled) is a review responsibility, and on Windows an added flag is
  invisible to that pin because the funnel's `custom_flags` sites spell `windows_sys`
  constants there.
- **The deny list names RESOLVED `std`/`libc` symbols**, so a Windows named-pipe
  creator (`CreateNamedPipeW`) or a raw `CreateFileW`/`NtCreateFile` reached
  through `windows_sys` is outside the clippy deny, and the `libc` pin is
  libc-specific. Rust's stable `std` exposes no named-pipe creator and this
  crate's own `windows_sys` uses are non-adopting (`GetFileInformationByHandle`,
  `LockFileEx`/`UnlockFileEx`), so the reach is a raw Win32 creator the crate
  would have to add; it is stated at `src/atomic/guard.rs` and in `clippy.toml`.
- **The funnel-side pin is a SYNTACTIC derivation**, so its guarantee has a shape
  boundary: it resolves a direct call, an inherent or builder method on a
  path-resolvable receiver, and a call held in an enclosing `let`. A name-adopting
  call whose receiver arrives as a FUNCTION PARAMETER, a RETURN, a STRUCT FIELD or a
  function pointer moves no pinned count, and inside a funnel module the deny is allowed
  there, so nothing else refuses it either. A CANONICAL path spelled inside a
  `macro_rules!` body IS counted (a test pins that); an ALIASED or non-canonical
  spelling there is not. That
  shape is outside the pin's guarantee rather than a hole in a promise, and the
  contract's clause (c) says so.
- **The `libc` pin is a TOKEN SCAN, not a call resolver.** Its miss-set is therefore not
  the std::fs pin's: inside a funnel module, a `c::openat(…)` reached through an
  existing `use libc as c;` is neither NAMED nor counted by the pin (the `use` was
  pinned once, as `libc`, when it was added — adding one is a new unpinned reference
  and fails the test). The completeness test still REFUSES a symbol the deny does not
  list, because it resolves the alias; what slips past all three devices is an
  ADDITIONAL alias-spelled call of a symbol the deny already names. The `std::fs` half is strict here: an aliased `std::fs` call is a
  `ModuleAlias` violation.
- **The far-side lock holder has no twin for the local arm's parent checks.** The local
  `FileLock` refuses a record whose final component or whose parent is a symlink, each
  with its own typed kind; the far-side holder's `perl` tests the directory with `-d`,
  which FOLLOWS a symlinked parent, and a symlinked record surfaces as a generic
  far-side script error. Mutual exclusion and the record-content rule still hold on
  both arms; the difference is the typed refusal, and no document claims parity on it.
- **`FileLock::acquire` checks the record's IMMEDIATE PARENT only.** It `lstat`s that
  parent's final component, so a symlink THERE is refused; a symlink in ANY HIGHER
  ancestor (grandparent and above) is still followed by its path-based helpers, and the
  refused-by-rule set covers a swapped component in-root, not this.
- **No deny entry is checked for RESOLUTION.** `clippy.toml` is a list of resolved
  symbols and nothing verifies that an entry resolves on the Unix target: a typo
  produces a non-fatal config-time `does not refer to a reachable function`
  diagnostic that the gate's `-D warnings` does not cover, so it can rot silently. A
  symbol the funnel DOES use is caught by the closure test, which checks the LIST,
  not the resolution, and DELETING an entry in the `libc` WIDER SET is noticed by
  nothing: the funnel-used subset is closure-checked and the `std::fs`/`std::os`
  halves carry a pinned expected table (removing `UnixDatagram::bind`'s entry fails
  `pathname_socket_binds_are_denied_and_counted`). FOUR unreferenced entries are named
  by hand and fail when their entry leaves the deny: `libc::bind` (in
  `pathname_socket_binds_are_denied_and_counted`) and `libc::chmod`, `libc::renameat2`,
  `libc::fopen` (in `libc_alias_routes_are_seen_by_the_scanner_or_the_deny`). For every
  OTHER `libc` entry naming a symbol no code path references there is no witness at
  all: deleting it leaves the suite green. Those entries are kept as the reviewed
  record rather than pruned.
- **The wire reader validates SHAPES, not cross-field consistency.** The reader now
  refuses an unknown kind, an invalid mode, a malformed path, and a malformed schema
  version, algorithm or tree digest — so a consumer reading `tree.json` with bare serde
  still gets those. It does NOT check `content_sha256`/`symlink_target` (the target's
  containment rule is relative to the entry's own path, so it cannot be a per-field
  check), nor the digest recomputation and the duplicate/ordering rules: those live in
  `verify_tree_metadata`, which a caller may skip.
- **The `std::fs` audit parses the crate's sources.** A value carried across a variable,
  `dyn` dispatch, an `extern "C"` declaration, or a proc-macro expansion is not seen.
- **Identity injectivity on folding hosts.** The reserved-spelling bookkeeping folds
  case and strips a trailing `.`/` `, so it is protected everywhere; two ORDINARY valid
  names can still collide on a case-insensitive or trailing-dot-folding host, which no
  per-name rule can decide. A caller must not use ids that fold together. (The
  trailing-dot half could be closed by refusing the spelling, at the cost of rejecting
  a legitimate absolute-FQDN host name.)
- **Races, interleavings and TOCTOU windows**, per the scope note above; where a window
  is known it is stated at the item that has it.
- **The Windows port compiles and lints, but has never been executed.** Its weaker
  guarantees are stated on each primitive and in `atomic::COMPONENT_CONFINED`.
- **Unforgeability is type-level, not cryptographic.** A caller inside the crate can
  construct anything; the fences are `private` fields, absent `Default`/`Clone`/`From`,
  and `compile_fail` doctests that pin the error codes.
