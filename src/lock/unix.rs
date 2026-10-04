//! The Unix advisory lock: POSIX `flock` on the open file descriptor.
//! Selected by the single `#[cfg(unix)]` `mod` declaration in [`super`].

pub(crate) use super::LockAttempt;
use std::os::unix::io::AsRawFd;

/// Open (creating on first use) the lock record WITHOUT FOLLOWING A SYMLINK at
/// the record's own path (`O_NOFOLLOW`) and without a truncating open
/// (`truncate(false)`). The record's stable inode is the whole point of the
/// lock, so a symlink planted at its spelling must fail closed here, before any
/// `set_permissions`/`set_len`/write can be redirected through the link into an
/// arbitrary victim file. A symlink makes `open` fail `ELOOP`; the caller
/// maps that to a typed refusal. `O_CLOEXEC` keeps the descriptor out of a
/// spawned far-side helper.
//
// The LOCK PROTOCOL's own record open: this is the ONE function that may ADOPT
// the lock record's name (its `create(true)` flag), at the path
// [`crate::lock::FileLock::acquire`] passes from the protocol — never a
// caller's store-relative name. What makes it safe is the `O_NOFOLLOW` above
// together with `truncate(false)`: a symlink planted at the spelling fails
// `ELOOP` before any chmod/write, and an existing record is opened, never
// replaced. The reserved-spelling guard points the OTHER way (it stops a
// caller from unlinking/replacing/truncating the holder's inode), so it has no
// jurisdiction here and the crate-root deny is relaxed for exactly this
// function.
#[allow(clippy::disallowed_methods)]
pub(crate) fn open_lock_file(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    opts.open(path)
}

/// Whether an error from [`open_lock_file`] is the SYMLINK refusal (rather
/// than a real open failure), so the caller can name the condition.
pub(crate) fn is_symlink_open_error(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(libc::ELOOP)
}

/// Try to acquire the exclusive, non-blocking advisory lock.
pub(crate) fn try_lock(file: &std::fs::File) -> LockAttempt {
    let fd = file.as_raw_fd();
    let ret = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
    if ret == 0 {
        LockAttempt::Acquired
    } else {
        let err = std::io::Error::last_os_error();
        match err.raw_os_error() {
            Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN => {
                LockAttempt::Contended
            }
            _ => LockAttempt::Failed(err),
        }
    }
}

/// Release the advisory lock (best-effort — the kernel releases it when
/// the descriptor drops even if this never ran).
pub(crate) fn unlock(file: &std::fs::File) {
    unsafe {
        libc::flock(file.as_raw_fd(), libc::LOCK_UN);
    }
}

/// The errno that means "another holder" (EWOULDBLOCK) — the wait/retry
/// policy's contention signal.
pub(crate) fn contended_errno() -> i32 {
    libc::EWOULDBLOCK
}
