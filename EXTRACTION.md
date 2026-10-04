# storekit extraction

**Historical**: how the crate was lifted out of `~/code/deploy`. `README.md` is the
current contract, [`docs/API-CONSTRAINTS.md`](docs/API-CONSTRAINTS.md) the constraints
the public API now carries, and [`docs/CONSISTENCY.md`](docs/CONSISTENCY.md) the rules
the work earned. Where a name below has since changed, the source is authoritative.

`~/code/deploy` was the read-only source of truth. Each slice ported the named
production code **and its tests**, applied the adaptations below, and had to pass the
gate. Do not invent a new design where a faithful port exists: the value of this crate
is the semantics already encoded in the source, and those live in the doc comments and
the tests.

## Adaptations (every slice)

1. **Error type.** `crate::error::Error` is the crate's own. Every condition a caller
   must branch on is a TYPED VALUE rather than message text (see
   [`docs/API-CONSTRAINTS.md`](docs/API-CONSTRAINTS.md) #4): the variants carry
   `{ kind, message }`, the constructors are `*_kind(kind, msg)`, and the MESSAGE TEXT
   is preserved verbatim so a text-matching caller keeps working. A
   `crate::kernel::KernelError` variant does not exist here; if a ported file needs
   one, that file is domain code and must be dropped.
2. **Visibility.** An item is `pub` only if a CONSUMER needs it; `pub(crate)` is the
   default answer for anything the crate's own paths can reach. A deletion justified
   by *"the crate's own tests do not use it"* is not justified at all. The consumers
   are `~/code/deploy` and `~/code/ckpt`; the durable guard for their needs is
   `tests/consumer_fit.rs`, which fails to COMPILE if a consumer-required name is
   removed.
3. **Test helpers.** `crate::testutil::{fixture_env, fixture_tmpdir, proptest_cases,
   slow_tests_enabled}` are `crate::test_support::{...}` here. Any other
   `crate::testutil::*` use means the test is domain-bound: drop it and record the drop.
4. **No application domain.** Forbidden anywhere in this crate: `config`, `ledger`,
   `kernel`, `retention`, `deploy::*`, `remote::helper`, `remote::layout`,
   `store::local`, `identity::{ReleaseId, DeploymentId, …}`. Where a ported file depends
   on one, apply that slice's **domain cut** — the cut must preserve the source's
   behaviour with the domain value supplied by the CALLER, never deleted silently.
5. **Doc comments are part of the port.** They carry the invariants and the rationale.
   Keep them; rewrite only intra-doc links that no longer resolve.
6. **Dropped tests are reported, not stubbed.** A test needing a domain fixture is
   dropped, and the report lists `deploy-file:line` and the reason. Never replace an
   assertion with a weaker one to keep a test compiling.
7. **No new dependencies** without a reason in the report.
8. **Windows.** The `#[cfg(windows)]` modules are required and part of the gate: a
   Windows file and its TESTS must COMPILE. Gate a test that genuinely needs a unix
   primitive with `#[cfg(unix)]` (and say what it needs); never gate a production
   Windows file. The port's RUNTIME remains unverified — no Windows host has executed
   it — and the crate says so.

## Slices

| slice | source (`~/code/deploy`) | what it is |
|---|---|---|
| core | `src/{digest,platform,trace}.rs`, `src/identity/**` | digests, platform primitives, tracing, the id newtypes (`id_newtype!`, `valid_name`, `valid_hex_digest`) |
| atomic | `src/store/atomic/{mod,unix,windows}.rs` | temp + fsync + rename, and the root-confined primitives |
| root | `src/store/local/owned_root.rs` | the ownership registry |
| lock | `src/lock/{mod,unix,windows}.rs` | the `flock` record |
| transport | `src/remote/transport/**`, `runner/**`, `ssh/**` | the `Remote` trait, the local transport, ssh |
| manifest | `src/remote/canonical/mod.rs` (tree half) | the two tree walks, assembly, the digest, the wire |
| sync | *(new code)* | diff, apply, ownership, residue, the report |
| — | `src/remote/canonical/materialize.rs` | NOT ported: a config-driven mapping/template materializer, i.e. consumer DOMAIN |

The `id` newtype macro names `serde` through the crate, so a call site needs no `serde`
dependency of its own.
