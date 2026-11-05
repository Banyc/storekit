# Inventory: a far-side CLI instead of shell and perl scripts

The SSH transport today sends shell and perl programs to the remote and parses their
bytes: a framed `lstat`, a directory listing, an upload/publish chain, a write-new
chain, a sidecar publish, rename, three fsync primitives, an fsync tree walk, two
compare-and-delete helpers, a far-side flock holder, and the recursive sync manifest
walk. Each is a string constant or a `format!` in `src/transport/ssh/mod.rs` (and
`remote_tree_verify_script` in `src/manifest/mod.rs`).

Add a `storekit-remote` binary that performs those operations natively. When the remote
has it at a compatible protocol version, the transport runs it; when it does not, the
transport runs exactly the script it runs today. The scripts are the fallback and stay
in the tree.

**The oracle.** Every op is accepted only when it is byte-identical to the script it
replaces: same stdout, same stderr the transport matches on, same exit statuses. Parity
tests run both forms on the same fixtures and compare, so the CLI cannot drift from the
protocol the near side already parses. A CLI that is present but fails is an error, not
a trigger to retry with the script; fallback is decided once, by the probe.

**Non-goals.** No wire-format change, no auto-upload of the binary, no new remote
dependency beyond the binary (perl and the shell remain the fallback, never a
requirement of the CLI path), and no removal of any script.

| Item | Status |
|---|---|
| `remote-cli-probe` | DONE — change `zusosvmn` (`f4645d7e`) |
| `remote-cli-read-ops` | DONE — change `zwwtxknu` (`e5c7a45f`) |
| `remote-cli-write-ops` | DONE — change `tkkvxwnw` (`ec33f629`) |
| `remote-cli-fsync-ops` | OPEN |
| `remote-cli-lock` | OPEN |
| `remote-cli-manifest` | OPEN |

## remote-cli-probe

**Goal.** The binary exists, states a protocol version, and the transport probes it once
per transport instance, prefers it, and falls back to the script only when it is absent
or incompatible. One op (`lstat`) is switched end to end so the mechanism has a live
caller.

**Carrier.** `[[bin]] name = "storekit-remote"` in `Cargo.toml`; `storekit-remote
protocol` prints `storekit-remote-protocol 1` (one constant shared by the bin and the
transport); a `FarSide` mode cached on `SshTransport` (probe once, never per op); one
command selector that both forms go through; `lstat` implemented in the bin and switched
in the transport.

**Watch.** The probe runs through the same bounded runner as every other command, so a
hung remote cannot hang the run without a deadline. `126`/`127` and a wrong token mean
"absent" (script fallback); `255` (ssh itself), a timeout, or any other status is an
error and must not be read as absence. A local transport never probes. The probe must
not run for a transport that never issues a remote command. The binary must build under
the Windows target (the far-side body is Unix-only; the non-Unix `main` refuses with a
message), because `--all-targets` is a gate.

**Acceptance.** A fake `ssh` on `PATH` records the remote command and runs it locally;
with a compatible `storekit-remote` on `PATH` the first op probes once and then runs
`storekit-remote lstat …` (the log contains no `perl`); a second op does not probe
again. With no CLI (127) the log shows the `perl -e` command and the run behaves
exactly as today. A wrong protocol token falls back the same way. An ssh-level 255
fails the run and is not treated as absence. `lstat` parity: `P`, `A` and `E` frames
and the exit status match the script for a file, an absent path, an absent parent, and
a permission error.

## remote-cli-read-ops

**Goal.** `list` and the descriptor-bound open/read helper (`verify_open_script`) are
native and preferred. `read_link` is the POSIX `readlink` and `read` is `cat`, so
neither is a script and neither moves.

**Carrier.** Subcommands in the bin; switches in `SshTransport::list` and the
verification-open call site; the existing near-side parsers are unchanged.

**Watch.** Names are RAW BYTES: the listing prints `kind\tmode\tsize\tname\0` with no
UTF-8 conversion. `readdir` order is the filesystem's; the near-side sort is unchanged.
An absent directory is an empty listing (exit 0); an unreadable one is an error, not an
empty listing. `O_NOFOLLOW` must be preserved for the descriptor-bound helper, and a
symlink there is `ELOOP`, never followed. Its frame is `O\t<rawmode>\n<content>` for a
regular file, `N\t<rawmode>` for anything else, and `E\t<errno>` for a failed
open/fstat/read, with exit 0 in every case. The helper's `#[cfg(test)]` swap seam has no
CLI equivalent: when that seam is installed the transport must use the script.

**Acceptance.** Differential tests over: an empty directory, a file, a directory, a
symlink, a FIFO, a dangling symlink, a non-UTF-8 name, a `chmod 400` directory, a
missing directory; and for the helper a regular file (content and raw mode), a symlink
(`ELOOP`), an absent path, a directory (`N`), and a FIFO (never blocks). Stdout bytes,
stderr and exit match the script. The transport-level tests assert the CLI form is used
when present and the script form when absent.

## remote-cli-write-ops

**Goal.** `publish` (upload), `write-new` and `sidecar-publish` are native and
preferred.

**Carrier.** Subcommands that read the payload from stdin; the same temp/mode/fsync/
rename ordering; the switches in `upload_bytes`, `write_new_cmd`'s call site and the
sidecar upload.

**Watch.** Exit `17` is conflict, `1` is pre-install, `74` is post-rename durability
unknown; a failed step before the rename removes the temp and leaves the old entry. The
received length check runs BETWEEN stdin EOF and the publish, so a truncated transfer is
refused. The temp name is unique and same-directory. A directory destination refuses
rather than being descended. The sidecar path keeps its flock while writing and installs
by hard link.

**Acceptance.** Differential tests for: a new file, replacing a file, replacing a
symlink, a directory destination, a truncated stdin, a missing parent (created), mode
permission, and a post-rename fsync failure (the exit-74 path). The transport-level
tests assert the CLI path and the fallback path separately.

## remote-cli-fsync-ops

**Goal.** `fsync-file`, `fsync-dir`, `fsync-tree`, `rename`, `remove-file-if` and
`remove-file-if-sidecar` are native and preferred.

**Carrier.** Subcommands; the switches in `fsync_tree`, `fsync_parent`, `rename`,
`rename_aside`, `remove_file_if` and the sidecar helper.

**Watch.** `fsync-tree` visits children before parents and skips symlinks and special
files (their directory entry is the parent fsync's job); a failed entry fails the whole
walk. `fsync` refuses a FIFO without blocking (open it `O_NONBLOCK`, classify the inode).
`rename` creates the parent and never moves into a directory target. `remove-file-if`
claims with `mv` to a same-directory temp and answers `R` or `M`, restoring on a
mismatch; an absent entry answers `A`.

**Acceptance.** Differential tests over a depth-3 tree, a tree containing a symlink and
a FIFO, a read-only directory, and each remove-if verdict. The transport-level tests
assert CLI-first and fallback.

## remote-cli-lock

**Goal.** The far-side flock holder is native and preferred.

**Carrier.** A `lock` subcommand that takes the record, the operation id and the record
header, holds the flock, prints the ready marker, and stays until stdin closes; the
switch in `lock_far_side`.

**Watch.** The statuses and markers are protocol: `LOCKOK`, `LOCKCONTENDED`, `LOCKERR`,
`LOCKRECORDNOTRECOGNIZED`, and exits `3`, `4`, `5`, `6`, `126`, `127`. A contended
record must not be rewritten. The lock is held for the session's lifetime, not for one
command. Releasing on stdin EOF must be the only release path.

**Acceptance.** Differential tests against `PERL_HOLD_LOCK_TEMPLATE` for: a free record,
a held record, a malformed record, a record whose directory is unwritable, and a probe
that distinguishes `126`/`127` from a real refusal. A transport-level test asserts the
CLI holder keeps a second run out exactly as the perl holder does.

## remote-cli-manifest

**Goal.** The recursive sync manifest walk (`remote_tree_verify_script`) is native and
preferred; the install and fallback rules are documented.

**Carrier.** A `manifest` subcommand with the same validation and wire output; the
switch in the remote/near-side manifest producers; README and `MIGRATION.md` gain the
install line (`cargo install --path . --bin storekit-remote` or the released tag), the
protocol gate, and the rule that the scripts remain the no-CLI path.

**Watch.** The output is the script's exact byte stream: tab-delimited records, hashes,
targets, and the same `die` diagnostics for an absent root, a non-directory root, an
unreadable directory, a non-UTF-8/non-NFC name, a NUL/LF/CR/TAB name or target, and
`nlink > 1`. The `Digest::SHA`/`Unicode::Normalize` behaviour must match byte for byte.

**Acceptance.** Differential tests over a representative tree (files, dirs, symlinks,
modes, empty dir, non-ASCII names), plus each refusal case, comparing stdout, stderr and
exit to the script. A transport-level test asserts the manifest op uses the CLI when
present and the script when not.
