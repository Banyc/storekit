//! Platform helpers: the small Unix/Windows divergences (file mode bits,
//! symlinks) consolidated behind ONE cfg switch at this module boundary.
//!
//! * [`chmod`] — set a file's mode bits; a no-op on Windows (no Unix mode
//!   bits; file ACLs are the privacy mechanism).
//! * [`file_mode`] — read a file's mode bits; a fixed conventional mode on
//!   Windows.
//! * [`symlink`] — create a symlink; on Windows, best-effort via the
//!   platform symlink API (which requires admin/developer mode — a failure
//!   propagates, documented).
//!
//! The rest of the crate calls these helpers and never sees the switch.

use std::path::Path;

/// Set a file's mode bits — a no-op on Windows (no Unix mode bits; file
/// ACLs are the privacy mechanism). Documented weaker guarantee of the
/// Windows port.
///
/// This is the crate's ONE PATH-BASED mode authority, so the crate-root
/// `#![deny(clippy::disallowed_methods)]` is relaxed here for this single
/// `std::fs::set_permissions` call: `set_permissions` changes an inode's mode,
/// never its name, so it cannot free or swap a lock record's inode — but it
/// RE-RESOLVES the path (and so can be redirected by a symlink at that path),
/// which is exactly why every path-based mode change must stay on this one
/// entry point.
///
/// The FD-BOUND `std::fs::File::set_permissions` method is the PERMITTED
/// second form. It chmods an already-open descriptor and cannot re-resolve a
/// name, so it needs no `allow` and none is granted for it. Only the
/// PATH-taking free function [`std::fs::set_permissions`] is denied (there is
/// no `Path::set_permissions` method; a path-based chmod outside `chmod` would
/// have to spell that free function, which the deny catches). The production
/// FD-bound sites are:
/// `lock::FileLock::acquire` (the newly opened lock record);
/// `sync::apply`'s `set_local_mode`; the `atomic::unix` fd-bound helpers
/// (`open_verbatim_source`, `replace_core`, `write_atomic_cas_fd`,
/// `ensure_private_dir_fd`, `ensure_private_dir_durable_fd_path`,
/// `set_private_fd`, `create_destination_chain`, `copy_dir_recursive_fd`,
/// `set_dir_mode_fd`); and `transport::LocalTransport`'s `write_confined` /
/// `set_mode_confined`. Every receiver is a descriptor obtained from an
/// `O_NOFOLLOW` `openat`/`dup`, or the newly opened lock-record [`File`].
///
/// [`std::fs::set_permissions`]: std::fs::set_permissions
/// [`File`]: std::fs::File
#[allow(clippy::disallowed_methods)]
pub fn chmod(path: &Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }
    #[cfg(windows)]
    {
        let _ = (path, mode);
        Ok(())
    }
}

/// Read a file's mode bits — a fixed conventional mode (0o644) on Windows
/// (no Unix mode bits). Documented weaker guarantee of the Windows port.
pub fn file_mode(path: &Path) -> std::io::Result<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(std::fs::metadata(path)?.mode())
    }
    #[cfg(windows)]
    {
        let _ = path;
        Ok(0o644)
    }
}

/// The mode bits of an already-read [`std::fs::Metadata`] — a fixed
/// conventional mode (0o644) on Windows (no Unix mode bits). Documented
/// weaker guarantee of the Windows port.
pub fn metadata_mode(m: &std::fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        m.mode()
    }
    #[cfg(windows)]
    {
        let _ = m;
        0o644
    }
}

/// Create a symlink — on Windows, best-effort via the platform symlink API
/// (which requires admin/developer mode; a failure propagates). Documented
/// weaker guarantee of the Windows port.
///
/// This is the crate's ONE production std-symlink site (a name CREATION), so
/// the crate-root `#![deny(clippy::disallowed_methods)]` is relaxed here for
/// exactly this function: the three platform-symlink wrappers below are on the
/// deny list and this is the single reviewed entry point to them. No other
/// production site may call them.
#[allow(clippy::disallowed_methods)]
pub fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        // Best-effort: try a directory symlink first (the common case — the
        // `current`/`root` layout links point at directories), then a file
        // symlink. Both require admin/developer mode on Windows; a failure
        // propagates. Written FULLY QUALIFIED (rather than through a `use`)
        // so the source audit sees the canonical `std::os::windows::fs::…`
        // spelling, not an import route.
        match std::os::windows::fs::symlink_dir(target, link) {
            Ok(()) => Ok(()),
            Err(_) => std::os::windows::fs::symlink_file(target, link),
        }
    }
}
