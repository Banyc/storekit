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
| I | an audit pin ↔ the actual count | the funnel `openat` count (moved deliberately, twice, each time with the reason at the pin) |
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
  cross-module alias route — are denied by the compiler on both targets in every module
  without the allow; every production `libc` reference is either inside a funnel module
  or NAMED in the audit's pin, with the unpinned map asserted empty; and the funnel's
  own call counts are pinned. The pin records a review, not a proof. Completeness of
  the SYMBOL SET is a review responsibility.
- **The funnel-side pin is a SYNTACTIC derivation**, so its guarantee has a shape
  boundary: it resolves a direct call, an inherent or builder method on a
  path-resolvable receiver, and a call held in an enclosing `let`. A name-adopting
  call whose receiver arrives as a FUNCTION PARAMETER, a RETURN, a STRUCT FIELD or a
  function pointer — or one a macro emits — moves no pinned count, and inside a
  funnel module the deny is allowed there, so nothing else refuses it either. That
  shape is outside the pin's guarantee rather than a hole in a promise, and the
  contract's clause (c) says so.
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
