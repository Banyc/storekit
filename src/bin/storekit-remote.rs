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
//! Each subcommand's stdout/stderr BYTES and its exit status are the contract
//! the corresponding transport script already has, because the transport
//! prefers the CLI and the two forms must be indistinguishable: `lstat` (the
//! framed `P`/`A`/`E` frames), `list` (the NUL-framed
//! `kind\tmode\tsize\tname` records, plus perl's `die` message and `die`
//! status on a real failure), and `verify-open` (the descriptor-bound
//! `O`/`N`/`E` frame).
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
        return match one_operand("lstat", &mut args) {
            Ok(path) => lstat(&path),
            Err(code) => code,
        };
    }
    if subcommand == OsStr::new("list") {
        return match one_operand("list", &mut args) {
            Ok(path) => list(&path),
            Err(code) => code,
        };
    }
    if subcommand == OsStr::new("verify-open") {
        return match one_operand("verify-open", &mut args) {
            Ok(path) => verify_open(&path),
            Err(code) => code,
        };
    }
    usage(&format!("unknown subcommand {subcommand:?}"))
}

/// The ONE operand a framed subcommand takes: `name` refuses with [`usage`]
/// when the operand is missing or a second one follows. No option parsing
/// exists, so a path that starts with `-` is always a path.
#[cfg(unix)]
fn one_operand<I: Iterator<Item = std::ffi::OsString>>(
    name: &str,
    args: &mut I,
) -> Result<std::ffi::OsString, std::process::ExitCode> {
    let Some(path) = args.next() else {
        return Err(usage(&format!("`{name}` requires exactly one path")));
    };
    if args.next().is_some() {
        return Err(usage(&format!("`{name}` requires exactly one path")));
    }
    Ok(path)
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

/// The framed `list` subcommand: the directory's entries as the transport's
/// perl listing helper prints them, BYTE for BYTE.
///
/// The frame is one NUL-terminated record per entry,
/// `kind\t<mode_hex>\t<size_decimal>\t<name>`, in `readdir` order (the
/// filesystem's order — nothing is sorted here; the near-side decoder sorts):
///
/// * `kind` is `l` for a symlink, `d` for a directory, and `f` for everything
///   else, classified from `lstat`'s `S_IFMT` bits (`lstat`, never `stat`, so a
///   DANGLING symlink is classified `l` and included — its size is the length
///   of its target);
/// * `<mode_hex>` is `st_mode & 0xffff` in lowercase hex, the same raw value
///   the `lstat` protocol carries;
/// * `<size_decimal>` is `st_size` in decimal;
/// * `<name>` is the `readdir` name as RAW BYTES — never re-encoded, never
///   trimmed — because NUL is the one byte a POSIX name cannot contain and is
///   therefore the only unambiguous delimiter.
///
/// `.`/`..` are skipped, an ABSENT directory is an empty listing with exit 0
/// (the transport treats an unprovisioned root as empty), and every other
/// `opendir`/`lstat` failure is a REAL failure: the perl script's `die` message
/// on stderr and its `die` status (see [`script_die`]).
#[cfg(unix)]
fn list(dir: &std::ffi::OsStr) -> std::process::ExitCode {
    use std::io::Write;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;

    // The one errno the script reads as confirmed absence (perl: `$! + 0 == 2`),
    // identical on Linux and macOS.
    const ENOENT: i32 = 2;

    let dir_bytes = dir.as_bytes();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => match error.raw_os_error() {
            Some(ENOENT) => return std::process::ExitCode::SUCCESS,
            errno => return list_failure(b"opendir", dir_bytes, errno),
        },
    };

    let mut out = std::io::stdout().lock();
    let mut failure: Option<(Vec<u8>, Option<i32>)> = None;
    for entry in entries {
        // The script's `for my $n (readdir($dh))` ends the loop when `readdir`
        // reports an error (`readdir` returns undef and its errno is never
        // inspected): an iteration error TRUNCATES the listing rather than
        // failing it, and this copy matches that exactly.
        let Ok(entry) = entry else { break };
        let name = entry.file_name();
        // `"$dir/$n"` is built UNCONDITIONALLY (so a `dir` already ending in
        // `/` yields `//` in BOTH the syscall path and the error message, just
        // as the script does).
        let mut child = dir_bytes.to_vec();
        child.push(b'/');
        child.extend_from_slice(name.as_bytes());
        let meta = match std::fs::symlink_metadata(std::ffi::OsStr::from_bytes(&child)) {
            Ok(meta) => meta,
            Err(error) => {
                failure = Some((child, error.raw_os_error()));
                break;
            }
        };
        let kind = meta.file_type();
        let tag = if kind.is_symlink() {
            b'l'
        } else if kind.is_dir() {
            b'd'
        } else {
            b'f'
        };
        let mut frame = Vec::with_capacity(child.len() + 24);
        frame.push(tag);
        frame.push(b'\t');
        frame.extend_from_slice(format!("{:x}", meta.mode() & 0xffff).as_bytes());
        frame.push(b'\t');
        frame.extend_from_slice(format!("{}", meta.len()).as_bytes());
        frame.push(b'\t');
        frame.extend_from_slice(name.as_bytes());
        frame.push(0);
        if let Err(error) = out.write_all(&frame) {
            return fail(&format!("writing the list frame failed: {error}"));
        }
    }

    // The script's stdout is buffered and flushed at exit — INCLUDING when a
    // later `lstat` dies — so the entries already printed stay on stdout.
    if let Err(error) = out.flush() {
        return fail(&format!("writing the list frame failed: {error}"));
    }
    match failure {
        Some((path, errno)) => list_failure(b"lstat", &path, errno),
        None => std::process::ExitCode::SUCCESS,
    }
}

/// The `list` subcommand's failure path: the perl script's `die` message for
/// `what`/`path` and its `die` exit status.
#[cfg(unix)]
fn list_failure(what: &[u8], path: &[u8], errno: Option<i32>) -> std::process::ExitCode {
    let mut message = b"list: ".to_vec();
    message.extend_from_slice(what);
    message.extend_from_slice(b" ");
    message.extend_from_slice(path);
    message.extend_from_slice(b": ");
    message.extend_from_slice(&errno_text(errno));
    // `die` appends ` at <file> line <n>.` because the message carries no
    // trailing newline; the script's perl is a ONE-LINE `-e` program, so the
    // location is always `-e line 1`. The bytes are part of the listing
    // contract the transport must not be able to tell apart.
    message.extend_from_slice(b" at -e line 1.\n");
    script_die(&message, errno)
}

/// Reproduce perl's `die`: write `message` (the exact bytes, location suffix
/// included) to stderr and exit with perl's status rule — `$!` when it is
/// nonzero, otherwise 255. The script's failure status is part of its contract,
/// so the CLI cannot invent a tidier one.
#[cfg(unix)]
fn script_die(message: &[u8], errno: Option<i32>) -> std::process::ExitCode {
    use std::io::Write;

    let _ = std::io::stderr().lock().write_all(message);
    match errno {
        Some(code) if (1..=255).contains(&code) => std::process::ExitCode::from(code as u8),
        _ => std::process::ExitCode::from(255),
    }
}

/// The C library's text for `errno` — the string perl's `$!` renders in a
/// string context (and so the one the transport's scripts print). Only the
/// `std` rendering (`… (os error N)`) would differ.
#[cfg(unix)]
fn errno_text(errno: Option<i32>) -> Vec<u8> {
    let Some(code) = errno else {
        // No errno set is the empty message, exactly as `$!` renders it.
        return Vec::new();
    };
    // SAFETY: `strerror` returns a pointer to a NUL-terminated, process-global
    // string for any code; it is copied into a fresh buffer immediately. A NULL
    // answer (only possible for a code the C library does not know) would make
    // `CStr::from_ptr` undefined, so it is checked rather than assumed.
    let ptr = unsafe { libc::strerror(code) };
    if ptr.is_null() {
        return format!("Unknown error {code}").into_bytes();
    }
    unsafe { std::ffi::CStr::from_ptr(ptr) }.to_bytes().to_vec()
}

/// The descriptor-bound `verify-open` subcommand: open `path` with
/// `O_NOFOLLOW | O_NONBLOCK`, `fstat` the SAME handle, and read a regular file
/// through that SAME handle, printing the transport's single frame and exiting
/// 0 for EVERY outcome (the frame carries the outcome; an exit status carries
/// no errno):
///
/// * `O\t<mode_hex>\n<content>` — a REGULAR file: the raw mode from the opened
///   descriptor's `fstat`, then the content read through that same descriptor
///   (raw bytes, possibly empty);
/// * `N\t<mode_hex>` — opened and `fstat`ed but NOT a regular file (directory,
///   FIFO, socket, device, ...);
/// * `E\t<errno>` — the open/`fstat`/read failed. The number is THIS host's
///   errno, exactly as the script prints it: `O_NOFOLLOW` makes a symlink
///   `ELOOP` (never followed, even to a matching regular file) and
///   `O_NONBLOCK` keeps a FIFO or device open from blocking.
///
/// The `custom_flags` call carries an item-level allow because the two bits it
/// forwards are NON-ADOPTING (`O_NOFOLLOW`, `O_NONBLOCK`): neither can create a
/// name, which is the adoption class that lint exists to close.
#[cfg(unix)]
#[allow(clippy::disallowed_methods)]
fn verify_open(path: &std::ffi::OsStr) -> std::process::ExitCode {
    use std::io::Write;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let frame = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Err(error) => e_frame(error.raw_os_error()),
        Ok(file) => {
            // `std`'s metadata on an OPEN FILE is `fstat` on that very
            // descriptor, so the mode below comes from the inode the open
            // returned, not from a second look at the path.
            match file.metadata() {
                Err(error) => e_frame(error.raw_os_error()),
                Ok(meta) => {
                    let raw = meta.mode() & 0xffff;
                    if meta.is_file() {
                        let mut content = Vec::new();
                        // `&File` implements `Read`: the content comes from the
                        // SAME descriptor the mode did.
                        match std::io::Read::read_to_end(&mut &file, &mut content) {
                            Ok(_) => {
                                let mut frame = format!("O\t{raw:x}\n").into_bytes();
                                frame.extend_from_slice(&content);
                                frame
                            }
                            Err(error) => e_frame(error.raw_os_error()),
                        }
                    } else {
                        format!("N\t{raw:x}\n").into_bytes()
                    }
                }
            }
        }
    };
    let mut out = std::io::stdout().lock();
    match out.write_all(&frame).and_then(|()| out.flush()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => fail(&format!("writing the verify-open frame failed: {error}")),
    }
}

/// The `E` frame: the errno as THIS host reports it (`$! + 0`, so an unset
/// errno renders as 0 exactly as perl would).
#[cfg(unix)]
fn e_frame(errno: Option<i32>) -> Vec<u8> {
    format!("E\t{}\n", errno.unwrap_or(0)).into_bytes()
}

#[cfg(unix)]
fn usage(problem: &str) -> std::process::ExitCode {
    fail(&format!(
        "{problem}\nusage: storekit-remote protocol\n       storekit-remote lstat <path>\n       storekit-remote list <path>\n       storekit-remote verify-open <path>"
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
