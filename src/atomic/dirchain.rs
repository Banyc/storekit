//! The durable directory-chain typestate shared by both platform ports.
//!
//! Ensuring a directory chain has TWO outcomes, and the `Created` outcome
//! carries an OBLIGATION: every directory this call created must be committed
//! durably (fsynced into its parent, deepest first, plus the parent-of-parent
//! entry) before the caller may report that it created anything. A bare `bool`
//! made that obligation implicit: a future edit could return `true` before the
//! fsync loop ran, and the created-but-not-yet-durable state was representable.
//!
//! [`CreatedChain`] is that state, and [`CreatedChain::commit`] is the only
//! function that can produce [`DirChain::Created`] — the `Created` arm's
//! [`CreatedProof`] field is private to this module, and [`ChainCommit`]
//! carries a PRIVATE SEAL (`sealed::Sealed`, nameable only here), so no other
//! module — not even a sibling in this crate — can hand `commit` a no-op chain
//! and obtain the verdict without running a port's commit. A `CreatedChain`
//! dropped WITHOUT `commit()` therefore does not fsync
//! (the documented Windows/Unix behaviour for a mid-chain failure); it cannot
//! report success either.
//!
//! The proof's exact meaning: `commit` ran and the port's commit returned
//! `Ok`. On Unix that commit is the fsync loop; on Windows it is the
//! documented no-op (that port makes no durability claim), so the verdict is
//! the same word on both platforms but only the Unix one carries durability.
//!
//! The port supplies the platform commit through [`ChainCommit`]: Unix fsyncs
//! the open descriptors, while Windows has no directory fsync and its commit
//! is the documented no-op (it makes no durability claim).

use crate::error::Result;

/// The outcome of ensuring a directory chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DirChain {
    /// Every component already existed: nothing was created, nothing to commit.
    AlreadyPresent,
    /// At least one component was created and its durable commit ran. The
    /// proof field can only be built by [`CreatedChain::commit`].
    Created(CreatedProof),
}

/// The unforgeable witness that a created chain ran its durable commit. The
/// tuple field is private to this module, so no other module can construct the
/// `Created` arm; the only construction is inside [`CreatedChain::commit`],
/// which any in-crate caller can reach only with a SEALED port chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CreatedProof(());

/// The seal that keeps [`ChainCommit`] implementable only inside this module.
///
/// `ChainCommit` is `pub(crate)`, so a plain trait bound would let any in-crate
/// module write `impl ChainCommit for NoOp` and hand it to
/// [`CreatedChain::commit`], obtaining [`DirChain::Created`] without a port's
/// commit ever running. A private supertrait closes that: a type can implement
/// `ChainCommit` only if it implements `Sealed`, and `Sealed` is not nameable
/// outside `dirchain`. The port's commit types below are the only
/// implementations, so the verdict's only producer is a real port commit.
mod sealed {
    pub(crate) trait Sealed {}
}

#[cfg(unix)]
impl sealed::Sealed for crate::atomic::unix::UnixChain<'_> {}
#[cfg(unix)]
impl sealed::Sealed for crate::atomic::unix::PathChain {}
#[cfg(windows)]
impl sealed::Sealed for crate::atomic::windows::WindowsChain {}

impl DirChain {
    /// Whether this call created (and committed) at least one directory.
    pub(crate) fn is_created(&self) -> bool {
        matches!(self, DirChain::Created(_))
    }
}

/// The platform-specific durable commit for a created chain.
///
/// SEALED: the `sealed::Sealed` supertrait is nameable only in this module, so
/// the only implementations are the port's commit types below (see the module
/// docs).
pub(crate) trait ChainCommit: sealed::Sealed {
    /// Fsync the created entries and the parent-of-parent entry.
    ///
    /// `created` holds the indices of the components this call created, in the
    /// order the port's chain vector was built; the implementation visits them
    /// deepest-first for its own protocol (see each port's implementation).
    fn commit_created_entries(&self, created: &[usize]) -> Result<()>;
}

/// A created-but-not-yet-committed directory chain.
///
/// Dropping it without calling [`commit`](Self::commit) does NOT run the
/// durable commit; that is deliberate, so a mid-chain failure leaves the
/// created components in place without a false durability claim.
pub(crate) struct CreatedChain<C: ChainCommit> {
    chain: C,
    created: Vec<usize>,
}

impl<C: ChainCommit> CreatedChain<C> {
    /// Build the chain from the port's state and the indices it created.
    pub(crate) fn new(chain: C, mut created: Vec<usize>) -> Self {
        created.sort_unstable();
        Self { chain, created }
    }

    /// Consume the chain and run its durable commit. This is the ONLY way to
    /// obtain [`DirChain::Created`]; on a commit failure the chain is dropped
    /// (the created components remain, no durability is claimed).
    pub(crate) fn commit(self) -> Result<DirChain> {
        if self.created.is_empty() {
            return Ok(DirChain::AlreadyPresent);
        }
        self.chain.commit_created_entries(&self.created)?;
        Ok(DirChain::Created(CreatedProof(())))
    }
}
