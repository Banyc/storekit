# Consistency

A defect in this file is **two things that must agree, disagreeing**. That is the
whole method, and it is deliberately bounded: a disagreement is deterministic, so
it can be found by a sweep, decided by reading two definitions, and fixed without
enumerating the space around it.

**Out of scope by decision:** races, interleavings, and TOCTOU windows. They cost
unbounded effort to chase and produce findings that are stated residuals anyway.
Where one is known it is recorded as a residual with the window named, not
pursued. The identity-based overlap refusal is correct when it runs; a mount
installed between the check and the walk is that kind of residual.

## The axes

| # | Two things that must agree | Examples this has caught |
|---|---|---|
| A | a doc claim ↔ the code | path-limit formula, `O(D²)` vs the measured exponent, "carries the refusal", "at least as broad as the host's fold", the removed `remove_dir_all` remedy, a stale line citation |
| B | `foo` ↔ `foo_fd` (one spelling guarded, the other not) | the lock-record guard, the residue guard, `validate_rel`/`parse` |
| C | local view ↔ wire view ↔ copy view | three accepted out-of-root escapes |
| D | a predicate's name ↔ the question it answers | `is_reserved_name` offered as an oracle; the temp/id overlap; the trailing-dot id hole |
| E | what `parse` accepts ↔ what every operation does with it | totality gaps |
| F | a constant ↔ its derivation ↔ the resource's limit | `NAME_MAX` temp overflow, the `sun_path` reserve, the path-limit parity |
| G | an error class ↔ the condition it reports | legacy marker vs corruption; `Err(_) => Absent`; a removal-worded message on a create |
| H | a test's name ↔ the failure it can express | a tautological assertion; a false proof in `id_macro.rs`; a count-based bound test blind to quadratic |
| I | an audit pin ↔ the actual count | the funnel `openat` count |
| J | the `unix` ↔ `windows` twin surface | a public function present on one platform only |
| K | the revision you are READING ↔ the revision you BELIEVE you are reading | a `pub fn` count and a gate read from a checkout still parented to the previous tip |
| L | the platform you COMPILE ↔ the platform you claim to support | a call site inside `#[cfg(target_os = "linux")]` that a macOS-only gate never compiles, so a signature change silently missed it |
| M | the evidence cited for a change ↔ the population it actually covers | "our production never did" used to delete a name a CONSUMER's interface declares |

## Findings

**A — line-number citations drift.** The README cited `src/atomic/unix.rs:2031`
and `:2309` for two primitives; those lines are now doc text. A stale citation
reads as verified, and this class has already recurred once (a `:2302` citation
that was seven lines off). **Fixed** by citing items by NAME everywhere in
`README.md`; line numbers are no longer used. The same rule covers the
CROSS-REPO citations this log makes to `~/code/deploy`: a line number there
drifts for reasons this crate never sees, so the entries below name the item
(`deploy`'s transport trait declaring `exists`; its Windows port's path-based
`write_atomic_replace`) rather than a line.

**A — the extraction spec contradicted the code in four places. FIXED.**
`EXTRACTION.md` is the plan the crate was built from, and it had not been
revisited while the crate changed under it: its Error adaptation listed tuple
variants that no longer exist (the variants carry `{ kind, message }`), its
visibility adaptation said `pub(crate)` becomes `pub` for every API item (the
surface is now deliberately SHRUNK, and a deletion is justified only by a
consumer's need), its Windows adaptation said this machine "cannot compile" the
Windows modules (they compile under the gate), and its Gate section listed three
commands where the gate is now both platforms plus doc-tests and `--all-targets`
for Windows. It is now labelled as the historical record it is, with a "since the
extraction" note on the slice file map.

**A — the design-conflict premise was FALSE for the record it described. FIXED.**
README's design conflict (a) and the `sync` module docs claimed an in-root
`state/operation.lock` "would create the destination ROOT and enter the
destination manifest the run is judging" — the stated reason the two records
were left uncomposed. The second half is false for the record: the DESTINATION
view strips it as residue. `sync::diff::apply_manifests` strips the destination
with `reserved::is_residue_path`, and `is_residue_path` is true for any path
with a component that is the application-lock spelling (`operation.lock`), so
the record is invisible to the diff, never transferred, and never destroyed;
it is reported in `SyncReport::residue`. What remains true is that the lock
CREATES the destination root when it is missing and creates the record's parent
directory, and an empty parent directory is ordinary content (under
`Extraneous::Delete` its removal is refused because it holds residue). The
premise is corrected in README (a) and in the `sync` module docs; the composed
ownership form takes the in-root record BY NAME (`DestinationOwnership::lock_with_in_root_lock`),
requires the destination root to pre-exist, and reports the record as residue.
The lesson is axis A's usual one: a documented objection read as verified had
not been checked against the strip the code actually performs.

**B/D — the one gate answered to two names, one of them misleading.**
`atomic::refuse_lock_record_mutation` was `refuse_reserved_mutation(rel,
Sanction::None)` — it runs BOTH the lock-record half and the residue half — while
its own doc block said there was deliberately no lock-only function. Call sites
were split between the two names, so a reader asking "does this primitive also
refuse a stranded aside?" could not tell from the call. **Fixed:** all 23 call
sites were verified to be `Sanction::None` — no site intended only the lock half,
which is what the design says cannot exist — so the alias was deleted and every
site now names `refuse_reserved_mutation(…, Sanction::None)`.

**D — one concept, two types. FIXED.** `sync::apply::UnsupportedDestination` was
a field-for-field copy of `manifest::UnsupportedEntry` (`path`, `reason`), built
by mapping one to the other, and its own doc said the values were *"preserved
verbatim from `crate::manifest::UnsupportedEntry`"*. Two names for one thing
meant any distinction typed on the manifest side had to be added twice or lost
in transit — which is exactly what happened when the tolerated reason gained a
kind. The duplicate was deleted; the report now carries the manifest type, so a
consumer sees the kind, and one `map(…clone…)` step disappeared with it.

**F — a duplicated constant across the platform twins.** `MAX_ANCESTRY` was
defined independently in `atomic/unix.rs` and `atomic/windows.rs`, both `1 << 16`.
The two ports enforce the same rule, so the values must agree; two literals are
free to drift. **Fixed:** single-sourced as `atomic::MAX_ANCESTRY`, referenced by
both ports.

**F (resolved, no action) — `SUN_PATH_BYTES` ×2 is correct.** One definition is
`#[cfg(unix)]` and computed from the platform's `sockaddr_un`; the other is the
`#[cfg(not(unix))]` placeholder so the module compiles everywhere. A `cfg`-gated
alternative is not a duplication.

**G — two candidates reviewed, BOTH conservative by construction. No defect.**
A removal path computes `confirmed = match rooted(&path) { Ok(rel) => …, Err(_) =>
false }`, and `RenamedEntryLocation::Unknown` collapses every probe error into
one variant. Both read like fail-open — an error becoming permission — and are
the opposite: `false` here means "not confirmed PRESENT", and the caller's
response to unconfirmed is to drop the candidate and emit *"its location is
unknown and it may be at <both spellings>, which must be checked by hand"*;
`Unknown` asserts nothing about location and pushes an `UnconfirmedMove` naming
both spellings. An unconfirmable probe therefore degrades to an explicitly
reported indeterminate state, never to a destructive decision.

The entry stays in this list as a RESOLVED false positive rather than being
deleted, because the shape is a trap for the next reader: the meaning of `false`
is only settled by reading the consumer, and the honest rule it demonstrates is
that "cannot determine" must be a state the caller reports, not a value that
silently feeds a branch.

**J — the public surface was not the same on both platforms. FIXED.**
The crate exposed unix-only public names while both ports `pub use …::*` into the
same namespaces, so a consumer using one compiled on unix and failed on windows.
The set was `atomic::{fsync_dir_fd, openat_no_follow, openat_no_follow_io}`
(`remove_dir_all_path` had already left the surface with API constraint #1) plus
`transport::kill_process_group`.

Every one of them turned out to be reachable only from inside the crate — the
`tests/confinement.rs` mention of `openat_no_follow` is prose, not a call — so
the fix is not to document the platform in the API but to stop exposing the
names: all three `atomic` primitives became `pub(crate)`, `kill_process_group`
stays public on neither port (it is the SSH runner's kill path, so it became
`pub(crate)` under the same `#[cfg(unix)]`), `RealKill` stays public because BOTH
ports implement it, and a dead transport-level re-export of `kill_process_group`
was removed outright.

Verified by re-running the sweep **twice**, and the first verification was
WRONG: it claimed the unix-only and windows-only public sets in `atomic` were
both EMPTY, and three platform-scoped `pub` items had survived it —
`atomic::temp_file_name` and `RootDir::as_fd` on unix, `RootDir::path` on
windows — so a consumer calling the first two compiles on macOS and fails to
compile for `x86_64-pc-windows-msvc` (`E0425`, `E0599`): exactly the failure this
entry says was eliminated. The sweep was wrong because it re-checked the
population the earlier fix had already emptied (the `pub use …::*` free-function
sets) instead of asking each surviving `pub` item whether it exists on both
ports. The second sweep asked a better question — does any CONSUMER need it —
and, with `~/code/deploy` the only dependent and zero hits for all three, demoted
them to `pub(crate)`. Measured with rustdoc JSON on both targets — and the COUNTS that stood here
(191 items crate-wide, 11 in `atomic`) are REMOVED, because a bare count without
its counting RULE is a label rather than a measurement (README's rule), and a
round-4 reviewer applying a stated but different rule (public, `crate_id == 0`,
excluding modules, `use` re-exports, struct fields and variants) counted 173 and
37. Both instruments agree on the PROPERTY, which is the claim; they disagree on
the number, which was never the claim. The property: the accessible public NAME
SETS are identical on `aarch64-apple-darwin` and `x86_64-pc-windows-msvc` — 0
unix-only and 0 windows-only names — reproduced independently by two reviewers
with their own instruments.

What REMAINS platform-dependent is on BOTH ports and states its MEANING rather
than its presence, so the next sweep checks the right property:
`atomic::COMPONENT_CONFINED` (`cfg!(unix)` — the flag a caller must consult to
know whether the live confinement check still applies to it at all) and `RootDir`
itself (an `OwnedFd` on unix against a `PathBuf` on windows, with a stronger
`read`/`open` guarantee on unix that the type documents). "Both sets empty" is
only a claim about PRESENCE; presence is now equal, and meaning is what a
consumer must read.

The crate's public-function total fell 186 → 174 across this and API constraint
#1 — the constraint's real product is a smaller surface, not a longer document.

**K — a reading taken from the wrong revision.** A `pub fn` surface count and a
full `cargo test --lib` were both run from a checkout whose working copy was
still parented to the PREVIOUS tip, so they described a tree that no longer
existed: the counts said the surface had not shrunk (34/30, not 29/26) and the
tree lacked `src/relpath.rs` entirely. The change's own `jj diff --stat`
contradicted the reading, and re-parenting the checkout showed the agent's
numbers were right. This is the checkout sibling of the stale-binary rule: a
green gate and a bad count are both only meaningful for a NAMED revision.

**L — a signature change missed a platform-gated call site. FIXED.** API
constraint #1 made every root-relative mutation take `RootedRelativePath`. The
macOS gate was green and stayed green; the Linux gate failed to **compile the
integration tests**, because the bind-mount regression lives in a
`#[cfg(target_os = "linux")]` block that macOS never compiles and it still
passed a `&Path`. The instance was one line; the class is that a platform-gated
call site is only ever built on ONE platform, so a signature change is
half-checked until the other gate runs. The fix is not a rule about care — it is
the Linux gate itself, which compiles what macOS cannot, which is why "a green
gate on one platform is not evidence for another" is mechanical here.

**E — the accepted set and the operations over it.** `validate_rel` accepts
`a/./b`, `a/b/` and `a//b`; `RootedRelativePath::parse` refuses a literal `.`
segment. Any primitive that takes a raw path therefore re-validates against a
rule STRICTER OR LOOSER than the one at the boundary. **Fixed** by API
constraint #1: the boundary is the type, `validate_rel` is deleted, and the
delta was measured to be **one-directional** — the type refuses a strict
superset, the only newly-refused inputs being spellings with a non-leading
literal `.` segment. No input the guard refused is now accepted.

**I — an audit pin moved, deliberately.** When `remove_dir_all_path` left
production the pinned `std::fs`/`libc` counts dropped with it (`remove_file`,
`rmdir`, `open`). That is the pin doing its job: the count changed, so the
change had to say so.

**D — the unconfined replace answered to the crate's DEFAULT name. FIXED.**
`atomic::write_atomic_replace` took a raw `&Path` and resolved every component
by that path (so an intermediate symlink was followed), yet its unqualified
name read as the crate's default atomic replace — the confined form is
`write_atomic_replace_fd`. It stayed public because an integration test used
it, which is not a consumer reason. **Fixed** by API constraint #8: it is now
`pub(crate)` (test-only on Unix, where no production body needs it; the body
of the fd surface on Windows), with a `compile_fail` doctest in `atomic`'s
module docs proving a caller cannot name the unconfined form. The integration
test's assertions are unchanged; it now drives the confined PUBLIC primitive
(`write_atomic_replace_fd`). This removed the LAST public mutation that did
not take `(&RootDir, &RootedRelativePath)`, completing constraint #1's stated
rule. A rename to a name stating the weakness was the alternative; demotion was
chosen because the only reason it was public was a test, and the constraint's
real product is a smaller surface.

**REVERSED under axis M.** The demotion looked safe because *this crate's* own
production never called the path-based replace — the wrong population. A
consumer's port does call it (deploy's Windows port, its path-based
`write_atomic_replace`), so
the name is public again (verdict **N**, named, not hidden), with its doc
stating exactly what it is: the UNCONFINED, absolute-path form. The
`compile_fail` doctest that proved a caller could not name it became false and
was replaced by a still-true one (the CONFINED replace's signature refuses a
raw `&Path`).

**D — an existence probe answered a question it could not answer. FIXED.**
`Remote::exists` returned `bool`: a permission error, an I/O fault and a
genuine absence all read as `false`. Its own trait documentation said callers
must never consult it, and the crate's production code never did — it was a
public trap. **Fixed** by deleting it from the trait; `Remote::metadata_opt`
(the typed `Ok(Some)` / `Ok(None)` / `Err` probe) is the only existence
primitive, and the few tests that used `exists` now branch on that distinction
(one assertion got STRONGER: a symlinked parent is now asserted as `Err`, not
as "not present").

**REVERSED under axis M.** The deletion was justified by "this crate's
production never did" — the wrong population again. deploy's own transport
trait DECLARES `exists` as a REQUIRED method and its production calls it,
so removing the name is a build break for the consumer. It is restored as a
DEFAULT method delegating to `metadata_opt`, with a doc that states exactly
what a `false` discards (`absent` conflated with `the probe could not tell`).

**D (stated residual, no action) — `is_reserved_name` is narrower than the
name rule.** `reserved::is_reserved_name` / `is_reserved_path` answer a
byte-exact reserved MATCH (what the sync strips), not "may I use this name" —
the authority is `is_unaddressable_name` / `is_unaddressable_path`. The names
do not say "narrow", but the README names the authority and the narrower pair
at the same place; renaming them would touch every call site for a predicate
whose distinction is already stated. Recorded as a residual rather than fixed.

**I — an audit pin moved again, deliberately (API constraint #8).** Making the
unconfined `write_atomic_replace` `#[cfg(test)]` on Unix removed its
`std::fs::rename` from the production count, so the pin entry
`("src/atomic/unix.rs", "rename", 1)` was removed in the same change, with the
reason recorded AT the pin. The guard still runs on the function; the call is
simply no longer production code, which is exactly what the audit excludes.

**REVERSED under axis M.** Restoring the public, production
`write_atomic_replace` put its `std::fs::rename` back into the production
count, so the pin entry `("src/atomic/unix.rs", "rename", 1)` is restored —
with the reason at the pin. The pin moved twice on purpose: the count changed,
so the change had to say so, both times.

**M — the evidence cited was about the wrong population. FIXED.** A pass
resolved two items of API constraint #8 by DELETION/DEMOTION, each justified by
"production never did [consult it]" — meaning THIS CRATE's own production. The
population that decides whether a public name may be deleted is the CONSUMER's
interface, and this crate's whole purpose is to be consumed by `~/code/deploy`
(and then `~/code/ckpt`). A consumer-fit audit against those consumers found
both deletions were over-reach:

* `Remote::exists` was REMOVED from the trait. deploy's own transport trait
  DECLARES it as a REQUIRED method — `fn exists(&self, rel: &RootedRelativePath)
  -> bool;` — and its production calls it (in `remote::helper`, in
  `remote::helper::durable`, and in `store::local::objects`). It is part of the
  interface this crate was extracted from.
* `atomic::write_atomic_replace(&Path)` was demoted to `pub(crate)`. deploy's
  production uses a path-based atomic replace: its Windows port
  (`store::atomic::windows`) calls `write_atomic_replace(&root.path().join(rel),
  ..)`, and `store::local` drives deploy's `write_atomic_replace_at`, whose
  confined body is `write_atomic_replace_fd`.

**Fixed:** `exists` is restored to the trait as a DEFAULT method delegating to
`metadata_opt` (no implementor is forced to write it; an implementor may
override with a cheaper probe), and its doc states exactly what a `false`
discards, pointing a caller that must distinguish *absent* from *could not
tell* at `metadata_opt`. `write_atomic_replace` is `pub fn` again, named for
what it is: the UNCONFINED, absolute-path form, the one mutation that does not
take a `(&RootDir, &RootedRelativePath)` pair and therefore the one to avoid
when a confined form exists. Do NOT restore anything else that pass removed:
the rest was verified against the consumers, the consumer-fit BLOCKED list was
otherwise empty, and every other removed path-based helper has an `_fd`
equivalent deploy can adapt to mechanically.

**The CLASS fix is `tests/consumer_fit.rs`**, because the crate had no test
that asserted the shapes its CONSUMERS require — which is why two deletions
could pass every gate. It exercises, against the PUBLIC API only, the call
shapes the consumers actually use: the cheap `Remote::exists` probe and the
typed `metadata_opt` alternative; the path-based `write_atomic_replace` AND its
confined equivalent; the tree pair with the exact consumer signatures (an
out-of-root `&Path` source copied into a root-confined `RootedRelativePath`
staging destination, then `fsync_tree_recursive_fd`); the ONE `sync` entry
point in BOTH ownership states (`DestinationOwnership::lock(..)` and
`DestinationOwnership::Unowned`); a `Layout` construction; and a typed error
branch by KIND, not message text. It is a genuine guard, not a document: a
control that removed the two names failed to COMPILE with `E0432` (unresolved
import `write_atomic_replace`) and `E0599` (`no method named exists`), which is
the failure mode we want — a deletion breaks the build, not a migration.

**D — one type carried both directions of the manifest. FIXED (API
constraint #7).** `TreeEntry.entry_type` was a `String` and `mode` an octal
`String` — the WIRE spellings — and every consumer re-projected them:
`EntryKind::of` at nine production sites (six fallible, two silently dropping a
path with `let Ok(..) else { continue }`, one filtering) and
`parse_mode(&entry.mode)` at seven, each a fallible branch. The field is
now the VALIDATED value (`EntryKind`, `u32`); the wire strings exist only
across serde, and the `unknown manifest entry type` / `invalid manifest mode`
refusals moved to the one wire boundary. The same shape held for the
tree-view family: `DestinationTree.meta` was a plain `TreeMetadata`, so the
destination observation could be serialized as a `tree.json`, fed to
`verify_tree_metadata`, or used as a source — the crate's own doc forbade all
three and NO runtime check existed to catch any of them (the count of such
guards was zero, which is why the fix is a TYPE, not a branch deletion, at
that edge). `DestinationTree`'s payload is now `pub(crate)` and the diff/apply
entry points are direction-typed. Proof: two `compile_fail` doctests, each
`E0308` (`expected &TreeMetadata, found &DestinationTree`).

What that fix CLOSES and what it does NOT, because this entry once listed all
three as closed: the `pub(crate)` payload closes SERIALIZATION, and the
direction-typed entry points close PASSING a destination where a source is
required — both proven by the `E0308` doctests. It does NOT close the REBUILD
route: `TreeMetadata`'s fields are `pub` by design, so a caller can construct one
from the destination's public accessors and `verify_tree_metadata` accepts it
(round 3 found exactly that, `src/manifest/mod.rs`'s doc states it, and the
CONSTRAINT doc's copy of the broad claim had to be corrected in round 5 because
the round-3 fix touched only `src/`). Closing it would require SEALING
`TreeMetadata`, a breaking change to a consumer that the crate does not make.
The one branch
class the split DID delete is the walk's runtime policy mode
(`UnsupportedPolicy` plus the deferred `unsupported_reason: Option<...>`),
now a `UnsupportedSink` type.

**A — a listing's doc described a kind it no longer reports. FIXED.** The doc
on `LocalSide::list` said "a NON-directory child is reported as
`EntryKind::File`, so a symlink is reported as a file ... recovering the
symlink kind here would need a second descriptor walk". The body directly
below the doc performs exactly that second walk
(`path_kind_fd`), reports `EntryKind::Symlink`, and refuses
`PathKind::Other`; `list_pinned_root` agrees. The stale paragraph is replaced
with the behaviour the code has. This is the axis-D trap the constraint is
about in miniature: a name (and a doc) saying one thing while the value says
another.

## The adversarial review (round 1)

Two independent reviewers were given a byte-identical brief and told to attack
this crate's OWN claims — every constraint marked done, every axis closed. To
keep the round comparable the prompt was fixed for both, and to keep it honest a
finding needed evidence and a clean report needed a coverage list. Twelve
findings were actionable; all twelve are fixed here. **The distribution is the
finding.**

| # | finding | axis | fixed by |
|---|---|---|---|
| 1 | Destination RESIDUE was invisible to the result-containment check. `apply_manifests` strips residue for the DIFF (correct — it is never transferred), and the index that decides the POST-RUN verdict was built from that same stripped view, so a source link whose target walked through a stranded claim-aside symlink was permitted and the run installed a link that escaped the root. Reproduced on macOS and Linux; the report simultaneously named the component as `residue` and as an unsupported escaping symlink while installing the link. | C | `ovwlkwmxkvpu` |
| 2 | The destination-ownership token was not bound to the TRANSPORT. `Prepared` recorded the destination's PATH (`normalize_root(remote.root())`) and never the endpoint, so a token minted against host A was accepted for a run against host B reporting the same root, and the run mutated B while holding A's record. | D | `lxxormsvvwkw` |
| 3 | `mode_octal::deserialize` MASKED (`& 0o7777`) where three documents claimed it refused, and `from_str_radix` also accepted a sign and any length (`"10644"` loaded as `0o644` with the setuid bit dropped; `"+755"` loaded at all). The parse was non-injective, and `compute_tree_digest` hashes the VALIDATED values, so two different wire records aliased to one `tree_sha256`. | A | `upksmsssrynr` |
| 4 | The `std::fs` mutation audit counted the literal text `std::fs::{symbol}(`, so `use std::fs::remove_file; remove_file(p)` in production left the pin GREEN. The libc half of the same audit already handled alias routes. | H | `znstvkxrwozv` |
| 5 | Constraint #1's "ONE tolerated exception" was false: `platform::chmod`, `platform::symlink`, `sync::retire_destination_lock`, the copy primitives, `lock::FileLock::acquire`, `sync::Residue::recover_to` and `Remote::lock_far_side` are public and take raw paths. | M | `kvyywnmrxnyn` |
| 6 | The two wrong-direction `compile_fail` doctests were bare annotations, which any compile error satisfies, while the doc claimed each failed with `E0308`. | H | `upksmsssrynr` |
| 7 | A source doc cited a test name that does not exist. | A | `upksmsssrynr` |
| 8 | `GuardedRel::new`'s "one of only TWO functions in the crate that run the guard (verified by the source audit)" was false — three minting constructors, ~21 direct `refuse_reserved_mutation` call sites, and no audit that counts guard callers. | A | `znstvkxrwozv` |
| 9 | README's "an unaddressable spelling ... is never destroyed by `Extraneous::Delete`" was overbroad: a crate TEMP shape IS removed by `Delete`. | A | `kvyywnmrxnyn` |
| 10 | `valid_name`'s injectivity claim was false on case-insensitive and trailing-dot-folding hosts. | A | `kvyywnmrxnyn`, `yvkvksnwxnor` |
| 11 | `the_table_covers_the_unicode_17_additions` named 28 mappings and asserted 6, so corrupting an unsampled one left all five casefold tests green (the table itself was correct, verified against the UCD). | H | `ruylovrpvvvs` |
| 12 | Constraint #5 claimed `Sanction`/`GuardedRel` were "unforgeable outside the crate" while both are `pub(crate)` — the row described no public surface at all. | A | `kvyywnmrxnyn` |

**Finding 2's fix left a residual, and the round closed it rather than shipping it.**
The token agent reported it plainly: `DestinationOwnership::lock_remote` refused a
`None` endpoint identity, but `DestinationOwnership::lock` did not — so a PULL
whose SOURCE was a third-party `Remote` that did not override
`endpoint_identity` still minted a token bound to nothing but a path spelling,
and the run would apply one host's plan to another host's data. The reach was
third-party only (`SshTransport` always states one), which is exactly why it was
closed rather than documented: **a check that holds only because every
transport author read the doc is not a check.** One authority,
`require_endpoint_identity`, is now called by all three minting paths —
`lock`, `lock_remote` and `lock_with_in_root_lock` — and refuses a NON-LOCAL
transport in EITHER role, because for a PULL the remote is the source whose
manifest the token carries as the plan. The refusal is a typed `Preflight`
naming both the override and the weaker `Unowned` path, and it does not touch an
UNOWNED run, which holds no token to bind. Evidence, PER PATH — because an
earlier version of this sentence claimed one test covered all three, and the
round-3 review showed that was false by REMOVING the third call and watching the
whole suite stay green (659 + 2 lib, every integration suite, 11 doc-tests):
`lock` is covered by
`a_non_local_source_without_an_endpoint_identity_cannot_mint_a_token`,
`lock_remote` by `a_transport_without_an_endpoint_identity_cannot_mint_a_remote_token`,
and `lock_with_in_root_lock` by `a_composed_mint_refuses_a_non_local_source_without_an_endpoint_identity`,
added in round 3 for exactly this reason (it must PRE-CREATE the destination root,
because the endpoint guard is ordered before `require_existing_root`, so an absent
root would refuse for a different reason and mask the guard's removal).
The guard was present on all three from the start; on the third it was
UNREGRESSIBLE, which is one step from absent. The lesson is this file's usual
one: a guard is a guard only if a test notices its removal, and a coverage claim
spanning several call sites must name a test for EACH, never one test for the set. A non-local `RecordingRemote` now states an endpoint at
construction, the way a real remote transport must.

**Six of the twelve are axis A** (rows 3, 7, 8, 9, 10 and 12), "a doc claim ↔
the code", and they are not six unrelated slips: they are one habit, a claim
ASSERTED in the confident register rather than MEASURED. The sharpest instance is
finding 3 — its paragraph was titled "**The delta, measured**" and was not measured;
it described a strictness the code did not have, and the crate's own rule ("prove
the delta") would have caught it had the delta actually been produced. The other
five axis-A rows share that shape: a sentence that reads as verified with nothing
behind it. (An earlier version of this paragraph said "EIGHT of the twelve" and
named rows 5 and 6 among them — a count that contradicted its own table directly
above it, written while diagnosing exactly that habit. Round 8 measured it: the
table gives six.)

Two axes gained a meaning they did not have. **C** now covers "a view prepared
for ONE purpose reused for another": the destination stripped for the diff is not
the destination as it will be after the run, and only the second decides whether
an installed link escapes. **D** now covers a TOKEN, not only a name predicate:
`matches` answered "same path spelling" while its callers and its own docs
claimed "same destination".

The mitigation is a rule, not a test — see README's "a claim is a measurement or
it is a label". Axis A cannot be closed mechanically here: this round's own
attempt to check it by scanning the docs for cited identifiers that no longer
exist was WRONG, because the docs legitimately cite removed items in the
historical register ("`parse_mode` was removed"), and no text scan distinguishes
"cited as current" from "cited as removed". That instrument was discarded rather
than shipped, and it is recorded here because a discarded instrument is the same
mistake as the defects it was meant to catch.

## The adversarial review (round 2)

Same two reviewers, same byte-identical prompt, run against the FIXED tree. Eight
findings, all fixed here; one is a P0.

| # | finding | axis | fixed by |
|---|---|---|---|
| 1 | **P0: the P0 of round 1, still open, with the FIX's reasoning as the cause.** The result-containment check decided what the destination will hold AFTER the run from the static diff plus the `Extraneous` value — but the run's actual occupants are decided later: a destination entry at a path the source also holds is NOT necessarily replaced (a `Refuse` policy, `AppendTail` on a non-file, `Diverged` and `ParentRefused` all leave it), and a destination-only entry under `Delete` is NOT necessarily removed (`remove_extraneous` skips entries a conflict prohibits, that alias an installed entry, or that are residue-guarded). Reproduced on macOS and Linux through the public API with a BUILT-IN policy as well as a custom one: `sync` returned `Ok`, installed a link, and the link resolved outside the root, while the report simultaneously named the component an unsupported escaping symlink. | C | `oywnzuuxmvkv` (+ `wqzpyltvyyyv`) |
| 2 | The `std::fs` audit was evaded by an ordinary alias of the `std` CRATE (`use std as s; use s::fs::remove_file;` — rustfmt- and clippy-clean), and the exact-count pin by a space before the call's paren. | H | `vskttuqsxtky` |
| 3 | `transport::with_operation_lock_sidecar` CREATED caller-chosen reserved spellings (`operation.lock`, `.sync-aside.1`, and their case/`state/` variants) where seven guarded paths refused, and the helper was absent from constraint #1's enumerated list — the list's SECOND omission of the same kind. | M | `nwqxqyokptst` |
| 4 | Both audits exempted any file named `*_regression.rs` / `*_test_support.rs`, so a production module with that name could contain a raw `libc::unlinkat` with every gate green. | H | `vskttuqsxtky` |
| 5 | Axis J's own verification claim was FALSE: the sweep re-checked the population the earlier fix had emptied and reported that the unix-only and windows-only public sets were "both EMPTY", while three platform-scoped `pub` items survived and broke a Windows build for a consumer. | A | `vrrrxukzkxty` |
| 6 | Two forgery `compile_fail` doctests were VACUOUS: `X {}` fails with `E0063` whether the fields are private or public, so making them public would have left the fence passing. | H | `oywnzuuxmvkv` |
| 7 | Constraint #4 was marked done while its FIFTH class (`Error::Preflight`) had no kind at all, and the crate's own newest tests told its conditions apart by message substring — the constraint's stated product, violated by the crate itself. | A | `npnmnroqpukq` |
| 8 | `MIGRATION.md` asserted both "deploy does not depend on the crate yet. Nothing below has been executed" and, sixty lines later, DONE. | A | `xmzpwuzsszpk` |

**Finding 1 is finding 1 of round 1, and that is the finding.** The previous fix
was not a patch that missed a case: it reasoned from a view (the static diff) to a
property of the result (what will be on disk), and the two are not the same
function. The remedy is not a better case analysis either — `extraneous` and every
`EntryPolicy` were REMOVED from the check, so for each traversed component the
decision reads only the SOURCE manifest's kind and the DESTINATION observation's
kind (symlink in either view → refuse; both present with different kinds → refuse;
destination-only → refuse; source-only or both absent or both the same
non-symlink kind → permit). The class is now unrepresentable rather than handled:
there is no plan in scope to be wrong about. The COST is stated with its number —
this refuses a destination-only component under BOTH `Extraneous` values (2 of 2),
including the 3 `Keep` cases the previous rule permitted and where
`remove_extraneous` is never even called — and it is the sanctioned direction: the
fold is a denial tool, never a permission tool.

**Two findings were invisible to both reviewers and appeared only because a fix
was done as a CONSTRAINT rather than as a patch.** Typing `PreflightKind` exposed
that `a_destination_ownership_token_is_bound_to_its_run` matched
`"destination ownership was taken for"` — an opening that BOTH the root-mismatch
and the run-binding refusals share — so it had been asserting the ROOT refusal
while claiming to assert the run binding; it now asserts `RemoteRootMismatch` and
a new test covers `RunBindingMismatch`. And guarding the sidecar helper turned
"it is missing from a list" into "it really creates `operation.lock`", a plain
gap in the one guarded funnel. Neither was reported; both came from doing the
constraint instead of documenting the gap.

**A correction is not a rewrite.** Fixing finding 1's justification produced six
corrected comments and a list of eleven comments VERIFIED as already accurate and
left alone — a refusal whose recorded reason names a mechanism that cannot run
(here: attributing a `Keep`-policy refusal to a `Delete` decision, when
`remove_extraneous` is not called under `Keep` at all) is the axis-A defect the
whole review keeps finding, and a correct behaviour with a false reason is a claim
the next reader will act on.

**The API change owed the consumer.** Typing `PreflightKind` made
`Error::Preflight(String)` a struct variant, which broke `~/code/deploy`'s own
error bridge (its `S::Preflight(message)` arm). The fix landed in the same round
(`deploy` `xlkvyqnomlxp`), because a public-API change justified by a constraint
is only justified if the consumer still compiles.

Residuals stated, not hidden: the `std::fs` audit now PARSES the crate's source
(round 3 replaced the hand-rolled text matching with `syn`), so what remains
outside it is named AT the audit — a call inside a MACRO or an `include!`d file
from outside the package, a value carried across a variable through a function
pointer or `dyn` dispatch, a raw `extern "C" { fn unlinkat(...); }` declaration
naming neither `libc` nor `std::fs`, and inode-preserving mutations. The sidecar
helper's raw `base: &Path` still follows pre-existing intermediate symlinks
(bounded by caller trust in `base` and by the spelling's lexical confinement,
now that the reserved-name hole is closed).

## The adversarial review (round 3)

Six findings, all fixed. Three of them are about ONE thing — the `std::fs` audit's
coverage versus its claim — and one of those is a claim I wrote in round 2.

| # | finding | axis | fixed by |
|---|---|---|---|
| 1 | The audit was a TEXT scanner losing to Rust, in three ways, each proven GREEN end-to-end with a real production module compiled into the library (macOS and Linux): a CROSS-FILE module alias (`pub(crate) use std::fs as hidden_fs;` in module A, called as `crate::a::hidden_fs::remove_file` in B — the scanner resolved `use` items per FILE); a RAW IDENTIFIER (`use std::fs as r#fx;` + `r#fx::remove_file` — names were matched as byte strings, so `r#fx != fx`); and exemption by path POSITION (`src/probe/tests.rs`, declared `mod tests;` with no `#[cfg]`, was treated as test-only and could hold a raw `libc::unlinkat`). | H | `pmxknosuoqzn` |
| 2 | Manifest paths were converted to host paths with the HOST path model, so on Windows the two distinct manifest entries `a\b` and `a/b` address ONE file — while the crate's own model holds both at once (`canonicalize_tree` yields `["a\\b", "a/b"]`), and its docs called the `/`-only wire rule "exactly the wire analogue of requiring every OS path `Component` to be `Normal`", which is false on Windows. | E | `vxpuyqzwuvpk` |
| 3 | "`DestinationTree` cannot be used as a SOURCE manifest" was broader than the compile-checked property: `TreeMetadata`'s fields are `pub`, so a caller can REBUILD one from the destination's public accessors, and `verify_tree_metadata` then accepts it. | A | `okwtkumrzkll` |
| 4 | Constraint #4's sentence "each enum carries an explicit `Unclassified` fallback" and its `*_reason()` convention are false for `ReservedKind` (a closed set of three, accessor `reserved_kind()`). Written this round while completing the row. | A | `xqtovuozkyst` |
| 5 | The composed mint's endpoint guard (`lock_with_in_root_lock`) had NO test — the round-3 review REMOVED the call and the whole suite stayed green — and the round-2 sentence claiming "the new test fails when the three calls are removed" was therefore false for one of the three. | H, A | `okwtkumrzkll` (test), `ukpvrzlplzvt` (doc) |
| 6 | The "ONE tolerated mutation" COUNT survived in four more places (`src/atomic/unix.rs` twice, `src/atomic/windows.rs` twice, and the constraint-8 bullet) after being refuted in round 1, and constraint #1's enumeration was missing a THIRD member, `lock::AdministrativeRecoveryGuard::acquire` — a public call that creates or truncates a lock record at a caller-supplied path. | M, A | `xqtovuozkyst`, `okwtkumrzkll` |

**Finding 1 is the round's lesson: three rounds of adversarial review each found a
NEW route through the same audit, so the fix was to change the MECHANISM rather
than to close the route.** The audit now PARSES the crate's source with `syn`:
`use` trees are expanded to leaves (raw identifiers normalised, inline `mod`
blocks advancing the module path, block-local `use` reached), a crate-wide alias
table is built to a FIXPOINT so resolution is transitive and independent of `use`
order, and the pin counts RESOLVED production calls instead of byte-matching the
canonical spelling (a superset of the old count, so "adding a production call
changes a number" still holds; no pinned number changed). Exemption under `src/`
is now derived ONLY from `cfg` gating, and the positional arm is restricted to the
crate-root `tests`/`benches`/`examples` DIRECTORIES. The claims that could not
survive were removed rather than reworded: the audit no longer says it refuses
"every IMPORT route", it says which routes it resolves and names its residue (a
call inside a macro or an `include!`d file from outside the package, a value
carried through a function pointer or `dyn` dispatch, a raw `extern "C"`
declaration, inode-preserving mutations). A text scanner cannot win against a
language with aliases, raw identifiers, cross-module re-exports and macros; a
parser cannot either, but it retires the whole SPELLING class instead of one
spelling at a time, and what is left is a short list of mechanisms rather than an
open-ended "any spelling we did not think of".

**Finding 2 shows the same "wrong population" mistake in the platform axis: the
wire model was host-independent but the CONVERSION to host paths was not.** The
fix routes every such conversion through one authority,
`RootedRelativePath::from_manifest(&str)`, which splits on `/` and requires each
segment to be exactly one host `Component::Normal` (17 sites: the address
dispatch, the parent/ancestor walks, three depth counts, `file_name`,
`strip_prefix`, and `reserved.rs`'s predicates). Unix is unchanged (a `\` is an
ordinary byte, one component); on Windows a segment the host cannot name is
REFUSED with a typed error instead of being silently split, and the flip of the
old Windows test records that its acceptance WAS the defect. The symlink TARGET
is deliberately NOT converted: it is link DATA the kernel dereferences with the
host's own model.

**Two of the six findings are sentences I wrote in round 2**, in the very
paragraphs recording that a constraint had been marked done while incomplete:
the `Unclassified`/`*_reason()` generalisation, and a coverage claim asserting
that ONE test proved three call sites when the third had no test at all. The
second is worth stating as a rule: **a coverage claim spanning several call sites
must name a test for EACH**, because "the new test fails when the calls are
removed" is exactly the kind of sentence that reads as verified and is not — the
round-3 reviewer removed one call and the entire suite stayed green.

**Finding 6 is the third recurrence of one error: a COUNT standing in for a
RULE.** Round 1 refuted "ONE tolerated exception"; the enumeration that replaced
it then missed the sidecar helper (round 2) and now `AdministrativeRecoveryGuard::acquire`
(round 3). A count is not a rule, and a list is not a population: only an
enumeration paired with the GENERAL rule it exemplifies survives contact with a
reviewer who greps for the next member.

Note on the LOG's own order: this file's round sections were briefly out of order
(round 3 sat before round 2) because one section was appended against the wrong
anchor. A log is a document like any other, and its order is a claim about which
round came first. It is fixed; the sections run 1, 2, 3, 4.

## The adversarial review (round 4)

Eight findings, all fixed — and six of them attack the SAME mechanism, which is
what made this the round that changed the mechanism rather than patching it again.

| # | finding | axis | fixed by |
|---|---|---|---|
| 1 | The `std::fs` audit, now parsing, was STILL a detector losing to Rust. Five routes, each proven GREEN end-to-end with real production code compiled into the library (macOS and Linux): a parenthesized callee `(std::fs::remove_file)(p)` and a referenced one `(&std::fs::rename)(a,b)` (canonical spellings, so the route scanner deliberately ignored them while the counter counted only `Expr::Path` callees); a cross-file alias reached through `#[path = "x.rs"] mod m;` (the alias table was keyed by a module path derived from the FILE's location); a glob over a local re-exporting module (`use crate::alias_a::*;` then `hidden_fs::remove_file`); a raw-identifier `libc::r#rmdir` INSIDE the funnel (keyed as `libc::r`, so the funnel's own pinned count saw no change); and a module declared BOTH `#[cfg(test)] mod x;` and `#[cfg(not(test))] mod x;`, which is compiled as production while the exemption (any test-implying declaration) skipped it. | H | `yuyktpzxplkm`, `ztpnpxuyuump` |
| 2 | The claim that the parsed count was a "SUPERSET of the old byte count" was FALSE: the byte scan counted a `macro_rules!` body containing `std::fs::remove_file($p)`, which a parse of EXPRESSIONS cannot see, so the replacement had quietly LOST coverage while claiming a gain. | A, H | `ztpnpxuyuump` |
| 3 | `platform::chmod`'s own doc called itself "the crate's ONE mode-bit authority ... the funnel rule keeps mode changes on this one entry point", while `src/atomic/unix.rs` called `std::fs::set_permissions` directly at five production sites. | A | `vkmwrqoqnzlt` |
| 4 | `src/reserved.rs` listed the local transport's staged-publish rename/remove/create as "remaining unguarded `std::fs` mutations" — they are capability-gated workers that TAKE a `GuardedRel`, i.e. the funnel's own code. | A, M | `vkmwrqoqnzlt` |
| 5 | Constraint #1's enumeration missed a FOURTH member, `sync::Residue::discard`. | M | the round-4 docs commits |
| 6 | Two cross-repo citations had drifted under the claim: deploy's `store::atomic::windows` (cited for the `write_atomic_replace` restoration) no longer exists at deploy's HEAD, and deploy's transport trait — cited as DECLARING `exists` as a required method — is now itself a re-export of the crate's. The substantive evidence (a consumer's call shapes) still held; the citations did not. | A, M | the round-4 docs commits |
| 7 | Axis J quoted bare counts (191 items crate-wide, 11 in `atomic`) with no counting rule; a reviewer reproducing under a STATED but different rule got 173 and 37. Both instruments agree on the property (0 unix-only, 0 windows-only) and disagree on a number that was never the claim. | A | the round-4 docs commits |
| 8 | README's "an audit's shape is part of its guarantee" bullet said a file gated `#[cfg(all(test, unix))] mod x;` "reads as production code and trips both pins" — false, because the crate's own `cfg_implies_test` treats `all(test, …)` as test-implying and exempts the file. | A | the round-4 docs commits |

**Finding 1 is why the ENFORCEMENT moved.** Rounds 2, 3 and 4 each found new
syntactic routes through a source-inspecting detector, and each fix made the
detector cleverer while the claim it had to support ("SPELLING is not a variable",
"refuses every IMPORT route") got harder to keep true. The resolution was to stop
asking one mechanism to do two jobs:

* **Completeness** — "no mutation outside the funnel, whatever the spelling" — is
  now a **resolved-symbol deny** (`clippy.toml` + crate-root
  `#![deny(clippy::disallowed_methods)]`). The compiler resolves every spelling to
  one symbol, so the shapes above are measured IMMUNE rather than enumerated
  further, and the allow list (three funnel modules, two capability-gated workers,
  `platform::chmod`, and one reviewed exception for the ssh hostkey cache) is the
  operative definition of the funnel's membership. It runs under `cargo clippy`,
  which is why that command is part of the gate and `cargo test` alone is not
  enough.
* **The funnel's own changes** are the audits' remaining job, and the one a lint
  structurally cannot do: inside the allowed modules the deny is blind, so
  `std_fs_name_mutation_counts_are_pinned` and
  `no_libc_reference_outside_the_funnel` are what notice a new or changed call
  there, under `cargo test`, i.e. always.

Neither device is claimed to cover the other, and each says so in its own doc.
This is the round where the loop stopped making a detector smarter and changed
WHAT enforces the rule — the same move as the earlier `Sanction`,
`DestinationOwnership` and plan-free-containment changes, applied to the crate's
own meta-rule.

**Two more instances of "a count, a list, or a number standing in for a rule":**
the enumeration gained a fourth missing member (`Residue::discard`) — the third
recurrence of the count→list→list error — and axis J's counts were unreproducible
because the counting RULE was never stated, which the crate's own rule ("a claim
is a measurement or it is a label") forbids. The numbers are gone; the property,
independently reproduced by two reviewers, stays.

**And the drift class reached across repositories.** Two citations of `deploy`
pointed at code deleted or replaced there since the claim was written. Axis A's
rule about citations applies to cross-repo citations too, and had not been
applied: a citation is a claim about a REVISION, and this crate cannot see the
revisions that drift under it.

## The adversarial review (round 5)

Five findings, all fixed. This round's theme is the one the log had already named
twice: **a LIST standing in for a RULE** — but this time the lists were the
enforcement artifacts themselves.

| # | finding | axis | fixed by |
|---|---|---|---|
| 1 | The completeness device denied `libc::mkdir`/`mkdirat`/`symlink`/`symlinkat` — i.e. it ASSERTS that creating a directory or symlink at a name is funnel-owned — while the idiomatic std wrappers for exactly those operations (`std::fs::create_dir`, `std::fs::create_dir_all`, `std::os::unix::fs::symlink`, `std::os::windows::fs::symlink_file`/`symlink_dir`) were in NEITHER device. Reproduced by both reviewers on both platforms: a new production module calling them left `cargo clippy -D warnings` at 0 and both audits passing, where the funnel's own `create_dir_fd`/`symlink_fd` refuse the reserved spellings those wrappers create happily. | H, M | `lppvstnuywvx` |
| 2 | The funnel-side pin covered a 14-name `MUTATING_LIBC_SYSCALLS` list, so `libc::mknod`, `mkfifo`, `renameat2`, `mknodat`, `mkfifoat`, `fchmod`, `fchmodat`, `remove` or a `syscall(SYS_*)` inserted INSIDE the funnel — the one place the pin's own doc says it is watching — changed no count and left every gate green. | H | `lppvstnuywvx` |
| 3 | Constraint #7 and axis D still asserted that a destination observation "cannot be fed to `verify_tree_metadata`" and "cannot be used as a SOURCE manifest" — refuted in round 3, admitted in `src/manifest/mod.rs`'s own doc, and left standing in BOTH `.md` copies because round 3's fix touched only `src/`. | A | the round-5 docs commit |
| 4 | The docs mis-filed the operative definition (README called "`clippy.toml`'s allow list" the funnel's membership; that file holds only the DENY side and has no allow list), and constraint #1's enumeration had missed a FIFTH member, `transport::ssh::hostkey::pin_known_hosts`'s cache-file removal. | A, M | the round-5 docs commit |
| 5 | `platform::chmod`'s own doc claimed to be "the crate's ONE mode-bit authority ... that single `std::fs::set_permissions` call", but the FD-bound METHOD form `std::fs::File::set_permissions` is a different resolved symbol with production sites of its own. | A | `lppvstnuywvx` |

**The remedy is the same one the last round used on the mechanism, applied to the
lists: DERIVE them, and prove the derivation can fail.** The deny list can no
longer drift from the code, because
`every_mutation_symbol_the_funnel_uses_is_denied_crate_wide` reads `clippy.toml`
and scans the funnel's resolved symbol surface, then names any symbol the funnel
USES that the deny list does not cover (measured: deleting `create_dir_all` from
`clippy.toml` makes it fail, naming the hole). The funnel-side libc pin is no
longer a 14-name list: it pins the funnel's WHOLE per-module `libc::<symbol>`
reference surface (38 entries), so a new syscall there changes a count (measured:
planting `libc::mknodat` in the funnel now fails the pin). Two consequences the
agent reported rather than hid: `libc::open`/`openat` had to be denied too, because
the funnel's `O_CREAT` open CAN adopt a name — the consistency property forced it;
and the creation wrappers needed item-level allows at eight legitimate production
sites, so the allow SET — not "the funnel modules" — is what the docs must name.

**Two copies of a claim, one fix, one round of lag.** Round 3 narrowed the
`DestinationTree` direction claim in `src/manifest/mod.rs` and left it standing in
`API-CONSTRAINTS.md` and axis D; round 5 found the survivors. The lesson is not
"be careful" — it is that a fix which narrows a CLAIM must cover every copy of it,
because the crate's docs and its code are one artifact, and the review reads the
docs.

**And a sixth enumeration omission would be the signal to abandon the form.**
Constraint #1's list has now been refuted five times (the count, the sidecar
helper, `AdministrativeRecoveryGuard::acquire`, `Residue::discard`, the ssh hostkey
cache). Each fix added a member. The next one should not: the population is
derivable from the public surface, so it should be DERIVED and asserted against
the prose, exactly as the symbol set now is — the rule is cheap to check and the
list is not.

## The adversarial review (round 6)

Four findings, all fixed — and the P1 is the fifth round in a row in which a class
declared CLOSED had a live instance the closure did not cover.

| # | finding | axis | fixed by |
|---|---|---|---|
| 1 | The completeness device denied the free functions and the round-5 creation wrappers, but NOT the name-ADOPTING inherent and builder forms: `std::fs::File::create`, `File::create_new`, `DirBuilder::create`, `std::fs::copy` (its destination), and `OpenOptions::new().create(true)/.create_new(true)….open(p)` — where the BUILDER FLAG, not the call, decides whether a name is adopted. A new production module could therefore adopt `operation.lock` with every gate green, while the crate's own create-or-truncate primitive refuses exactly that spelling. **And the shape was already shipped:** `src/lock/unix.rs` and its Windows twin both adopt the lock-record name through `OpenOptions…create(true)…open()` with no allow and no deny — so round 5's meta-claim ("the deny list can no longer drift from the code") was false when written. The derived test could not see it either, because its own helper excluded inherent-type paths while its doc promised "the funnel's whole resolved call surface". | H, M | `opxqouknzxww` |
| 2 | The derived check visited `syn::Item` but not `ImplItem`, so a `#[allow]`-ed METHOD's un-denied call was invisible to it (the freestanding-allow control DID fail, isolating the arm). **THIS ROW WAS FALSE WHEN WRITTEN and is corrected here:** it recorded `opxqouknzxww` as the fix, but that change never contained the arm — the finding was listed as dispatched and the brief omitted it. Both round-7 reviewers re-derived and caught it (one via `jj show`, one via an A/B probe); the fix actually landed in round 7 as `rxwnmspylryu`, with a four-arm regression test (impl allow, trait allow, freestanding control, no-allow negative) and a disabled-arm failure proof. | H | `rxwnmspylryu` (round 7) |
| 3 | The completeness device was never run for the WINDOWS target: `cargo clippy --all-targets` compiles the host only and `cargo check --target …` runs no lints, so a `#[cfg(windows)]`-only module could call denied symbols with the gate green (measured: host run clean, Windows-target run red). | L | `opxqouknzxww` + the gate |
| 4 | Three prose claims had drifted from the code: the README's deny-symbol ENUMERATION (stale within the round that removed enumerations), constraint #1's justification that `Residue::recover_to` "takes no path argument at all" (it takes `target: impl AsRef<Path>` and validates it), and the README's "they resolve the enumerated import routes by PARSING the sources" — only the `std::fs` audit parses; the `libc` audit is a reference scanner that does not even resolve `use libc as c`. | A | the round-6 docs commits |

**The fix is the one the pattern demands: extend the DERIVATION, not the list.**
The deny list gained the adopting symbols (measured: each planted form went from
exit 0 to exit 101, and the read-only/truncate-only opens stayed legal — the lint
is name-based, so denying `OpenOptions::truncate` would have flagged harmless
`.truncate(false)` calls and was correctly declined). But the durable half is that
`funnel_symbol` now accepts ANY module-path depth and the visitor records
associated-function calls, builder-method chains and block-scoped `let` builders,
so the next un-denied form inside the funnel is NAMED BY A FAILING TEST rather
than found by a reviewer: three planted arms (an associated call, a builder chain,
a split `let` local) each made
`every_mutation_symbol_the_funnel_uses_is_denied_crate_wide` fail where all three
had left it green. Measuring also surfaced a false attribution — `.open(p).map_err(…)`
was being recorded as `OpenOptions::map_err` — which the fix stopped by ending the
chain-walk at the terminal call.

**A residual is stated at the test rather than left implied:** the derivation
recognises a builder only by syntactic chain or enclosing `let`; one reached through
a parameter, a field, a return value, a function pointer or a macro body is not
named THERE — the crate-wide resolve-by-type deny is what refuses those, which is
why the two devices remain complementary rather than redundant.

The gate itself changed: **two** clippy commands are now required, the host one and
the Windows-target one, because a lint that never compiles `#[cfg(windows)]` code
cannot be the completeness device for a crate that supports Windows. That is the
same lesson as axis L, one level up: "a green gate on one platform is not evidence
for another" applies to the DEVICES in the gate, not only to the code they check.

## The adversarial review (round 7)

Five findings, all fixed. The most important one is not in the code: it is that
this log ASSERTED a fix that was never written, and only independent re-derivation
caught it.

| # | finding | axis | fixed by |
|---|---|---|---|
| 1 | The derived closure was blind to `#[allow(clippy::disallowed_methods)]` on **impl and trait METHODS**, and round 6's row above recorded it as fixed by `opxqouknzxww` when no commit ever implemented it. Five production impl-method allow sites (`LocalTransport::root_dir`/`provision_layout`/`remove_file_if_inner`, `SshTransport::prepare_identity`, `LocalBackend::root_for_mutation`) were invisible, so an `#[allow]`-ed method could call a symbol neither denied nor reviewed with the closure green — while the IDENTICAL freestanding allow made it fail. Both reviewers found this independently and both verified with `jj show` that no commit adds the arm. **The defect was the orchestrator's**: the finding was listed as dispatched, the brief omitted it, and the log recorded it as done. | H, M | `rxwnmspylryu`; the round-6 row is corrected in place |
| 2 | `std::fs::write` was a LIVE un-denied name-ADOPTING symbol: a non-funnel production `std::fs::write(p, b"x")` adopted any absent name — a reserved spelling included — with the whole gate green. It sat in `FUNNEL_SYMBOLS_NOT_DENIED`, whose own criterion is "each reviewed as unable to ADOPT a name", so its own entry violated the list's rule, and the stated reason ("every funnel use targets an already-open descriptor or a path the funnel just created") was false at the one production site it excused (`write_file_fd` is a documented create-or-truncate on a caller-named path). | H, M | `rxwnmspylryu` |
| 3 | The libc "belt" — the assertion that makes a REVIEWED pin safe — was a 14-name hand list, so a reviewer could pin `libc::mkfifo` in the outside map and the audit passed, and a name-creating syscall was authorized by review. | H | `rxwnmspylryu` |
| 4 | Constraint #1's enumeration had a SIXTH omission (`Remote::provision_layout`, a public trait method whose override creates the layout tree) AND existed as TWO divergent lists, each missing members the other named — precisely the form round 5's log predicted would fail again. | A, M | `rxwnmspylryu` + one marked block |
| 5 | README claimed the two audits "run under … BOTH targets' `cargo check`"; `cargo check` compiles test targets and executes no tests. | A | the round-7 docs commit |

**A log that claims a fix nobody wrote is the worst instance of this crate's own
recurring defect.** The loop's whole premise is that a claim is a measurement; the
orchestrator's summary of "what I briefed" was treated as a measurement of "what
was implemented", with nothing between them, and it was wrong. Two independent
reviewers re-deriving the property is the only reason it surfaced — which is the
argument for the re-run step stated better than any of the previous rounds managed.
The rule adopted here: **a log cites a change id only after the orchestrator has
read that change's diff**, not the fix agent's summary of it.

**And the derivations that rounds 5 and 6 kept promising finally exist for both
lists.** The libc belt is no longer a list: it is the mutation family UNION a
default-deny of every `libc::<fn>` call the tree references, minus a reviewed
`NON_MUTATING_LIBC_CALLS`, and a test derives the 33 referenced call symbols and
fails on any unclassified one (measured: pinning `libc::mkfifo` is now refused by
the belt before the map comparison; removing `mkfifo` from the family fails the
classification test). The pair-less enumeration is ONE marker-delimited block in
`API-CONSTRAINTS.md`, checked by a test that extracts the block, resolves every
name to a real item, and derives the 41-item public raw-path surface so each item
must be either listed or exempted — 26 exemptions, each with a stated reason, and
a stale exemption fails the test. A planted new public raw-path mutator fails it,
which is the property six hand-found omissions never had.

## The adversarial review (round 8)

Eight findings, all fixed — the largest round since round 1, and the first in which
the ORCHESTRATOR's errors outnumber the code's.

| # | finding | axis | fixed by |
|---|---|---|---|
| 1 | The funnel-side count pin's symbol table (`NAME_MUTATION_SYMBOLS`) omitted the name-ADOPTING symbols the DENY list names (`File::create`, `File::create_new`, `OpenOptions::create`/`create_new`, `DirBuilder::create`, `std::fs::copy`, `std::fs::write`). Inside a funnel module the clippy deny is allowed, so the pin is the only device left — and it did not count them: planting `std::fs::write` or `File::create` in `src/atomic/mod.rs` left `cargo test`, host clippy AND Windows clippy green, while the identical call outside the funnel was exit 101. | M, H | `oklvktzqonoy` |
| 2 | `OpenOptions::custom_flags` forwards ARBITRARY bits to `open(2)`, so `O_CREAT` through it adopts a name — and `FUNNEL_SYMBOLS_NOT_DENIED` excused it with the comment "these are not and **cannot adopt**". A production call adopted any absent name with clippy and all four derived audits green. | M, H | `oklvktzqonoy` |
| 3 | `std::os::unix::net::{UnixListener,UnixDatagram}::bind` creates a directory entry and was in NO device (not denied, not counted, and outside `funnel_symbol`'s domain), so a production module could bind a socket at `state/operation.lock` with every gate green. | M | `oklvktzqonoy` |
| 4 | The pair-less derivation could not see a `&str`-spelled public raw-path mutator (`pub fn f(name: &str)` then `Path::new(name)`), so "a SEVENTH omission is a failing test" was false for that class. | A, H | `oklvktzqonoy`, class STATED |
| 5 | `Residue::discard` was NOT serialized by the destination operation lock, unlike its sibling `recover_to`: a cooperating second process could `detect` a live run's claim-aside and `discard` it, destroying the run's only copy of the pre-replace original before its rollback. | C | `vnxwyvyvtlxw` |
| 6 | The real-`sshd` harness had a PORT TOCTOU: `free_port` released the port before `sshd` bound it, `wait_for_port` accepted a FOREIGN listener, and the client then failed to authenticate against the wrong `AuthorizedKeysFile`. **The Linux gate was nondeterministic — measured 2 failures in 240 runs (0.83%) under load** — so every "the Linux gate is green" claim in this log is weaker than it read. | L | `vnxwyvyvtlxw` |
| 7 | `atomic`'s doc claimed a failed replace "leaves the directory exactly as it found it"; when the target's PARENT CHAIN was missing, the durable parents created before the failure remain. The existing test pre-created the destination directory, so it could not express the case. | A | `vnxwyvyvtlxw` |
| 8 | THIS LOG's round-1 paragraph said "Eight of the twelve are axis A" while its own table gave six (3, 7, 8, 9, 10, 12), and the follow-up sentence named rows 5 and 6, which are `M` and `H`. | A | the round-8 docs commits |

**The strongest medicine was applied to the last hand list.** The pin's symbol
set is no longer a const: it is DERIVED from `clippy.toml`'s deny entries (with a
synthetic builder mapping so a denied trait path and the syntax a caller writes
name the same key), so the deny and the pin cannot drift apart again — the exact
failure of finding 1, where round 7 added the adopting symbols to one device and
not the other. Measured: all seven adopting symbols planted in a funnel module now
change a pinned count, with a read-only control unchanged.

**And a gate that can fail for reasons unrelated to the code is a finding about
the ARTIFACT, not about the test.** Finding 6 was measured before and after
(2/240 → 0/240 under identical load), the harness now serializes port-pick/spawn/
verify, polls its own child, and verifies the presented host key — so a taken port
now says "taken by a NON-SSH listener" or "a FOREIGN listener presenting a
different ed25519 key" instead of surfacing as an authentication failure against
the wrong sshd.

**The orchestrator's own defect rate is now the story.** Finding 8 and the two
counts corrected in `b5a6f2d0` ("nine" Windows warnings for thirteen; "25
exemptions" for 26) are FOUR wrong numbers from me in three rounds, all of the same
shape: a figure transcribed from an agent's report instead of measured — the habit
this log exists to diagnose. They were caught only because independent reviewers
re-derive instead of trusting, which is the argument for the re-run step restated
once more. The rule is now mechanical: a count, and any claim that a change closed
a finding, is measured or diffed before it enters a document — and where a number
was already wrong, the document says so rather than presenting a silently edited
digit.

## The adversarial review (round 9)

Eight findings, all fixed. One is a P1 runtime defect that only ONE of the two
reviewers found — and the other reviewer reported, in writing, that it could not
break the behaviour at all.

| # | finding | axis | fixed by |
|---|---|---|---|
| 1 | The ownership token was not bound to the remote's LOCALNESS. `Prepared` recorded only `remote_identity: Option<String>`, so `None == None` passed, and the localness derivation hardcoded `true` for `Pull`. A token minted against a LOCAL source was therefore accepted for a run whose live source was a different NON-LOCAL transport stating no identity: source A's plan was applied against source B and B's bytes reached the destination — with a digest failure afterwards when the content differed, and **silently (`Ok`) when it matched**. This is round 1/2's "apply R1's plan to R2" re-opened through the one `None`-identity path, the documented third-party extension point. **Only B found it; A explicitly reported the behaviour held.** | D | `voqovystpzns` |
| 2 | Round 8's "derived from `clippy.toml`" reader was not a TOML reader: it accepted only a line beginning exactly `{ path = "`, so a valid TOML reformatting made the DENY and the PIN disagree — clippy refused a non-funnel call while the pin silently lost the symbol (round 8's exact failure mode, re-introduced by round 8's own fix). | M, H | `xurptntxropu` |
| 3 | The libc belt could be silently disarmed by moving one symbol from `MUTATING_LIBC_SYSCALLS` into the reviewed `NON_MUTATING_LIBC_CALLS`: the classification test checked only UNION membership, so the device cited to make a reviewed pin safe could not fail for the property it is quoted to protect (repro: `unlinkat` misfiled + a pinned non-funnel `libc::unlinkat` → all green). | H | `xurptntxropu` |
| 4 | The funnel closure's NAMED RESIDUAL claimed the crate-wide deny "still refuses the call everywhere" — false inside a funnel region, the one place the closure is the device of record. | A | `xurptntxropu` |
| 5 | A test arm was a TAUTOLOGY: it asserted the symbol table contained what the function that BUILT the table had put there, so it could not fail. It is now a genuine cross-check against an independent raw read of `clippy.toml`. | H | `xurptntxropu` |
| 6 | `clippy.toml`'s allow-site list — designated AUTHORITATIVE by `reserved.rs` — omitted a production item allow (`transport::open_verify_local`) and its "every OTHER is test-only" sentence was false. | A | `xurptntxropu` |
| 7 | The pair-less derivation's stated class boundary omitted `use … as` ALIASES of `Path`/`PathBuf`, so an alias-spelled public raw-path mutator was neither in the stated in-class set nor the out-of-class set. | A | `xurptntxropu` |
| 8 | Two "the TWO `GuardedRel` constructors" claims survived round 1's fix for that EXACT error (the code has three, and the third — `new_for_residue`, which waives the residue authority — is the one the security argument turns on). | A | `xurptntxropu` |

**One class, five instances: a device's CLAIM outran its MECHANISM.** A reader that
does not parse; a belt disarmable by a one-line misfiling; a residual whose
mitigation is false exactly where it counts; a boundary that omits aliases; a test
arm that compares a value to its own producer. Findings 2–6 are not five unrelated
slips — they are the same overreach five earlier rounds found in the CRATE's
guarantees, now located inside the enforcement apparatus itself, and the remedy is
the same one that worked on the mechanism: **make each device falsifiable**. The
libc belt gained an oracle that does not consult the review list and refuses a
known name mutator pinned outside the funnel (proved failing under the reviewer's
exact misfiling); the pin's reader now parses TOML and cross-checks its output
against an independent raw scan (proved failing when a path-shaped string is not
classified); the tautological arm now compares against that same raw read (proved
failing when the table drops an entry); the boundary test pins the alias case in
BOTH directions.

**And the round's most important fact is an asymmetry.** Reviewer A could not break
the crate's behaviour and said so; reviewer B found a P1 in the ownership binding.
A single-reviewer round would have advanced believing the runtime was clean, and
the P1 would have survived into the next round or beyond — which is the argument
for two INDEPENDENT reviewers stated as evidence rather than as doctrine.

**My own figures are now handled by construction, not by care.** The Windows
config-warning count had been written as 9, then 13, then 16 across three rounds,
every figure transcribed from a report: the README now states the COMMAND that
produces it and says the number moves. The same treatment applies to the six
hand-found omission rounds, now attributed correctly (1, 2, 3, 4, 5, 7 — round 6
corrected a REASON and round 8 found a class boundary, neither added a member).

## The adversarial review (round 10)

Seven findings, all fixed, plus one NON-finding that was investigated to a measured
conclusion. The P1 is once again in the audit's own module.

| # | finding | axis | fixed by |
|---|---|---|---|
| 1 | **The funnel region was derived PER-FILE, not by Rust attribute inheritance.** `FunnelSymbols::visit_file` read only the file's own `#![allow]`, so `src/atomic/guard.rs` — a CHILD of the allow-bearing `src/atomic/mod.rs`, and therefore inside the funnel — was treated as OUTSIDE by the closure. Combined with `builder_type` not recognising the stable synonym `File::options()`, a production adopting call in `guard.rs` was invisible to the closure AND to the count pin, while clippy is inherited-allowed there: the reviewer measured closure **ok**, pin **ok**, source audit **ok**, host clippy **0**, full suite **green** — where the identical call in `src/error.rs` is a hard clippy error. The audit's own module was the last place the closure looked the other way, and it took TWO mechanisms failing at once. | M, H | `mzvotpqsozup` |
| 2 | The libc belt's round-9 "oracle" covered **7 of ~50** family members, so the belt stayed silently disarmable for the rest: moving `chmod`/`chown`/`mknodat`/… into `NON_MUTATING_LIBC_CALLS` left all six libc tests green, and for symbols absent from `clippy.toml` (e.g. `libc::chmod`) the belt is the SOLE device. Found independently by both reviewers with different symbols. | H | `mzvotpqsozup` |
| 3 | `Prepared::matches` step (5) was DEAD CODE: after steps (1)–(4) passed, its condition was algebraically unenterable, so the "derived destination shape" axis the docs presented could never refuse anything. | H | `zmlnqmkrkunq` |
| 4 | The DIRECTION axis had no failing test: removing just that sub-clause left the whole suite green (the derived-shape check only catches a direction swap when the roots differ). | H | `zmlnqmkrkunq` |
| 5 | The pair-less derivation was blind to `macro_rules!`-generated public fns, so "a SEVENTH omission is a failing test" was false for that class, which the stated boundary did not mention. | A | `mzvotpqsozup` |
| 6 | The pair-less boundary omitted CONTAINER types (`&[PathBuf]`, tuples, `Result<PathBuf>`) — neither in the stated in-class set nor the out-of-class set. | A | `mzvotpqsozup` |
| 7 | Constraint #3's "Verified: all three views build `SymlinkContainmentIndex` from `live_entry_kinds`" named one constructor for all three; the two canonicalizers use `from_entries` (measured; the substantive property holds). | A | the round-10 docs commit |

**Every fix again made the device falsifiable rather than merely correct.** The belt
oracle is now a 54-entry independent literal with SET EQUALITY against the family,
a disjointness assertion, and an ANTI-CIRCULAR half (a default-deny-only belt), with
the misfiling proved for two different symbols; the funnel region is derived by
transitive attribute reach and both P1 halves have a pre/post measurement; the dead
check is deleted and every surviving token axis has a remove-and-test entry —
EXCEPT ONE, which this paragraph must not hide: the LOCALNESS axis is compared
unconditionally in code, but only its PULL direction is pinned by a test (mint
against a local source, run against a non-local one that states no identity). An
earlier version of this sentence claimed a table of axis-to-test failures lived in
this log; a second version claimed that table and a PUSH twin lived "in the
round-11 entry below" — a section that had not been written, asserting a test that
did not exist. Both were claims about content that was not there, which is the
defect class this log exists to catch, and the second was written while correcting
the first. The table and the PUSH twin are written WITH round 11's fixes; until they
land, this sentence states the GAP rather than the artifact. The direction
axis has the test that notices its removal; the macro class is STATED with a
tripwire (expansion is unsound — metavariables and repetitions), and the container
class is extended with the boundary test pinned in both directions.

**The non-finding became a finding by being measured.** Reviewer B reported one
unreproduced failure in
transport::ssh::runner::runner_property_tests::every_outcome_leaves_zero_live_waiters_and_a_reaped_child
and explicitly did NOT assert it. Chasing it produced: the production invariant IS
guaranteed (all five return paths in `SshRunner::run` join the wait thread, cited at
file:line); the flake is REAL (macOS 12/1000 and Linux 5/1000 at 40-way concurrency,
always the same case and assertion); and the cause is the TEST's premise — its
`AfterReap` placement ordered the post-reap block against the deadline but not the
reap itself, so under load the runner CORRECTLY took its documented kill path and
returned `Timeout` while the test had asserted a scheduling accident as a premise.
The fix is test-only and STRENGTHENS the property: the fake arms `reaped` before
`spawn` returns for those cases, and a green run now additionally asserts NO kill
occurred, so it proves the reaped path was taken rather than that a race was won.
Measured after: 0/1000 on Linux under the same 40-way load (0/3680 macOS).

**An honest note about what these rounds are converging on.** Rounds 8–10 each
found defects in the DEVICES rather than in what the crate does, and the last three
have all been the same shape: a claim whose mechanism cannot actually fail for it —
an oracle covering 13% of what it certifies, a check that cannot be entered, an axis
no test notices. The runtime itself has now survived several rounds of attack,
including a reviewer's algebraic audit of every `matches` axis; that is evidence,
but it is evidence about the SHAPE of the remaining risk (self-audit overreach), not
a claim that no defect remains.

## The adversarial review (round 11)

Ten findings, all fixed — and two of them are P1s in devices the previous round had
just "made falsifiable", one of which is the orchestrator's own correction. (This
header said "Nine" while the table below lists ten: row 2 IS a finding — the
orchestrator's fix asserting an artifact that did not exist — and the count had been
written from memory rather than from the table, the same defect round 8 fixed in the
round-1 paragraph. Totals: rounds 1–10 are 71 findings, round 11 is ten, so rounds
1–11 are 81.)

| # | finding | axis | fixed by |
|---|---|---|---|
| 1 | The belt "oracle" was a CONSISTENCY CHECK BETWEEN TWO CO-EDITABLE LITERALS: `KNOWN_NAME_MUTATORS` and `MUTATING_LIBC_SYSCALLS` are both hand-written in the same file and every arm compared them to each other (arm 5 anchored only `chmod`/`renameat2`), so deleting a member from BOTH and adding it to the review list disarmed the belt — 52 of the 54 members — with all six libc tests green. | H | `mywyuvnnmsqx` |
| 2 | THE ORCHESTRATOR'S OWN FIX: the round-10 paragraph was corrected to say the axis-to-test table and a "PUSH twin" were "recorded in the round-11 entry below" — a section that had not been written, asserting a test that did not exist. An earlier version of the same sentence had claimed that table lived in this log at all. Both were claims about content that was not there; the second was written while correcting the first. | A | `kutvqmmryruq`, then this entry |
| 3 | The round-10 macro tripwire was evaded by a GENERIC `pub fn` (it expected `(` immediately after the name; a generic writes `<`), so a real public raw-path mutator shipped with the tripwire and the derivation both green. | H | `mywyuvnnmsqx` |
| 4 | A local `type` ALIAS of the builder type (`type ZZBuilder = std::fs::OpenOptions`) left the funnel closure AND the count pin blind inside a funnel module — a `use … as` alias was resolved, a `type` alias was not. | H | `mywyuvnnmsqx` |
| 5 | `platform::symlink` — a public, path-based name CREATOR — never ran the reserved-name guard while its `_fd` twin did: a consumer could create `operation.lock` and `.sync-aside.…` names through it. Constraint #1's "Removes from the impl" column claims that class was removed. | A, M | `oxmkqtnktvrz` |
| 6 | The ownership LOCALNESS axis was compared for both directions in code but test-pinned only for `Pull`: gating step 4 on `Pull` left the entire suite green. Corroborated by both reviewers. | H | `oxmkqtnktvrz` |
| 7 | The round-10 transitive funnel-region derivation had NO failing test: replacing the ancestor-or-self test with an exact-module match left all 690 tests green. | H | `mywyuvnnmsqx` |
| 8 | The pair-less boundary named `use … as` aliases but omitted local `type` aliases of `Path`/`PathBuf`. | A | `mywyuvnnmsqx` |
| 9 | The belt's literal boundary was UNSTATED: a real name mutator in NEITHER list (`lutimes`, `futimes`, `fchmodat2`, `mq_open`, `sem_open`, `shm_open`, …) could be moved into the review list with nothing failing. | H | `mywyuvnnmsqx` |
| 10 | Three claims of the orchestrator's: the mode-delta prose said "six" spellings where its own table lists EIGHT; the round-10 sentence claimed a table that existed only in a fix agent's report; and the README stated an `iff` between `valid_name` and `is_unaddressable_name` that the identifier rule does not satisfy (`Ünïcode` is refused and is not unaddressable). | A | the round-11 docs commits |

**The belt's oracle is now anchored OUTSIDE the co-editable pair.** A third table,
`INDEPENDENT_KNOWN_NAME_MUTATORS` — 71 entries, each with a CLASS and a REASON,
covering the POSIX/Linux/BSD name-mutation surface and including **17 symbols that
were in neither round-10 list** — is what the oracle iterates: every anchor member
must be refused by the derived belt AND by the default-deny belt alone, must not
appear in the review list, and the non-vacuity count must be real. Co-editing both
family literals for `chown` now FAILS ("these ANCHOR name mutators are absent from
`MUTATING_LIBC_SYSCALLS`"), and moving the new control `lutimes` into the review
list FAILS too. The anchor is itself a list, so its own boundary is STATED (a raw
syscall by number or a local `extern "C"` declaration is not keyable) rather than
implied.

**`platform::symlink` is guarded, and the choice was made on CONSUMER evidence.**
The brief's preferred option (demote the function to `pub(crate)`) was REFUTED by
measurement: `~/code/deploy` re-exports the whole module (`pub(crate) use
storekit::platform::*;`) and calls `platform::symlink` in three places, so demotion
would break a real consumer. The fix instead runs `refuse_reserved_mutation` at the
public entry (returning the crate's `Result`, the same type as its `_fd` twin) and
moves the unguarded std calls into a NAMED `pub(crate) symlink_verbatim`, which is
what `copy_tree_verbatim` uses — with that primitive's reserved-name carrying left
byte-identical and its test STRENGTHENED to include a reserved-named symlink.

**The axis-to-test table this log twice claimed, now real.** Each comparison was
removed in a scratch copy; these are the tests that fail:

| axis | comparison removed | tests that fail |
|---|---|---|
| direction | `self.direction != direction` | `a_destination_ownership_token_is_bound_to_its_direction`, `..._refuses_a_derived_destination_shape_swap` |
| pinned local root | `normalize_root(local_root) != self.local.root_path` | `a_destination_ownership_token_is_bound_to_its_local_root` |
| remote root | `normalize_root(remote.root()) != self.remote_root` | `..._is_bound_to_its_run`, `a_remote_token_is_refused_across_roots_with_the_same_endpoint`, `a_pull_token_is_refused_against_a_different_source_root`, `the_endpoint_and_root_refusals_carry_distinct_typed_kinds` |
| endpoint identity | `remote.endpoint_identity() != self.remote_identity` | `a_remote_token_is_refused_across_endpoints_with_the_same_root`, `a_pull_token_is_refused_against_a_different_source_endpoint` |
| remote localness | `remote.is_local() != self.remote_is_local` | `a_pull_token_is_refused_when_{the_source_flips_from_local_to_non_local,equal_content_hides_the_local_to_non_local_flip}` and `a_push_token_is_refused_when_{the_destination_flips_from_local_to_non_local,equal_content_hides_the_local_to_non_local_flip}` |

The localness row is the one round 10 could not fill: it had a PULL test and no PUSH
twin, so gating that comparison on `Pull` was invisible — and the PUSH twin now
reproduces round 9's P1 in the other direction (a token minted against a LOCAL
destination replayed against a non-local one that states the same root and no
identity mutated the non-local destination, and returned `Ok` when the content
matched).

**My own documentation defects are now the largest single category in this round**
(items 2 and 10): four claims across three commits, all of a kind — an artifact I
asserted was there and was not, and a number I wrote without counting. The remedy is
in the text rather than in a resolution: the round-10 paragraph now states the
COVERAGE GAP it has (localness pinned for PULL only), and this entry carries the
filled gap. A claim about content is a measurement like any other.
