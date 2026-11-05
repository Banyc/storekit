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
//! status on a real failure), `verify-open` (the descriptor-bound
//! `O`/`N`/`E` frame), the three payload writers `publish` (atomic
//! replacement: temp + mode + file fsync + `rename(2)` + parent fsync),
//! `write-new` (no-clobber install by `link(2)`) and `sidecar-publish` (the
//! lock-record install under the sidecar's `flock`), and the durability and
//! residue operations: `fsync-file`/`fsync-dir` (the perl fsync primitive),
//! `fsync-tree` (the `find -depth` walk), `rename` (parent + `rename(2)`), and
//! `remove-file-if`/`remove-file-if-sidecar` (compare-and-delete, the second
//! under the sidecar's `flock`), whose one-byte `R`/`M`/`A` frame is match,
//! mismatch and absent, `manifest` (the recursive sync tree walk), whose
//! tab-delimited records, refusals and exit statuses are
//! [`storekit::manifest::remote_tree_verify_script`]'s byte for byte, and `lock`
//! (the long-lived far-side operation-lock HOLDER), whose `LOCKOK` readiness
//! line, reserved exit codes and anchored diagnostics are the transport's.
//!
//! `lock` is the ONE long-lived subcommand: it prints its readiness line, then
//! blocks reading STDIN until EOF while it KEEPS the `flock`, exactly as the
//! perl holder does — the lock is held for the caller's SESSION, not for one
//! command, and process death (a dropped connection) releases it.
//!
//! The payload is NEVER an argv token: every writer reads its raw bytes from
//! STDIN, exactly as the transport pipes them, so arbitrary bytes round-trip.
//! `publish` and `write-new` take `<path> <mode> <len>` where `<mode>` is the
//! OCTAL spelling `chmod` accepts (so `0` means "no chmod" for `publish`) and
//! `<len>` is the DECIMAL payload length the writer verifies after STDIN EOF
//! and before the install.
//!
//! The far-side body is Unix-only (it is built around `lstat`, `MetadataExt`,
//! and POSIX errnos). The binary still COMPILES for a non-Unix target — the
//! `--all-targets` gate builds it — but its `main` refuses to run there.
//!
//! [`SshTransport`]: storekit::transport::SshTransport

#[cfg(unix)]
use storekit::transport::{
    SSH_TWRITE_CONFLICT_EXIT, SSH_TWRITE_PREINSTALL_EXIT, SSH_UPLOAD_POST_RENAME_EXIT,
    STOREKIT_REMOTE_PROTOCOL_TOKEN,
};

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
    if subcommand == OsStr::new("manifest") {
        return match one_operand("manifest", &mut args) {
            Ok(root) => manifest(&root),
            Err(code) => code,
        };
    }
    if subcommand == OsStr::new("verify-open") {
        return match one_operand("verify-open", &mut args) {
            Ok(path) => verify_open(&path),
            Err(code) => code,
        };
    }
    if subcommand == OsStr::new("publish") {
        return match three_operands("publish", &mut args) {
            Ok((path, mode, len)) => publish(&path, mode, len),
            Err(code) => code,
        };
    }
    if subcommand == OsStr::new("write-new") {
        return match three_operands("write-new", &mut args) {
            Ok((path, mode, len)) => write_new(&path, mode, len),
            Err(code) => code,
        };
    }
    if subcommand == OsStr::new("sidecar-publish") {
        return match four_operands("sidecar-publish", &mut args) {
            Ok((sidecar, lock, mode, len)) => sidecar_publish(&sidecar, &lock, mode, len),
            Err(code) => code,
        };
    }
    if subcommand == OsStr::new("fsync-file") {
        return match one_operand("fsync-file", &mut args) {
            Ok(path) => fsync_primitive(&path),
            Err(code) => code,
        };
    }
    if subcommand == OsStr::new("fsync-dir") {
        return match one_operand("fsync-dir", &mut args) {
            Ok(path) => fsync_primitive(&path),
            Err(code) => code,
        };
    }
    if subcommand == OsStr::new("fsync-tree") {
        return match one_operand("fsync-tree", &mut args) {
            Ok(path) => fsync_tree(&path),
            Err(code) => code,
        };
    }
    if subcommand == OsStr::new("rename") {
        return match two_operands("rename", &mut args) {
            Ok((from, to)) => rename(&from, &to),
            Err(code) => code,
        };
    }
    if subcommand == OsStr::new("remove-file-if") {
        return match two_operands("remove-file-if", &mut args) {
            Ok((path, expected)) => remove_file_if(&path, &expected),
            Err(code) => code,
        };
    }
    if subcommand == OsStr::new("remove-file-if-sidecar") {
        return match three_operands_raw("remove-file-if-sidecar", &mut args) {
            Ok((sidecar, lock, expected)) => remove_file_if_sidecar(&sidecar, &lock, &expected),
            Err(code) => code,
        };
    }
    if subcommand == OsStr::new("lock") {
        return match two_operands("lock", &mut args) {
            Ok((record, op_id)) => lock_holder(&record, &op_id),
            Err(code) => code,
        };
    }
    usage(&format!("unknown subcommand {subcommand:?}"))
}

/// The payload-writer operand triple: `<path> <octal mode> <decimal len>`.
/// The mode is parsed as OCTAL because it is the spelling `chmod` accepts and
/// the spelling the transport's perl scripts hand it; the length is DECIMAL.
#[cfg(unix)]
fn three_operands<I: Iterator<Item = std::ffi::OsString>>(
    name: &str,
    args: &mut I,
) -> Result<(std::ffi::OsString, u32, u64), std::process::ExitCode> {
    let path = args
        .next()
        .ok_or_else(|| usage(&format!("`{name}` requires <path> <mode> <len>")))?;
    let mode = args
        .next()
        .ok_or_else(|| usage(&format!("`{name}` requires <path> <mode> <len>")))?;
    let len = args
        .next()
        .ok_or_else(|| usage(&format!("`{name}` requires <path> <mode> <len>")))?;
    if args.next().is_some() {
        return Err(usage(&format!("`{name}` requires <path> <mode> <len>")));
    }
    Ok((path, parse_mode(name, &mode)?, parse_len(name, &len)?))
}

/// The sidecar operand quadruple: `<sidecar> <lock> <octal mode> <decimal len>`.
#[cfg(unix)]
fn four_operands<I: Iterator<Item = std::ffi::OsString>>(
    name: &str,
    args: &mut I,
) -> Result<(std::ffi::OsString, std::ffi::OsString, u32, u64), std::process::ExitCode> {
    let sidecar = args
        .next()
        .ok_or_else(|| usage(&format!("`{name}` requires <sidecar> <lock> <mode> <len>")))?;
    let lock = args
        .next()
        .ok_or_else(|| usage(&format!("`{name}` requires <sidecar> <lock> <mode> <len>")))?;
    let mode = args
        .next()
        .ok_or_else(|| usage(&format!("`{name}` requires <sidecar> <lock> <mode> <len>")))?;
    let len = args
        .next()
        .ok_or_else(|| usage(&format!("`{name}` requires <sidecar> <lock> <mode> <len>")))?;
    if args.next().is_some() {
        return Err(usage(&format!(
            "`{name}` requires <sidecar> <lock> <mode> <len>"
        )));
    }
    Ok((
        sidecar,
        lock,
        parse_mode(name, &mode)?,
        parse_len(name, &len)?,
    ))
}

/// Parse the OCTAL mode operand, bounded to the 12 permission/setid bits the
/// transport masks with (`mode & 0o7777`).
#[cfg(unix)]
fn parse_mode(name: &str, token: &std::ffi::OsStr) -> Result<u32, std::process::ExitCode> {
    let text = token
        .to_str()
        .ok_or_else(|| usage(&format!("`{name}` mode must be octal digits")))?;
    match u32::from_str_radix(text, 8) {
        Ok(mode) if mode <= 0o7777 => Ok(mode),
        _ => Err(usage(&format!("`{name}` mode must be octal digits"))),
    }
}

/// Parse the DECIMAL payload-length operand.
#[cfg(unix)]
fn parse_len(name: &str, token: &std::ffi::OsStr) -> Result<u64, std::process::ExitCode> {
    let text = token
        .to_str()
        .ok_or_else(|| usage(&format!("`{name}` len must be decimal digits")))?;
    text.parse::<u64>()
        .map_err(|_| usage(&format!("`{name}` len must be decimal digits")))
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

/// The TWO operands `rename` and `remove-file-if` take, in order. Like
/// [`one_operand`], there is no option parsing: a leading `-` is an operand.
#[cfg(unix)]
fn two_operands<I: Iterator<Item = std::ffi::OsString>>(
    name: &str,
    args: &mut I,
) -> Result<(std::ffi::OsString, std::ffi::OsString), std::process::ExitCode> {
    let first = args
        .next()
        .ok_or_else(|| usage(&format!("`{name}` requires exactly two operands")))?;
    let second = args
        .next()
        .ok_or_else(|| usage(&format!("`{name}` requires exactly two operands")))?;
    if args.next().is_some() {
        return Err(usage(&format!("`{name}` requires exactly two operands")));
    }
    Ok((first, second))
}

/// The THREE unparsed operands `remove-file-if-sidecar` takes (`<sidecar>
/// <lock> <expected>`), in order.
#[cfg(unix)]
fn three_operands_raw<I: Iterator<Item = std::ffi::OsString>>(
    name: &str,
    args: &mut I,
) -> Result<(std::ffi::OsString, std::ffi::OsString, std::ffi::OsString), std::process::ExitCode> {
    let problem = || usage(&format!("`{name}` requires exactly three operands"));
    let first = args.next().ok_or_else(problem)?;
    let second = args.next().ok_or_else(problem)?;
    let third = args.next().ok_or_else(problem)?;
    if args.next().is_some() {
        return Err(problem());
    }
    Ok((first, second, third))
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

/// The `manifest` subcommand: the recursive tree walk of
/// [`storekit::manifest::remote_tree_verify_script`] as a native recursion,
/// byte-identical to it.
///
/// It prints one `\n`-terminated line per entry, `path<TAB>type<TAB>mode_hex
/// <TAB>nlink<TAB>content_sha256<TAB>symlink_target`, in `readdir` order and
/// depth-first (an entry before the entries inside it), where `type` is `f`,
/// `d`, `l` (a symlink, never followed) or `o` (any other kind, whose hash and
/// target are empty), `mode_hex` is `st_mode & 07777` in lowercase hex, and
/// `content_sha256` hashes the file's bytes or the symlink's RAW target. The
/// near-side assembler parses these bytes, so this subcommand's ONLY contract
/// is that stream, its refusals and their exit statuses.
///
/// THE REFUSALS ARE THE SCRIPT'S, INCLUDING THEIR DIAGNOSTIC BYTES AND STATUS:
///
/// * any failing `stat` on the ROOT is `absent: <root>` — the script's `-e`
///   test cannot distinguish ENOENT from EACCES — exiting with that errno;
/// * a root that exists but is not a directory is `not a directory: <root>`,
///   exiting 2 (perl's module loading leaves `$!` = ENOENT, which is what the
///   script's `die` exits with);
/// * an unreadable directory is `opendir <dir>: <err>`, a failed `readdir` is
///   `readdir <dir>: <err>`, and a failed `lstat`/`readlink` likewise `<syscall>
///   <path>: <err>`, each exiting with that errno;
/// * a file whose `lstat` succeeds but whose `open` fails is the script's ONE
///   `die` without a trailing newline, so perl appends its location:
///   `open <path>: <err> at -e line 20.`;
/// * a name that contains NUL/LF/CR/TAB is refused with its hex, a name that is
///   not valid UTF-8 likewise, and a name that is not NFC with the name itself;
///   a symlink TARGET is refused for NUL/LF/CR/TAB or invalid UTF-8 (a target
///   need not be NFC). These are shell-free copies of the script's checks; each
///   exits with perl's `die` rule — 255 when no earlier failed syscall left an
///   errno, which is the normal case, because the walk tracks the `$!` a failed
///   read or filetest would leave, exactly as the script's `$!` carries it.
///
/// `utf8::decode` is NOT `std::str::from_utf8`: Perl accepts its EXTENDED UTF-8
/// (surrogates, code points above U+10FFFF, 5/6/7-byte forms), so
/// [`perl_utf8_valid`] accepts exactly what Perl accepts and the name check
/// reproduces Perl's verdict for those byte sequences.
///
/// There is NO hard-link refusal: the script prints `nlink` and never dies on
/// it. The near-side assembler refuses `nlink > 1`, which is where that rule
/// lives; adding it here would make this form observably differ from the
/// script it replaces.
///
/// The walk holds no descriptor per level beyond the directory it is reading:
/// `read_dir`'s iterator is consumed into a complete name list (as the script's
/// `my @names=readdir($dh)` does) and dropped before any entry is emitted, so
/// the depth bound is the process stack rather than the descriptor limit.
#[cfg(unix)]
fn manifest(root: &std::ffi::OsStr) -> std::process::ExitCode {
    use std::io::Write;
    use std::os::unix::ffi::OsStrExt;

    let root = root.as_bytes().to_vec();
    // Perl's `$!` is ENOENT once the script's two `use` lines have loaded, and a
    // successful filetest does not clear it: that is why the `not a directory:`
    // `die` exits 2. A failed root `stat` overwrites it with its own errno, which
    // is the script's `absent:` status.
    let mut state = ManifestWalkState {
        errno: Some(MANIFEST_PRELUDE_ERRNO),
    };
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let outcome = manifest_root_checks(&root, &mut state)
        .and_then(|()| manifest_walk(&mut out, &root, b"", &mut state));
    // The script's stdout is buffered and flushed at exit, INCLUDING on a `die`,
    // so a refusal after some records keeps those bytes on stdout.
    let flushed = out.flush();
    match outcome {
        Ok(()) => match flushed {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => fail(&format!("writing the manifest failed: {error}")),
        },
        Err(ManifestFailure::Die { message, errno }) => script_die(&message, errno),
        Err(ManifestFailure::Io(message)) => fail(&message),
    }
}

/// The `$!` the script's `die` exits with when no syscall the walk made has set
/// it: perl's own module loading leaves ENOENT, so the `not a directory:`
/// refusal exits 2. Measured on the script's one-line `-e` form (a script FILE
/// form leaves a different `$!`), which is exactly how the transport runs it.
#[cfg(unix)]
const MANIFEST_PRELUDE_ERRNO: i32 = 2;

/// The `$!` perl would hold at the next `die`. `None` is zero, which perl's
/// `die` turns into exit status 255. Only a FAILED syscall sets it and only the
/// script's `$! = 0` after `opendir` clears it — successful syscalls leave it
/// alone, exactly as in perl.
#[cfg(unix)]
struct ManifestWalkState {
    errno: Option<i32>,
}

/// Why the manifest walk stopped: a script `die` (diagnostic bytes and the
/// `$!` that decides the status), or a failure writing stdout.
#[cfg(unix)]
enum ManifestFailure {
    Die {
        message: Vec<u8>,
        errno: Option<i32>,
    },
    Io(String),
}

/// The script's `-e`/`-d` root tests, in its order, with its diagnostics.
#[cfg(unix)]
fn manifest_root_checks(root: &[u8], state: &mut ManifestWalkState) -> Result<(), ManifestFailure> {
    use std::os::unix::ffi::OsStrExt;

    // `-e $root` is `stat`, following a symlink; ANY failure is the ABSENT arm.
    match std::fs::metadata(std::ffi::OsStr::from_bytes(root)) {
        Ok(meta) => {
            if !meta.is_dir() {
                return Err(ManifestFailure::Die {
                    message: manifest_message(&[b"not a directory: ", root], b"\n"),
                    errno: state.errno,
                });
            }
            Ok(())
        }
        Err(error) => {
            state.errno = error.raw_os_error();
            Err(ManifestFailure::Die {
                message: manifest_message(&[b"absent: ", root], b"\n"),
                errno: state.errno,
            })
        }
    }
}

/// One directory of the walk: `opendir`, read EVERY name (as the script's list
/// context `readdir` does, so a read error is a `die` before any record from
/// this directory), then emit and recurse.
#[cfg(unix)]
fn manifest_walk<W: std::io::Write>(
    out: &mut W,
    dir: &[u8],
    prefix: &[u8],
    state: &mut ManifestWalkState,
) -> Result<(), ManifestFailure> {
    use std::os::unix::ffi::OsStrExt;

    let reader = match std::fs::read_dir(std::ffi::OsStr::from_bytes(dir)) {
        Ok(reader) => reader,
        Err(error) => {
            state.errno = error.raw_os_error();
            return Err(ManifestFailure::Die {
                message: manifest_errno_message(b"opendir ", dir, state.errno),
                errno: state.errno,
            });
        }
    };
    // The script's `$! = 0` sits between `opendir` and `readdir`; everything
    // before the next failing syscall therefore exits 255 on a validation `die`.
    state.errno = None;
    let mut names: Vec<std::ffi::OsString> = Vec::new();
    for entry in reader {
        match entry {
            Ok(entry) => names.push(entry.file_name()),
            Err(error) => {
                state.errno = error.raw_os_error();
                return Err(ManifestFailure::Die {
                    message: manifest_errno_message(b"readdir ", dir, state.errno),
                    errno: state.errno,
                });
            }
        }
    }
    // `read_dir` omits `.` and `..` itself; the script skips them explicitly.
    for name in names {
        let name = name.as_bytes();
        manifest_check_name(name, dir, state)?;
        // `"$dir/$n"`, built UNCONDITIONALLY so a root ending in `/` yields
        // `//` in both the syscall path and the diagnostics, as the script does.
        let mut path = Vec::with_capacity(dir.len() + 1 + name.len());
        path.extend_from_slice(dir);
        path.push(b'/');
        path.extend_from_slice(name);
        // `length($prefix) ? "$prefix/$name" : $name`
        let mut rel = Vec::with_capacity(prefix.len() + 1 + name.len());
        if !prefix.is_empty() {
            rel.extend_from_slice(prefix);
            rel.push(b'/');
        }
        rel.extend_from_slice(name);
        manifest_emit(out, &rel, &path, state)?;
        // `-d $p && !-l $p`: recurse into a real directory, never a symlink.
        // The two filetests are re-run exactly as the script runs them, ERRNO
        // EFFECT INCLUDED: a failed one does not stop the walk, but its errno
        // becomes the status of a later validation `die`, as perl's `$!` would.
        let is_dir = match std::fs::metadata(std::ffi::OsStr::from_bytes(&path)) {
            Ok(meta) => meta.is_dir(),
            Err(error) => {
                state.errno = error.raw_os_error();
                false
            }
        };
        if is_dir {
            let is_link = match std::fs::symlink_metadata(std::ffi::OsStr::from_bytes(&path)) {
                Ok(meta) => meta.file_type().is_symlink(),
                Err(error) => {
                    state.errno = error.raw_os_error();
                    false
                }
            };
            if !is_link {
                manifest_walk(out, &path, &rel, state)?;
            }
        }
    }
    Ok(())
}

/// The script's `$check_name` closure: NUL/LF/CR/TAB (by raw byte), then Perl's
/// UTF-8 validity, then NFC — in that order, with the same diagnostic bytes.
#[cfg(unix)]
fn manifest_check_name(
    name: &[u8],
    dir: &[u8],
    state: &ManifestWalkState,
) -> Result<(), ManifestFailure> {
    if manifest_wire_unrepresentable(name) {
        let hex = hex::encode(name);
        return Err(ManifestFailure::Die {
            message: manifest_message(
                &[
                    b"entry name under ",
                    dir,
                    b" contains a tab, newline, carriage return, or NUL (the manifest wire refuses NUL/LF/CR/TAB): ",
                    hex.as_bytes(),
                ],
                b"\n",
            ),
            errno: state.errno,
        });
    }
    if !perl_utf8_valid(name) {
        let hex = hex::encode(name);
        return Err(ManifestFailure::Die {
            message: manifest_message(
                &[
                    b"entry name under ",
                    dir,
                    b" is not valid UTF-8: ",
                    hex.as_bytes(),
                ],
                b"\n",
            ),
            errno: state.errno,
        });
    }
    if !perl_nfc(name) {
        return Err(ManifestFailure::Die {
            message: manifest_message(
                &[b"entry name under ", dir, b" is not NFC-normalized: ", name],
                b"\n",
            ),
            errno: state.errno,
        });
    }
    Ok(())
}

/// The script's `$check_target` closure: NUL/LF/CR/TAB, then Perl's UTF-8
/// validity. NFC is deliberately NOT required of a target: it is link data the
/// kernel dereferences verbatim, not an addressable name.
#[cfg(unix)]
fn manifest_check_target(
    target: &[u8],
    rel: &[u8],
    state: &ManifestWalkState,
) -> Result<(), ManifestFailure> {
    if manifest_wire_unrepresentable(target) {
        let hex = hex::encode(target);
        return Err(ManifestFailure::Die {
            message: manifest_message(
                &[
                    b"symlink target of ",
                    rel,
                    b" contains a tab, newline, carriage return, or NUL (the manifest wire refuses NUL/LF/CR/TAB): ",
                    hex.as_bytes(),
                ],
                b"\n",
            ),
            errno: state.errno,
        });
    }
    if !perl_utf8_valid(target) {
        let hex = hex::encode(target);
        return Err(ManifestFailure::Die {
            message: manifest_message(
                &[
                    b"symlink target of ",
                    rel,
                    b" is not valid UTF-8: ",
                    hex.as_bytes(),
                ],
                b"\n",
            ),
            errno: state.errno,
        });
    }
    Ok(())
}

/// One entry's record, from `lstat` to the printed line, exactly as the
/// script's `$emit` closure does it.
#[cfg(unix)]
fn manifest_emit<W: std::io::Write>(
    out: &mut W,
    rel: &[u8],
    path: &[u8],
    state: &mut ManifestWalkState,
) -> Result<(), ManifestFailure> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;

    let meta = match std::fs::symlink_metadata(std::ffi::OsStr::from_bytes(path)) {
        Ok(meta) => meta,
        Err(error) => {
            state.errno = error.raw_os_error();
            return Err(ManifestFailure::Die {
                message: manifest_errno_message(b"lstat ", path, state.errno),
                errno: state.errno,
            });
        }
    };
    // `-l $p ? l : (-d $p ? d : (-f $p ? f : o))`: the symlink test FIRST, so a
    // symlink to a directory is `l` and is never walked into.
    let file_type = meta.file_type();
    let tag = if file_type.is_symlink() {
        b'l'
    } else if meta.is_dir() {
        b'd'
    } else if meta.is_file() {
        b'f'
    } else {
        b'o'
    };
    let mode = meta.mode() & 0o7777;
    let nlink = meta.nlink();
    let mut hash = String::new();
    let mut target: Vec<u8> = Vec::new();
    if tag == b'f' {
        let mut file = match std::fs::File::open(std::ffi::OsStr::from_bytes(path)) {
            Ok(file) => file,
            Err(error) => {
                state.errno = error.raw_os_error();
                let errno = errno_text(state.errno);
                return Err(ManifestFailure::Die {
                    message: manifest_message(
                        &[
                            b"open ",
                            path,
                            b": ",
                            &errno,
                            // No trailing newline in the script: perl appends
                            // its location, and the one-line program makes the
                            // location `-e line 20`.
                            b" at -e line 20.\n",
                        ],
                        b"",
                    ),
                    errno: state.errno,
                });
            }
        };
        let mut data = Vec::new();
        if let Err(error) = std::io::Read::read_to_end(&mut file, &mut data) {
            // The script's slurp leaves `$d` undef and `sha256_hex(undef)` hashes
            // the EMPTY string; warnings are off in the one-line program, so
            // nothing is printed. The failed read's errno stays in `$!`.
            state.errno = error.raw_os_error();
            data.clear();
        }
        hash = storekit::digest::sha256_bytes(&data);
    } else if tag == b'l' {
        let link = match std::fs::read_link(std::ffi::OsStr::from_bytes(path)) {
            Ok(link) => link,
            Err(error) => {
                state.errno = error.raw_os_error();
                return Err(ManifestFailure::Die {
                    message: manifest_errno_message(b"readlink ", path, state.errno),
                    errno: state.errno,
                });
            }
        };
        target = link.as_os_str().as_bytes().to_vec();
        manifest_check_target(&target, rel, state)?;
        hash = storekit::digest::sha256_bytes(&target);
    }
    let mut record = Vec::with_capacity(rel.len() + path.len() + 96);
    record.extend_from_slice(rel);
    record.push(b'\t');
    record.push(tag);
    record.push(b'\t');
    record.extend_from_slice(format!("{mode:x}").as_bytes());
    record.push(b'\t');
    record.extend_from_slice(nlink.to_string().as_bytes());
    record.push(b'\t');
    record.extend_from_slice(hash.as_bytes());
    record.push(b'\t');
    record.extend_from_slice(&target);
    record.push(b'\n');
    out.write_all(&record)
        .map_err(|error| ManifestFailure::Io(format!("writing the manifest failed: {error}")))
}

/// Concatenate byte fragments (the script interpolates raw bytes into its
/// diagnostics, so these are never built through a lossy string).
#[cfg(unix)]
fn manifest_message(parts: &[&[u8]], tail: &[u8]) -> Vec<u8> {
    let mut message = Vec::new();
    for part in parts {
        message.extend_from_slice(part);
    }
    message.extend_from_slice(tail);
    message
}

/// `<what><path>: <strerror>\n` — the script's `die qq{...: $!\n}` shape for a
/// failing syscall (`what` carries its own trailing space).
#[cfg(unix)]
fn manifest_errno_message(what: &[u8], path: &[u8], errno: Option<i32>) -> Vec<u8> {
    let errno = errno_text(errno);
    manifest_message(&[what, path, b": ", &errno], b"\n")
}

/// Whether `bytes` holds one of the four characters the wire cannot carry:
/// NUL, LF, CR, or TAB.
#[cfg(unix)]
fn manifest_wire_unrepresentable(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .any(|&byte| matches!(byte, 0 | b'\n' | b'\r' | b'\t'))
}

/// Whether Perl's `utf8::decode` accepts `bytes`.
///
/// NOT `std::str::from_utf8`: Perl's decoder accepts its EXTENDED UTF-8, which
/// additionally encodes SURROGATES (a lone `ED A0 80` is U+D800), code points
/// above U+10FFFF, and 5-, 6- and 7-byte forms. The script validates names and
/// targets with `utf8::decode`, so a sequence Perl accepts must not be reported
/// here as "not valid UTF-8". The per-length minimum rejects exactly the
/// OVERLONG forms Perl rejects, and a `0xFF` lead is refused outright.
///
/// The boundaries are the measured ones: `C2 80` accepted, `C1 BF` refused;
/// `E0 A0 80` accepted, `E0 9F BF` refused; `F0 90 80 80` accepted, `F0 8F BF
/// BF` refused; `F8 88 80 80 80` accepted, `F8 87 BF BF BF` refused; `FC 84 80
/// 80 80 80` accepted, `FC 83 …` refused; `FE 82 …` accepted, `FE 81 …`
/// refused, `FF …` refused.
#[cfg(unix)]
fn perl_utf8_valid(bytes: &[u8]) -> bool {
    perl_utf8_decode(bytes).is_some()
}

/// Decode `bytes` as Perl's extended UTF-8 — the decoder behind
/// [`perl_utf8_valid`] — one code point per element, or `None` for exactly the
/// byte strings Perl refuses. The elements are `u64` because the 7-byte form
/// carries 36 value bits, past `u32`; every such value is invalid Unicode and
/// is used only as a BOUNDARY by [`perl_nfc`].
#[cfg(unix)]
fn perl_utf8_decode(bytes: &[u8]) -> Option<Vec<u64>> {
    let mut code_points = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let lead = bytes[index];
        // (total length, smallest code point that needs that length)
        let (len, minimum): (usize, u64) = match lead {
            0x00..=0x7F => {
                code_points.push(u64::from(lead));
                index += 1;
                continue;
            }
            0xC0..=0xDF => (2, 0x80),
            0xE0..=0xEF => (3, 0x800),
            0xF0..=0xF7 => (4, 0x1_0000),
            0xF8..=0xFB => (5, 0x20_0000),
            0xFC..=0xFD => (6, 0x400_0000),
            0xFE => (7, 0x8000_0000),
            // A continuation byte with no lead, and 0xFF, are never a start.
            _ => return None,
        };
        if index + len > bytes.len() {
            return None;
        }
        // The lead contributes the low (7 - len) bits of the first byte.
        let mut value = u64::from(lead & ((1u16 << (7 - len)) - 1) as u8);
        for continuation in &bytes[index + 1..index + len] {
            if !(0x80..=0xBF).contains(continuation) {
                return None;
            }
            value = (value << 6) | u64::from(continuation & 0x3F);
        }
        if value < minimum {
            return None;
        }
        code_points.push(value);
        index += len;
    }
    Some(code_points)
}

/// The script's NFC check (`NFC($c) ne $c`) for a name that
/// [`perl_utf8_valid`] accepted.
///
/// A code point valid in Perl's EXTENDED UTF-8 but not in Unicode (a
/// surrogate, a code point above U+10FFFF, or a 5/6/7-byte form) is PASSED
/// THROUGH by Perl's normalizer: `Unicode::Normalize` normalizes the runs of
/// valid scalars and leaves each such code point untouched, so those code
/// points are BOUNDARIES and the check is per RUN. A name that mixes an
/// extended code point with a decomposable valid run (`65 CC 81 ED A0 80` is
/// `e`, U+0301, U+D800: the `e`+U+0301 run becomes U+00E9) therefore STILL
/// fails and the script refuses it, while a name with no decomposable run is
/// printed. Measured on perl 5.34/5.42.
#[cfg(unix)]
fn perl_nfc(bytes: &[u8]) -> bool {
    // The caller refuses Perl-invalid bytes with the hex diagnostic BEFORE this
    // check, so `None` is unreachable; it is not an NFC change either way.
    let Some(code_points) = perl_utf8_decode(bytes) else {
        return true;
    };
    let mut run: Vec<char> = Vec::new();
    for code_point in code_points {
        // `char::from_u32` is exactly the VALID scalar test: `None` for a
        // surrogate and for anything above U+10FFFF.
        match u32::try_from(code_point).ok().and_then(char::from_u32) {
            Some(scalar) => run.push(scalar),
            None => {
                if !nfc_preserved(&run) {
                    return false;
                }
                run.clear();
            }
        }
    }
    nfc_preserved(&run)
}

/// Whether `run` equals its own NFC form — the script's `NFC($c) ne $c`.
#[cfg(unix)]
fn nfc_preserved(run: &[char]) -> bool {
    use unicode_normalization::UnicodeNormalization;

    run.iter().copied().nfc().eq(run.iter().copied())
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

/// The `publish` subcommand: the upload chain the transport's perl script runs,
/// byte-for-byte in its observable behaviour.
///
/// 1. Create the destination's PARENT (`mkdir -p`).
/// 2. Allocate a UNIQUE temp in the destination's OWN directory with a bounded
///    name (`temp_name_for` is the crate's one temp authority, so the name is
///    also recognised by the recovery sweep) and the private `mktemp` mode
///    (0o600).
/// 3. Read the payload from STDIN into the temp (`cat > "$tmp"`).
/// 4. Verify the RECEIVED length against the caller's `<len>` BETWEEN STDIN EOF
///    and the install, so a clean, short EOF cannot publish a truncated temp.
/// 5. Apply `<mode>` to the temp when it is NON-ZERO (0 means no chmod at all;
///    the temp keeps its private mode).
/// 6. fsync the temp.
/// 7. `rename(2)` the temp over the destination: ATOMIC, replacing a regular
///    file or symlink, NEVER descending into a directory destination (the
///    rename fails and the temp is removed).
/// 8. fsync the PARENT directory.
///
/// Exit 0 on success; 1 on every failure BEFORE the rename (the temp is
/// removed and the old entry is intact); and
/// [`SSH_UPLOAD_POST_RENAME_EXIT`] when ONLY the parent fsync after the rename
/// fails (the new content IS visible, its durability is unconfirmed).
///
/// The rename failure is deliberately SILENT: the transport's script performs
/// the rename with a bare `rename(2)` one-liner that exits nonzero without a
/// diagnostic, so the CLI printing one would make the two forms observably
/// different (a directory destination is the reachable case).
#[cfg(unix)]
#[allow(clippy::disallowed_methods)]
fn publish(path: &std::ffi::OsStr, mode: u32, expected_len: u64) -> std::process::ExitCode {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let parent = parent_of(path);
    if let Err(error) = std::fs::create_dir_all(&parent) {
        return pre_install_failure(&format!(
            "publish: creating {} failed: {error}",
            parent.display()
        ));
    }
    let (tmp, mut file) = match create_private_temp(path) {
        Ok(created) => created,
        Err(error) => {
            return pre_install_failure(&format!("publish: creating a temp failed: {error}"));
        }
    };
    let data = match read_stdin_payload() {
        Ok(data) => data,
        Err(error) => {
            remove_temp(&tmp);
            return pre_install_failure(&format!("publish: reading stdin failed: {error}"));
        }
    };
    if let Err(error) = file.write_all(&data) {
        remove_temp(&tmp);
        return pre_install_failure(&format!("publish: writing the temp failed: {error}"));
    }
    // The payload is complete and the descriptor is closed before the length
    // check, exactly as the script's `cat` exits before `perl -s` runs.
    drop(file);
    if let Some(message) = received_length_mismatch(&tmp, expected_len) {
        remove_temp(&tmp);
        let _ = std::io::stderr().lock().write_all(&message);
        return std::process::ExitCode::from(SSH_TWRITE_PREINSTALL_EXIT as u8);
    }
    if mode != 0
        && let Err(error) =
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode & 0o7777))
    {
        remove_temp(&tmp);
        return pre_install_failure(&format!(
            "publish: chmod of {} failed: {error}",
            tmp.display()
        ));
    }
    if let Err((_, message)) = fsync_object(&tmp) {
        remove_temp(&tmp);
        let _ = std::io::stderr().lock().write_all(&message);
        return std::process::ExitCode::from(SSH_TWRITE_PREINSTALL_EXIT as u8);
    }
    // ATOMIC REPLACE. `std::fs::rename` is `rename(2)`: it replaces a regular
    // file or a symlink entry (never following the symlink) and REFUSES a
    // directory destination instead of moving the temp inside it.
    if std::fs::rename(&tmp, path).is_err() {
        // The script's rename one-liner is SILENT on failure; stay silent.
        remove_temp(&tmp);
        return std::process::ExitCode::from(SSH_TWRITE_PREINSTALL_EXIT as u8);
    }
    match fsync_object(&parent) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err((_, message)) => {
            let _ = std::io::stderr().lock().write_all(&message);
            std::process::ExitCode::from(SSH_UPLOAD_POST_RENAME_EXIT as u8)
        }
    }
}

/// The `write-new` subcommand: the no-clobber install chain of the transport's
/// `write_new_cmd`.
///
/// Identical temp/payload/length/chmod/file-fsync steps to [`publish`], but the
/// install is `link(2)` — it FAILS when the destination exists in ANY form (a
/// regular file, a directory, a symlink), so a concurrent winner is never
/// replaced and a directory destination is never descended into. The temp is
/// ALWAYS removed. The parent directory is fsynced only after a successful
/// install, and its failure propagates as the failing primitive's own status
/// (the perl `die` status: its errno, or 255), exactly as the script's
/// `exit $?` does — never a silent success.
///
/// Exit [`SSH_TWRITE_CONFLICT_EXIT`] when the install is EEXIST or the
/// destination is present after a failed link, [`SSH_TWRITE_PREINSTALL_EXIT`]
/// on any other pre-install failure, 0 on a durable install.
#[cfg(unix)]
#[allow(clippy::disallowed_methods)]
fn write_new(path: &std::ffi::OsStr, mode: u32, expected_len: u64) -> std::process::ExitCode {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let parent = parent_of(path);
    if let Err(error) = std::fs::create_dir_all(&parent) {
        return pre_install_failure(&format!(
            "write-new: creating {} failed: {error}",
            parent.display()
        ));
    }
    let (tmp, mut file) = match create_private_temp(path) {
        Ok(created) => created,
        Err(error) => {
            return pre_install_failure(&format!("write-new: creating a temp failed: {error}"));
        }
    };
    let data = match read_stdin_payload() {
        Ok(data) => data,
        Err(error) => {
            remove_temp(&tmp);
            return pre_install_failure(&format!("write-new: reading stdin failed: {error}"));
        }
    };
    if let Err(error) = file.write_all(&data) {
        remove_temp(&tmp);
        return pre_install_failure(&format!("write-new: writing the temp failed: {error}"));
    }
    drop(file);
    if let Some(message) = received_length_mismatch(&tmp, expected_len) {
        remove_temp(&tmp);
        let _ = std::io::stderr().lock().write_all(&message);
        return std::process::ExitCode::from(SSH_TWRITE_PREINSTALL_EXIT as u8);
    }
    // `write_new_cmd` applies the mode UNCONDITIONALLY (there is no "0 means no
    // chmod" arm in that script).
    if let Err(error) =
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode & 0o7777))
    {
        remove_temp(&tmp);
        return pre_install_failure(&format!(
            "write-new: chmod of {} failed: {error}",
            tmp.display()
        ));
    }
    if let Err((_, message)) = fsync_object(&tmp) {
        remove_temp(&tmp);
        let _ = std::io::stderr().lock().write_all(&message);
        return std::process::ExitCode::from(SSH_TWRITE_PREINSTALL_EXIT as u8);
    }
    // `link(2)`: no-clobber, and it never follows or links-into an existing
    // destination.
    match std::fs::hard_link(&tmp, path) {
        Ok(()) => {
            remove_temp(&tmp);
            match fsync_object(&parent) {
                Ok(()) => std::process::ExitCode::SUCCESS,
                Err((errno, message)) => {
                    let _ = std::io::stderr().lock().write_all(&message);
                    std::process::ExitCode::from(perl_die_code(errno))
                }
            }
        }
        Err(error) => {
            let errno = error.raw_os_error();
            remove_temp(&tmp);
            if errno == Some(SSH_TWRITE_CONFLICT_EXIT) || std::fs::symlink_metadata(path).is_ok() {
                return std::process::ExitCode::from(SSH_TWRITE_CONFLICT_EXIT as u8);
            }
            std::process::ExitCode::from(SSH_TWRITE_PREINSTALL_EXIT as u8)
        }
    }
}

/// The `sidecar-publish` subcommand: the operation-lock record install under
/// the sidecar's `flock`, matching the transport's perl sidecar script.
///
/// The SIDECAR FILE EXISTS FIRST — the script runs `mkdir -p`, `touch` and
/// `chmod 644` on it before it reads any payload — then the CLI opens it and
/// takes an exclusive NON-BLOCKING `flock` with the prelude's policy: `EINTR`
/// retries immediately, `EAGAIN`/`EWOULDBLOCK` waits 5ms against a 2-second
/// monotonic deadline (mirroring `SIDECAR_FLOCK_*` in the transport), any other
/// errno fails, and a deadline expiry is the contention refusal. The lock is
/// held across the whole write/install sequence (the descriptor lives to the
/// end of the command).
///
/// STDIN is read and length-checked while the lock is held; the truncated
/// payload diagnostic is the sidecar script's (`received <n> bytes`). A unique
/// temp beside the lock is written, chmodded, fsynced, installed with
/// `link(2)` (never replacing), the temp is removed, and the directory is
/// fsynced. Exit [`SSH_TWRITE_CONFLICT_EXIT`] on EEXIST, 1 on every other
/// failure (the install-success parent fsync included, exactly as the script's
/// `exit preinst`).
#[cfg(unix)]
#[allow(clippy::disallowed_methods)]
fn sidecar_publish(
    sidecar: &std::ffi::OsStr,
    lock: &std::ffi::OsStr,
    mode: u32,
    expected_len: u64,
) -> std::process::ExitCode {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::io::AsRawFd;

    let sidecar_path = std::path::Path::new(sidecar);
    let lock_path = std::path::Path::new(lock);
    let parent = parent_of(lock);
    if let Err(error) = std::fs::create_dir_all(&parent) {
        return pre_install_failure(&format!(
            "sidecar-publish: creating {} failed: {error}",
            parent.display()
        ));
    }
    // `touch`: create an absent sidecar, leave an existing one's content alone.
    match std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(sidecar_path)
    {
        Ok(_) => {}
        Err(error) => {
            return pre_install_failure(&format!(
                "sidecar-publish: creating {} failed: {error}",
                sidecar_path.display()
            ));
        }
    }
    if let Err(error) =
        std::fs::set_permissions(sidecar_path, std::fs::Permissions::from_mode(0o644))
    {
        return pre_install_failure(&format!(
            "sidecar-publish: chmod of {} failed: {error}",
            sidecar_path.display()
        ));
    }
    // `open my $fh, "+<", $ARGV[0]` — read+write, no truncation.
    let sidecar_file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(sidecar_path)
    {
        Ok(file) => file,
        Err(error) => {
            let errno = error.raw_os_error();
            let mut message = b"open sidecar ".to_vec();
            message.extend_from_slice(path_bytes(sidecar_path));
            message.extend_from_slice(b": ");
            message.extend_from_slice(&errno_text(errno));
            // The `open` is on the one-line program's FIRST line (the prelude's
            // first statement is appended to it), so perl's `die` location is
            // `-e line 1` — byte-for-byte what the script prints.
            message.extend_from_slice(b" at -e line 1.\n");
            return script_die(&message, errno);
        }
    };
    if let Err(failure) = flock_exclusive(sidecar_file.as_raw_fd()) {
        return match failure {
            FlockFailure::Failed(errno) => {
                let mut message = b"sidecar flock failed: ".to_vec();
                message.extend_from_slice(&errno_text(Some(errno)));
                // The `die` sits on the prelude's eighth line.
                message.extend_from_slice(b" at -e line 8.\n");
                script_die(&message, Some(errno))
            }
            FlockFailure::Contended(errno) => {
                // `die "sidecar contended"` sits on the prelude's sixteenth
                // line, and `$!` still holds the failed `flock`'s errno.
                script_die(b"sidecar contended at -e line 16.\n", errno)
            }
        };
    }
    let data = match read_stdin_payload() {
        Ok(data) => data,
        Err(error) => {
            return pre_install_failure(&format!("sidecar-publish: reading stdin failed: {error}"));
        }
    };
    if data.len() as u64 != expected_len {
        let mut message = b"storekit: sidecar payload truncated: received ".to_vec();
        message.extend_from_slice(data.len().to_string().as_bytes());
        message.extend_from_slice(b" bytes, expected ");
        message.extend_from_slice(expected_len.to_string().as_bytes());
        message.extend_from_slice(b"; refusing to publish\n");
        let _ = std::io::stderr().lock().write_all(&message);
        return std::process::ExitCode::from(SSH_TWRITE_PREINSTALL_EXIT as u8);
    }
    let (tmp, mut file) = match create_private_temp(lock) {
        Ok(created) => created,
        Err(error) => {
            return pre_install_failure(&format!(
                "sidecar-publish: creating a temp failed: {error}"
            ));
        }
    };
    if let Err(error) = file.write_all(&data) {
        remove_temp(&tmp);
        return pre_install_failure(&format!(
            "sidecar-publish: writing the temp failed: {error}"
        ));
    }
    if let Err(error) =
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode & 0o7777))
    {
        remove_temp(&tmp);
        return pre_install_failure(&format!(
            "sidecar-publish: chmod of {} failed: {error}",
            tmp.display()
        ));
    }
    // The sidecar script SWALLOWS its own fsync diagnostics (`... or exit
    // preinst`), so the CLI stays silent on this path too.
    if file.sync_all().is_err() {
        remove_temp(&tmp);
        return std::process::ExitCode::from(SSH_TWRITE_PREINSTALL_EXIT as u8);
    }
    drop(file);
    match std::fs::hard_link(&tmp, lock_path) {
        Ok(()) => {
            remove_temp(&tmp);
            // The directory fsync failure is `exit preinst` (1) and carries no
            // diagnostic in the script.
            match fsync_object(&parent) {
                Ok(()) => std::process::ExitCode::SUCCESS,
                Err(_) => std::process::ExitCode::from(SSH_TWRITE_PREINSTALL_EXIT as u8),
            }
        }
        Err(error) => {
            let errno = error.raw_os_error();
            remove_temp(&tmp);
            if errno == Some(SSH_TWRITE_CONFLICT_EXIT) {
                return std::process::ExitCode::from(SSH_TWRITE_CONFLICT_EXIT as u8);
            }
            std::process::ExitCode::from(SSH_TWRITE_PREINSTALL_EXIT as u8)
        }
    }
}

/// The `fsync-file` / `fsync-dir` subcommands: the transport's perl fsync
/// primitive (`PERL_FSYNC_FILE` / `PERL_FSYNC_DIR`) as a native sequence.
///
/// The two subcommands run the IDENTICAL sequence — the scripts differ only in
/// the trailing test-hook COMMENT — so the classification and the diagnostics
/// live in one place ([`fsync_object`]): open `O_RDONLY | O_NONBLOCK` (so a
/// FIFO with no writer cannot block the open), `fstat` that same descriptor,
/// accept ONLY a regular file or a directory, then `fsync(2)`.
///
/// The failure bytes and the exit status are perl's `die`: `<what> <path>:
/// <strerror> at -e line 1.` on stderr, exiting with `$!` when it is nonzero and
/// 255 otherwise. For a failed `sysopen`/`stat`/`sync` the errno IS the failure's
/// ([`fsync_object`] returns it, so the status is the real one), and for the
/// non-regular refusal it is the errno perl's own handle setup left behind —
/// see [`perl_handle_setup_errno`], which reproduces it rather than guessing.
#[cfg(unix)]
fn fsync_primitive(path: &std::ffi::OsStr) -> std::process::ExitCode {
    match fsync_object(std::path::Path::new(path)) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err((errno, message)) => script_die(&message, errno),
    }
}

/// The `fsync-tree` subcommand: the transport's `find -depth … -exec … +` walk
/// as a native recursion.
///
/// Every entry BELOW `path` is visited before `path` itself (children before
/// parents), so a staged bundle is durable deepest-first. Only DIRECTORIES and
/// REGULAR FILES are fsynced: a symlink and every other entry are SKIPPED —
/// their durability is their directory entry, which the parent's own fsync
/// covers — so a FIFO is never opened and a dangling symlink is never a
/// failure. The FIRST failing entry fails the whole walk.
///
/// The failure shapes are the script's two: a failing fsync prints the perl
/// primitive's `die` bytes and the command exits 1 (the script's `sh -c '… ||
/// exit 1'` wrapper collapses the primitive's status and `find` reports 1); an
/// entry the walk cannot even read is the script's `find` diagnostic with the
/// same status 1. `find`'s own wording is userland-specific (GNU quotes the
/// path, BSD does not), so that diagnostic states the path and the errno
/// itself rather than pretending to be one particular `find`.
#[cfg(unix)]
fn fsync_tree(path: &std::ffi::OsStr) -> std::process::ExitCode {
    match fsync_tree_entry(std::path::Path::new(path)) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(code) => code,
    }
}

/// One entry of the [`fsync_tree`] walk: recurse into a directory's children
/// first, then fsync the entry itself.
#[cfg(unix)]
fn fsync_tree_entry(path: &std::path::Path) -> Result<(), std::process::ExitCode> {
    use std::io::Write;

    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) => return Err(walk_failure(path, &error)),
    };
    if meta.is_dir() {
        let entries = match std::fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) => return Err(walk_failure(path, &error)),
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => return Err(walk_failure(path, &error)),
            };
            fsync_tree_entry(&entry.path())?;
        }
    } else if !meta.is_file() {
        // SKIPPED: a symlink (never followed, and `-type` does not follow one
        // either) and every special entry. Their directory entry is durable
        // with their parent's fsync, and a FIFO must never be opened.
        return Ok(());
    }
    match fsync_object(path) {
        Ok(()) => Ok(()),
        Err((_errno, message)) => {
            let _ = std::io::stderr().lock().write_all(&message);
            Err(std::process::ExitCode::from(
                SSH_TWRITE_PREINSTALL_EXIT as u8,
            ))
        }
    }
}

/// The `fsync-tree` walk's own failure (an entry the walk cannot `lstat` or
/// `readdir`): the diagnostic names the path and the errno, and the status is
/// the one the script's `find` exits with. See [`fsync_tree`] for why the
/// wording is not `find`'s.
#[cfg(unix)]
fn walk_failure(path: &std::path::Path, error: &std::io::Error) -> std::process::ExitCode {
    use std::io::Write;

    let mut message = b"storekit-remote: fsync-tree: ".to_vec();
    message.extend_from_slice(path_bytes(path));
    message.extend_from_slice(b": ");
    message.extend_from_slice(&errno_text(error.raw_os_error()));
    message.push(b'\n');
    let _ = std::io::stderr().lock().write_all(&message);
    std::process::ExitCode::from(SSH_TWRITE_PREINSTALL_EXIT as u8)
}

/// The `rename` subcommand: the transport's `mkdir -p <parent> && perl -e
/// 'rename(…)'` chain.
///
/// The target's PARENT is created first (so a rename into a fresh directory
/// works), then `rename(2)` runs: ATOMIC, and it replaces an existing
/// NON-DIRECTORY target (a regular file, a symlink — its own entry, never the
/// link's target) while a DIRECTORY target is refused instead of being moved
/// into. The failure text is the script's perl `die` (`rename <from> -> <to>:
/// <strerror> at -e line 1.`) and its status is that `die`'s.
///
/// A failing `mkdir -p` is the script's OTHER failure shape — a shell
/// coreutil's diagnostic with status 1 — so the CLI reports the parent it could
/// not create and exits 1 (the status `mkdir` exits with).
#[cfg(unix)]
#[allow(clippy::disallowed_methods)]
fn rename(from: &std::ffi::OsStr, to: &std::ffi::OsStr) -> std::process::ExitCode {
    let parent = parent_of(to);
    if let Err(error) = std::fs::create_dir_all(&parent) {
        return pre_install_failure(&format!(
            "rename: creating {} failed: {error}",
            parent.display()
        ));
    }
    match std::fs::rename(from, to) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            let errno = error.raw_os_error();
            let mut message = b"rename ".to_vec();
            message.extend_from_slice(path_bytes(std::path::Path::new(from)));
            message.extend_from_slice(b" -> ");
            message.extend_from_slice(path_bytes(std::path::Path::new(to)));
            message.extend_from_slice(b": ");
            message.extend_from_slice(&errno_text(errno));
            message.extend_from_slice(b" at -e line 1.\n");
            script_die(&message, errno)
        }
    }
}

/// The `remove-file-if` subcommand: the transport's `remove_file_if_cmd`
/// compare-and-delete.
///
/// The entry is CLAIMED with `rename(2)` to a UNIQUE, same-directory, BOUNDED
/// temp (created `O_EXCL` first and then removed, exactly as the script's
/// `mktemp` + `rm -f` claims the name, so no existing entry is ever adopted),
/// the CLAIMED bytes are compared, and exactly ONE one-byte verdict frame is
/// printed — the only channel: `R` (claimed and removed), `M` (mismatch; the
/// claim is re-published NO-REPLACE with `link(2)` and then discarded, so a
/// concurrent install is never replaced and a failed restore still leaves its
/// content), or `A` (the claim found nothing, i.e. the entry was absent).
/// `M` and `A` follow the script's own tests, including their fail-open reading
/// of a failed existence check as absence.
///
/// A failing `mkdir -p`/`mktemp` is the script's coreutil failure shape: the CLI
/// names the step it could not do and exits 1, the status both coreutils use.
#[cfg(unix)]
#[allow(clippy::disallowed_methods)]
fn remove_file_if(path: &std::ffi::OsStr, expected: &std::ffi::OsStr) -> std::process::ExitCode {
    use std::os::unix::ffi::OsStrExt;

    let expected = expected.as_bytes();
    let parent = parent_of(path);
    if let Err(error) = std::fs::create_dir_all(&parent) {
        return pre_install_failure(&format!(
            "remove-file-if: creating {} failed: {error}",
            parent.display()
        ));
    }
    let claim = match create_private_temp(path) {
        Ok((claim, file)) => {
            // The script's `mktemp` creates the name and `rm -f` frees it again
            // so the CLAIM is the `mv`; the created file only reserves a unique
            // spelling, exactly as `mktemp` does.
            drop(file);
            remove_temp(&claim);
            claim
        }
        Err(error) => {
            return pre_install_failure(&format!(
                "remove-file-if: creating a claim temp failed: {error}"
            ));
        }
    };
    if std::fs::rename(path, &claim).is_err() {
        // The script's `mv` failed: the entry is either still there (someone
        // else published again, or the claim is not ours to take) or gone.
        if std::fs::symlink_metadata(path).is_ok() {
            return verdict_frame(b"M");
        }
        return verdict_frame(b"A");
    }
    // The comparison is against the CLAIMED inode, so a concurrent installer
    // that re-creates the path cannot change what is being compared.
    let matched = std::fs::read(&claim)
        .map(|content| content == expected)
        .unwrap_or(false);
    if matched {
        remove_temp(&claim);
        return verdict_frame(b"R");
    }
    // RESTORE no-replace: `link(2)` fails with EEXIST when the path already
    // holds a concurrent install, so the winner is never replaced; the claim is
    // discarded either way, exactly as the script's `ln` + `rm -f` do.
    let _ = std::fs::hard_link(&claim, path);
    remove_temp(&claim);
    verdict_frame(b"M")
}

/// The `remove-file-if-sidecar` subcommand: the transport's
/// `remove_file_if_sidecar_cmd` compare-and-delete under the sidecar's `flock`.
///
/// The sidecar file exists FIRST (`mkdir -p` of its parent, `touch`, `chmod
/// 644`), then the CLI opens it and takes the same exclusive NON-BLOCKING
/// `flock` [`sidecar_publish`] takes (`EINTR` retries, `EAGAIN`/`EWOULDBLOCK`
/// waits against a 2-second monotonic deadline, any other errno fails, a
/// deadline expiry is the contention refusal). Under that lock the lock record
/// is read and compared with `<expected>`: equal ⇒ unlink and answer `R`,
/// different (or unreadable) ⇒ `M` without touching it, absent ⇒ `A`. The
/// frame is one byte with no newline, exactly as the script's `print`.
///
/// Every failure is the script's own perl `die`, at ITS line: the records the
/// helper opens sit on the multiline program, so the location suffixes are
/// `-e line 3` (the sidecar open), `-e line 11` (a failed flock), `-e line 19`
/// (contention) and `-e line 26` (the unlink).
#[cfg(unix)]
#[allow(clippy::disallowed_methods)]
fn remove_file_if_sidecar(
    sidecar: &std::ffi::OsStr,
    lock: &std::ffi::OsStr,
    expected: &std::ffi::OsStr,
) -> std::process::ExitCode {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::io::AsRawFd;

    let expected = expected.as_bytes();
    let sidecar_path = std::path::Path::new(sidecar);
    let lock_path = std::path::Path::new(lock);
    let parent = parent_of(sidecar);
    if let Err(error) = std::fs::create_dir_all(&parent) {
        return pre_install_failure(&format!(
            "remove-file-if-sidecar: creating {} failed: {error}",
            parent.display()
        ));
    }
    // `touch`: create an absent sidecar, leave an existing one's content alone.
    match std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(sidecar_path)
    {
        Ok(_) => {}
        Err(error) => {
            return pre_install_failure(&format!(
                "remove-file-if-sidecar: creating {} failed: {error}",
                sidecar_path.display()
            ));
        }
    }
    if let Err(error) =
        std::fs::set_permissions(sidecar_path, std::fs::Permissions::from_mode(0o644))
    {
        return pre_install_failure(&format!(
            "remove-file-if-sidecar: chmod of {} failed: {error}",
            sidecar_path.display()
        ));
    }
    let sidecar_file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(sidecar_path)
    {
        Ok(file) => file,
        Err(error) => {
            let errno = error.raw_os_error();
            let mut message = b"open sidecar: ".to_vec();
            message.extend_from_slice(&errno_text(errno));
            message.extend_from_slice(b" at -e line 3.\n");
            return script_die(&message, errno);
        }
    };
    if let Err(failure) = flock_exclusive(sidecar_file.as_raw_fd()) {
        return match failure {
            FlockFailure::Failed(errno) => {
                let mut message = b"sidecar flock failed: ".to_vec();
                message.extend_from_slice(&errno_text(Some(errno)));
                message.extend_from_slice(b" at -e line 11.\n");
                script_die(&message, Some(errno))
            }
            FlockFailure::Contended(errno) => {
                script_die(b"sidecar contended at -e line 19.\n", errno)
            }
        };
    }
    // `! -e $lock && ! -l $lock`, i.e. `lstat` and not `stat`: a DANGLING
    // symlink is present (and is then read through, as the script's `open`
    // follows it).
    if std::fs::symlink_metadata(lock_path).is_err() {
        return verdict_frame(b"A");
    }
    let matched = std::fs::read(lock_path)
        .map(|content| content == expected)
        .unwrap_or(false);
    if !matched {
        return verdict_frame(b"M");
    }
    if let Err(error) = std::fs::remove_file(lock_path) {
        let errno = error.raw_os_error();
        let mut message = b"unlink: ".to_vec();
        message.extend_from_slice(&errno_text(errno));
        message.extend_from_slice(b" at -e line 26.\n");
        return script_die(&message, errno);
    }
    verdict_frame(b"R")
}

/// The far-side lock protocol's markers and reserved exit codes, re-spelled
/// from `SshTransport`'s private `holder` module: that module is inside the
/// library crate and a separate binary crate cannot reach it. The near side
/// classifies this program by these EXACT values (`far_side_lock_refusal`), and
/// `tests/remote_cli_lock.rs` drives the readiness line and the contention and
/// record refusals through the REAL transport, so a drift on either side fails
/// there.
#[cfg(unix)]
mod holder {
    /// A live holder owns the record (the single non-blocking `flock` reported
    /// `EWOULDBLOCK`/`EAGAIN`).
    pub(super) const CONTENDED: u8 = 3;
    /// The record could not be MADE usable: the parent `make_path`, the
    /// `O_NOFOLLOW` open, or the `chmod 0600` tightening failed.
    pub(super) const RECORD_UNUSABLE: u8 = 4;
    /// `flock` failed for a reason OTHER than contention.
    pub(super) const FLOCK_FAILED: u8 = 5;
    /// The entry is NON-EMPTY and does not begin with
    /// [`storekit::lock::RECORD_HEADER`] — not a record this crate wrote.
    pub(super) const RECORD_NOT_RECOGNIZED: u8 = 6;
    /// The readiness line's leading token, followed by the holder's pid.
    pub(super) const READY: &str = "LOCKOK";
    /// Leading token of the contention diagnostic (exit [`CONTENDED`]).
    pub(super) const MARK_CONTENDED: &str = "LOCKCONTENDED";
    /// Leading token of an unusable-record / failed-flock diagnostic (exit
    /// [`RECORD_UNUSABLE`] or [`FLOCK_FAILED`]).
    pub(super) const MARK_ERR: &str = "LOCKERR";
    /// Leading token of the "not a record this crate wrote" refusal (exit
    /// [`RECORD_NOT_RECOGNIZED`]).
    pub(super) const MARK_NOT_RECOGNIZED: &str = "LOCKRECORDNOTRECOGNIZED";
}

/// The `lock` subcommand: the far-side operation-lock HOLDER, the CLI form of
/// the transport's `PERL_HOLD_LOCK_TEMPLATE` (`perl_hold_lock`). Whatever this
/// program makes visible — stdout bytes, stderr bytes, the exit status, and the
/// record's bytes and mode on disk — is the contract the transport's near side
/// already reads, so the two forms must be indistinguishable:
///
/// 1. the record's PARENT chain is created when `dirname(record)` is not
///    already a directory (`File::Path::make_path`); a failure is
///    `LOCKERR mkdir <dir>: <strerror>`, exit 4;
/// 2. the record is opened `O_RDWR | O_CREAT | O_NOFOLLOW` at `0600`, so a
///    symlink planted at the record fails CLOSED (`ELOOP`) instead of
///    redirecting the write into a victim; a failure is `LOCKERR open <path>:
///    <strerror>`, exit 4;
/// 3. ONE non-blocking `flock(LOCK_EX | LOCK_NB)`: `EWOULDBLOCK`/`EAGAIN` is
///    `LOCKCONTENDED <path>: <strerror>`, exit 3; any other failure is
///    `LOCKERR flock <path>: <strerror>`, exit 5. No case ever waits;
/// 4. AFTER the flock is held — the same order the local `FileLock::acquire`
///    uses — the entry is read to the length of
///    [`storekit::lock::RECORD_HEADER`]: a NON-EMPTY entry that does not begin
///    with that header is not a record this crate wrote, so it is REFUSED
///    (`LOCKRECORDNOTRECOGNIZED <path>: the entry is not a lock record this
///    crate wrote; refusing to truncate it`, exit 6) and left byte-for-byte
///    and mode-for-mode untouched;
/// 5. the record is chmodded `0600` (`fchmod` on the open descriptor); a
///    failure is `LOCKERR chmod <path>: <strerror>`, exit 4;
/// 6. the header followed by `op_id` is written over the record from offset 0;
/// 7. `LOCKOK <pid>` is printed and flushed;
/// 8. the process BLOCKS reading STDIN until EOF, then `flock(LOCK_UN)`s and
///    closes. Holding the flock for the caller's SESSION is the contract, not
///    for one command; a dropped connection EOFs this stdin and process death
///    releases the lock regardless.
///
/// The diagnostics are `print STDERR`, NOT `die`: they carry no perl
/// ` at -e line N.` location suffix.
#[cfg(unix)]
#[allow(clippy::disallowed_methods)]
fn lock_holder(record: &std::ffi::OsStr, op_id: &std::ffi::OsStr) -> std::process::ExitCode {
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::os::unix::io::AsRawFd;

    let path = std::path::Path::new(record);
    let dir = dirname_of(record);
    if !dir.is_dir()
        && let Err(error) = std::fs::create_dir_all(&dir)
    {
        return holder_error(
            holder::MARK_ERR,
            Some("mkdir"),
            dir.as_os_str().as_bytes(),
            error.raw_os_error(),
            holder::RECORD_UNUSABLE,
        );
    }
    // The `0600` is a REQUEST subject to the umask, exactly like perl's
    // `sysopen` mode (the umask can only remove bits) — and the chmod below
    // re-applies it unconditionally, so the record ends up private either way.
    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(error) => {
            return holder_error(
                holder::MARK_ERR,
                Some("open"),
                path_bytes(path),
                error.raw_os_error(),
                holder::RECORD_UNUSABLE,
            );
        }
    };
    // ONE attempt: a live holder is a refusal, never a wait.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let error = std::io::Error::last_os_error();
        let errno = error.raw_os_error();
        if errno == Some(libc::EWOULDBLOCK) || errno == Some(libc::EAGAIN) {
            return holder_error(
                holder::MARK_CONTENDED,
                None,
                path_bytes(path),
                errno,
                holder::CONTENDED,
            );
        }
        return holder_error(
            holder::MARK_ERR,
            Some("flock"),
            path_bytes(path),
            errno,
            holder::FLOCK_FAILED,
        );
    }
    // Read at most the header's length. The script leaves its read loop on EOF
    // OR on a read error and then compares whatever it accumulated, so a failed
    // read is not its own failure arm; `read_to_end` keeps the bytes read
    // before the error, which is exactly that accumulated `$head`.
    let header = storekit::lock::RECORD_HEADER.as_bytes();
    let mut head = Vec::with_capacity(header.len());
    let _ = (&file).take(header.len() as u64).read_to_end(&mut head);
    if !head.is_empty() && !head.starts_with(header) {
        let mut message = format!("{} ", holder::MARK_NOT_RECOGNIZED).into_bytes();
        message.extend_from_slice(path_bytes(path));
        message.extend_from_slice(
            b": the entry is not a lock record this crate wrote; refusing to truncate it\n",
        );
        let _ = std::io::stderr().lock().write_all(&message);
        return std::process::ExitCode::from(holder::RECORD_NOT_RECOGNIZED);
    }
    if let Err(error) = file.set_permissions(std::fs::Permissions::from_mode(0o600)) {
        return holder_error(
            holder::MARK_ERR,
            Some("chmod"),
            path_bytes(path),
            error.raw_os_error(),
            holder::RECORD_UNUSABLE,
        );
    }
    // The script does NOT check these four calls (there is no `or die`), so the
    // CLI must not invent a failure arm it does not have: it proceeds to the
    // ready line exactly as the script does. `set_len` does not move the
    // offset; the seek does.
    let _ = file.seek(SeekFrom::Start(0));
    let _ = file.set_len(0);
    let _ = file.write_all(header);
    let _ = file.write_all(op_id.as_bytes());
    let _ = file.flush();
    let mut out = std::io::stdout().lock();
    let ready = format!("{} {}\n", holder::READY, std::process::id());
    if let Err(error) = out.write_all(ready.as_bytes()).and_then(|()| out.flush()) {
        return fail(&format!("writing the lock ready line failed: {error}"));
    }
    // The stdout lock must not be held across the (unbounded) stdin wait.
    drop(out);
    // BLOCK until EOF. The bytes are discarded: EOF is the whole signal.
    let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
    // The explicit release, then close. Best-effort exactly as the script's
    // unchecked `flock($fh, LOCK_UN); close($fh);` is: process death has
    // already released the flock if either call is skipped.
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
    drop(file);
    std::process::ExitCode::SUCCESS
}

/// `File::Basename::dirname` for a Unix path — the semantics the holder script
/// uses (`use File::Basename qw(dirname)`) — rather than `Path::parent`'s:
/// trailing slashes are removed (`a/` is `a`), repeated separators are
/// collapsed (`a//b` is `a`), a path with no directory component is `.`, and
/// the filesystem root is `/`.
#[cfg(unix)]
fn dirname_of(path: &std::ffi::OsStr) -> std::path::PathBuf {
    use std::os::unix::ffi::OsStrExt;

    let bytes = path.as_bytes();
    if bytes.is_empty() {
        return std::path::PathBuf::from(".");
    }
    let mut end = bytes.len();
    while end > 1 && bytes[end - 1] == b'/' {
        end -= 1;
    }
    if end == 1 && bytes[0] == b'/' {
        return std::path::PathBuf::from("/");
    }
    match bytes[..end].iter().rposition(|&b| b == b'/') {
        None => std::path::PathBuf::from("."),
        Some(0) => std::path::PathBuf::from("/"),
        Some(separator) => {
            let mut cut = separator;
            while cut > 1 && bytes[cut - 1] == b'/' {
                cut -= 1;
            }
            std::path::PathBuf::from(std::ffi::OsStr::from_bytes(&bytes[..cut]))
        }
    }
}

/// Print ONE anchored holder diagnostic to STDERR — `<marker>[ <what>] <path>:
/// <strerror>` plus one LF — and return the arm's reserved exit code. It is a
/// `print STDERR`, never a `die`: no ` at -e line N.` suffix is appended, and
/// the errno text is the C library's (`$!`'s), not `std`'s rendering.
#[cfg(unix)]
fn holder_error(
    marker: &str,
    what: Option<&str>,
    path: &[u8],
    errno: Option<i32>,
    code: u8,
) -> std::process::ExitCode {
    use std::io::Write;

    let mut message = marker.as_bytes().to_vec();
    message.push(b' ');
    if let Some(what) = what {
        message.extend_from_slice(what.as_bytes());
        message.push(b' ');
    }
    message.extend_from_slice(path);
    message.extend_from_slice(b": ");
    message.extend_from_slice(&errno_text(errno));
    message.push(b'\n');
    let _ = std::io::stderr().lock().write_all(&message);
    std::process::ExitCode::from(code)
}

/// Print the ONE verdict frame the transport's parser reads. It is the script's
/// `printf 'R'` / perl `print "M"`: the byte alone, no newline, flushed before
/// exit.
#[cfg(unix)]
fn verdict_frame(frame: &[u8]) -> std::process::ExitCode {
    use std::io::Write;

    let mut out = std::io::stdout().lock();
    match out.write_all(frame).and_then(|()| out.flush()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => fail(&format!("writing the verdict frame failed: {error}")),
    }
}

/// The parent directory a writer's `mkdir -p` targets — `path.parent()`, or
/// `.` when the path has no parent component, exactly as the transport's
/// scripts compute it.
#[cfg(unix)]
fn parent_of(path: &std::ffi::OsStr) -> std::path::PathBuf {
    std::path::Path::new(path)
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

/// Read the WHOLE payload from STDIN. The payload is never an argv token.
#[cfg(unix)]
fn read_stdin_payload() -> std::io::Result<Vec<u8>> {
    let mut data = Vec::new();
    std::io::Read::read_to_end(&mut std::io::stdin().lock(), &mut data)?;
    Ok(data)
}

/// Create a UNIQUE temp beside `dest` with `O_EXCL` and the `mktemp` private
/// mode (0o600). The name comes from the crate's one temp authority
/// ([`storekit::atomic::temp_name_for`]), so it is dot-prefixed, same-directory,
/// BOUNDED for a destination at `NAME_MAX`, and recognised by the recovery
/// sweep. A name that already exists (a stale temp from a crashed writer) is
/// skipped: the counter advances, so no existing entry is ever truncated.
#[cfg(unix)]
#[allow(clippy::disallowed_methods)]
fn create_private_temp(
    dest: &std::ffi::OsStr,
) -> std::io::Result<(std::path::PathBuf, std::fs::File)> {
    use std::os::unix::fs::OpenOptionsExt;

    let dest = std::path::Path::new(dest);
    for _ in 0..64 {
        let tmp = storekit::atomic::temp_name_for(dest);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
        {
            Ok(file) => return Ok((tmp, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "no unique temp name available",
    ))
}

/// Best-effort temp removal. Every writer removes its temp on EVERY failure
/// path; this is the one unlink helper they share.
#[cfg(unix)]
#[allow(clippy::disallowed_methods)]
fn remove_temp(tmp: &std::path::Path) {
    let _ = std::fs::remove_file(tmp);
}

/// The truncation guard run BETWEEN STDIN EOF and the install: compare the
/// temp's received size with the caller's expected length. `None` means the
/// sizes agree; otherwise the returned bytes are the transport script's exact
/// `storekit: remote payload truncated: <temp> holds <n|?> bytes, expected
/// <m>; refusing to publish` diagnostic.
#[cfg(unix)]
fn received_length_mismatch(tmp: &std::path::Path, expected: u64) -> Option<Vec<u8>> {
    let held = std::fs::metadata(tmp).map(|meta| meta.len());
    match held {
        Ok(held) if held == expected => None,
        Ok(held) => Some(truncation_message(tmp, Some(held), expected)),
        Err(_) => Some(truncation_message(tmp, None, expected)),
    }
}

/// The `PERL_VERIFY_LEN` diagnostic, byte for byte (the path is the TEMP, as it
/// is in the script).
#[cfg(unix)]
fn truncation_message(tmp: &std::path::Path, held: Option<u64>, expected: u64) -> Vec<u8> {
    let mut message = b"storekit: remote payload truncated: ".to_vec();
    message.extend_from_slice(path_bytes(tmp));
    message.extend_from_slice(b" holds ");
    match held {
        Some(held) => message.extend_from_slice(held.to_string().as_bytes()),
        None => message.push(b'?'),
    }
    message.extend_from_slice(b" bytes, expected ");
    message.extend_from_slice(expected.to_string().as_bytes());
    message.extend_from_slice(b"; refusing to publish\n");
    message
}

/// `fsync(2)` exactly as the far-side perl primitive performs it, with the
/// primitive's `die` diagnostics: open `O_RDONLY | O_NONBLOCK` (so a FIFO can
/// never block the open), `fstat` the SAME descriptor, REFUSE anything that is
/// neither a regular file nor a directory, then sync. The error carries the
/// errno (for the caller's exit-status rule) and the perl-format message
/// (` at -e line 1.` included, because the primitive is a one-line `-e`).
#[cfg(unix)]
#[allow(clippy::disallowed_methods)]
fn fsync_object(path: &std::path::Path) -> Result<(), (Option<i32>, Vec<u8>)> {
    use std::os::unix::fs::OpenOptionsExt;

    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) => {
            return Err((
                error.raw_os_error(),
                fsync_message(b"fsync-open", path, &error),
            ));
        }
    };
    let meta = match file.metadata() {
        Ok(meta) => meta,
        Err(error) => {
            return Err((
                error.raw_os_error(),
                fsync_message(b"fsync-stat", path, &error),
            ));
        }
    };
    if !meta.is_file() && !meta.is_dir() {
        let mut message = b"fsync-refuse ".to_vec();
        message.extend_from_slice(path_bytes(path));
        message.extend_from_slice(b": not a regular file or directory at -e line 1.\n");
        return Err((perl_handle_setup_errno(&file), message));
    }
    match file.sync_all() {
        Ok(()) => Ok(()),
        Err(error) => Err((error.raw_os_error(), fsync_message(b"fsync", path, &error))),
    }
}

/// One perl `die` diagnostic from the fsync primitive: `<what> <path>: <text>
/// at -e line 1.`.
#[cfg(unix)]
fn fsync_message(what: &[u8], path: &std::path::Path, error: &std::io::Error) -> Vec<u8> {
    let mut message = what.to_vec();
    message.push(b' ');
    message.extend_from_slice(path_bytes(path));
    message.extend_from_slice(b": ");
    message.extend_from_slice(&errno_text(error.raw_os_error()));
    message.extend_from_slice(b" at -e line 1.\n");
    message
}

/// The `$!` perl's `sysopen` leaves on the descriptor when the fsync primitive's
/// REFUSAL fires, which the script's `die` therefore exits with.
///
/// A successful `sysopen` still lays a buffered handle over the descriptor, and
/// that setup probes it with `isatty` and then `lseek`; neither probe's failure
/// is cleared, so the LAST failure's errno is what `$!` holds when the refusal
/// `die` runs and perl's exit-status rule reads it. Reproduced with the SAME two
/// calls on the SAME descriptor instead of a table of file types, so the result
/// is the platform's own answer: a FIFO or a tty leaves ESPIPE (the `lseek`
/// failure, 29 on Linux and macOS), a character device leaves ENOTTY (the
/// `isatty` failure, 25) where `lseek` succeeds, and only a probe pair that both
/// SUCCEED leaves no errno — the `die` rule's 255. A probe that succeeds leaves
/// `$!` untouched, exactly as it does in perl, which is why the sequence (not a
/// single call) is reproduced.
///
/// This is reached ONLY for an already-OPENED descriptor whose kind was
/// classified, so a real open/stat/fsync failure — which does carry a genuine
/// errno — never goes through it.
#[cfg(unix)]
fn perl_handle_setup_errno(file: &std::fs::File) -> Option<i32> {
    use std::os::unix::io::AsRawFd;

    let fd = file.as_raw_fd();
    let mut errno = None;
    // SAFETY: `fd` is open for the whole call and `isatty` neither retains nor
    // closes it.
    if unsafe { libc::isatty(fd) } == 0 {
        errno = std::io::Error::last_os_error().raw_os_error();
    }
    // SAFETY: as above; `lseek` on an already-open descriptor neither retains
    // nor closes it, and `SEEK_CUR` with a zero offset changes nothing.
    if unsafe { libc::lseek(fd, 0, libc::SEEK_CUR) } < 0 {
        errno = std::io::Error::last_os_error().raw_os_error();
    }
    errno
}

/// The raw bytes of a path, for the byte-exact diagnostics above.
#[cfg(unix)]
fn path_bytes(path: &std::path::Path) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes()
}

/// A pre-install failure: the temp (if any) is already gone and the previous
/// entry is intact. [`SSH_TWRITE_PREINSTALL_EXIT`] is the reserved
/// "nothing was installed" status both create-new scripts use.
#[cfg(unix)]
fn pre_install_failure(message: &str) -> std::process::ExitCode {
    eprintln!("storekit-remote: {message}");
    std::process::ExitCode::from(SSH_TWRITE_PREINSTALL_EXIT as u8)
}

/// perl's `die` EXIT STATUS rule: `$!` when it is nonzero, otherwise 255.
#[cfg(unix)]
fn perl_die_code(errno: Option<i32>) -> u8 {
    match errno {
        Some(code) if (1..=255).contains(&code) => code as u8,
        _ => 255,
    }
}

/// Why the sidecar's `flock` did not succeed.
#[cfg(unix)]
enum FlockFailure {
    /// The `flock` failed for a reason other than contention, carrying its
    /// errno (the script's `die "sidecar flock failed: $!"`).
    Failed(i32),
    /// The monotonic deadline expired while the lock stayed held elsewhere,
    /// carrying the errno the last `flock` left in `$!` — `EAGAIN`/
    /// `EWOULDBLOCK`, which perl's `die "sidecar contended"` also EXITS with.
    Contended(Option<i32>),
}

/// The sidecar's exclusive NON-BLOCKING `flock` with the prelude's policy:
/// `EINTR` retries immediately, `EAGAIN`/`EWOULDBLOCK` waits
/// `min(5ms, remaining)` against a 2-second monotonic deadline, any other errno
/// fails. The lock is held by `fd`'s open file description until the command
/// exits.
#[cfg(unix)]
fn flock_exclusive(fd: std::os::unix::io::RawFd) -> Result<(), FlockFailure> {
    use std::time::{Duration, Instant};

    const DEADLINE: Duration = Duration::from_secs(2);
    const INTERVAL: Duration = Duration::from_millis(5);

    let deadline = Instant::now() + DEADLINE;
    loop {
        // SAFETY: `fd` is an open descriptor owned by the caller for the whole
        // call; `flock` neither retains nor closes it.
        if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        match error.kind() {
            std::io::ErrorKind::Interrupted => continue,
            std::io::ErrorKind::WouldBlock => {
                let now = Instant::now();
                if now >= deadline {
                    // `flock`'s EWOULDBLOCK carries the errno perl's `$!` holds at
                    // the script's `die`; a code the OS did not report (which
                    // `flock` cannot produce) leaves `$!` to the `die` rule.
                    return Err(FlockFailure::Contended(error.raw_os_error()));
                }
                std::thread::sleep(INTERVAL.min(deadline - now));
            }
            _ => {
                return Err(FlockFailure::Failed(error.raw_os_error().unwrap_or(0)));
            }
        }
    }
}

#[cfg(unix)]
fn usage(problem: &str) -> std::process::ExitCode {
    fail(&format!(
        "{problem}\nusage: storekit-remote protocol\n       storekit-remote lstat <path>\n       storekit-remote list <path>\n       storekit-remote manifest <root>\n       storekit-remote verify-open <path>\n       storekit-remote publish <path> <mode> <len>\n       storekit-remote write-new <path> <mode> <len>\n       storekit-remote sidecar-publish <sidecar> <lock> <mode> <len>\n       storekit-remote fsync-file <path>\n       storekit-remote fsync-dir <path>\n       storekit-remote fsync-tree <path>\n       storekit-remote rename <from> <to>\n       storekit-remote remove-file-if <path> <expected>\n       storekit-remote remove-file-if-sidecar <sidecar> <lock> <expected>\n       storekit-remote lock <record> <op_id>"
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
