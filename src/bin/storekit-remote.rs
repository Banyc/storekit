//! `storekit-remote`: the far-side executable an [`SshTransport`] prefers over
//! its portable perl scripts, when the remote account has it on `PATH` and it
//! states a compatible protocol.
//!
//! The transport probes `storekit-remote protocol` ONCE per instance (see
//! `SshTransport`'s far-side mode); this binary's answer is the protocol token
//! ([`STOREKIT_REMOTE_PROTOCOL_TOKEN`]) and one newline, nothing else. A
//! different version is a WRONG TOKEN and the transport falls back to the
//! scripts, so the token — not the binary's name — is the compatibility
//! contract.
//!
//! Subcommands take their operands as [`std::ffi::OsString`] (`args_os`), so a
//! path's bytes reach the system call unmodified; nothing here re-encodes a
//! name.
//!
//! The far-side body is Unix-only (it is built around `lstat`, `MetadataExt`,
//! and POSIX errnos). The binary still COMPILES for a non-Unix target — the
//! `--all-targets` gate builds it — but its `main` refuses to run there.
//!
//! [`SshTransport`]: storekit::transport::SshTransport

#[cfg(unix)]
use storekit::transport::STOREKIT_REMOTE_PROTOCOL_TOKEN;

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    use std::ffi::OsStr;

    let mut args = std::env::args_os();
    // The manual argv parser: `argv[0]` is the program, every later element is
    // an operand. No option parsing exists, so a path that starts with `-` is
    // always a path.
    let _program = args.next();
    let Some(subcommand) = args.next() else {
        return usage("missing subcommand");
    };
    if subcommand == OsStr::new("protocol") {
        if args.next().is_some() {
            return usage("`protocol` takes no operands");
        }
        let mut out = std::io::stdout().lock();
        return match std::io::Write::write_all(&mut out, protocol_wire().as_bytes())
            .and_then(|()| std::io::Write::flush(&mut out))
        {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => fail(&format!("writing the protocol token failed: {error}")),
        };
    }
    if subcommand == OsStr::new("lstat") {
        let Some(path) = args.next() else {
            return usage("`lstat` requires exactly one path");
        };
        if args.next().is_some() {
            return usage("`lstat` requires exactly one path");
        }
        return lstat(&path);
    }
    usage(&format!("unknown subcommand {subcommand:?}"))
}

/// The `protocol` subcommand's whole stdout: the shared token and one LF.
#[cfg(unix)]
fn protocol_wire() -> String {
    format!("{STOREKIT_REMOTE_PROTOCOL_TOKEN}\n")
}

/// The framed `lstat` subcommand: print EXACTLY the frame the transport's perl
/// helper prints, and exit 0 for every outcome (the frame carries the outcome;
/// an exit code carries no errno):
///
/// * `P\t<size>\t<rawmode_hex>` — `lstat` succeeded; `<size>` is decimal and
///   `<rawmode>` is the raw `st_mode` masked to 16 bits, matching the script's
///   `$s[2] & 0xffff`;
/// * `A\t<errno>` — the failure's errno is ENOENT (2) or ENOTDIR (20), the two
///   errnos that mean "no such entry";
/// * `E\t<errno>` — any other failure (EACCES, EIO, ELOOP, ...).
///
/// `symlink_metadata` is `lstat`: it reports a symlink itself, never its
/// target.
#[cfg(unix)]
fn lstat(path: &std::ffi::OsStr) -> std::process::ExitCode {
    use std::os::unix::fs::MetadataExt;

    // The two errnos that mean confirmed absence; identical on Linux and macOS
    // (POSIX), and the same pair the transport's lstat contract names.
    const ENOENT: i32 = 2;
    const ENOTDIR: i32 = 20;

    let frame = match std::fs::symlink_metadata(path) {
        Ok(meta) => format!("P\t{}\t{:x}\n", meta.len(), meta.mode() & 0xffff),
        Err(error) => match error.raw_os_error() {
            Some(errno) => {
                let tag = if errno == ENOENT || errno == ENOTDIR {
                    "A"
                } else {
                    "E"
                };
                format!("{tag}\t{errno}\n")
            }
            // Every filesystem failure comes from a syscall and so carries an
            // errno. Fabricating one would turn "unknown" into a verdict, so
            // this refuses instead of printing a frame.
            None => {
                return fail(&format!("lstat failed without an errno: {error}"));
            }
        },
    };
    let mut out = std::io::stdout().lock();
    match std::io::Write::write_all(&mut out, frame.as_bytes())
        .and_then(|()| std::io::Write::flush(&mut out))
    {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => fail(&format!("writing the lstat frame failed: {error}")),
    }
}

#[cfg(unix)]
fn usage(problem: &str) -> std::process::ExitCode {
    fail(&format!(
        "{problem}\nusage: storekit-remote protocol\n       storekit-remote lstat <path>"
    ))
}

/// Report a usage/IO/refusal problem on stderr and exit non-zero. Never used
/// for an `lstat` OUTCOME: those are frames on stdout with exit 0.
#[cfg(unix)]
fn fail(message: &str) -> std::process::ExitCode {
    eprintln!("storekit-remote: {message}");
    std::process::ExitCode::from(2)
}

/// The far-side body is Unix-only; a non-Unix build refuses to run.
#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    eprintln!(
        "storekit-remote: this far-side executable is Unix-only; build and install it on the \
         remote Unix host (the transport falls back to its perl scripts when the probe cannot \
         run here)"
    );
    std::process::ExitCode::from(2)
}
