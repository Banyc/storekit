//! The validated relative path that names every entry a mutation touches.
//!
//! This module sits BELOW [`crate::atomic`] and [`crate::sync`]: it has no
//! dependencies inside the crate, so the descriptor-relative primitives and
//! the transport can both name a path with the same validated type. It is
//! re-exported from [`crate::transport`] and from the crate root, so no
//! existing consumer path changes.
//!
//! A [`RootedRelativePath`] is validated at construction to reject ABSOLUTE
//! paths, `.`/`..` components, a literal `.` SEGMENT, and EMPTY paths, so a
//! `root.join(rel)` is safe by construction — a caller can never escape the
//! deployment root through a transport operation or a mutating primitive, and
//! a traversal path can never be joined onto the root.
//!
//! The traversal/absolute decision in [`RootedRelativePath::parse`] is made
//! with the PLATFORM's own path model ([`Path::components`]) rather than a
//! hardcoded separator, so a `\` is a separator exactly where the platform
//! says it is: on Windows `..\escape` is a traversal and is refused, while on
//! Unix it is one legal filename byte and is accepted (this crate preserves
//! the names of the trees it manages; a Unix tree may legitimately contain
//! it). `parse` validates a path SPELLED IN THE HOST'S OWN MODEL — a live
//! directory entry, a lock spelling, a caller-supplied root-relative path.
//!
//! A MANIFEST path is a different model: it is host-INDEPENDENT and
//! `/`-separated, and a literal `\` is an ordinary NAME byte on every
//! platform (see [`TreeEntry::path`](crate::manifest::TreeEntry::path)). A
//! manifest string therefore must NOT be handed to `parse`/`Path::new`, whose
//! host model on Windows would silently split the DISTINCT manifest paths
//! `a\b` (one component) and `a/b` (two) onto the SAME host path. The ONE
//! conversion from a manifest string to a host path is
//! [`RootedRelativePath::from_manifest`], which splits on `/` only.
//!
//! The type deliberately carries NO `Default` (an empty path would be an
//! unrooted path constructible by anyone — the exact gap this hardening
//! closes) and NO `From<PathBuf>`/`From<&Path>` (a raw path must pass the
//! validated [`RootedRelativePath::parse`] — or the safe-by-construction
//! internal [`RootedRelativePath::from_validated`] used by the layout
//! builders, whose components are validated identities).

use crate::error::{Error, Result};
use std::path::{Component, Path, PathBuf};

/// A validated RELATIVE path that stays inside an owned root: never absolute,
/// and free of `.`/`..` components (including a literal `.` segment). It is
/// never empty for a MANIFEST address; the ONE empty value the crate mints
/// names the owned root itself for a listing, and only `from_validated`
/// (`pub(crate)`) can build it. A caller-supplied [`Layout`](crate::transport::Layout) produces
/// these from validated identities; every path that crosses the
/// [`Remote`](crate::transport::Remote) trait boundary AND every
/// root-relative argument of a mutating primitive is one, so `root.join(rel)`
/// is safe by construction.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RootedRelativePath(PathBuf);

impl RootedRelativePath {
    /// Validate `p` and construct. Rejects: an EMPTY path, an ABSOLUTE path,
    /// and any `.`/`..` component at any position (a traversal path can never
    /// cross the boundary).
    ///
    /// The traversal/absolute decision is made with the PLATFORM's own path
    /// model, [`Path::components`], never with a hardcoded `'/'`: only
    /// [`Component::Normal`] is admitted, so `RootDir`/`Prefix` (an absolute
    /// path, or a Windows `\`-root that `Path::join` uses to REPLACE the
    /// deployment root), `ParentDir`, and `CurDir` are all refused — and on
    /// Windows a `\` IS a separator, so `..\escape`, `\absolute`, and
    /// `a\..\b` are seen for what they are. On Unix a `\` is an ordinary
    /// filename byte, so those same spellings are each ONE literal component
    /// and are ACCEPTED: this crate preserves the names of the trees it
    /// manages, and refusing them would reject legal trees.
    ///
    /// [`Path::components`] also erases a `.` segment that is not at the
    /// start (`a/.`, `a/./b`), so the raw spelling is scanned as well for a
    /// literal `.` segment. That scan splits on [`std::path::is_separator`] —
    /// the same platform predicate [`Path::components`] uses — so it inherits
    /// the platform's separator model instead of introducing a second one.
    pub fn parse(p: &Path) -> Result<RootedRelativePath> {
        if p.as_os_str().is_empty() {
            return Err(Error::transport(format!(
                "invalid relative path {:?}: the path must not be empty",
                p
            )));
        }
        // The platform's component model is the ONE authority on what is a
        // traversal, a root, or a prefix. Anything that is not a plain name
        // is refused, and this single rule is what refuses an ABSOLUTE path too
        // (a leading `/` makes the first component a `RootDir`): `ParentDir` walks
        // above the root; `RootDir`/`Prefix` name an absolute location (and on Windows `Path::join` REPLACES
        // the base for a rooted/`\`-leading path, cancelling the deployment
        // root entirely); `CurDir` names a directory rather than an entry.
        if !p.components().all(|c| matches!(c, Component::Normal(_))) {
            return Err(Error::transport(format!(
                "invalid relative path {:?}: traversal components (`.`/`..`) and absolute paths are not allowed",
                p
            )));
        }
        // A non-leading `.` segment never reaches the loop above because
        // `Path::components` erases it, so refuse a literal `.` segment
        // textually too. The split uses the platform's separator predicate
        // (never a hardcoded '/'), so a `\`-bearing name is one segment on
        // Unix while Windows sees the `.`/`..` segment it really is.
        for segment in p.to_string_lossy().split(std::path::is_separator) {
            if segment == "." || segment == ".." {
                return Err(Error::transport(format!(
                    "invalid relative path {:?}: traversal components (`.`/`..`) are not allowed",
                    p
                )));
            }
        }
        Ok(RootedRelativePath(p.to_path_buf()))
    }

    /// Convert a canonical MANIFEST (wire) path into the validated host
    /// relative path. This is the ONE authority for that conversion: every
    /// place that turns a manifest entry path (or a bookkeeping key derived
    /// from one) into a host path a mutating primitive accepts goes through
    /// this constructor, never through [`RootedRelativePath::parse`] or
    /// `Path::new`.
    ///
    /// The manifest model is host-INDEPENDENT and `/`-separated: a literal
    /// `\` is an ordinary NAME byte, not a separator. The host path model is
    /// NOT that model — on Windows `\` IS a separator — so handing the whole
    /// string to the host parser maps the two DISTINCT manifest paths `a\b`
    /// (one component) and `a/b` (two) onto the SAME host path. One source
    /// entry could then clobber the other (and a destination entry) while the
    /// manifest, the diff, and the digest all treat them as distinct. This
    /// constructor splits on `/` ONLY, so the manifest's segment sequence is
    /// exactly the host path's component sequence and the conversion is
    /// INJECTIVE where it is defined.
    ///
    /// Every segment must be a single host [`Component::Normal`] carrying the
    /// segment's own bytes. On Unix every non-empty, non-`.`/`..` segment is
    /// representable (a `\` is an ordinary byte), so this accepts exactly the
    /// wire validator's set and behaviour is unchanged. On Windows a segment
    /// the host path model would split (a `\`) or read as a prefix (a drive
    /// spelling) cannot name one entry, so it is REFUSED with a typed error
    /// naming the path rather than silently split — the correct direction,
    /// because a fold is a denial tool. Refusing only where the host cannot
    /// represent the name keeps the wire validator host-independent, so a
    /// Unix tree that legally holds `a\b` is still accepted everywhere it can
    /// be addressed.
    pub fn from_manifest(path: &str) -> Result<RootedRelativePath> {
        if path.is_empty() {
            return Err(Error::transport(format!(
                "invalid manifest path {path:?}: the path must not be empty"
            )));
        }
        let mut out = PathBuf::new();
        for segment in path.split('/') {
            if segment.is_empty() {
                return Err(Error::transport(format!(
                    "invalid manifest path {path:?}: an empty segment is not a name"
                )));
            }
            if segment == "." || segment == ".." {
                return Err(Error::transport(format!(
                    "invalid manifest path {path:?}: traversal components (`.`/`..`) are not allowed"
                )));
            }
            if !is_single_host_component(segment) {
                return Err(Error::transport(format!(
                    "manifest path {path:?} cannot be represented on this host: the segment \
                     {segment:?} is not a single file name (the host path model would split it or \
                     read a prefix out of it), so the path cannot address one entry"
                )));
            }
            out.push(segment);
        }
        Ok(RootedRelativePath(out))
    }

    /// Internal constructor for paths whose components are VALIDATED
    /// IDENTITIES (the layout builders) — the caller proves safety by
    /// construction: a validated identifier is a single safe path segment,
    /// so the built path is relative and traversal-free. Production callers
    /// must construct through the validated [`RootedRelativePath::parse`] (a
    /// path spelled in the HOST's model) or [`RootedRelativePath::from_manifest`]
    /// (a `/`-separated MANIFEST path).
    pub(crate) fn from_validated(p: PathBuf) -> RootedRelativePath {
        RootedRelativePath(p)
    }

    /// The validated relative path.
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// Join `component` onto this path and RE-VALIDATE the result: the
    /// joined path must still be relative and traversal-free (an absolute
    /// component or a `.`/`..` component is rejected).
    pub fn join(&self, component: impl AsRef<Path>) -> Result<RootedRelativePath> {
        RootedRelativePath::parse(&self.0.join(component))
    }

    /// The final component of the path, if any.
    pub fn file_name(&self) -> Option<&std::ffi::OsStr> {
        self.0.file_name()
    }

    /// The parent directory of the path, if any. `None` when the path has no
    /// parent that is itself a valid rooted relative path (a single-component
    /// path's parent is the empty path, which is not a valid
    /// [`RootedRelativePath`]).
    pub fn parent(&self) -> Option<RootedRelativePath> {
        self.0
            .parent()
            .and_then(|p| RootedRelativePath::parse(p).ok())
    }

    /// Replace the final component, re-validating the result.
    pub fn with_file_name(&self, name: impl AsRef<std::ffi::OsStr>) -> Result<RootedRelativePath> {
        RootedRelativePath::parse(&self.0.with_file_name(name))
    }

    /// The display form of the underlying path (for error messages).
    pub fn display(&self) -> std::path::Display<'_> {
        self.0.display()
    }
}

impl AsRef<Path> for RootedRelativePath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl std::fmt::Display for RootedRelativePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

/// Whether `segment` is exactly ONE host path [`Component::Normal`] carrying
/// the segment's own bytes — i.e. the host path model neither splits it nor
/// reads a root/prefix out of it. On Unix only `/` separates, so every
/// non-empty, non-`.`/`..` segment qualifies (a `\` is an ordinary byte). On
/// Windows `\` is a separator and a drive spelling is a prefix, so such a
/// segment is exactly the case this must report as NOT a single component.
#[cfg(not(windows))]
fn is_single_host_component(segment: &str) -> bool {
    let mut components = Path::new(segment).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(name)), None) => name == std::ffi::OsStr::new(segment),
        _ => false,
    }
}

/// WINDOWS: a single component under the host path model is not yet a name
/// the filesystem can hold. Windows additionally forbids these bytes in a
/// file name, and strips a trailing space or dot — so `a.` and `a` (and `a `
/// and `a`) would name the SAME entry, which is the same injectivity failure
/// in a different guise. Such a segment is therefore REFUSED here too. The
/// set is the documented Windows filename rule; reserved device names
/// (`CON`, `NUL`, ...) are left to the filesystem, because they do not make
/// two distinct manifest paths collide.
#[cfg(windows)]
fn is_single_host_component(segment: &str) -> bool {
    const WINDOWS_FORBIDDEN: [char; 8] = ['<', '>', ':', '"', '\\', '|', '?', '*'];
    if segment
        .chars()
        .any(|c| WINDOWS_FORBIDDEN.contains(&c) || c.is_control())
        || segment.ends_with([' ', '.'])
    {
        return false;
    }
    let mut components = Path::new(segment).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(name)), None) => name == std::ffi::OsStr::new(segment),
        _ => false,
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use proptest::test_runner::RngSeed;

    /// The boundary rule: a validated relative path accepts every safe
    /// relative form and rejects every unsafe one (empty, absolute, `.`/`..`
    /// at any position).
    #[test]
    fn parse_accepts_safe_rejects_unsafe() {
        for ok in [
            "a",
            "a/b",
            "a/b/c.json",
            "generations/gen-1/assignment.json",
            "objects/sha256/abc/root",
            "a//b",
            "a/",
        ] {
            let p = RootedRelativePath::parse(Path::new(ok))
                .unwrap_or_else(|e| panic!("{ok:?} must parse: {e}"));
            assert_eq!(p.as_path(), Path::new(ok));
        }
        for bad in [
            "",
            "/",
            "//",
            "/abs",
            "/abs/rel",
            ".",
            "..",
            "./a",
            "a/.",
            "a/..",
            "../a",
            "a/../b",
            "a/b/../../c",
            "/a/../b",
        ] {
            assert!(
                RootedRelativePath::parse(Path::new(bad)).is_err(),
                "{bad:?} must be rejected"
            );
        }
    }

    /// THE DELTA between the former private `validate_rel` guard and this
    /// type, pinned: `Path::components` ERASES a non-leading `.` segment, so a
    /// pure component-model check accepts `a/./b`; this type scans the literal
    /// spelling and REFUSES it. That is the one input the two disagree on, in
    /// the STRICTER direction, and the reason adopting the boundary is a
    /// behaviour change rather than a refactor.
    #[test]
    fn a_literal_dot_segment_is_invisible_to_components_but_refused_here() {
        let p = Path::new("a/./b");
        assert!(
            p.components().all(|c| matches!(c, Component::Normal(_))),
            "Path::components erases the `.`, which is why a component-model guard accepts it"
        );
        assert!(
            RootedRelativePath::parse(p).is_err(),
            "the literal `.` segment must still be refused"
        );
    }

    /// Joining re-validates: a safe component joins, an absolute or
    /// traversal component is rejected.
    #[test]
    fn join_revalidates() {
        let base = RootedRelativePath::parse(Path::new("a/b")).unwrap();
        assert_eq!(
            base.join("c.json").unwrap().as_path(),
            Path::new("a/b/c.json")
        );
        for bad in ["/abs", "..", "../x", ".", "/"] {
            base.join(bad)
                .expect_err(&format!("{bad:?} must be rejected"));
        }
    }

    /// Arbitrary untyped path text covering every unsafe class: empty,
    /// absolute, `.`/`..` at any position, separators, whitespace, unicode,
    /// control characters, and clean safe relative values.
    fn arbitrary_path_text() -> impl Strategy<Value = String> {
        prop_oneof![
            prop::sample::select(vec![
                String::new(),
                "/".to_string(),
                "//".to_string(),
                "/abs".to_string(),
                "/abs/rel".to_string(),
                ".".to_string(),
                "..".to_string(),
                "./a".to_string(),
                "a/.".to_string(),
                "a/..".to_string(),
                "../a".to_string(),
                "a/../b".to_string(),
                "a/b/../../c".to_string(),
                "/a/../b".to_string(),
                "a".to_string(),
                "a/b".to_string(),
                "a/b/c.json".to_string(),
                "generations/gen-1/assignment.json".to_string(),
                "objects/sha256/abc/root".to_string(),
                "a//b".to_string(),
                "a/".to_string(),
                " x".to_string(),
                "x ".to_string(),
                "a\nb".to_string(),
                "α".to_string(),
                "a\u{0}b".to_string(),
            ]),
            prop::collection::vec(prop::char::any(), 0..48).prop_map(|v| v.into_iter().collect()),
        ]
    }

    proptest! {
        // THE BOUNDARY PROPERTY: over ARBITRARY untyped path text, the
        // validated parse accepts EXACTLY the safe relative forms and
        // rejects every unsafe one — a path that parses is relative,
        // non-empty, and free of `.`/`..` components; a path that is
        // rejected is absolute, empty, or traversal-bearing. Bounded 16
        // cases, fixed seed 0x5EED_5EED (house style), no failure
        // persistence.
        #![proptest_config(ProptestConfig {
            cases: crate::test_support::proptest_cases(16),
            rng_seed: RngSeed::Fixed(0x5EED_5EED),
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn arbitrary_untyped_paths_are_rejected_or_safe(s in arbitrary_path_text()) {
            let p = Path::new(&s);
            match RootedRelativePath::parse(p) {
                Ok(r) => {
                    // A path that parses is SAFE: relative, non-empty, and
                    // every component is a NORMAL component (no `.`/`..`,
                    // no root).
                    prop_assert!(!r.as_path().is_absolute(), "{s:?} must not be absolute");
                    prop_assert!(!r.as_path().as_os_str().is_empty(), "{s:?} must not be empty");
                    for c in r.as_path().components() {
                        prop_assert!(
                            matches!(c, std::path::Component::Normal(_)),
                            "{s:?} has an unsafe component {:?}",
                            c
                        );
                    }
                }
                Err(_) => {
                    // A path that is rejected is UNSAFE under the SAME
                    // platform component model `parse` uses: absolute, empty,
                    // any non-Normal component (`ParentDir`/`CurDir`/
                    // `RootDir`/`Prefix`), or a literal `.`/`..` segment on a
                    // platform separator (which `Path::components` would
                    // erase when it is a non-leading `.`). The classification
                    // must NOT hardcode '/': on Windows `..\x` is one
                    // `ParentDir`-bearing path even though it has no '/', and
                    // `s.split('/')` would miss it.
                    let unsafe_class = p.is_absolute()
                        || p.as_os_str().is_empty()
                        || p.components()
                            .any(|c| !matches!(c, std::path::Component::Normal(_)))
                        || s.split(std::path::is_separator)
                            .any(|seg| seg == "." || seg == "..");
                    prop_assert!(
                        unsafe_class,
                        "rejected path {s:?} must be absolute, empty, or traversal-bearing"
                    );
                }
            }
        }
    }

    /// The property the boundary exists for: for EVERY path [`RootedRelativePath::parse`]
    /// accepts, `root.join(rel)` cannot resolve outside `root`.
    ///
    /// THE ARGUMENT (lexical, and identical on every platform):
    /// `Path::join` either REPLACES the base — only when the joined path is
    /// absolute or carries a root/prefix — or APPENDS its components. `parse`
    /// refuses absolute paths, every non-`Normal` component, and every
    /// literal `.`/`..` segment, so an accepted `rel` contributes only plain
    /// names. Therefore the joined components are exactly the root's
    /// components followed by `Normal` names: no `..` can walk above the
    /// root and no root/prefix can reset it, so the resolved path stays
    /// strictly under the root.
    ///
    /// RUNS ON BOTH PLATFORMS. The Windows arm cannot be RUN here (Windows
    /// is type-checked only), so this pins the Unix behavior by construction
    /// and the Windows behavior by the same component-model argument; no
    /// runtime Windows claim is made.
    #[test]
    fn accepted_paths_join_cannot_escape_the_root() {
        let root = std::env::temp_dir().join("storekit-rooted-join-property");
        // The accepted set is platform-specific: on Unix a `\` is an ordinary
        // name byte, so a `\`-bearing spelling is ONE legal component and
        // must be accepted (and must still not escape); on Windows those
        // spellings are traversal/root-bearing and `parse` REFUSES them, so
        // they are not in the accepted set there.
        #[cfg(unix)]
        let accepted = [
            "a",
            "a/b",
            "a/b/c.json",
            "a//b",
            "a/",
            "generations/gen-1/assignment.json",
            r"..\escape",
            r"a\..\b",
            r"\absolute",
            r"..\x",
        ];
        #[cfg(not(unix))]
        let accepted = [
            "a",
            "a/b",
            "a/b/c.json",
            "a//b",
            "a/",
            "generations/gen-1/assignment.json",
        ];

        for text in accepted {
            let rel = RootedRelativePath::parse(Path::new(text))
                .unwrap_or_else(|e| panic!("{text:?} must be accepted: {e}"));
            let joined = root.join(rel.as_path());

            // (1) The joined path is anchored at the root...
            assert!(
                joined.starts_with(&root),
                "{text:?}: {joined:?} must start with {root:?}"
            );
            // (2) ...the join APPENDED rather than replaced the base
            //     (component counts add up exactly)...
            assert_eq!(
                joined.components().count(),
                root.components().count() + rel.as_path().components().count(),
                "{text:?}: join must append to, not replace, the root"
            );
            // (3) ...and every appended component is a plain NAME, so none
            //     can walk above the root or reset it.
            let tail: Vec<std::path::Component> = joined
                .components()
                .skip(root.components().count())
                .collect();
            assert!(!tail.is_empty(), "{text:?}: the join contributed nothing");
            assert!(
                tail.iter()
                    .all(|c| matches!(c, std::path::Component::Normal(_))),
                "{text:?}: every appended component must be a Normal name, got {tail:?}"
            );
        }
    }

    /// UNIX: a backslash is an ordinary filename byte, NOT a separator, so a
    /// name that merely LOOKS like a traversal or an absolute path is ONE
    /// legal component and stays accepted — pinning this crate's name
    /// fidelity. The genuine unsafe spellings are still refused.
    ///
    /// The Unix arm: it runs wherever this suite runs on a Unix host. The Windows
    /// arm below is compiled only on Windows and never executed by this suite.
    #[cfg(unix)]
    #[test]
    fn unix_backslash_is_an_ordinary_name_byte() {
        for ok in [r"..\escape", r"a\..\b", r"\absolute", r"..\x", r"a\b"] {
            let p = RootedRelativePath::parse(Path::new(ok))
                .unwrap_or_else(|e| panic!("{ok:?} is one legal Unix name and must parse: {e}"));
            assert_eq!(p.as_path(), Path::new(ok), "{ok:?} must round-trip");
            // Exactly one component, and it is a plain name.
            let comps: Vec<_> = p.as_path().components().collect();
            assert_eq!(comps.len(), 1, "{ok:?} must be a single component");
            assert!(matches!(comps[0], std::path::Component::Normal(_)));
        }
        // The same refusals as before: genuine `.`/`..` components (however
        // spelled with `/`), absolute paths, and the empty path.
        for bad in [
            "", ".", "..", "./a", "a/.", "a/..", "../a", "a/../b", "/", "/abs", "/a/../b",
        ] {
            assert!(
                RootedRelativePath::parse(Path::new(bad)).is_err(),
                "{bad:?} must still be refused on Unix"
            );
        }
    }

    /// WINDOWS (compiled only on Windows, and never executed by this suite — the
    /// port is type-checked only, so this is an UNVERIFIED-at-runtime assertion
    /// rather than a measured result).
    ///
    /// FLIPPED: this test previously asserted that `parse` ACCEPTED
    /// `a\b` and `a\b\c.json` because the Windows path model treats `\` as a
    /// separator. That acceptance IS the defect. The manifest path `a\b` is
    /// ONE component (a literal backslash name, legal on Unix); the host-model
    /// parse silently maps it onto the two-component host path `a/b`, so the
    /// DISTINCT manifest entries `a\b` and `a/b` would address the SAME file
    /// and one could clobber the other. The manifest conversion therefore
    /// REFUSES a backslash-bearing segment on Windows. `parse` still refuses
    /// the genuine HOST traversals, and the ordinary manifest spellings (no
    /// backslash) convert.
    #[cfg(windows)]
    #[test]
    fn windows_backslash_is_a_separator_so_a_manifest_segment_is_refused() {
        // The manifest conversion refuses the host-splittable segment.
        for bad in [r"a\b", r"a\b\c.json", r"\x"] {
            assert!(
                RootedRelativePath::from_manifest(bad).is_err(),
                "{bad:?} is one manifest component a Windows host cannot name and must be refused"
            );
        }
        // The host-path validator still refuses genuine host traversals.
        for bad in [r"..\x", r"\x", r"a\..\b", r".\a", r"a\."] {
            assert!(
                RootedRelativePath::parse(Path::new(bad)).is_err(),
                "{bad:?} must be refused by the host-path validator on Windows"
            );
        }
        // The ordinary MANIFEST spellings (no backslash) convert.
        for ok in ["a", "a/b", "a/b/c.json"] {
            RootedRelativePath::from_manifest(ok)
                .unwrap_or_else(|e| panic!("{ok:?} must convert on Windows: {e}"));
        }
    }

    /// UNIX: name fidelity end to end — a source file literally named
    /// `..\escape` (and `a\..\b`) round-trips through the filesystem as ONE
    /// directory entry, and `root.join(rel)` addresses exactly that entry.
    /// This is what would break if the fix refused backslash-bearing names.
    #[cfg(unix)]
    #[test]
    fn unix_backslash_name_round_trips_through_the_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        for name in [r"..\escape", r"a\..\b", r"\absolute"] {
            let rel = RootedRelativePath::parse(Path::new(name)).unwrap();
            let on_disk = dir.path().join(rel.as_path());
            std::fs::write(&on_disk, name.as_bytes()).unwrap();
            // Reading it back by the plain path proves the join addresses the
            // same entry the name denotes.
            assert_eq!(
                std::fs::read(dir.path().join(name)).unwrap(),
                name.as_bytes()
            );
            // It is exactly ONE entry whose name is the whole backslash-bearing
            // string — not `..` followed by `escape`.
            let entries: Vec<std::ffi::OsString> = std::fs::read_dir(dir.path())
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            assert_eq!(entries, vec![std::ffi::OsString::from(name)], "{name:?}");
            std::fs::remove_file(&on_disk).unwrap();
        }
    }

    /// THE DEFECT, PINNED AT THE CONVERSION. The HOST path model is not the
    /// manifest model: it treats a byte as a separator exactly where the
    /// platform says so. On Windows `\` IS a separator, so the two DISTINCT
    /// manifest paths `a\b` (ONE component) and `a/b` (TWO) both join to the
    /// same host path — the injectivity failure this fix removes. On Unix the
    /// same two paths stay distinct, so the collision is an accident of the
    /// host and NO Unix run can expose it. This test RUNS on both platforms
    /// (the assertion is selected by cfg); it is the honest evidence that no
    /// executed Unix test can catch the defect.
    #[test]
    fn the_host_path_model_collapses_distinct_manifest_paths_on_a_backslash_host() {
        let backslash = Path::new(r"a\b");
        let slash = Path::new("a/b");
        let backslash_components = backslash.components().count();
        let slash_components = slash.components().count();
        let backslash_host = PathBuf::from(backslash);
        let slash_host = PathBuf::from(slash);
        // The raw output, kept: the host model's two results and counts.
        println!(
            "host path model: {:?} -> {:?} ({} components); {:?} -> {:?} ({} components)",
            r"a\b", backslash_host, backslash_components, "a/b", slash_host, slash_components,
        );
        #[cfg(windows)]
        {
            assert_eq!(
                backslash_components, 2,
                "Windows splits the backslash, so the ONE-component manifest path `a\\b` is read as two"
            );
            assert_eq!(
                backslash_host, slash_host,
                "...so the distinct manifest paths `a\\b` and `a/b` collapse onto one host path"
            );
        }
        #[cfg(not(windows))]
        {
            assert_eq!(
                backslash_components, 1,
                "Unix keeps the backslash as an ordinary byte, so `a\\b` is ONE component"
            );
            assert_ne!(
                backslash_host, slash_host,
                "...so the host model is injective HERE and the collision is invisible to a Unix run"
            );
        }
    }

    /// THE FIX. `from_manifest` splits on `/` ONLY, so the manifest's segment
    /// sequence is exactly the host path's component sequence and the
    /// conversion is INJECTIVE: two DISTINCT manifest paths never yield the
    /// same host path. `a/b` is TWO components; on Unix `a\b` is ONE component
    /// with the backslash preserved verbatim (behaviour unchanged).
    #[test]
    fn manifest_conversion_splits_on_slash_only_and_is_injective() {
        let two = RootedRelativePath::from_manifest("a/b").unwrap();
        assert_eq!(two.as_path().components().count(), 2, "a/b is two segments");
        assert_eq!(two.as_path(), Path::new("a/b"));

        #[cfg(unix)]
        {
            let one = RootedRelativePath::from_manifest(r"a\b").unwrap();
            assert_eq!(
                one.as_path().components().count(),
                1,
                "a\\b is ONE manifest segment on Unix"
            );
            let only = one.as_path().components().next().unwrap();
            assert_eq!(
                only.as_os_str(),
                std::ffi::OsStr::new(r"a\b"),
                "the backslash is preserved verbatim as one name"
            );
        }

        // Injectivity over the spellings that matter: the two host-model
        // colliders plus neighbours that differ only by segmentation.
        let manifest_paths = [
            "a",
            "a/b",
            "a/b/c",
            r"a\b",
            r"a\b/c",
            r"a/b\c",
            r"a\b\c",
            "ab",
            "a/bc",
            "a/b/c.json",
            "generations/gen-1/assignment.json",
        ];
        let mut seen: std::collections::BTreeMap<PathBuf, &str> = std::collections::BTreeMap::new();
        for text in manifest_paths {
            // A manifest path the host cannot represent is refused, never
            // silently folded: that is the other half of injectivity.
            let Ok(rel) = RootedRelativePath::from_manifest(text) else {
                #[cfg(windows)]
                assert!(
                    text.contains('\\'),
                    "{text:?} must not be refused on Windows"
                );
                continue;
            };
            let key = rel.as_path().to_path_buf();
            if let Some(previous) = seen.insert(key.clone(), text) {
                panic!(
                    "distinct manifest paths {previous:?} and {text:?} both convert to the host path {key:?}"
                );
            }
        }
        #[cfg(not(windows))]
        assert_eq!(
            seen.len(),
            manifest_paths.len(),
            "on Unix every distinct manifest path converts to a distinct host path"
        );
    }

    /// THE FIX refuses a manifest path the host cannot represent instead of
    /// folding it. Platform-independent: on Unix `\` is an ordinary byte so
    /// the path is ACCEPTED as one component; on Windows the same conversion
    /// is REFUSED. Both directions are asserted here so the one authority's
    /// host-dependence is explicit.
    #[test]
    fn a_manifest_segment_the_host_cannot_name_is_refused_not_folded() {
        let conversion = RootedRelativePath::from_manifest(r"a\b");
        #[cfg(unix)]
        {
            let rel = conversion.expect("Unix represents a backslash byte");
            assert_eq!(rel.as_path().components().count(), 1);
        }
        #[cfg(windows)]
        {
            assert!(
                conversion.is_err(),
                "Windows cannot name a backslash byte, so the conversion must refuse it"
            );
            // The typed refusal names the path (and the offending segment).
            let message = conversion.unwrap_err().to_string();
            assert!(
                message.contains(r"a\b"),
                "the refusal must name the path: {message}"
            );
        }
        // Genuinely unsafe segment shapes are refused on EVERY platform.
        for bad in ["", "/a", "a/", "a//b", ".", "..", "a/./b", "a/../b"] {
            assert!(
                RootedRelativePath::from_manifest(bad).is_err(),
                "{bad:?} is not a canonical manifest path and must be refused"
            );
        }
    }
}
