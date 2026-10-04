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
`E0308` (`expected &TreeMetadata, found &DestinationTree`). The one branch
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

**Eight of the twelve are axis A**, "a doc claim ↔ the code", and they are not
eight unrelated slips: they are one habit, a claim ASSERTED in the confident
register rather than MEASURED. The sharpest instance is finding 3 — its paragraph
was titled "**The delta, measured**" and was not measured; it described a
strictness the code did not have, and the crate's own rule ("prove the delta")
would have caught it had the delta actually been produced. Findings 3, 5, 6, 7,
10 and 12 all share the shape: a sentence that reads as verified with nothing
behind it.

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
