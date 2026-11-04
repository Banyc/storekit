//! The `sync-exclusions` contract, end to end through the PUBLIC API.
//!
//! A run may OMIT a caller-named set of root-relative subtrees from both views.
//! Every test here names the rule it pins; the rule numbers are the ones in
//! `docs/INVENTORY.md`'s `sync-exclusions` section.
//!
//! The subtrees used are the ones that make the rule necessary in practice: a
//! `.git` directory holding an ABSOLUTE symlink and a HARD LINK, and a FIFO.
//! The crate's strict source walk refuses each of them, so without the exclusion
//! the presence of the subtree alone refuses the run.
//!
//! Unix only: a hard link's `nlink`, an absolute symlink, and a FIFO all need
//! POSIX semantics to build. The Windows port cannot see `nlink`, and the
//! Windows gate compiles this file out rather than pretending to test it.

#![cfg(unix)]
#![allow(clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::path::Path;

use storekit::env::SysEnv;
use storekit::error::{MaterializationKind, PreflightKind, StoreKind};
use storekit::manifest::canonicalize_tree;
use storekit::sync::{
    DestinationOwnership, Direction, Exclusions, Extraneous, ReplaceAll, SyncError, SyncOptions,
    SyncReport, sync,
};
use storekit::transport::{Layout, LocalTransport};

fn env() -> SysEnv {
    SysEnv::from_process()
}

fn transport(root: &Path) -> LocalTransport {
    LocalTransport::new(&env(), root.to_path_buf(), Layout::empty()).unwrap()
}

fn write(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, bytes).unwrap();
}

fn options(extraneous: Extraneous, exclusions: &[&str]) -> SyncOptions {
    SyncOptions::new(extraneous, Exclusions::parse(exclusions).unwrap())
}

/// Run the OWNED path: acquire the destination's operation lock with the SAME
/// exclusions the run presents, then [`sync`].
fn owned(
    direction: Direction,
    local_root: &Path,
    remote: &dyn storekit::transport::Remote,
    options: &SyncOptions,
) -> Result<SyncReport, SyncError> {
    let ownership = DestinationOwnership::lock(direction, local_root, remote, &options.exclusions)?;
    sync(
        direction,
        local_root,
        remote,
        &ReplaceAll,
        options.clone(),
        ownership,
    )
}

/// A `.git` subtree that the strict source walk refuses: an ABSOLUTE symlink and
/// a HARD LINK. Nothing outside `.git` is unrepresentable, so the ONLY refusal
/// a run can raise for this tree comes from inside the subtree.
fn seed_refusing_git(root: &Path) {
    write(&root.join("f"), b"payload");
    write(&root.join(".git/HEAD"), b"ref: refs/heads/main\n");
    write(&root.join(".git/hard-a"), b"hard-linked\n");
    std::fs::hard_link(root.join(".git/hard-a"), root.join(".git/hard-b")).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", root.join(".git/absolute-link")).unwrap();
}

/// A `.git` subtree holding only a HARD LINK, so the strict walk's refusal is
/// unambiguously [`MaterializationKind::HardLink`].
fn seed_hard_linked_git(root: &Path) {
    write(&root.join("f"), b"payload");
    write(&root.join(".git/hard-a"), b"hard-linked\n");
    std::fs::hard_link(root.join(".git/hard-a"), root.join(".git/hard-b")).unwrap();
}

/// A `.git` subtree holding only an ABSOLUTE symlink, so the strict walk's
/// refusal is unambiguously [`MaterializationKind::AbsoluteSymlink`].
fn seed_absolute_symlink_git(root: &Path) {
    write(&root.join("f"), b"payload");
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", root.join(".git/absolute-link")).unwrap();
}

/// The NAME of a non-regular entry's kind, so a snapshot records that a FIFO
/// survived AS a FIFO (the property the exclusion promises) rather than merely
/// recording "something" at that spelling.
fn special_kind(meta: &std::fs::Metadata) -> &'static str {
    use std::os::unix::fs::FileTypeExt;
    let kind = meta.file_type();
    if kind.is_fifo() {
        "fifo"
    } else if kind.is_socket() {
        "socket"
    } else if kind.is_char_device() {
        "char-device"
    } else if kind.is_block_device() {
        "block-device"
    } else {
        "other"
    }
}

fn mkfifo(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let status = std::process::Command::new("mkfifo")
        .arg(path)
        .status()
        .expect("run mkfifo");
    assert!(status.success(), "mkfifo {path:?} failed");
}

/// A total, order-independent description of a tree: every entry's relative
/// spelling, its kind, and its bytes (a file's content, a symlink's raw target,
/// or a hard link's INODE IDENTITY — two names on one inode hash equal, so a
/// copy that split the link would differ). A non-regular entry (a FIFO, socket,
/// or device) is described WITHOUT opening it: opening a FIFO would block.
fn snapshot(root: &Path) -> BTreeMap<String, String> {
    use std::os::unix::fs::MetadataExt;
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            let kind = meta.file_type();
            let description = if kind.is_symlink() {
                format!("symlink:{}", std::fs::read_link(&path).unwrap().display())
            } else if kind.is_dir() {
                stack.push(path);
                format!("dir:{:04o}", meta.mode() & 0o7777)
            } else if kind.is_file() {
                format!(
                    "file:{:04o}:inode{}:{}",
                    meta.mode() & 0o7777,
                    meta.ino(),
                    storekit::digest::sha256_bytes(&std::fs::read(&path).unwrap())
                )
            } else {
                // A non-regular entry is described by KIND and mode alone, and
                // is never opened: opening a FIFO for read with no writer blocks
                // forever.
                format!(
                    "special:{}:{:04o}:inode{}",
                    special_kind(&meta),
                    meta.mode() & 0o7777,
                    meta.ino()
                )
            };
            out.insert(rel, description);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Rule 5 (the source walk) + the acceptance bullet: the same tree refuses
// WITHOUT the exclusion and syncs WITH it. Both directions.
// ---------------------------------------------------------------------------

#[test]
fn a_push_syncs_a_hard_linked_git_subtree_only_when_it_is_excluded() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    seed_hard_linked_git(&src);
    std::fs::create_dir_all(&dst).unwrap();
    let before = snapshot(&dst);

    let refusal = owned(
        Direction::Push,
        &src,
        &transport(&dst),
        &SyncOptions::default(),
    )
    .expect_err("a hard link in the source must refuse a run with no exclusions");
    assert_eq!(
        refusal.error().materialization_reason(),
        Some(MaterializationKind::HardLink),
        "the refusal must be the typed hard-link condition: {refusal:?}"
    );
    assert_eq!(
        snapshot(&dst),
        before,
        "the refused run must mutate nothing"
    );

    let report = owned(
        Direction::Push,
        &src,
        &transport(&dst),
        &options(Extraneous::Keep, &[".git"]),
    )
    .expect("the excluded subtree must not refuse the run");
    assert_eq!(
        report.matched_exclusions,
        vec![".git".to_string()],
        "the report names the exclusion that covered an entry"
    );
    // The transferred tree is the source WITHOUT `.git`, and `.git` was never
    // read, so nothing under it was copied.
    let transferred = snapshot(&dst);
    assert!(transferred.contains_key("f"));
    assert!(
        !transferred.keys().any(|path| path.starts_with(".git")),
        "no excluded entry may be transferred: {transferred:?}"
    );
}

#[test]
fn a_push_syncs_a_git_subtree_with_an_absolute_symlink_only_when_it_is_excluded() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    seed_absolute_symlink_git(&src);
    std::fs::create_dir_all(&dst).unwrap();

    let refusal = owned(
        Direction::Push,
        &src,
        &transport(&dst),
        &SyncOptions::default(),
    )
    .expect_err("an absolute symlink in the source must refuse a run with no exclusions");
    assert_eq!(
        refusal.error().materialization_reason(),
        Some(MaterializationKind::AbsoluteSymlink),
        "the refusal must be the typed absolute-symlink condition: {refusal:?}"
    );

    owned(
        Direction::Push,
        &src,
        &transport(&dst),
        &options(Extraneous::Keep, &[".git"]),
    )
    .expect("the excluded subtree must not refuse the run");
    assert!(dst.join("f").exists());
    assert!(!dst.join(".git").exists());
}

#[test]
fn a_pull_syncs_a_git_subtree_with_a_hard_link_and_an_absolute_symlink_when_excluded() {
    let tmp = tempfile::tempdir().unwrap();
    let remote_root = tmp.path().join("remote");
    let dst = tmp.path().join("dst");
    seed_refusing_git(&remote_root);
    std::fs::create_dir_all(&dst).unwrap();

    let refusal = owned(
        Direction::Pull,
        &dst,
        &transport(&remote_root),
        &SyncOptions::default(),
    )
    .expect_err("the unrepresentable source subtree must refuse a run with no exclusions");
    assert!(
        matches!(
            refusal.error().materialization_reason(),
            Some(MaterializationKind::HardLink | MaterializationKind::AbsoluteSymlink)
        ),
        "the refusal must be one of the typed address-fidelity conditions the strict walk \
         raises for the entries inside `.git` (the walk order decides which comes first): \
         {refusal:?}"
    );
    assert_eq!(
        std::fs::read_dir(&dst).unwrap().count(),
        0,
        "a refused pull must create nothing in the destination root"
    );

    let report = owned(
        Direction::Pull,
        &dst,
        &transport(&remote_root),
        &options(Extraneous::Keep, &[".git"]),
    )
    .expect("the excluded subtree must not refuse the pull");
    assert_eq!(report.matched_exclusions, vec![".git".to_string()]);
    assert_eq!(std::fs::read(dst.join("f")).unwrap(), b"payload");
    assert!(!dst.join(".git").exists());
}

/// Rule 5's "not walked" in its strongest form: a FIFO inside the excluded
/// subtree. The strict walk refuses a special file, and it never opens one (it
/// classifies from `symlink_metadata`), so the excluded run provably never
/// described the entry.
#[test]
fn a_push_never_walks_an_excluded_subtree_holding_a_fifo() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    write(&src.join("f"), b"payload");
    mkfifo(&src.join(".git/pipe"));
    std::fs::create_dir_all(&dst).unwrap();

    let refusal = owned(
        Direction::Push,
        &src,
        &transport(&dst),
        &SyncOptions::default(),
    )
    .expect_err("a FIFO in the source must refuse a run with no exclusions");
    assert_eq!(
        refusal.error().materialization_reason(),
        Some(MaterializationKind::SpecialFile),
        "the refusal must be the typed special-file condition: {refusal:?}"
    );

    owned(
        Direction::Push,
        &src,
        &transport(&dst),
        &options(Extraneous::Keep, &[".git"]),
    )
    .expect("the excluded subtree must not be walked");
    assert!(dst.join("f").exists());
    assert!(!dst.join(".git").exists());
}

// ---------------------------------------------------------------------------
// Rule 6 + the acceptance bullet: `Extraneous::Delete` leaves an excluded
// destination subtree BYTE-IDENTICAL, in both directions, and it is never
// reported as extraneous.
// ---------------------------------------------------------------------------

#[test]
fn delete_leaves_an_excluded_destination_subtree_byte_identical_on_a_push() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    write(&src.join("f"), b"payload");
    // The destination holds the excluded subtree (with an absolute symlink and
    // a hard link, which the destination walk TOLERATES but the exclusion means
    // it does not even read) plus an ordinary destination-only entry.
    seed_absolute_symlink_git(&dst);
    write(&dst.join(".git/hard-a"), b"hard-linked\n");
    std::fs::hard_link(dst.join(".git/hard-a"), dst.join(".git/hard-b")).unwrap();
    write(&dst.join("stale"), b"destination-only");
    let git_before = snapshot(&dst.join(".git"));

    let report = owned(
        Direction::Push,
        &src,
        &transport(&dst),
        &options(Extraneous::Delete, &[".git"]),
    )
    .expect("the run succeeds");

    assert_eq!(
        report.extraneous,
        vec!["stale".to_string()],
        "ONLY the addressable destination-only entry is extraneous; the excluded subtree is not \
         even in the view"
    );
    assert!(
        !dst.join("stale").exists(),
        "the sanctioned removal must still happen"
    );
    assert_eq!(
        snapshot(&dst.join(".git")),
        git_before,
        "the excluded destination subtree must survive Delete byte-identical"
    );
    assert_eq!(report.matched_exclusions, vec![".git".to_string()]);
}

#[test]
fn delete_leaves_an_excluded_destination_subtree_byte_identical_on_a_pull() {
    let tmp = tempfile::tempdir().unwrap();
    let remote_root = tmp.path().join("remote");
    let dst = tmp.path().join("dst");
    write(&remote_root.join("f"), b"payload");
    // The destination's `.git` holds an absolute symlink and a hard link, which
    // the destination-tolerant walk RECORDS but does not refuse.
    seed_absolute_symlink_git(&dst);
    write(&dst.join(".git/hard-a"), b"hard-linked\n");
    std::fs::hard_link(dst.join(".git/hard-a"), dst.join(".git/hard-b")).unwrap();
    write(&dst.join("stale"), b"destination-only");
    let git_before = snapshot(&dst.join(".git"));

    let report = owned(
        Direction::Pull,
        &dst,
        &transport(&remote_root),
        &options(Extraneous::Delete, &[".git"]),
    )
    .expect("the run succeeds");

    assert_eq!(report.extraneous, vec!["stale".to_string()]);
    assert!(!dst.join("stale").exists());
    assert_eq!(std::fs::read(dst.join("f")).unwrap(), b"payload");
    assert_eq!(
        snapshot(&dst.join(".git")),
        git_before,
        "the excluded destination subtree must survive Delete byte-identical"
    );
    assert_eq!(report.matched_exclusions, vec![".git".to_string()]);
}

/// The DELIBERATE LIMIT of a LOCAL destination: it is read WHOLE so the
/// destination-aware containment check sees every component a surviving
/// entry occupies, and only its VIEW is filtered. A special file inside an
/// excluded destination subtree is therefore still refused by the tolerant
/// walk (it is the one refusal tolerance does not cover). The remedy is the
/// caller's: remove the special file, or move the subtree out of the root.
/// A FAR-SIDE destination does not have this limit: its listing is filtered
/// before assembly (rule 7), so the excluded line is never classified.
#[test]
fn a_local_destination_holding_a_special_file_inside_an_excluded_subtree_still_refuses() {
    let tmp = tempfile::tempdir().unwrap();
    let remote_root = tmp.path().join("remote");
    let dst = tmp.path().join("dst");
    write(&remote_root.join("f"), b"payload");
    write(&dst.join("f"), b"old");
    mkfifo(&dst.join(".git/pipe"));

    let refusal = owned(
        Direction::Pull,
        &dst,
        &transport(&remote_root),
        &options(Extraneous::Keep, &[".git"]),
    )
    .expect_err("the local destination walk reads the excluded subtree");
    assert_eq!(
        refusal.error().materialization_reason(),
        Some(MaterializationKind::SpecialFile),
        "the refusal must be the typed special-file condition: {refusal:?}"
    );
    assert!(
        std::fs::symlink_metadata(dst.join(".git/pipe")).is_ok(),
        "the refusal mutates nothing"
    );
}

// ---------------------------------------------------------------------------
// Rule 3: component boundaries.
// ---------------------------------------------------------------------------

#[test]
fn node_modules_does_not_exclude_node_modules_bak() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    write(&src.join("node_modules/x"), b"excluded");
    write(&src.join("node_modules.bak/y"), b"kept");
    std::fs::create_dir_all(&dst).unwrap();

    owned(
        Direction::Push,
        &src,
        &transport(&dst),
        &options(Extraneous::Keep, &["node_modules"]),
    )
    .expect("the run succeeds");

    assert!(!dst.join("node_modules").exists());
    assert_eq!(
        std::fs::read(dst.join("node_modules.bak/y")).unwrap(),
        b"kept",
        "a component boundary is not a prefix of a NAME"
    );
}

#[test]
fn a_child_spelling_excludes_only_the_child() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    write(&src.join("node_modules/x/keep"), b"excluded");
    write(&src.join("node_modules/y"), b"kept");
    std::fs::create_dir_all(&dst).unwrap();

    let report = owned(
        Direction::Push,
        &src,
        &transport(&dst),
        &options(Extraneous::Keep, &["node_modules/x"]),
    )
    .expect("the run succeeds");

    assert!(
        !dst.join("node_modules/x").exists(),
        "the named child must not be transferred"
    );
    assert_eq!(
        std::fs::read(dst.join("node_modules/y")).unwrap(),
        b"kept",
        "the child spelling covers the child, not its siblings"
    );
    assert_eq!(
        report.matched_exclusions,
        vec!["node_modules/x".to_string()]
    );
}

// ---------------------------------------------------------------------------
// Rules 1-2: the spelling refusals, with their typed kinds.
// ---------------------------------------------------------------------------

#[test]
fn the_empty_exclusion_and_a_root_spelling_are_refused_with_the_typed_kind() {
    for spelling in ["", ".", "..", "a/../b", "/absolute"] {
        let error = Exclusions::parse([spelling]).expect_err("the spelling must be refused");
        assert_eq!(
            error.store_reason(),
            Some(StoreKind::ExclusionSpelling),
            "the refusal for {spelling:?} must be the typed exclusion-spelling condition: {error:?}"
        );
    }
}

#[test]
fn a_wire_unrepresentable_exclusion_is_refused_with_the_typed_kind() {
    for spelling in ["a\nb", "a\rb", "a\tb", "a\0b"] {
        let error = Exclusions::parse([spelling]).expect_err("the spelling must be refused");
        assert_eq!(
            error.store_reason(),
            Some(StoreKind::ExclusionSpelling),
            "the refusal for {spelling:?} must be the typed exclusion-spelling condition: {error:?}"
        );
    }
}

#[test]
fn a_reserved_exclusion_is_refused_with_the_typed_kind() {
    for spelling in [
        "operation.lock",
        "state/operation.lock",
        ".dest.operation.lock",
        ".sync-aside.1",
        "d/.sync-aside.42.0",
        ".f.tmp.1.0",
        ".SYNC-ASIDE.1",
    ] {
        let error = Exclusions::parse([spelling]).expect_err("the spelling must be refused");
        assert_eq!(
            error.store_reason(),
            Some(StoreKind::ExclusionReserved),
            "the refusal for {spelling:?} must be the typed reserved condition: {error:?}"
        );
    }
}

#[test]
fn one_bad_spelling_refuses_the_whole_set() {
    let error = Exclusions::parse([".git", "operation.lock", "target"])
        .expect_err("one refused spelling refuses the set");
    assert_eq!(
        error.store_reason(),
        Some(StoreKind::ExclusionReserved),
        "a partially applied set would be a run whose views disagree with the caller: {error:?}"
    );
}

// ---------------------------------------------------------------------------
// Rule 8: the report names MATCHED exclusions and omits unmatched ones.
// ---------------------------------------------------------------------------

#[test]
fn the_report_names_matched_exclusions_and_omits_unmatched_ones() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    write(&src.join(".git/HEAD"), b"ref\n");
    write(&src.join("target/x"), b"excluded\n");
    write(&src.join("f"), b"payload");
    std::fs::create_dir_all(&dst).unwrap();

    let report = owned(
        Direction::Push,
        &src,
        &transport(&dst),
        &options(Extraneous::Keep, &["target", ".git", "typo"]),
    )
    .expect("the run succeeds");

    assert_eq!(
        report.matched_exclusions,
        vec![".git".to_string(), "target".to_string()],
        "the report names the exclusions that covered an entry, sorted, and NEVER names a \
         spelling that matched nothing (a typo must stay visible)"
    );
    assert!(
        !report.matched_exclusions.iter().any(|s| s == "typo"),
        "an unmatched exclusion must not be reported as applied"
    );
}

#[test]
fn an_exclusion_that_matches_only_a_destination_entry_is_named() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    write(&src.join("f"), b"payload");
    write(&dst.join("vendor/lib"), b"destination-only");
    let before = snapshot(&dst.join("vendor"));

    let report = owned(
        Direction::Push,
        &src,
        &transport(&dst),
        &options(Extraneous::Delete, &["vendor"]),
    )
    .expect("the run succeeds");

    assert_eq!(
        report.matched_exclusions,
        vec!["vendor".to_string()],
        "the destination view matched the exclusion, so it is named"
    );
    assert!(report.extraneous.is_empty());
    assert_eq!(snapshot(&dst.join("vendor")), before);
}

// ---------------------------------------------------------------------------
// Empty exclusions: byte-identical to today's behaviour (a HARD requirement,
// so it is pinned against behaviour that does not know about exclusions at all).
// ---------------------------------------------------------------------------

#[test]
fn empty_exclusions_change_nothing_about_a_refused_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    seed_refusing_git(&src);
    std::fs::create_dir_all(&dst).unwrap();

    // THE ORACLE is the pre-existing strict walk, which knows nothing about
    // exclusions: the run must refuse with the SAME type and the SAME message.
    let strict = canonicalize_tree(&src).expect_err("the strict walk refuses the tree");
    let refusal = owned(
        Direction::Push,
        &src,
        &transport(&dst),
        &SyncOptions::without_exclusions(Extraneous::Keep),
    )
    .expect_err("an empty exclusion set must not suppress the strict refusal");
    assert_eq!(
        refusal.error().materialization_reason(),
        strict.materialization_reason()
    );
    assert_eq!(
        refusal.error().to_string(),
        strict.to_string(),
        "the empty set must produce the strict walk's refusal verbatim"
    );
    assert_eq!(refusal.report().matched_exclusions, Vec::<String>::new());
    assert_eq!(
        std::fs::read_dir(&dst).unwrap().count(),
        0,
        "the refused run must create nothing"
    );
}

#[test]
fn empty_exclusions_change_nothing_about_a_successful_run() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    write(&src.join("a"), b"a");
    write(&src.join("d/b"), b"b");
    write(&src.join("d/c"), b"c");
    write(&dst.join("d/b"), b"b");
    write(&dst.join("gone"), b"destination-only");
    write(&dst.join("gone-dir/x"), b"destination-only");

    let report = owned(
        Direction::Push,
        &src,
        &transport(&dst),
        &SyncOptions::without_exclusions(Extraneous::Delete),
    )
    .expect("the run succeeds");

    // The EXACT report, pinned: with no exclusions the run's decisions are the
    // ones the engine made before the option existed.
    assert_eq!(report.applied, vec!["a".to_string(), "d/c".to_string()]);
    assert_eq!(report.skipped, vec!["d".to_string(), "d/b".to_string()]);
    assert_eq!(
        report.extraneous,
        vec![
            "gone".to_string(),
            "gone-dir".to_string(),
            "gone-dir/x".to_string(),
        ],
        "every destination-only path is extraneous with no exclusions"
    );
    assert_eq!(
        report.transfers, 5,
        "the pinned transfer count is unchanged"
    );
    assert!(report.transient_dirs.is_empty());
    assert!(report.residue.is_empty());
    assert!(report.verify_failures.is_empty());
    assert!(report.indeterminate.is_empty());
    assert!(report.unsupported_destination.is_empty());
    assert!(report.conflicts.is_empty(), "{:?}", report.conflicts);
    assert!(
        report.matched_exclusions.is_empty(),
        "the empty set matches nothing, so the report names nothing"
    );
    assert_eq!(
        canonicalize_tree(&dst).unwrap(),
        canonicalize_tree(&src).unwrap(),
        "the destination is byte-for-byte the source tree's manifest"
    );
}

#[test]
fn an_empty_exclusion_set_is_also_a_no_op_on_the_diff_view() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    write(&src.join("a"), b"a");
    write(&dst.join("b"), b"b");
    let source = canonicalize_tree(&src).unwrap();
    let destination = storekit::manifest::canonicalize_tree_destination(&dst).unwrap();

    let (filtered_source, filtered_destination) =
        storekit::sync::apply_manifests(&source, &destination, &Exclusions::none());
    assert_eq!(filtered_source, source);
    assert_eq!(
        filtered_destination.entries(),
        destination.entries(),
        "the empty set leaves the assembled view untouched"
    );
    assert_eq!(
        filtered_destination.tree_sha256(),
        destination.tree_sha256()
    );
}

/// The filter is IDEMPOTENT on the public diff view: applying the same set to
/// an already-filtered pair changes nothing (the source the engine produces is
/// filtered twice — once by its walk, once here).
#[test]
fn the_exclusion_filter_is_idempotent_on_the_public_diff_view() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    write(&src.join(".git/x"), b"excluded");
    write(&src.join("f"), b"payload");
    write(&dst.join(".git/y"), b"excluded");
    write(&dst.join("stale"), b"destination-only");
    let source = canonicalize_tree(&src).unwrap();
    let destination = storekit::manifest::canonicalize_tree_destination(&dst).unwrap();
    let exclusions = Exclusions::parse([".git"]).unwrap();

    let (source_once, destination_once) =
        storekit::sync::apply_manifests(&source, &destination, &exclusions);
    assert!(
        !source_once
            .entries
            .iter()
            .any(|entry| entry.path.starts_with(".git")),
        "the exclusion removes the source subtree from the view"
    );
    assert!(
        !destination_once
            .entries()
            .iter()
            .any(|entry| entry.path.starts_with(".git")),
        "the exclusion removes the destination subtree from the view"
    );

    let (source_twice, destination_twice) =
        storekit::sync::apply_manifests(&source_once, &destination_once, &exclusions);
    assert_eq!(source_once, source_twice, "the filter is idempotent");
    assert_eq!(destination_once.entries(), destination_twice.entries());
    assert_eq!(
        destination_once.tree_sha256(),
        destination_twice.tree_sha256()
    );
}

// ---------------------------------------------------------------------------
// Rules 9 + the watch: the exclusion set is the token's plan too, so a run
// that presents a different set is refused (the views would disagree).
// ---------------------------------------------------------------------------

#[test]
fn a_run_that_presents_a_different_exclusion_set_than_the_token_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    write(&src.join("f"), b"payload");
    write(&src.join(".git/HEAD"), b"ref\n");
    std::fs::create_dir_all(&dst).unwrap();
    let before = snapshot(&dst);

    let planned = options(Extraneous::Keep, &[".git"]);
    let presented = options(Extraneous::Keep, &["target"]);
    let ownership =
        DestinationOwnership::lock(Direction::Push, &src, &transport(&dst), &planned.exclusions)
            .expect("the token is minted with the plan's set");

    let error = sync(
        Direction::Push,
        &src,
        &transport(&dst),
        &ReplaceAll,
        presented,
        ownership,
    )
    .expect_err("a run with another exclusion set must be refused");
    assert_eq!(
        error.error().preflight_reason(),
        Some(PreflightKind::ExclusionsMismatch),
        "the refusal must be the typed exclusion-set mismatch: {error:?}"
    );
    assert_eq!(snapshot(&dst), before, "the refused run mutates nothing");
}

/// The SAME set in another ORDER is the same set: the comparison is set
/// equality, so a caller that builds the options twice is not refused.
#[test]
fn the_same_exclusion_set_in_another_order_is_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    write(&src.join("f"), b"payload");
    write(&src.join(".git/HEAD"), b"ref\n");
    write(&src.join("target/x"), b"excluded\n");
    std::fs::create_dir_all(&dst).unwrap();

    let planned = options(Extraneous::Keep, &[".git", "target"]);
    let presented = options(Extraneous::Keep, &["target", ".git"]);
    let ownership =
        DestinationOwnership::lock(Direction::Push, &src, &transport(&dst), &planned.exclusions)
            .expect("mint with the plan's set");
    let report = sync(
        Direction::Push,
        &src,
        &transport(&dst),
        &ReplaceAll,
        presented,
        ownership,
    )
    .expect("the same set in another order is the same set");
    assert_eq!(
        report.matched_exclusions,
        vec![".git".to_string(), "target".to_string()]
    );
}

// ---------------------------------------------------------------------------
// The view set is SHARED: the source walk and the destination view apply the
// same exclusions, so a tree that holds the subtree on BOTH sides syncs and
// neither side's copy moves.
// ---------------------------------------------------------------------------

#[test]
fn the_source_and_destination_views_agree_on_the_excluded_subtree() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    seed_refusing_git(&src);
    write(&dst.join("f"), b"old-payload");
    seed_absolute_symlink_git(&dst);
    let src_git = snapshot(&src.join(".git"));
    let dst_git = snapshot(&dst.join(".git"));

    owned(
        Direction::Push,
        &src,
        &transport(&dst),
        &options(Extraneous::Delete, &[".git"]),
    )
    .expect("the excluded subtree on both sides must not refuse the run");

    assert_eq!(
        std::fs::read(dst.join("f")).unwrap(),
        b"payload",
        "the ordinary content is still synced"
    );
    assert_eq!(
        snapshot(&src.join(".git")),
        src_git,
        "the source is untouched"
    );
    assert_eq!(
        snapshot(&dst.join(".git")),
        dst_git,
        "the destination's own `.git` is neither replaced nor removed"
    );
}

/// The far-side listing is filtered BEFORE assembly (rule 7), which is the one
/// place a consumer cannot reach through `LocalTransport`: with the exclusion
/// the listing's hard-link line is never assembled, so it cannot trip the
/// hard-link refusal. The remote source is a LOCAL tree here, and the same
/// filter runs on the in-process walk; the far-side branch is pinned separately
/// by `the_far_side_source_listing_is_filtered_before_assembly` and
/// `the_far_side_destination_listing_is_filtered_before_assembly`.
#[test]
fn a_pull_does_not_refuse_on_an_excluded_subtree_the_destination_never_held() {
    let tmp = tempfile::tempdir().unwrap();
    let remote_root = tmp.path().join("remote");
    let dst = tmp.path().join("dst");
    write(&remote_root.join("f"), b"payload");
    write(&remote_root.join(".git/HEAD"), b"ref\n");
    std::fs::create_dir_all(&dst).unwrap();

    let report = owned(
        Direction::Pull,
        &dst,
        &transport(&remote_root),
        &options(Extraneous::Keep, &[".git"]),
    )
    .expect("the pull succeeds");
    assert_eq!(report.matched_exclusions, vec![".git".to_string()]);
    assert!(!dst.join(".git").exists());
}

/// A mis-spelled exclusion is VISIBLE: the run succeeds, but the report names
/// nothing, so the caller can see the spelling covered no entry (and the strict
/// refusal for the subtree they meant to exclude is not silently suppressed by
/// a near-miss spelling either).
#[test]
fn a_typo_covers_nothing_and_is_not_reported_as_applied() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    write(&src.join(".git/HEAD"), b"ref\n");
    write(&src.join("f"), b"payload");
    std::fs::create_dir_all(&dst).unwrap();

    let report = owned(
        Direction::Push,
        &src,
        &transport(&dst),
        &options(Extraneous::Keep, &[".gti"]),
    )
    .expect("a spelling that matches nothing is not an error");
    assert!(
        report.matched_exclusions.is_empty(),
        "a typo must be reported as covering nothing, not as applied: {:?}",
        report.matched_exclusions
    );
    assert_eq!(
        std::fs::read(dst.join(".git/HEAD")).unwrap(),
        b"ref\n",
        "the mis-spelled exclusion excluded nothing, so `.git` was transferred"
    );
}

/// The NEAR-MISS spelling `.git/` is a DIFFERENT thing: the host path parse
/// drops the trailing separator, so it names `.git` and excludes it. Stated as
/// a test so the two outcomes ("covered nothing" vs "covered `.git`") are not
/// confused.
#[test]
fn a_trailing_separator_spelling_names_the_same_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    seed_hard_linked_git(&src);
    std::fs::create_dir_all(&dst).unwrap();

    let report = owned(
        Direction::Push,
        &src,
        &transport(&dst),
        &options(Extraneous::Keep, &[".git/"]),
    )
    .expect("`.git/` names the `.git` entry, so the refusing subtree is excluded");
    assert_eq!(report.matched_exclusions, vec![".git".to_string()]);
    assert!(!dst.join(".git").exists());
}

/// `SyncOptions::default` is the no-exclusion, `Keep` run: the value a caller
/// gets without naming either decision, and exactly today's behaviour.
#[test]
fn the_default_options_are_keep_with_no_exclusions() {
    let options = SyncOptions::default();
    assert_eq!(options.extraneous, Extraneous::Keep);
    assert!(options.exclusions.is_empty());
    assert_eq!(options.exclusions.spellings().count(), 0);
}
