//! The Windows implementation of the bounded child-runner: no process
//! groups (the child is spawned plain and terminated via the OWNED handle),
//! no foreground-only group check (no process-group enumeration exists on
//! Windows), and reader THREADS for the pipe drains (no `poll`/`fcntl`
//! non-blocking pipes). The documented weaker guarantees of the Windows
//! port: a background descendant survives a timeout (only the direct child
//! is terminated), and a command that leaves background processes is
//! detected only when it holds the output pipes (the bounded drain reports
//! the violation; a fully daemonized descendant is outside the contract, as
//! on Unix). Selected by the single `#[cfg(windows)]` `mod` declaration in
//! [`super`].

use super::*;
use std::io::Read;
use std::path::Path;
use std::sync::mpsc;
use std::time::Instant;

/// Production seam: Windows has no process groups — the group signal is a
/// no-op (the owned-child signal is the termination path).
pub struct RealKill;

impl KillSeam for RealKill {
    fn kill_group(&self, _group: &ProcessGroup, _sig: GroupSignal) -> std::io::Result<()> {
        // No process groups on Windows: the group signal is a no-op (the
        // owned-child signal is the termination path).
        Ok(())
    }
    fn kill_owned(&self, group: &mut ProcessGroup) -> std::io::Result<()> {
        match group.live_child() {
            Some(child) => child.kill(),
            None => Ok(()),
        }
    }
}

/// Spawn rule for Windows: NO process group. The owned handle
/// (`TerminateProcess`) is the whole termination path, so `live_members`
/// reports nothing and [`ProcessGroup::settle`] settles at once — the
/// documented weaker guarantee of this port.
pub(crate) fn own_process_group(_cmd: &mut std::process::Command) {}

/// Windows has no process-group enumeration: no member is ever reported, so the
/// foreground-only check never fires and the owned handle remains the whole
/// termination path.
pub(crate) fn live_group_members(_pgid: i32, _exclude_pid: u32) -> Vec<i32> {
    Vec::new()
}

/// The Windows termination ladder's ONE transition. Windows has no process
/// groups, so the ladder has no graceful and no group rung: `Armed` →
/// `ChildKilled` in a single transition, the owned handle being the whole
/// termination path. It matches its required predecessor (`Armed`) and returns
/// the ladder UNCHANGED from any other rung, dispatching nothing, so it is the
/// rung's one order authority — a rule its own `match` enforces at runtime, not
/// a typing fact. `at` is the DEADLINE the rung crossed, not the instant the
/// kill landed, because this port measures its reap bound from the deadline
/// (`now - deadline`).
impl Termination {
    fn child_killed(self, at: Instant, dispatch: impl FnOnce() -> Option<KillFailure>) -> Self {
        match self {
            Termination::Armed { .. } => Termination::ChildKilled(Escalating {
                escalation_start: at,
                failure: dispatch(),
            }),
            other => other,
        }
    }
}

/// Execute `argv` (no shell) bounded by `timeout` — the Windows lifecycle:
/// spawn plain, drain stdout/stderr via reader THREADS, poll `try_wait`
/// with the timeout, terminate the OWNED child on timeout, reap, then
/// collect the readers (bounded). No foreground-only group check (no
/// process-group enumeration on Windows) — the documented weaker
/// guarantee. See [`super::ChildRunner::exec`] for the contract.
pub(crate) fn exec(
    env: &SysEnv,
    cwd: &Path,
    config: &RunnerConfig,
    argv: &[String],
    timeout: Duration,
) -> std::result::Result<RunOutcome, RunError> {
    let mut group = ProcessGroup::spawn(argv, cwd, env, config.kill.clone())
        .map_err(|e| RunError::Spawn(format!("spawn {argv:?}: {e}")))?;
    let pid = group.pid();
    // The parent records the pid synchronously at spawn time — before the
    // timeout clock starts — so tests can assert the pid is gone after the
    // outcome without a child-written pidfile (which would race the
    // deadline kill).
    if let Some(observer) = &config.spawn_observer {
        observer(pid);
    }
    // Reader threads: drain stdout/stderr to EOF (the pipes EOF when the
    // child — and any descendant that kept them — dies). Each thread sends
    // its buffer on a channel; the main loop collects them bounded.
    let stdout_rx = spawn_reader(group.stdout_mut().take());
    let stderr_rx = spawn_reader(group.stderr_mut().take());

    let mut termination = Termination::armed(Instant::now() + timeout);
    // Wait loop: poll `try_wait` (no `waitid` WNOWAIT peek on Windows — the
    // child is reaped by the final `wait`). On timeout, terminate the OWNED
    // child (TerminateProcess); the ladder records the signal and its failure.
    loop {
        // `try_wait` records the status it consumes: from the reap on the
        // lifecycle is `Reaped`, so nothing can reach the handle to signal it.
        match group.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(e) => return Err(RunError::Wait(format!("wait {argv:?}: {e}"))),
        }
        let now = Instant::now();
        // The single rung: `Armed` → `ChildKilled`, the transition that has the
        // order authority and the ONLY writer of the kill failure, so a
        // failure cannot exist without the timeout. `deadline()` is the DUE
        // test; the transition refuses the rung from any other state, so it
        // cannot repeat.
        if let Some(deadline) = termination.deadline()
            && now >= deadline
        {
            // Terminate the OWNED child. No process group: a background
            // descendant survives. The handle is reachable only while the
            // leader is LIVE (a reaped child's pid may already be recycled).
            termination = termination.child_killed(deadline, || {
                group
                    .signal_leader()
                    .err()
                    .map(|e| KillFailure::new(format!("kill child {pid}: {e}")))
            });
        }
        if let Some(escalation_start) = termination.escalation_start()
            && now.duration_since(escalation_start) >= config.reap_bound
        {
            // The child is STILL alive after the termination attempt: the
            // kill did not take effect. This is a reap failure — NEVER a
            // successful timeout outcome.
            return Err(RunError::Reap(format!(
                "child {pid} still alive {:?} after the timeout termination",
                config.reap_bound
            )));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    // The leader is the only collectable child on Windows (no process groups):
    // collect it — the SINGLE reap, CONSUMING the capability, so nothing can
    // signal the released pid. On Unix the foreground-only group check runs
    // between the peek and this collect.
    let mut reaped = collect(group, config.reap_observer.as_ref(), pid, argv)?;
    // Collect the reader buffers, BOUNDED: the pipes EOF when the last
    // holder dies. A channel still open at the bound proves a live
    // descendant HOLDS the pipe — the pipe-EOF containment signal (the
    // same violation the Unix drain reports). The blocked reader thread is
    // left to EOF when the descendant dies.
    let drain_bound = config.reap_bound;
    let stdout = recv_bounded(&stdout_rx, drain_bound).ok_or_else(|| {
        RunError::Background(format!(
            "command {argv:?} left processes holding its output pipes open; \
             commands are foreground-only"
        ))
    })?;
    let stderr = recv_bounded(&stderr_rx, drain_bound).ok_or_else(|| {
        RunError::Background(format!(
            "command {argv:?} left processes holding its error pipes open; \
             commands are foreground-only"
        ))
    })?;

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
        exit_code: reaped.status().code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

/// Spawn a thread that reads `stream` to EOF into a buffer and sends it on
/// a channel. The channel is the bounded-collection handle: the main loop
/// receives with a deadline, so a pipe held open by a live descendant is
/// DETECTED (the receive times out) instead of hanging the outcome.
fn spawn_reader<R: Read + Send + 'static>(stream: Option<R>) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut s) = stream {
            // Best-effort: a read error mid-drain (e.g. the child dying)
            // yields the partial buffer; the outcome's captured output is
            // best-effort on Windows.
            let _ = s.read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });
    rx
}

/// Receive the reader's buffer within `bound`, or `None` when the bound
/// expires (a live descendant holds the pipe open).
fn recv_bounded(rx: &mpsc::Receiver<Vec<u8>>, bound: Duration) -> Option<Vec<u8>> {
    rx.recv_timeout(bound).ok()
}
