//! The Unix implementation of the bounded child-runner: process-group
//! lifecycle (`process_group(0)` spawn, `waitid` WNOWAIT peek, `killpg`
//! termination), the foreground-only group check (Linux `/proc` scan,
//! macOS `proc_listpgrp`), and `poll`/`fcntl` non-blocking pipe drains.
//! Selected by the single `#[cfg(unix)]` `mod` declaration in [`super`].

use super::*;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Stdio};
use std::time::Instant;

pub fn kill_process_group(pgid: i32, sig: i32) -> std::io::Result<()> {
    // SAFETY: `killpg` on a process group this runner created for its own
    // child; `pgid` is the child's pid (positive) and `sig` is a valid libc
    // signal constant.
    let rc = unsafe { libc::killpg(pgid, sig) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// True when the direct child has EXITED but has NOT been reaped yet
/// (`waitid(2)` with `WNOHANG | WNOWAIT | WEXITED`): the child remains a
/// ZOMBIE, and a zombie holds its pid — and therefore its process-group id
/// (the child is the group leader, pgid == pid) — allocated until reaped.
/// The foreground-only check runs between this peek and the reap, so a
/// `killpg(pgid, ...)` in that window can never race a pid the OS recycled
/// for an unrelated process: the group being signalled is provably ours.
/// ECHILD (the child is gone — reaped or never ours) is treated as exited so
/// the caller proceeds to the reap instead of spinning.
fn child_exited_unreaped(pid: u32) -> std::io::Result<bool> {
    let mut si: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: `waitid` on our own child (a positive pid, the direct child of
    // this process); `WNOWAIT` leaves it waitable for the subsequent reap;
    // `WNOHANG` never blocks; `WEXITED` reports the exited transition; the
    // zero-initialized siginfo is written by the kernel only on success.
    let rc = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as _,
            &mut si,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if rc != 0 {
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() == Some(libc::ECHILD) {
            return Ok(true);
        }
        return Err(e);
    }
    Ok(siginfo_pid(&si) != 0)
}

/// The `si_pid` of a `siginfo_t` after a successful `waitid`: a FIELD on
/// macOS, a METHOD on Linux (libc 0.2.155+ exposes the union members as
/// methods there) — the accessor hides the platform difference so the
/// caller is portable.
#[cfg(target_os = "linux")]
fn siginfo_pid(si: &libc::siginfo_t) -> libc::pid_t {
    // SAFETY: the union field is valid after a successful `waitid` wrote
    // the siginfo.
    unsafe { si.si_pid() }
}
#[cfg(not(target_os = "linux"))]
fn siginfo_pid(si: &libc::siginfo_t) -> libc::pid_t {
    si.si_pid
}

/// The LIVE members of the process group `pgid` — judged by the shared
/// `is_live_state` rule, so `Z` (EXIT_ZOMBIE) AND `X` (EXIT_DEAD) are
/// excluded — minus the runner's own child `exclude_pid`. This is the
/// FOREGROUND-ONLY detection:
/// after the direct child exits (held as a zombie), any remaining live member
/// is a background descendant the command left behind. The enumeration never
/// uses the fault-injected [`KillSeam`] — it is a pure detection primitive,
/// so an injected kill fault cannot turn a clean group into a false
/// "leftover". A scan error (a vanished/EPERM process mid-scan) skips that
/// entry; only a fully failed scan degrades to an empty list.
#[cfg(target_os = "linux")]
fn live_group_members(pgid: i32, exclude_pid: u32) -> Vec<i32> {
    let mut members = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return members;
    };
    for entry in entries.flatten() {
        // Bind the OsString first: `file_name().to_str()` borrows from a
        // temporary that dies at the end of the let-else, so the `&str`
        // would dangle (E0716).
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let Ok(pid) = name.parse::<i32>() else {
            continue;
        };
        if pid == exclude_pid as i32 {
            continue;
        }
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        // Format: `pid (comm) state ppid pgrp session ...` — `comm` may
        // contain spaces AND ')' — anchor on the LAST ')'.
        let Some(rest) = stat.rsplit_once(')') else {
            continue;
        };
        let mut fields = rest.1.split_whitespace();
        let state = fields.next().unwrap_or("");
        let _ppid = fields.next();
        let pgrp: i32 = fields.next().and_then(|s| s.parse().ok()).unwrap_or(-1);
        if pgrp == pgid && is_live_state(state) {
            members.push(pid);
        }
    }
    members
}

#[cfg(target_os = "macos")]
fn live_group_members(pgid: i32, exclude_pid: u32) -> Vec<i32> {
    // `proc_listpgrppids(3)`: the pids of every process in the group —
    // ZOMBIES INCLUDED (a killed descendant that launchd has not yet reaped
    // is still listed). A zombie is NOT live, so every member's state is
    // read via `macos_is_not_live` (which reads the `pbi_status` field at
    // byte offset 4 through `proc_pidinfo(PROC_PIDTBSDINFO)`) and zombies
    // are excluded — otherwise a command whose descendants were killed would
    // be falsely reported as having left background processes. Our own
    // zombie child is excluded by pid (it is the group leader, still
    // waitable until we reap it). A member whose state cannot be read has
    // vanished (reaped) in the window between the enumeration and the read —
    // it is not live, so it is excluded too.
    let mut buf = [0i32; 4096]; // room for up to 4096 group members
    let n = unsafe { proc_listpgrppids(pgid, buf.as_mut_ptr().cast(), (buf.len() * 4) as i32) };
    if n <= 0 {
        return Vec::new();
    }
    let n = (n as usize).min(buf.len());
    buf[..n]
        .iter()
        .copied()
        .filter(|p| *p != exclude_pid as i32 && !macos_is_not_live(*p))
        .collect()
}

/// THE ONE macOS "is this BSD process status NOT live?" decision, shared by
/// production's group-member enumeration (`macos_is_not_live`, which reads
/// `pbi_status` through `proc_pidinfo(PROC_PIDTBSDINFO)`) and the test
/// oracle (which reads `p_stat` through `sysctl(KERN_PROC_PID)`), so a future
/// change to the rule cannot update one arm and miss the other. The two
/// ACCESSORS legitimately differ — the oracle needs the start-time token, and
/// `proc_pidinfo` reports nothing for a zombie — but the DECISION is this
/// one comparison.
///
/// Only `SZOMB` (= 5 from `sys/proc.h`) is not live: a zombie holds no live
/// resources. Every other status (`SIDL` idle, `SRUN` running, `SSLEEP`
/// sleeping, `SSTOP` stopped, and any value a future kernel adds) counts as
/// LIVE, so an unfamiliar status can never let a leftover escape the
/// foreground-only check.
#[cfg(target_os = "macos")]
pub(crate) fn macos_status_is_not_live(status: u32) -> bool {
    const SZOMB: u32 = 5;
    status == SZOMB
}

/// Whether the member is NOT live — a zombie (`SZOMB` = 5, judged by the
/// shared `macos_status_is_not_live`) or already vanished (reaped in the
/// window between the enumeration and the read, which makes `proc_pidinfo`
/// fail): either way it must be EXCLUDED from the live-members list, or a
/// command whose descendants were killed would be falsely reported as having
/// left background processes.
#[cfg(target_os = "macos")]
fn macos_is_not_live(pid: i32) -> bool {
    // The first 8 bytes of `struct proc_bsdinfo` are `pbi_flags` (offset 0)
    // and `pbi_status` (offset 4, a uint32 copy of the process state); the
    // full struct (with rusage) is ~136 bytes on modern macOS, so the buffer
    // must be at least that large for `proc_pidinfo` to write anything. The
    // live/not-live DECISION is `macos_status_is_not_live`, never a second
    // copy of the `SZOMB` comparison here. A failed read means the process
    // has vanished — not live either.
    const PROC_PIDTBSDINFO: i32 = 3;
    let mut bsd = [0u8; 256];
    let n = unsafe {
        proc_pidinfo(
            pid,
            PROC_PIDTBSDINFO,
            0,
            bsd.as_mut_ptr().cast(),
            bsd.len() as i32,
        )
    };
    if n < 8 {
        return true; // gone (or unreadable) — not a live member
    }
    macos_status_is_not_live(u32::from_le_bytes([bsd[4], bsd[5], bsd[6], bsd[7]]))
}

#[cfg(target_os = "macos")]
#[link(name = "proc")]
unsafe extern "C" {
    fn proc_listpgrppids(pid: i32, buffer: *mut std::ffi::c_void, buffersize: i32) -> i32;
    fn proc_pidinfo(
        pid: i32,
        flavor: i32,
        arg: u64,
        buffer: *mut std::ffi::c_void,
        buffersize: i32,
    ) -> i32;
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn live_group_members(_pgid: i32, _exclude_pid: u32) -> Vec<i32> {
    compile_error!("live group-member enumeration is implemented for Linux and macOS only");
}

/// The kill seam behind [`ChildRunner`]: the syscall-level termination
/// surface, injectable for tests (a kill-function pointer seam). The runner
/// reports a kill failure as an error only when the seam says the signal
/// could not be delivered; an inert seam (returns `Ok` without signalling) is
/// caught by the reap bound instead.
///
/// Public because [`RunnerConfig`] is public and takes an `Arc<dyn KillSeam>`:
/// a caller building its own [`RunnerConfig`] (and a test injecting a
/// syscall-level fault) needs the trait to name its seam. The crate's own
/// real-process lifecycle tests drive the runner under injected kill faults
/// through it.
pub struct RealKill;

impl KillSeam for RealKill {
    fn kill_group(&self, pgid: i32, sig: i32) -> std::io::Result<()> {
        kill_process_group(pgid, sig)
    }
    fn kill_owned(&self, child: &mut Child) -> std::io::Result<()> {
        child.kill()
    }
}

/// The Unix termination ladder's transitions: the platform's three rungs, in
/// ASCENDING order, and the ONE authority for that order. Each transition
/// matches its required PREDECESSOR into the next rung and hands that rung's
/// failure through the caller's `dispatch` closure — which it calls ONLY in that
/// arm, so a transition from any other rung signals nothing and returns the
/// ladder unchanged — and it carries the previous rung's [`KillFailure`]
/// forward unless this rung failed too (the LAST failure is the reported one),
/// so the accumulation has no writer outside this state.
impl Termination {
    /// The GRACEFUL rung: `Armed` → `Termed`, anchored at `now` (the instant
    /// the timeout fired). `dispatch` is the group SIGTERM, called only when the
    /// ladder is still `Armed`, so a kill failure is only ever recorded by the
    /// transition that dispatched the signal it names.
    fn termed(self, now: Instant, dispatch: impl FnOnce() -> Option<KillFailure>) -> Self {
        match self {
            Termination::Armed { .. } => Termination::Termed(Escalating {
                escalation_start: now,
                failure: dispatch(),
            }),
            other => other,
        }
    }

    /// The escalated rung: `Termed` → `GroupKilled`, at the SAME start, so
    /// every rung of one timeout is measured from the instant it fired.
    /// `dispatch` is the group SIGKILL, called only in the `Termed` arm.
    fn group_killed(
        self,
        escalation_start: Instant,
        dispatch: impl FnOnce() -> Option<KillFailure>,
    ) -> Self {
        match self {
            Termination::Termed(previous) => Termination::GroupKilled(Escalating {
                escalation_start,
                failure: dispatch().or(previous.failure),
            }),
            other => other,
        }
    }

    /// The last-resort rung: `GroupKilled` → `ChildKilled`. `dispatch` is the
    /// kill on the OWNED handle, called only in the `GroupKilled` arm.
    fn child_killed(
        self,
        escalation_start: Instant,
        dispatch: impl FnOnce() -> Option<KillFailure>,
    ) -> Self {
        match self {
            Termination::GroupKilled(previous) => Termination::ChildKilled(Escalating {
                escalation_start,
                failure: dispatch().or(previous.failure),
            }),
            other => other,
        }
    }
}

/// THE shared bounded child-runner for local command execution: spawn the
/// child into its OWN process group with piped stdout/stderr, wait with the
/// caller's timeout, on timeout terminate the GROUP (TERM, grace, KILL) and
/// escalate to the owned handle, and return every outcome — success, timeout,
/// error — only after the child was REAPED exactly once. A timeout-kill or
/// reap failure is an ERROR, never a successful timeout outcome.
///
/// The runner is per-exec and owns nothing between calls: the child lives in
/// `OwnedChild` inside [`exec`] and is collected before the call returns, so
/// there are no leaked threads, handles, or processes across calls — the
/// lifecycle is bounded.
///
/// Execute `argv` (no shell) bounded by `timeout` — the Unix lifecycle:
/// spawn into an OWN process group, `waitid` WNOWAIT peek, `killpg`
/// termination, the foreground-only group check, and `poll`/`fcntl`
/// non-blocking pipe drains. See [`super::ChildRunner::exec`] for the
/// contract.
pub(crate) fn exec(
    env: &SysEnv,
    cwd: &Path,
    config: &RunnerConfig,
    argv: &[String],
    timeout: Duration,
) -> std::result::Result<RunOutcome, RunError> {
    let mut cmd = std::process::Command::new(&argv[0]);
    env.apply_to_command(&mut cmd);
    cmd.args(&argv[1..]);
    cmd.current_dir(cwd);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    // The child becomes its OWN process-group leader (pgid == pid):
    // timeout termination signals the WHOLE group, so grandchildren die
    // with it. Unix-only crate; `process_group` is the std Unix API.
    cmd.process_group(0);
    let child = cmd
        .spawn()
        .map_err(|e| RunError::Spawn(format!("spawn {argv:?}: {e}")))?;
    let mut owned = OwnedChild::new(child, config.kill.clone());
    let pid = owned.pid();
    let pgid = pid as i32;
    // The parent records the pid synchronously at spawn time — before the
    // timeout clock starts — so tests can assert the pid is gone after
    // the outcome without a child-written pidfile (which would race the
    // deadline kill).
    if let Some(observer) = &config.spawn_observer {
        observer(pid);
    }
    // Non-blocking pipe read ends: the wait loop drains without blocking
    // and the post-reap EOF drain is bounded — a grandchild that keeps a
    // pipe open can never hang the outcome.
    set_nonblocking(owned.stdout_mut()).map_err(|e| RunError::Wait(e.to_string()))?;
    set_nonblocking(owned.stderr_mut()).map_err(|e| RunError::Wait(e.to_string()))?;

    let mut termination = Termination::armed(Instant::now() + timeout);
    let mut stdout: Vec<u8> = Vec::new();
    let mut stderr: Vec<u8> = Vec::new();

    // Wait loop: detect the child's exit WITHOUT reaping it (`waitid`
    // WNOWAIT peek — the child becomes a ZOMBIE and stays waitable,
    // holding its pid/pgid allocated for the foreground-only check
    // that follows the loop). On timeout, walk the termination ladder:
    // TERM the group, escalate to KILL, then the owned handle — each rung
    // dispatched once, in that order, by the transition that names it (the
    // ladder's ONE order authority).
    loop {
        // Bytes drained THIS pass: a pass that moved data must not sleep
        // before the next one, or the drain would be capped at
        // `chunk / 1 ms` no matter how fast the link is. A silent child (no
        // progress) yields to the scheduler so the loop does not busy-spin.
        let mut drained = 0usize;
        drained += drain_available(owned.stdout_mut(), &mut stdout)
            .map_err(|e| RunError::Wait(e.to_string()))?;
        drained += drain_available(owned.stderr_mut(), &mut stderr)
            .map_err(|e| RunError::Wait(e.to_string()))?;
        if child_exited_unreaped(pid).map_err(|e| RunError::Wait(format!("wait {argv:?}: {e}")))? {
            break;
        }
        let now = Instant::now();
        // The timeout rung: `Armed` → `Termed`, recording the graceful
        // signal's failure. `deadline()` is the DUE test (the ladder is still
        // `Armed` and its deadline passed); the transition is the ORDER
        // authority and the ONLY writer of that failure, so a kill failure
        // cannot exist without the timeout.
        if let Some(deadline) = termination.deadline()
            && now >= deadline
        {
            // Graceful TERM of the WHOLE process group.
            termination =
                termination.termed(now, || match config.kill.kill_group(pgid, libc::SIGTERM) {
                    Ok(()) => None,
                    // ESRCH is benign only when the child itself is already
                    // gone (it exited as the deadline fired — the peek
                    // reports it next); a live child behind an unreachable
                    // group is a real termination failure. The liveness
                    // check must NOT reap: the child stays a zombie until
                    // the post-loop foreground check.
                    Err(e) => {
                        let alive = child_exited_unreaped(pid)
                            .map(|exited| !exited)
                            .unwrap_or(false);
                        alive.then(|| KillFailure::new(format!("TERM group {pgid}: {e}")))
                    }
                });
        }
        if let Some(escalation_start) = termination.escalation_start() {
            let since = now.duration_since(escalation_start);
            // The escalated rung: `Termed` → `GroupKilled`. A child that
            // ignores TERM must still die. The grace is the DUE test; the
            // transition refuses the rung from any other state, so the KILL
            // cannot precede the TERM or repeat.
            if since >= config.term_to_kill_grace {
                termination = termination.group_killed(escalation_start, || {
                    match config.kill.kill_group(pgid, libc::SIGKILL) {
                        Ok(()) => None,
                        Err(e) => {
                            let alive = child_exited_unreaped(pid)
                                .map(|exited| !exited)
                                .unwrap_or(false);
                            alive.then(|| KillFailure::new(format!("KILL group {pgid}: {e}")))
                        }
                    }
                });
            }
            // The last-resort rung: `GroupKilled` → `ChildKilled`, a direct
            // kill on the OWNED handle that catches a child which escaped its
            // group (e.g. setsid). The handle is reachable only while the
            // child is LIVE, so a reaped child (whose pid the OS may have
            // recycled) is never signalled.
            if since >= config.term_to_kill_grace * 2 {
                termination =
                    termination.child_killed(escalation_start, || match owned.live_child() {
                        Some(child) => config
                            .kill
                            .kill_owned(child)
                            .err()
                            .map(|e| KillFailure::new(format!("kill child {pid}: {e}"))),
                        None => None,
                    });
            }
            if since >= config.reap_bound {
                // The child is STILL alive after every termination
                // attempt: the kill did not take effect. This is a reap
                // failure — NEVER a successful timeout outcome.
                return Err(RunError::Reap(format!(
                    "child {pid} still alive {:?} after the timeout termination",
                    config.reap_bound
                )));
            }
        }
        if drained == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    // The child has EXITED but is still a ZOMBIE: the `waitid` WNOWAIT
    // peek above left it waitable, so its pid — and therefore its
    // process-group id (the child is the group leader, pgid == pid) — is
    // still allocated. Every foreground-only check and group termination
    // below happens while the zombie holds the pgid, so a `killpg` can
    // NEVER race a pid the OS recycled for an unrelated process (the
    // failure mode that made a probe-after-reap racy under parallel
    // execution).
    //
    // FOREGROUND-ONLY: enumerate the LIVE members of the child's process
    // group (our zombie excluded). If any remain, the command left a
    // background descendant: terminate the WHOLE group (TERM → grace →
    // KILL, the timeout path's escalation) and report the violation as
    // an ERROR, never a successful outcome; the essential contract — no
    // live process of the group after the return — is enforced BEFORE
    // the outcome escapes. A CLEAN command (no live members — the common
    // case) pays one enumeration and proceeds exactly as before.
    let live = live_group_members(pgid, pid);
    if !live.is_empty() {
        // Terminate the whole group; a kill failure is surfaced inside
        // the violation error (the leftover member must not survive even
        // when a kill fails — the fault-injected paths that cannot land
        // a kill are covered by the caller's own cleanup, and the drop
        // backstop remains the final resort for the owned child).
        let mut term_error: Option<String> = None;
        if let Err(e) = config.kill.kill_group(pgid, libc::SIGTERM) {
            term_error = Some(format!("TERM group {pgid}: {e}"));
        }
        std::thread::sleep(config.term_to_kill_grace);
        if let Err(e) = config.kill.kill_group(pgid, libc::SIGKILL)
            && term_error.is_none()
        {
            term_error = Some(format!("KILL group {pgid}: {e}"));
        }
        // Confirm the group is gone (bounded): a killed descendant is
        // reparented to init and reaped there; the poll covers the
        // transient zombie window. On expiry (an injected inert kill) the
        // error still names the violation — the fault IS the kill not
        // working.
        let verify_deadline = Instant::now() + config.reap_bound;
        while !live_group_members(pgid, pid).is_empty() && Instant::now() < verify_deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        // Reap the direct child (a zombie — the wait returns immediately,
        // releasing the pid) BEFORE the error escapes.
        owned
            .wait()
            .map_err(|e| RunError::Wait(format!("wait {argv:?}: {e}")))?;
        if let Some(observer) = &config.reap_observer {
            observer(pid);
        }
        let detail = term_error.map(|e| format!(" ({e})")).unwrap_or_default();
        let leftover = live
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(",");
        return Err(RunError::Background(format!(
            "command {argv:?} left background processes in its process group \
                 (live members: {leftover}); commands are foreground-only{detail}"
        )));
    }

    // The direct child is the only group member: reap it — the SINGLE
    // reap, releasing the pid. From here on nothing signals anything.
    let status = owned
        .wait()
        .map_err(|e| RunError::Wait(format!("wait {argv:?}: {e}")))?;
    if let Some(observer) = &config.reap_observer {
        observer(pid);
    }
    // Bounded drain to EOF: the child is dead and its pipes hold the
    // remaining output. A grandchild that keeps a pipe open cannot hang
    // the outcome — the drain gives up after the reap bound.
    // PIPE-EOF CONTAINMENT: the direct child is reaped (its descriptors
    // closed by the kernel); the inherited stdout/stderr write ends EOF
    // exactly when the LAST holder — the child or any descendant that
    // kept the pipes — dies. EOF within the bound proves no pipe-holding
    // descendant lives (the clean path stays clean); a pipe still open
    // at the bound proves a live descendant HOLDS it — a descendant that
    // escaped the group via `setsid` but kept the inherited pipes is
    // still DETECTED here, and the violation is reported as an ERROR,
    // never a successful outcome. Only a FULLY daemonized descendant
    // (`setsid` AND closed descriptors) is outside the contract (see the
    // module doc) — commands must not daemonize.
    let drain_bound = config.reap_bound;
    let stdout_drain = drain_to_eof(owned.stdout_mut(), &mut stdout, drain_bound)
        .map_err(|e| RunError::Wait(e.to_string()))?;
    if matches!(stdout_drain, DrainState::BoundExpired) {
        return Err(RunError::Background(format!(
            "command {argv:?} left processes holding its output pipes open; \
                 commands are foreground-only"
        )));
    }
    let stderr_drain = drain_to_eof(owned.stderr_mut(), &mut stderr, drain_bound)
        .map_err(|e| RunError::Wait(e.to_string()))?;
    if matches!(stderr_drain, DrainState::BoundExpired) {
        return Err(RunError::Background(format!(
            "command {argv:?} left processes holding its error pipes open; \
                 commands are foreground-only"
        )));
    }

    if termination.timed_out() {
        // A timeout outcome is legitimate ONLY when the termination was
        // effective: a kill failure is an ERROR, never a fake timeout.
        if let Some(failure) = termination.failure() {
            return Err(RunError::Kill(format!(
                "timeout termination failed: {}",
                failure.message()
            )));
        }
        return Ok(RunOutcome::TimedOut {
            stderr: format!("timed out after {timeout:?}"),
        });
    }
    Ok(RunOutcome::Exited {
        exit_code: status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

/// Put a child pipe read end into non-blocking mode, so reads never block and
/// the bounded post-exit drain can give up on schedule. `pub(crate)` because
/// the SSH runner's Unix seam reuses this and the bounded drain below, so the
/// pipe-containment discipline has ONE implementation for both transports.
pub(crate) fn set_nonblocking<R: AsRawFd>(stream: &mut Option<R>) -> std::io::Result<()> {
    let Some(stream) = stream.as_mut() else {
        return Ok(());
    };
    let fd = stream.as_raw_fd();
    // SAFETY: fcntl on a pipe read end this runner opened for its own child.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: as above; O_NONBLOCK only changes the read blocking semantics.
    let rc = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// The per-CALL byte cap on [`drain_available`]: large enough that a normal
/// transfer drains a pipe in a few calls (and, since a caller that sees
/// progress does not sleep, at syscall speed), small enough that the wait
/// loop regains control — and consults its deadline — promptly even against a
/// writer that keeps the pipe continuously readable.
pub(crate) const DRAIN_AVAILABLE_CALL_CAP: usize = 1 << 20;

/// Drain EVERYTHING a running child currently has buffered in a pipe WITHOUT
/// blocking, returning the number of bytes appended to `buf` by THIS call.
///
/// `poll(2)` with a zero timeout reports readability, then a `read` appends a
/// chunk; the poll/read pair repeats until the pipe reports no more readable
/// data (or `read` returns `WouldBlock`/EOF). The wait loop therefore never
/// parks on a pipe while the child is still running, and the OLD single-read
/// shape — one 8192-byte chunk per wait-loop pass, with the caller sleeping
/// 1 ms between passes — is gone: that shape capped EVERY remote read at
/// ~8 MiB/s regardless of the link, because the pipe was drained one chunk
/// per millisecond. A caller that receives a nonzero return has made
/// progress and must NOT sleep before the next pass.
///
/// `pub(crate)`: shared with the SSH runner's Unix seam (see
/// [`set_nonblocking`]), so the drain policy has ONE implementation.
pub(crate) fn drain_available<R>(
    stream: &mut Option<R>,
    buf: &mut Vec<u8>,
) -> std::io::Result<usize>
where
    R: Read + AsRawFd,
{
    let Some(stream) = stream.as_mut() else {
        return Ok(0);
    };
    let mut total = 0usize;
    // A larger chunk cuts syscalls without changing the drain's semantics: a
    // short read is appended and the loop re-polls, so a full pipe is emptied
    // in as few reads as its contents allow.
    let mut chunk = [0u8; 65536];
    loop {
        // Return after a bounded amount so the WAIT LOOP regains control and
        // can consult its deadline: a writer that keeps the pipe continuously
        // readable must not keep this call in the loop forever (the same
        // unbounded-loop hazard [`drain_to_eof`] bounds). The cap is per
        // CALL, never cumulative, and is large enough that a normal transfer
        // pays it rarely: a caller that sees progress does not sleep, so the
        // loop still runs at syscall speed.
        if total >= DRAIN_AVAILABLE_CALL_CAP {
            return Ok(total);
        }
        let mut pfd = libc::pollfd {
            fd: stream.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `poll` with a zero timeout on a real pipe read end this
        // runner opened for its own child; the fd is always valid here and
        // never blocks.
        if unsafe { libc::poll(&mut pfd, 1, 0) } <= 0 {
            return Ok(total);
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Ok(total),
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                total += n;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(total),
            Err(e) => return Err(e),
        }
    }
}

/// The outcome of a bounded post-exit drain: EOF proves the pipe's last
/// writer closed (no live descendant holds it); a bound expiry proves a
/// live writer STILL holds it (a descendant that escaped the group but kept
/// the inherited stdio pipes — the pipe-EOF containment signal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrainState {
    /// `read` returned 0: every write end closed — no pipe-holding
    /// descendant remains.
    Eof,
    /// The drain gave up with the pipe still open: the deadline passed (in
    /// ANY arm — a continuously-readable writer included), OR the
    /// [`MAX_DRAIN_BYTES`] cap was reached. Either way a live writer is
    /// holding the pipe — a contract violation the caller reports, never a
    /// silent clean outcome.
    BoundExpired,
}

/// The cap on how many bytes ONE bounded post-exit drain may append. The
/// child is already reaped, so the post-exit tail is whatever its pipes still
/// hold — normally at most the pipe capacity plus the last race window. A
/// writer that keeps producing past this cap is a runaway descendant holding
/// the pipe open, so the drain gives up and the caller reports the
/// violation; the buffered growth is bounded WITHOUT silently truncating a
/// successfully drained result (the caller sees [`DrainState::BoundExpired`],
/// never a quietly shortened buffer).
pub(crate) const MAX_DRAIN_BYTES: usize = 8 * 1024 * 1024;

/// Drain a child pipe to EOF, bounded in BOTH time and size: reads never
/// block (non-blocking read ends), the absolute `bound` deadline is consulted
/// on EVERY loop iteration (a writer that keeps the pipe continuously
/// readable can no longer spin in the `Ok(n)` arm), and at most
/// [`MAX_DRAIN_BYTES`] may be appended. Returns [`DrainState::Eof`] when the
/// pipe reached EOF within both bounds (no live holder remains) and
/// [`DrainState::BoundExpired`] when either bound was reached with the pipe
/// still open (a live holder — a contract violation the caller reports, never
/// a silent clean outcome). `pub(crate)`: shared with the SSH runner's Unix
/// seam, so the bound that makes the deadline truly bound the operation has
/// ONE implementation ([`super::KILL_REAP_BOUND`] is the production value
/// both pass).
pub(crate) fn drain_to_eof<R>(
    stream: &mut Option<R>,
    buf: &mut Vec<u8>,
    bound: Duration,
) -> std::io::Result<DrainState>
where
    R: Read + AsRawFd,
{
    let Some(stream) = stream.as_mut() else {
        return Ok(DrainState::Eof);
    };
    let deadline = Instant::now() + bound;
    // The bytes THIS drain appended, so the cap bounds the drain's growth
    // and never discards output the running phase already buffered.
    let start_len = buf.len();
    let mut chunk = [0u8; 8192];
    loop {
        // Consult the ABSOLUTE deadline on EVERY arm, before the read: the
        // former shape checked it only in the `WouldBlock` arm, so a writer
        // that kept the pipe continuously readable kept the loop in `Ok(n)`
        // and grew `buf` without bound.
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(DrainState::BoundExpired);
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Ok(DrainState::Eof),
            Ok(n) => {
                let appended = buf.len() - start_len;
                let room = MAX_DRAIN_BYTES.saturating_sub(appended);
                if room == 0 {
                    // The cap was reached: a live writer is still producing.
                    // Give up (the caller reports the violation) rather than
                    // grow memory or silently truncate.
                    return Ok(DrainState::BoundExpired);
                }
                buf.extend_from_slice(&chunk[..n.min(room)]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                let ms = remaining.as_millis().min(i32::MAX as u128) as i32;
                let mut pfd = libc::pollfd {
                    fd: stream.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                // SAFETY: `poll` on a real pipe read end this runner owns.
                let rc = unsafe { libc::poll(&mut pfd, 1, ms) };
                if rc < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if rc == 0 {
                    return Ok(DrainState::BoundExpired);
                }
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::macos_status_is_not_live;

    /// The macOS liveness decision, pinned so the ONE shared rule cannot
    /// silently change: only `SZOMB` (= 5) is not live; a running (`SRUN` =
    /// 2), sleeping (`SSLEEP` = 3), idle (`SIDL` = 0) or stopped (`SSTOP` =
    /// 4) process is live; and an unknown status conservatively counts as
    /// LIVE, so a leftover can never hide behind a status this crate does
    /// not recognise. This is the macOS side of the guarantee that
    /// production's enumeration and the oracle agree.
    #[test]
    fn macos_status_classifies_bsd_process_states() {
        assert!(
            macos_status_is_not_live(5),
            "SZOMB (= 5) is the ONE not-live status"
        );
        assert!(!macos_status_is_not_live(2), "SRUN (running) is live");
        assert!(!macos_status_is_not_live(3), "SSLEEP (sleeping) is live");
        assert!(!macos_status_is_not_live(0), "SIDL (idle) is live");
        assert!(!macos_status_is_not_live(4), "SSTOP (stopped) is live");
        assert!(
            !macos_status_is_not_live(99),
            "an unknown status defaults to LIVE, so a leftover cannot escape"
        );
    }
}

/// The bounded drain must consult its deadline on EVERY arm and bound the
/// buffered growth.
#[cfg(all(test, unix))]
mod drain_tests {
    use super::{
        DRAIN_AVAILABLE_CALL_CAP, DrainState, MAX_DRAIN_BYTES, drain_available, drain_to_eof,
    };
    use std::io::Read;
    use std::os::fd::{AsRawFd, RawFd};
    use std::time::{Duration, Instant};

    /// A `Read` that yields `remaining` bytes in `chunk`-sized pieces and then
    /// reports `WouldBlock`. Its fd is `/dev/null`'s, which `poll` reports as
    /// always readable, so the drain's `poll` gate always passes and only the
    /// `Read` result ends the loop — exactly the shape needed to observe how
    /// much ONE `drain_available` call drains.
    struct BurstRead {
        fd: RawFd,
        remaining: usize,
        chunk: usize,
    }

    impl Read for BurstRead {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.remaining == 0 {
                return Err(std::io::Error::from(std::io::ErrorKind::WouldBlock));
            }
            let n = buf.len().min(self.remaining).min(self.chunk);
            buf[..n].fill(0x5A);
            self.remaining -= n;
            Ok(n)
        }
    }

    impl AsRawFd for BurstRead {
        fn as_raw_fd(&self) -> RawFd {
            self.fd
        }
    }

    /// ONE `drain_available` call must drain the WHOLE readable backlog,
    /// not a single 8192-byte chunk. Pre-fix the wait loop appended one chunk
    /// per call and slept 1 ms between passes, capping every remote read at
    /// ~8 MiB/s regardless of the link; the fix loops until the pipe reports
    /// no more data. (Verified before the fix: this assertion sees 8192, not
    /// 65536.)
    #[test]
    fn drain_available_drains_the_whole_backlog_in_one_call() {
        let dev_null = std::fs::File::open("/dev/null").unwrap();
        let mut stream = Some(BurstRead {
            fd: dev_null.as_raw_fd(),
            remaining: 64 * 1024,
            // The pre-fix per-read chunk, so a single-read implementation
            // would stop at exactly one of these.
            chunk: 8192,
        });
        let mut buf = Vec::new();
        let drained = drain_available(&mut stream, &mut buf).unwrap();
        assert_eq!(
            drained,
            64 * 1024,
            "one call must drain the whole backlog, not one 8192-byte chunk"
        );
        assert_eq!(buf.len(), 64 * 1024);
    }

    /// The per-call cap keeps the running-phase drain bounded: a writer that
    /// keeps the pipe continuously readable must not hold `drain_available`
    /// forever (the wait loop needs the return to consult its deadline). The
    /// call returns at the cap instead of looping without end.
    #[test]
    fn drain_available_is_bounded_per_call() {
        let dev_null = std::fs::File::open("/dev/null").unwrap();
        let mut stream = Some(BurstRead {
            fd: dev_null.as_raw_fd(),
            remaining: usize::MAX,
            chunk: 65536,
        });
        let mut buf = Vec::new();
        let start = Instant::now();
        let drained = drain_available(&mut stream, &mut buf).unwrap();
        assert_eq!(drained, DRAIN_AVAILABLE_CALL_CAP);
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the per-call cap must return promptly: {:?}",
            start.elapsed()
        );
    }

    /// A `Read` that is ALWAYS readable: every `read` returns data, so the
    /// drain loop can never reach its `WouldBlock` arm. Before the fix the
    /// deadline was consulted ONLY in that arm, so this stream drove the loop
    /// in `Ok(n)` forever and grew the buffer without bound (the test would
    /// hang). A descriptor is still required by `AsRawFd`; `/dev/null`'s is
    /// never read (the override never delegates).
    struct EndlessRead {
        fd: RawFd,
    }

    impl Read for EndlessRead {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = buf.len();
            buf[..n].fill(0x5A);
            Ok(n)
        }
    }

    impl AsRawFd for EndlessRead {
        fn as_raw_fd(&self) -> RawFd {
            self.fd
        }
    }

    /// The pre-fix shape: a writer that keeps the pipe continuously readable
    /// spun in the `Ok(n)` arm, so the absolute deadline was never consulted
    /// and `buf` grew without bound. With the deadline checked in EVERY arm
    /// and the [`MAX_DRAIN_BYTES`] cap, the drain returns promptly with
    /// [`DrainState::BoundExpired`] and a bounded buffer instead of hanging.
    /// (Verified before the fix: this test hangs — the drain never returns.)
    #[test]
    fn drain_to_eof_bounds_a_continuously_readable_writer() {
        let dev_null = std::fs::File::open("/dev/null").unwrap();
        let mut stream = Some(EndlessRead {
            fd: dev_null.as_raw_fd(),
        });
        let mut buf = Vec::new();
        let start = Instant::now();
        let state = drain_to_eof(&mut stream, &mut buf, Duration::from_secs(30)).unwrap();
        let elapsed = start.elapsed();
        assert_eq!(
            state,
            DrainState::BoundExpired,
            "a continuously-readable writer must hit the size bound"
        );
        assert_eq!(
            buf.len(),
            MAX_DRAIN_BYTES,
            "the buffered growth must stop at the cap, not grow without bound"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "the size bound must return promptly, not wait out the 30s deadline: {elapsed:?}"
        );
    }

    /// The deadline is consulted even when the pipe is continuously readable:
    /// a stream that yields a LITTLE data per read (so the size cap is not the
    /// thing that stops it) must still return [`DrainState::BoundExpired`] near
    /// the deadline. Before the fix this also hung, because `Ok(n)` never
    /// re-checked the clock.
    #[test]
    fn drain_to_eof_consults_the_deadline_in_the_data_arm() {
        struct TrickleRead {
            fd: RawFd,
        }
        impl Read for TrickleRead {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let n = buf.len().min(1);
                buf[..n].fill(0x5A);
                Ok(n)
            }
        }
        impl AsRawFd for TrickleRead {
            fn as_raw_fd(&self) -> RawFd {
                self.fd
            }
        }
        let dev_null = std::fs::File::open("/dev/null").unwrap();
        let mut stream = Some(TrickleRead {
            fd: dev_null.as_raw_fd(),
        });
        let mut buf = Vec::new();
        let start = Instant::now();
        let state = drain_to_eof(&mut stream, &mut buf, Duration::from_millis(50)).unwrap();
        let elapsed = start.elapsed();
        assert_eq!(state, DrainState::BoundExpired);
        assert!(
            elapsed >= Duration::from_millis(50) && elapsed < Duration::from_secs(5),
            "the deadline must stop a continuously-readable trickle near the bound: {elapsed:?}"
        );
        assert!(
            buf.len() < MAX_DRAIN_BYTES,
            "the size cap was not the stopper"
        );
    }
}

/// The Unix termination ladder as a VALUE: every rung past `Armed` carries the
/// instant the timeout fired, and the kill-failure accumulator keeps the LAST
/// FAILED signal — a later successful rung does not erase an earlier failure,
/// and a later failure replaces it (exactly the rule the ladder replaced).
#[cfg(all(test, unix))]
mod termination_tests {
    use super::{KillFailure, Termination};
    use std::time::{Duration, Instant};

    /// A regression that re-anchored an escalation rung (so the grace and reap
    /// bounds were measured from the wrong instant) or that dropped the earlier
    /// failure on a successful later signal fails here: neither property is
    /// reachable from an end-to-end test without an injected kill fault, and
    /// the ladder's own transitions are where both are decided.
    #[test]
    fn the_ladder_carries_its_start_and_keeps_the_last_failed_signal() {
        let deadline = Instant::now() + Duration::from_millis(5);
        let fired = Instant::now();

        let armed = Termination::armed(deadline);
        assert_eq!(armed.deadline(), Some(deadline), "the armed deadline");
        assert!(!armed.timed_out(), "an armed ladder has not timed out");
        assert!(
            armed.escalation_start().is_none(),
            "no bound runs before the timeout fires"
        );
        assert!(armed.failure().is_none(), "nothing was signalled yet");

        // The graceful signal failed.
        let termed = armed.termed(fired, || {
            Some(KillFailure::new("TERM group 7: denied".into()))
        });
        assert!(termed.timed_out());
        assert!(
            termed.deadline().is_none(),
            "the deadline is consumed by the timeout that fired"
        );
        assert_eq!(termed.escalation_start(), Some(fired));
        assert_eq!(
            termed.failure().map(KillFailure::message),
            Some("TERM group 7: denied")
        );

        // The escalated signal landed: the earlier failure SURVIVES it.
        let group_killed = termed.group_killed(fired, || None);
        assert_eq!(
            group_killed.escalation_start(),
            Some(fired),
            "every rung of one timeout measures from the instant it fired"
        );
        assert_eq!(
            group_killed.failure().map(KillFailure::message),
            Some("TERM group 7: denied"),
            "a successful later signal must not erase an earlier failure"
        );

        // A later failure REPLACES it: the LAST failure is the reported one.
        let child_killed = group_killed.child_killed(fired, || {
            Some(KillFailure::new("kill child 7: denied".into()))
        });
        assert_eq!(child_killed.escalation_start(), Some(fired));
        assert_eq!(
            child_killed.failure().map(KillFailure::message),
            Some("kill child 7: denied")
        );
    }

    /// WITNESS that each transition is the ORDER AUTHORITY for its own rung:
    /// attempted from any rung but its predecessor it dispatches NOTHING (the
    /// closure is never called) and returns the ladder UNCHANGED, so KILL
    /// cannot precede TERM, a rung cannot repeat, and no failure is recorded
    /// for a signal that was not sent. The chain asserted here is the one the
    /// deleted `matches!` guards at the `exec` call sites used to enforce, and
    /// `exec` now relies on the transitions alone for it.
    #[test]
    fn a_transition_from_the_wrong_rung_dispatches_nothing_and_returns_the_ladder() {
        let deadline = Instant::now() + Duration::from_millis(5);
        let fired = Instant::now();
        let mut dispatched = 0usize;

        // `group_killed` and `child_killed` from `Armed` are no-ops.
        let armed = Termination::armed(deadline);
        let still_armed = armed.group_killed(fired, || {
            dispatched += 1;
            None
        });
        let still_armed = still_armed.child_killed(fired, || {
            dispatched += 1;
            None
        });
        assert_eq!(
            still_armed.deadline(),
            Some(deadline),
            "a rung attempted before its predecessor must leave the ladder armed"
        );
        assert!(!still_armed.timed_out());
        assert_eq!(dispatched, 0, "a refused rung must dispatch no signal");

        // `termed` twice: the second attempt is a no-op, not a re-dispatch.
        let termed = still_armed.termed(fired, || None);
        let termed_again = termed.termed(fired + Duration::from_millis(1), || {
            dispatched += 1;
            None
        });
        assert_eq!(
            termed_again.escalation_start(),
            Some(fired),
            "a repeated rung must not re-anchor the ladder"
        );
        assert_eq!(dispatched, 0, "a repeated rung must dispatch no signal");

        // `child_killed` from `Termed` is a no-op; only `group_killed` advances.
        let termed_still = termed_again.child_killed(fired, || {
            dispatched += 1;
            None
        });
        assert_eq!(
            termed_still.failure().map(KillFailure::message),
            None,
            "a refused rung must record no failure"
        );
        assert_eq!(dispatched, 0, "a skipped rung must dispatch no signal");

        // The ascending order still advances one rung at a time.
        let group_killed = termed_still.group_killed(fired, || {
            dispatched += 1;
            None
        });
        let child_killed = group_killed.child_killed(fired, || {
            dispatched += 1;
            None
        });
        assert!(child_killed.timed_out());
        assert_eq!(dispatched, 2, "each due rung dispatches exactly once");
    }
}
