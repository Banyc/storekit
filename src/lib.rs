//! A durable, symlink-confined local store, and the transport that moves a
//! tree of it between hosts.
//!
//! Two halves, one crate:
//!
//! * the **store substrate** — a directory tree the process owns, with
//!   descriptor-relative path resolution that refuses a symlink injected into
//!   any PARENT component and the FINAL component on the open/create-new/read
//!   paths (the atomic replace instead REPLACES the final entry and never
//!   follows it, so it cannot escape), durable atomic writes with explicit
//!   commit points, an
//!   advisory lock, and the validated-identifier machinery for the names of
//!   the things stored in it ([`atomic`], [`root`], [`lock`], [`id`]);
//! * the **transport** — the [`transport::Remote`] trait and its two
//!   realizations (an in-process [`transport::LocalTransport`] and an
//!   [`transport::SshTransport`] over `ssh`), plus the [`manifest`] machinery
//!   that describes a tree by content hash so two hosts can agree on what
//!   differs without shipping the bytes ([`sync`]).
//!
//! [`mod@env`], [`digest`], [`platform`], and [`trace`] are the small shared
//! helpers the rest of the crate is built on.
//!
//! The crate is deliberately free of any application's domain model: no
//! deployment, release, ledger, or record-book concept appears here. A store
//! is a root directory; a name is a validated string; a transfer is a
//! comparison of two manifests.
//!
//! # The name-mutation funnel: what the compiler refuses, and what is NOT promised
//!
//! THREE devices, each with a different job, and none of them a completeness proof:
//!
//! * **The resolved-symbol deny** — `#![deny(clippy::disallowed_methods)]` below,
//!   with the symbol list in `clippy.toml` — refuses a call to a LISTED symbol from
//!   any site not carrying `#[allow(clippy::disallowed_methods)]`: a funnel module
//!   with the module-level attribute, or a single reviewed function with the
//!   item-level one. Because the lint matches the RESOLVED symbol, no spelling route
//!   reaches a listed symbol — an alias, a re-export, a raw identifier, a
//!   parenthesized or referenced callee, a `macro_rules!` body, or a
//!   `#[path]`-relocated module all resolve to the same disallowed path — and
//!   `atomic::guard::tests::every_mutation_symbol_the_funnel_uses_is_denied_crate_wide`
//!   keeps the list from drifting from what the funnel itself calls. The list is
//!   deliberately WIDER than the funnel: naming a symbol the funnel does not call
//!   today is how a route it could acquire later is refused in advance.
//! * **The `libc` pin**
//!   (`atomic::guard::tests::every_production_libc_reference_is_pinned`) enumerates
//!   every production `libc` reference by file, symbol and count and asserts that the
//!   map of references NOT on the pin is EMPTY, so an unreviewed reference fails a
//!   test. It records a REVIEW; it does not prove a pinned reference harmless.
//! * **The count pins** notice a change INSIDE the funnel — where this lint is
//!   allowed and therefore blind — for every call their derivation can RESOLVE: a
//!   direct call, an inherent or builder method on a path-resolvable receiver, or a
//!   call held in an enclosing `let`. A call whose receiver arrives as a parameter, a
//!   return, a struct field or a function pointer moves no count.
//!
//! **NOT promised:** that EVERY name mutation anywhere goes through this funnel. That
//! claim quantifies over the whole language and is not certifiable — a mutating symbol
//! nobody listed, a raw `syscall(SYS_…)`, a local `extern "C"` declaration, a
//! `windows_sys` creator, a macro that emits a call, and third-party code are outside
//! every device above. Keeping the symbol set complete is a REVIEW responsibility. The
//! contract this crate DOES hold is in `README.md`, and every stated residual with its
//! reach is in `docs/CONSISTENCY.md`.
#![deny(clippy::disallowed_methods)]

pub mod atomic;
mod casefold;
pub mod digest;
pub mod env;
pub mod error;
pub mod id;
pub mod lock;
pub mod manifest;
pub mod platform;
pub mod relpath;
pub mod reserved;
pub mod root;
pub mod sync;
pub mod trace;
pub mod transport;

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(all(test, unix))]
mod deep_tree_regression;

#[cfg(all(test, unix))]
mod fifo_regression;

/// The serde crate re-exported under a hidden name so the exported
/// [`id_newtype!`] macro can name serde's traits through `$crate::__serde`.
/// This is what lets a downstream crate invoke the macro with ONLY
/// `storekit` in its `[dependencies]`: the expansion never resolves a bare
/// `serde::` path, so the consumer needs no `serde` dependency and no
/// `derive` feature of its own. Not part of the public API surface (it exists
/// solely for macro hygiene); do not depend on it directly.
#[doc(hidden)]
pub use ::serde as __serde;

pub use error::{
    Error, MaterializationKind, PreflightKind, ReservedKind, Result, StoreKind, TransportKind,
};
pub use relpath::RootedRelativePath;
pub use reserved::{
    APPLICATION_LOCK_NAME, ASIDE_PREFIX, OPERATION_LOCK_SUFFIX, RESIDUE_BELOW,
    is_application_lock_name, is_lock_record_name, is_reserved_case_alias, is_reserved_name,
    is_reserved_path, is_unaddressable_name, is_unaddressable_path,
};
pub use sync::Residue;
