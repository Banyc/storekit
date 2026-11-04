//! The validated set of root-relative paths a sync OMITS from both of its
//! views.
//!
//! An exclusion names one entry or a whole subtree that the run must not walk,
//! must not transfer, must not remove, and must not report as destination-only.
//! It exists for the subtrees that a sync of a working tree legitimately holds
//! but must never manage: a `.git` directory, a `node_modules` tree, a virtual
//! environment. Such a subtree routinely holds what the crate's address rules
//! refuse — an absolute symlink, a hard link, a special file — and the strict
//! source walk REFUSES a tree containing one, so without an exclusion the
//! presence of that subtree alone fails the run.
//!
//! # One value, three views
//!
//! The same [`Exclusions`] value is applied at exactly three places, and each
//! applies it to the same set:
//!
//! * the LOCAL SOURCE walk
//!   ([`crate::manifest::canonicalize_tree_omitting`]), where an excluded
//!   DIRECTORY is not descended into at all, so nothing inside it is stat'ed,
//!   hashed, or refused;
//! * the FAR-SIDE listing, filtered BEFORE assembly
//!   ([`crate::manifest::canonicalize_remote_entries_omitting`] and its
//!   destination form), so an excluded entry never becomes part of the
//!   manifest and cannot trip the hard-link, kind, or containment refusals;
//! * the destination VIEW ([`crate::sync::diff::apply_manifests`]), filtered
//!   before the diff, so an excluded destination entry is neither extraneous nor
//!   a deletion target under either [`super::Extraneous`] value.
//!
//! A LOCAL DESTINATION is read WHOLE and only its view is filtered: the
//! observation is what the destination-aware containment check indexes, and an
//! excluded destination entry SURVIVES the run, so it must still constrain a
//! source link's target walk. A far-side destination listing is the one
//! observation filtered before assembly, because it can be read exactly once
//! and an excluded line must not trip a kind refusal.
//!
//! Filtering is IDEMPOTENT and every view is handed the same value, so the
//! source walk and the far-side assembly cannot exclude a spelling in one view
//! and miss it in the other.
//!
//! # Matching
//!
//! A spelling matches at COMPONENT BOUNDARIES with the same component fold the
//! containment rule uses (`crate::manifest::fold_component`):
//! `node_modules` covers `node_modules` and `node_modules/x` and NOT
//! `node_modules.bak`; `node_modules/x` covers only the child. Because the
//! comparison is folded, a spelling that is not byte-identical but folds onto
//! an excluded component is covered too — the fold only ever excludes MORE,
//! which is the same denial direction the containment rule errs in.
//!
//! # What an exclusion costs
//!
//! * The SOURCE view and a FAR-SIDE destination listing do not contain the
//!   excluded subtree, so a source symlink whose relative target walks through
//!   an excluded component is checked against the views WITHOUT it, exactly as
//!   if the subtree did not exist. If that subtree survives in the destination
//!   and holds a redirector, the installed link resolves through it (a LOCAL
//!   destination keeps the subtree in the containment observation, so the
//!   check still sees it there). An exclusion is the caller's statement that
//!   the subtree is not part of the transfer, and the crate cannot police a
//!   path it is told not to read.
//! * An exclusion is not a lock and not an ownership claim: it changes which
//!   entries a run considers, never who owns the destination, so the run's
//!   overlap refusal and every other report count are unchanged.
//! * [`super::apply::Extraneous::Delete`] leaves an excluded subtree in place,
//!   byte-identical, because the subtree is not in the destination view the
//!   diff classifies.
//! * A LOCAL DESTINATION is read WHOLE (see above), so an entry there that the
//!   tolerant destination walk REFUSES — a special file (FIFO, socket, device),
//!   or a name that is not NFC/UTF-8 — still refuses the run even when it sits
//!   inside an excluded subtree. The refusal is typed
//!   ([`crate::error::MaterializationKind::SpecialFile`] for a special file),
//!   and the remedy is the caller's: remove that entry, or move the subtree out
//!   of the synced root. This is the price of the complete containment
//!   observation; a FAR-SIDE destination listing does not pay it, because its
//!   excluded lines are dropped before assembly.
//!
//! # What a spelling must be
//!
//! Each spelling is parsed as a [`RootedRelativePath`] and validated as a
//! manifest path, so an exclusion can only name a path a manifest ENTRY could
//! have:
//!
//! * the EMPTY path (the root itself) is refused — excluding the root would
//!   exclude the whole run, which is not what an exclusion means;
//! * an ABSOLUTE path and any `.`/`..` component are refused (the parse);
//! * NUL, LF, CR, and TAB, a name that is not UTF-8 or not NFC, and a component
//!   longer than the filesystem name bound are refused (the manifest path
//!   validator), because no manifest entry could equal such a spelling and the
//!   exclusion would silently match nothing;
//! * a RESERVED spelling — the lock record, the `.sync-aside.` namespace, a
//!   crate temp shape, or a case/trailing-dot alias of any of them — is refused
//!   ([`crate::reserved::is_unaddressable_path`]), because excluding the crate's
//!   own bookkeeping would bypass the residue and recovery rules.
//!
//! The first three are [`StoreKind::ExclusionSpelling`], the last is
//! [`StoreKind::ExclusionReserved`]. A repeated spelling is ONE member of the
//! set, not two.

use crate::error::{Error, Result, StoreKind};
use crate::manifest::ExclusionSet;
use crate::relpath::RootedRelativePath;
use std::path::Path;

/// The validated exclusions a run applies to every view of both trees.
///
/// Build it ONCE with [`Exclusions::parse`] (or state the empty set with
/// [`Exclusions::none`]) and pass it in every [`super::SyncOptions`] the run
/// uses, exactly as the ownership token requires.
///
/// Matching records which spellings covered an entry, so
/// [`super::SyncReport::matched_exclusions`] can name them; an exclusion that
/// covered nothing is NOT named, so a typo is visible instead of being reported
/// as applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exclusions {
    set: ExclusionSet,
}

impl Exclusions {
    /// The empty set: every view is the view the crate builds without
    /// exclusions, and the run is byte-identical to one that never had this
    /// option.
    pub fn none() -> Exclusions {
        Exclusions {
            set: ExclusionSet::none(),
        }
    }

    /// Validate each spelling and build the set, or refuse with the typed
    /// reason (see the module docs for the exact accepted set).
    ///
    /// Each spelling is a ROOT-RELATIVE path in the HOST's path model (the same
    /// model [`RootedRelativePath::parse`] validates), naming an entry or a
    /// subtree under the synced root. A spelling that is refused here is
    /// refused for the WHOLE set: a partially-applied exclusion set would be a
    /// run whose views disagree with the caller's intent.
    pub fn parse<P, I>(spellings: I) -> Result<Exclusions>
    where
        P: AsRef<Path>,
        I: IntoIterator<Item = P>,
    {
        let mut validated: Vec<String> = Vec::new();
        for spelling in spellings {
            let spelling = spelling.as_ref();
            // (1) The path model: refuses empty, absolute, `.` and `..`.
            let rel = RootedRelativePath::parse(spelling).map_err(|error| {
                Error::store_kind(
                    StoreKind::ExclusionSpelling,
                    format!(
                        "invalid sync exclusion {}: {error}. An exclusion must name a path INSIDE \
                         the synced root, so the root itself, an absolute path, and a `.`/`..` \
                         component are refused",
                        spelling.display()
                    ),
                )
            })?;
            // (2) The manifest model. Every manifest entry path is the
            // `/`-joined spelling of its components (see
            // [`crate::manifest::canonical_rel_string`]); a host path that
            // cannot be spelled that way could never be matched, so it is
            // refused rather than silently matching nothing.
            let manifest_spelling = crate::manifest::canonical_rel_string(rel.as_path())
                .ok_or_else(|| {
                    Error::store_kind(
                        StoreKind::ExclusionSpelling,
                        format!(
                            "invalid sync exclusion {}: a component is not a UTF-8 file name, so no \
                             manifest entry could have that spelling and the exclusion could never \
                             match one",
                            spelling.display()
                        ),
                    )
                })?;
            // (3) The SAME validator every manifest path passes: NUL/LF/CR/TAB,
            // empty/traversal components, the filesystem name bound, and NFC.
            crate::manifest::validate_entry_path(&manifest_spelling).map_err(|error| {
                Error::store_kind(
                    StoreKind::ExclusionSpelling,
                    format!("invalid sync exclusion {}: {error}", spelling.display()),
                )
            })?;
            // (4) The crate's own bookkeeping is not excludable: excluding it
            // would bypass the residue and recovery rules. The predicate is the
            // ONE authority the id rule and the sync's manifest models use.
            if crate::reserved::is_unaddressable_path(&manifest_spelling) {
                return Err(Error::store_kind(
                    StoreKind::ExclusionReserved,
                    format!(
                        "refusing to exclude {}: it names one of the crate's own bookkeeping \
                         spellings (the lock record, the `.sync-aside.` namespace, a crate temp \
                         shape, or a case/trailing-dot alias of one of them). Excluding that \
                         spelling would bypass the residue and recovery rules, so it is never a \
                         caller exclusion",
                        spelling.display()
                    ),
                ));
            }
            validated.push(manifest_spelling);
        }
        Ok(Exclusions {
            set: ExclusionSet::from_spellings(validated),
        })
    }

    /// Whether this is the empty set.
    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    /// The exclusion spellings, as validated root-relative MANIFEST paths, in
    /// the order they were supplied.
    pub fn spellings(&self) -> impl Iterator<Item = &str> {
        self.set.spellings()
    }

    /// The exclusion matcher, for the callers that hold the public type: whether
    /// `manifest_path` (a root-relative `/`-joined manifest spelling) is
    /// covered, recording the spelling as matched when it is. The manifest walk
    /// and assembly take [`Exclusions::set`] instead and call the same matcher,
    /// so both routes read ONE implementation.
    pub(crate) fn omits(&self, manifest_path: &str) -> bool {
        self.set.omits(manifest_path)
    }

    /// The exclusion matcher itself, for the manifest walk and assembly entry
    /// points, which take the matcher rather than this wrapper so the
    /// manifest layer does not depend on the sync layer.
    pub(crate) fn set(&self) -> &ExclusionSet {
        &self.set
    }

    /// The spellings that covered at least one entry, sorted. An exclusion
    /// that covered nothing is absent, so [`super::SyncReport`] cannot report a
    /// typo as applied.
    pub(crate) fn matched(&self) -> Vec<String> {
        self.set.matched_spellings()
    }

    /// Take the match observations of the token's equal set, so the report
    /// names the entries the SOURCE WALK (which ran while the token was minted)
    /// omitted as well as the ones the run's own views omitted.
    pub(crate) fn adopt_matches(&self, from: &Exclusions) {
        self.set.adopt_matches(from.set());
    }
}

impl Default for Exclusions {
    /// The EMPTY set: the safe default, and the run that is byte-identical to
    /// one built before exclusions existed.
    fn default() -> Exclusions {
        Exclusions::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(spellings: &[&str]) -> StoreKind {
        Exclusions::parse(spellings)
            .expect_err("the spelling must be refused")
            .store_reason()
            .expect("the refusal must be typed")
    }

    #[test]
    fn the_empty_set_matches_nothing() {
        let exclusions = Exclusions::none();
        assert!(exclusions.is_empty());
        assert!(!exclusions.omits("a"));
        assert_eq!(exclusions.spellings().count(), 0);
        assert_eq!(exclusions.matched(), Vec::<String>::new());
    }

    #[test]
    fn a_root_and_a_parent_are_refused_as_a_spelling() {
        assert_eq!(refusal(&[""]), StoreKind::ExclusionSpelling);
        assert_eq!(refusal(&["."]), StoreKind::ExclusionSpelling);
        assert_eq!(refusal(&[".."]), StoreKind::ExclusionSpelling);
        assert_eq!(refusal(&["a/../b"]), StoreKind::ExclusionSpelling);
        // An absolute spelling is refused by the path parse, before the
        // component walk could see it.
        assert_eq!(refusal(&["/abs"]), StoreKind::ExclusionSpelling);
    }

    #[test]
    fn a_wire_unrepresentable_name_is_refused_as_a_spelling() {
        for spelling in ["a\nb", "a\rb", "a\tb", "a\0b"] {
            assert_eq!(
                refusal(&[spelling]),
                StoreKind::ExclusionSpelling,
                "{spelling:?}"
            );
        }
    }

    #[test]
    fn the_crates_own_bookkeeping_is_refused_as_reserved() {
        for spelling in [
            "operation.lock",
            "state/operation.lock",
            ".dest.operation.lock",
            ".sync-aside.1",
            "d/.sync-aside.42.0",
            // A crate temp shape, and a case alias of a reserved spelling.
            ".f.tmp.1.0",
            ".SYNC-ASIDE.1",
        ] {
            assert_eq!(
                refusal(&[spelling]),
                StoreKind::ExclusionReserved,
                "{spelling:?}"
            );
        }
    }

    #[test]
    fn matching_is_at_component_boundaries_under_the_containment_fold() {
        let exclusions = Exclusions::parse(["node_modules"]).unwrap();
        assert!(exclusions.omits("node_modules"));
        assert!(exclusions.omits("node_modules/x"));
        assert!(exclusions.omits("node_modules/x/y"));
        assert!(!exclusions.omits("node_modules.bak"));
        assert!(!exclusions.omits("node_modules.bak/x"));
        assert!(!exclusions.omits("x/node_modules"));

        let child = Exclusions::parse(["node_modules/x"]).unwrap();
        assert!(!child.omits("node_modules"));
        assert!(!child.omits("node_modules/y"));
        assert!(child.omits("node_modules/x"));
        assert!(child.omits("node_modules/x/y"));

        // The fold is the containment fold, so a case alias is covered (the
        // denial direction) and a trailing dot folds away too.
        let folded = Exclusions::parse(["Node_Modules"]).unwrap();
        assert!(folded.omits("node_modules"));
        assert!(folded.omits("NODE_MODULES/y"));
        let dotted = Exclusions::parse(["node_modules."]).unwrap();
        assert!(dotted.omits("node_modules"));
    }

    #[test]
    fn a_match_is_recorded_once_and_an_unmatched_spelling_is_not() {
        let exclusions = Exclusions::parse(["node_modules", "target"]).unwrap();
        assert!(exclusions.omits("node_modules/x"));
        assert!(exclusions.omits("node_modules/x"));
        assert_eq!(exclusions.matched(), vec!["node_modules".to_string()]);
        assert!(exclusions.omits("target"));
        assert_eq!(
            exclusions.matched(),
            vec!["node_modules".to_string(), "target".to_string()]
        );
    }

    #[test]
    fn a_repeated_spelling_is_one_member_of_the_set() {
        let exclusions = Exclusions::parse(["a", "a"]).unwrap();
        assert_eq!(exclusions.spellings().collect::<Vec<_>>(), vec!["a"]);
        assert!(exclusions.omits("a"));
        assert_eq!(exclusions.matched(), vec!["a".to_string()]);
    }

    #[test]
    fn set_equality_ignores_order_and_observations() {
        let a = Exclusions::parse(["a", "b"]).unwrap();
        let b = Exclusions::parse(["b", "a"]).unwrap();
        assert_eq!(a, b);
        assert!(a.omits("a"));
        assert_eq!(a, b, "a matched spelling is not part of the set's identity");
        assert_ne!(a, Exclusions::parse(["a"]).unwrap());
        assert_eq!(a.clone(), a, "a clone is an equal set");
        assert_eq!(
            a.clone().matched(),
            a.matched(),
            "a clone carries the observations made before it"
        );
    }
}
