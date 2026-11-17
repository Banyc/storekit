//! THE shared bounded child-runner for local command execution: the ONE owner
//! of every local command child, from spawn to the mandatory reap.
//!
//! # The lifecycle contract
//!
//! * **One authority per spawned child** — the child is held as a
//!   [`ProcessGroup`] capability, minted only by spawning and never from a
//!   pid: every signal, every enumeration and the single reap go through that
//!   ONE value, and [`ProcessGroup::reap`] CONSUMES it. A pgid is a pid, and
//!   the kernel recycles a pid the instant its child is reaped, so a
//!   remembered number is not an authority: a `killpg` on it can reach
//!   whatever process later inherited the id.
//! * **Synchronized child ownership** — the wait runs inline on the calling
//!   thread (no wait is delegated to a spawned thread), and a capability
//!   dropped before it was reaped goes through the drop backstop, which kills
//!   and waits (bounded) as a final resort — and signals NOTHING once the
//!   leader was collected.
//! * **Process-group termination (Unix)** — the child is spawned into its
//!   OWN process group (`process_group(0)` — the child becomes the group
//!   leader, pgid == pid), so a timeout terminates the WHOLE group (`killpg`
//!   SIGTERM, then — after a short grace — SIGKILL) and GRANDCHILDREN die
//!   with it. Windows has no process groups: the timeout terminates the
//!   direct child only (a background descendant survives).
//! * **Mandatory reap before an outcome** — a returned outcome (success or
//!   timeout) happens only after the child was REAPED: the runner waits
//!   synchronously on its owned handle and "proven dead" means the wait
//!   returned. The CONSUMING call is per platform: Windows polls `try_wait`,
//!   which consumes the exit status; Unix PEEKS with `waitid(2)`/`WNOWAIT`
//!   (`ProcessGroup::leader_exited` — no status consumed) and consumes it once
//!   through [`ProcessGroup::wait`] → [`Child::wait`]. Both reap exactly once;
//!   the drop backstop reaps through `try_wait`. An error path that cannot
//!   reap drops the handle, and the drop backstop's bounded kill-and-wait is
//!   the final resort.
//! * **Foreground-only (Unix)** — commands must not daemonize. After the
//!   direct child exits, the runner checks its process group for LIVE
//!   leftover members (a background descendant the command left behind). The
//!   check is race-free by construction: the child is held as an UNREAPED
//!   ZOMBIE for its duration (`waitid(2)` with `WNOWAIT` — a zombie holds
//!   its pid, and therefore its process-group id (the child is the group
//!   leader, pgid == pid), allocated until reaped, so a `killpg` in that
//!   window can never hit a pid the OS recycled for an unrelated process),
//!   the group is ENUMERATED (Linux: a `/proc/*/stat` scan; macOS:
//!   `proc_listpgrppids`) with our own zombie excluded, and any LIVE leftover
//!   member triggers the termination (TERM, grace, KILL — the timeout path's
//!   escalation) plus an ERROR — a command that leaves background processes
//!   is a contract violation, NEVER a successful outcome. A descendant that
//!   ESCAPED the group via `setsid` is OUTSIDE the guarantee: the runner can
//!   DETECT a pipe-holding escapee — the inherited stdio pipes EOF exactly
//!   when the last holder dies, so a pipe still open at the drain bound is a
//!   provable violation → error — but it CANNOT TERMINATE an escaped
//!   process. The ONE documented exclusion covers BOTH setsid flavors: the
//!   pipe-holding escapee (detected → error, but not terminated) and the
//!   FULLY daemonized descendant (`setsid` AND closed descriptors — not even
//!   detectable); commands must not daemonize. A CLEAN command (no live
//!   members — the common case) pays one enumeration. On Windows the
//!   foreground-only check is NOT performed (no process-group enumeration
//!   exists).
//! * **A timed-out group is SETTLED, never sampled once** — the runner's own
//!   TERM→grace→KILL ladder already swept that group, so its emptiness is
//!   proved by ITERATION ([`ProcessGroup::settle`]) and not by one
//!   enumeration: `fork(2)` records the new child in its parent's process group
//!   (`copy_process` in `kernel/fork.c`), so a descendant can join AFTER a
//!   sweep has passed — it inherits the group but not the signal — and a
//!   signalled process keeps a non-live-but-not-yet-`do_exit` state, so a
//!   sample can still list the sweep's own victims mid-death. A group still
//!   live at the bound is a termination that did not take effect: [`RunError::Kill`]
//!   when a kill was refused, [`RunError::Reap`] otherwise — never a clean
//!   timeout. The leader is reaped (once) before that error escapes.
//! * **A timeout-kill failure is an ERROR** — if the group kill fails (a real
//!   failure, not the benign ESRCH of a group that is already gone), or the
//!   escalated kill fails, or the reap cannot be confirmed within the bound,
//!   the runner returns `Err` — NEVER a successful `exit_code: -1,
//!   "timed out"` outcome. Only a confirmed terminated-and-reaped group yields
//!   the timeout outcome.
//! * **Bounded** — the lifecycle is bounded: per-exec, every kill/reap wait
//!   is bounded by a configured policy value, the wait itself runs on the
//!   calling thread, and the child is never leaked. The ONE named exception
//!   is the Windows pipe-holding case: when a descendant keeps the inherited
//!   output pipes open past the drain bound, the runner reports the violation
//!   and returns while a pipe reader thread is still blocked in
//!   `read_to_end` — deliberately left to EOF, which is when the descendant
//!   dies. Unix has no reader threads: the drain is non-blocking
//!   `poll`/`fcntl`, so nothing is left behind there.
//!
//! The kill path is a [`KillSeam`] (a kill-function seam): production uses
//! [`RealKill`] (Unix: `killpg(2)` — no shell, no external `kill` binary;
//! Windows: the owned `Child::kill`), and the tests inject their own seam (an
//! inert kill) without any subprocess fakery. The seam takes the CAPABILITY,
//! so its implementor can only signal a group it was handed; the SSH runner's
//! real seam shares the same [`ProcessGroup`] and the same
//! [`TERM_TO_KILL_GRACE`], so on Unix both transports terminate process
//! groups, not bare pids.
//!
//! # The platform split (ONE cfg switch at the module boundary)
//!
//! The platform-dependent lifecycle — spawn (process group vs plain), the
//! wait loop (`waitid` WNOWAIT peek vs `try_wait` poll), termination
//! (`killpg` vs `Child::kill`), the foreground-only check (group enumeration
//! vs none), and the pipe drain (`poll`/`fcntl` non-blocking vs reader
//! threads) — lives in the [`unix`] / `windows` submodules, selected by
//! the TWO `mod` declarations below. The rest of the crate calls the
//! re-exported surface and never sees the switch.
//!
//! # The shared liveness decision (single-sourced PER PLATFORM)
//!
//! "Is this process a LIVE member of the group?" must have exactly ONE
//! answer within a platform, or production's enumeration and the test oracle
//! drift. The decision is single-sourced per platform — NOT by one
//! cross-platform predicate, because the two kernels report a process's state
//! in incompatible shapes:
//!
//! * Linux — `is_live_state` over the `/proc/<pid>/stat` state CHARACTER:
//!   `X` (`EXIT_DEAD`) and `Z` (`EXIT_ZOMBIE`) are not live; every other
//!   state, including an unknown one, is live. Production's
//!   `live_group_members` and the oracle's `probe_process` both call it.
//! * macOS — `unix::macos_status_is_not_live` over the BSD process STATUS
//!   (a number): `SZOMB` (= 5) is not live; every other status is live.
//!   Production reads the status as `pbi_status` through
//!   `proc_pidinfo(PROC_PIDTBSDINFO)`; the oracle reads it as `p_stat`
//!   through `sysctl(KERN_PROC_PID)` (it also needs the start-time token,
//!   and `proc_pidinfo` describes no zombie). The accessors differ; the
//!   decision is the one function.

use crate::env::SysEnv;
use std::path::PathBuf;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use unix as platform;
#[cfg(windows)]
use windows as platform;

#[cfg(unix)]
pub use unix::RealKill;
// The bounded pipe-drain discipline (non-blocking setup, the running-drain
// and the bounded post-exit drain) is re-exported so the SSH runner's Unix
// seam reuses ONE implementation instead of keeping a divergent copy. The
// [`ProcessGroup`] capability is NOT platform-specific (it is defined below,
// in this module), so BOTH platforms' seams share it.
#[cfg(unix)]
pub(crate) use unix::{DrainState, drain_available, drain_to_eof, set_nonblocking};
#[cfg(windows)]
pub use windows::RealKill;

/// Grace between the group SIGTERM and the escalated group SIGKILL: a child
/// (or grandchild) that handles TERM gracefully gets a chance to clean up,
/// one that ignores it is force-killed. Shared with the SSH runner's real
/// seam so both transports use the same termination policy.
pub(crate) const TERM_TO_KILL_GRACE: Duration = Duration::from_millis(200);

/// Bound on the post-termination reap: after the escalation the child must be
/// collected within this window or the runner reports a reap failure (see
/// [`RunnerConfig::reap_bound`]).
pub(crate) const KILL_REAP_BOUND: Duration = Duration::from_secs(2);

/// THE single LINUX definition of "this `/proc/<pid>/stat` state field names
/// a LIVE process", shared by production's group-member enumeration (the
/// Linux arm of `live_group_members`) and the test oracle's classification
/// (`probe_process`), so the two cannot drift apart. macOS has its
/// own single definition, `unix::macos_status_is_not_live`; the two
/// platforms do NOT share one predicate, because their state encodings are
/// incompatible (a state character vs a numeric BSD status).
///
/// The kernel emits exactly the state characters in `fs/proc/array.c`'s
/// `task_state_array`, selected by `task_index_to_char` in
/// `include/linux/sched.h`:
///
/// * LIVE — `R` running, `S` sleeping, `D` disk sleep, `T` stopped,
///   `t` tracing stop, `P` parked, `I` idle. A stopped/traced task is not
///   exiting: it keeps its resources and can resume, so it is a live group
///   member.
/// * NOT live — `X` dead and `Z` zombie. `X` is `EXIT_DEAD`: the task has
///   already finished exiting (`__task_state_index` reports `X` whenever
///   `exit_state` holds `EXIT_DEAD`) and only awaits its final release, so
///   it must NOT be counted as a live member; `Z` is `EXIT_ZOMBIE`: exited
///   but held unreaped.
///
/// `task_index_to_char` is `"RSDTtXZPI"`: the kernel never emits a lowercase
/// `z` or `x`, but they are accepted as the same dead states defensively. Any
/// other string is not a state a kernel can report (a malformed `stat` line);
/// it conservatively counts as LIVE, so a malformed entry can never let a
/// leftover escape the foreground-only check.
#[cfg(any(target_os = "linux", all(test, unix)))]
pub(crate) fn is_live_state(state: &str) -> bool {
    !matches!(state, "X" | "Z" | "x" | "z")
}

/// The two rungs of a process group's termination ladder — the only signals
/// this crate has business sending a group. The enum is the authority: a caller
/// cannot express a signal outside these two rungs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupSignal {
    /// Ask the members to terminate.
    Term,
    /// Force the members to terminate.
    Kill,
}

impl GroupSignal {
    /// The libc constant behind the rung. Unix only: Windows has no process
    /// groups, so its [`KillSeam::kill_group`] is a no-op and never names one.
    #[cfg(unix)]
    fn raw(self) -> i32 {
        match self {
            GroupSignal::Term => libc::SIGTERM,
            GroupSignal::Kill => libc::SIGKILL,
        }
    }
}

/// The kill seam behind [`ChildRunner`]: the syscall-level termination
/// surface, injectable for tests (a kill-function pointer seam). The runner
/// reports a kill failure as an error only when the seam says the signal
/// could not be delivered; an inert seam (returns `Ok` without signalling) is
/// caught by the reap bound instead.
///
/// Both methods take the [`ProcessGroup`] CAPABILITY, never a pid: a pgid is a
/// pid, and the kernel recycles it the instant its child is reaped, so the seam
/// can only signal a group whose leader is still un-reaped.
///
/// Public because [`RunnerConfig`] is public and takes an `Arc<dyn KillSeam>`:
/// a caller building its own [`RunnerConfig`] (and a test injecting a
/// syscall-level fault) needs the trait to name its seam. The crate's own
/// tests drive the runner under an injected kill seam through it.
pub trait KillSeam: Send + Sync {
    /// Signal the WHOLE group. Windows has no process groups: there the group
    /// signal is a no-op and the owned-handle signal is the termination path.
    fn kill_group(&self, group: &ProcessGroup, sig: GroupSignal) -> std::io::Result<()>;
    /// Signal the group's OWNED leader alone (`Child::kill`): the last-resort
    /// rung that catches a leader which escaped its group (e.g. `setsid`),
    /// where the group signal reports the group unreachable. The leader is
    /// reachable only while it is un-reaped, so this cannot hit a recycled pid.
    fn kill_owned(&self, group: &mut ProcessGroup) -> std::io::Result<()>;
}

/// The [`ProcessGroup::drop`] backstop's bounded wait: after signalling the
/// group and the owned leader, drop waits this long for the collection before
/// giving up.
const DROP_REAP_BOUND: Duration = Duration::from_millis(100);

/// A child's lifecycle as a VALUE: `Live` until the single reap consumes its
/// exit status, `Reaped(status)` from then on. The variant — not a bare
/// boolean beside the handle — is what makes "signal after the reap"
/// unrepresentable: the only way a caller outside this type reaches the
/// [`Child`] is [`ProcessGroup::live_child`], which is `None` in the `Reaped`
/// state, and the exit status is carried with the state instead of being
/// discarded.
#[derive(Debug)]
pub(crate) enum ChildLifecycle {
    /// The child has not been reaped: it is running or an uncollected zombie.
    Live,
    /// The child's exit status was consumed by the single reap. The pid is
    /// released: nothing may signal it again.
    Reaped(ExitStatus),
}

/// The LAST termination signal the runner failed to deliver, in the message
/// form [`RunError::Kill`] reports. A newtype rather than a bare `String` so an
/// escalation's failure cannot be confused with any other message, and so a
/// failure reaches a [`Termination`] only from the dispatch closure a
/// transition calls in its own arm: a transition attempted from any rung but
/// its predecessor dispatches nothing and records nothing (see [`Escalating`]).
#[derive(Debug)]
pub(crate) struct KillFailure(String);

impl KillFailure {
    pub(crate) fn new(message: String) -> Self {
        KillFailure(message)
    }

    pub(crate) fn message(&self) -> &str {
        &self.0
    }
}

/// What every termination rung past `Armed` carries: the instant its bounds
/// are measured from, and the failure accumulator. One value, so a rung cannot
/// carry a bound without carrying its failure. Only a [`Termination`] transition
/// constructs it, and only the arm that advances supplies the failure, so the
/// accumulator's only writer is a transition that dispatched a signal.
#[derive(Debug)]
pub(crate) struct Escalating {
    /// The instant this rung's bounds are measured from: on Unix the instant
    /// the timeout fired (every rung of the ladder measures from the same
    /// anchor), on Windows the deadline the single rung crossed, because that
    /// port measures its reap bound from the deadline.
    escalation_start: Instant,
    /// The LAST failed termination signal (`None` while every signal so far
    /// was delivered). A later successful signal does NOT erase an earlier
    /// failure, and a later failure replaces it.
    failure: Option<KillFailure>,
}

/// The termination ladder as ONE ascending value: the timeout rung and every
/// escalation after it.
///
/// The ladder ASCENDS — `Armed` → `Termed` → `GroupKilled` → `ChildKilled` on
/// Unix; `Armed` → `ChildKilled` on Windows, which has no process groups and
/// therefore no graceful or group rung. The ORDER of that ascent has ONE
/// authority, the transition methods: each is the authority for its own rung,
/// matching its required PREDECESSOR into the next rung, and called from any
/// other rung it dispatches nothing and returns the ladder UNCHANGED. That is a
/// rule the transition's own `match` enforces at runtime, not a typing fact —
/// `self` is accepted in every state — so a call site needs no state guard of
/// its own: the same rung cannot be dispatched twice and KILL cannot precede
/// TERM, because `group_killed` matches only `Termed`. The caller supplies the
/// DUE test (the bound that decides whether the rung is attempted now) and the
/// dispatch; the transition supplies the order. The escalation's instant and its
/// [`KillFailure`] accumulator are carried BY the value rather than kept beside
/// it, so a kill failure can only be recorded on a ladder whose timeout fired.
///
/// This type records WHICH SIGNAL WAS DISPATCHED, never whether it took effect.
/// "Is this process alive?" is not asked here and must not be: the liveness
/// decision is single-sourced PER PLATFORM (see the module docs), so no
/// cross-platform "is live" predicate can grow on this type, and each
/// platform's arm keeps its own ESRCH-benign rule.
#[derive(Debug)]
pub(crate) enum Termination {
    /// Nothing was signalled yet: the child runs against `deadline`.
    Armed { deadline: Instant },
    /// The GRACEFUL rung: the timeout fired and the platform's graceful
    /// signal was dispatched (Unix: SIGTERM to the whole process group).
    #[cfg(unix)]
    Termed(Escalating),
    /// The escalated rung: the platform's group KILL was dispatched (Unix
    /// only — Windows has no process group to escalate to).
    #[cfg(unix)]
    GroupKilled(Escalating),
    /// The last rung: the OWNED-handle kill was dispatched, on either
    /// platform. On Unix it catches a child that escaped its group.
    ChildKilled(Escalating),
}

impl Termination {
    /// A ladder armed against `deadline`, with nothing signalled yet.
    pub(crate) fn armed(deadline: Instant) -> Self {
        Termination::Armed { deadline }
    }

    /// The deadline an ARMED ladder fires against, `None` once the timeout
    /// fired (the ladder left `Armed`).
    pub(crate) fn deadline(&self) -> Option<Instant> {
        match self {
            Termination::Armed { deadline } => Some(*deadline),
            #[cfg(unix)]
            Termination::Termed(_) | Termination::GroupKilled(_) => None,
            Termination::ChildKilled(_) => None,
        }
    }

    /// The instant this ladder's bounds are measured from, `None` while
    /// `Armed` (no signal was dispatched, so no bound is running).
    pub(crate) fn escalation_start(&self) -> Option<Instant> {
        self.escalating().map(|rung| rung.escalation_start)
    }

    /// Whether the timeout fired — the ladder left `Armed`. Says nothing about
    /// the child: which rung is due, and whether the child is alive, are each
    /// platform's own answer.
    pub(crate) fn timed_out(&self) -> bool {
        self.escalating().is_some()
    }

    /// The LAST termination signal this ladder failed to deliver.
    pub(crate) fn failure(&self) -> Option<&KillFailure> {
        self.escalating().and_then(|rung| rung.failure.as_ref())
    }

    /// The rung payload, `None` while `Armed`.
    fn escalating(&self) -> Option<&Escalating> {
        match self {
            Termination::Armed { .. } => None,
            #[cfg(unix)]
            Termination::Termed(rung) | Termination::GroupKilled(rung) => Some(rung),
            Termination::ChildKilled(rung) => Some(rung),
        }
    }
}

/// The process group of ONE spawned child — the authority to signal it,
/// enumerate it and collect it. A pgid is a pid, and the kernel recycles a pid
/// the instant its child is reaped, so a remembered number is not an authority:
/// a `killpg` on it can reach whatever unrelated process later inherited the id.
/// This value IS that authority: its fields are private, it is minted only by
/// spawning ([`ProcessGroup::spawn`], [`ProcessGroup::spawn_configured`],
/// [`ProcessGroup::from_child`]) and never from a pid, and
/// [`ProcessGroup::reap`] CONSUMES it, so nothing can signal the group once the
/// kernel may have recycled its pgid.
///
/// Two parties that need the group at once (a wait path draining pipes and a
/// deadline path signalling — the SSH runner's seam) share ONE
/// `Arc<Mutex<Option<ProcessGroup>>>` and compete for the instance: whoever
/// `take`s it consumes the capability, and every later signal finds `None` and
/// is a no-op by construction.
///
/// A capability dropped before its reap goes through the drop backstop, which
/// signals the group and the leader and waits (bounded) for the collection — the
/// ONE "every error path leaves no uncollected child" implementation, shared by
/// BOTH real runners and both platforms. Its group signal is a no-op on Windows
/// (no process groups), so there it terminates the direct child only.
pub struct ProcessGroup {
    /// The owned child, PRIVATE to this type: the seams drain its pipes and
    /// poll it through the accessors below, and reach the handle for a direct
    /// signal only through [`ProcessGroup::live_child`], so the handle of a
    /// reaped child is unreachable. On Unix the group signal (`killpg`) is the
    /// primary termination path and this handle is the last-resort rung.
    child: Child,
    kill: Arc<dyn KillSeam>,
    /// LIVE until the single reap consumes the exit status, `Reaped(status)`
    /// from then on: from the reap on nothing may signal anything (a pid the
    /// OS recycled after the reap can never be hit — the drop backstop returns
    /// early).
    lifecycle: ChildLifecycle,
    /// Set once [`ProcessGroup::settle`] iterated the enumeration to empty: the
    /// group is then PROVEN empty, so [`ProcessGroup::live_members`] reports
    /// nothing afterwards and a settled group can never be re-reported as a
    /// foreground-only violation.
    settled: bool,
}

impl ProcessGroup {
    /// Spawn `argv` (no shell) as its OWN group leader with piped stdout and
    /// stderr, in `cwd` under the environment snapshot `env`.
    pub(crate) fn spawn(
        argv: &[String],
        cwd: &std::path::Path,
        env: &SysEnv,
        kill: Arc<dyn KillSeam>,
    ) -> std::io::Result<ProcessGroup> {
        let mut cmd = Command::new(&argv[0]);
        env.apply_to_command(&mut cmd);
        cmd.args(&argv[1..]);
        cmd.current_dir(cwd);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        ProcessGroup::spawn_configured(cmd, kill)
    }

    /// Adopt an already-configured `Command`, applying the platform's group rule
    /// HERE so no caller can spawn a child outside a group whose authority it
    /// holds.
    pub(crate) fn spawn_configured(
        mut cmd: Command,
        kill: Arc<dyn KillSeam>,
    ) -> std::io::Result<ProcessGroup> {
        platform::own_process_group(&mut cmd);
        let child = cmd.spawn()?;
        Ok(ProcessGroup::from_child(child, kill))
    }

    /// Take authority over `child`. The caller MUST have spawned it as its own
    /// group leader where the platform has process groups — the rule
    /// [`ProcessGroup::spawn_configured`] applies — which is what makes
    /// pgid == pid.
    pub(crate) fn from_child(child: Child, kill: Arc<dyn KillSeam>) -> ProcessGroup {
        ProcessGroup {
            child,
            kill,
            lifecycle: ChildLifecycle::Live,
            settled: false,
        }
    }

    /// The leader's pid, which is also the group id. A recorded number, not a
    /// handle: it stays readable after the reap for error messages, and nothing
    /// in this crate signals it — every signal is a [`KillSeam`] call handed
    /// this capability.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// The live child handle, or `None` once the exit status was consumed.
    /// This is the ONLY route to the [`Child`]: a reaped child's handle cannot
    /// be reached, so it cannot be signalled.
    pub(crate) fn live_child(&mut self) -> Option<&mut Child> {
        match self.lifecycle {
            ChildLifecycle::Live => Some(&mut self.child),
            ChildLifecycle::Reaped(_) => None,
        }
    }

    /// Take the stdin write end (the SSH wait closure writes the payload from
    /// inside itself, without holding the group slot).
    pub(crate) fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.stdin.take()
    }

    /// The stdout read end, for the running-phase and bounded post-exit
    /// drains. Available in BOTH lifecycle states: the post-exit drain runs
    /// after the reap, when the child's own write end is already closed.
    pub(crate) fn stdout_mut(&mut self) -> &mut Option<ChildStdout> {
        &mut self.child.stdout
    }

    /// The stderr read end, for the running-phase and bounded post-exit
    /// drains (see [`ProcessGroup::stdout_mut`]).
    pub(crate) fn stderr_mut(&mut self) -> &mut Option<ChildStderr> {
        &mut self.child.stderr
    }

    /// Whether the leader has exited but is not collected yet — a `waitid(2)`
    /// `WNOWAIT` peek that leaves it a ZOMBIE: the pid, and therefore the pgid,
    /// stays allocated, so every signal in this window is still addressed to
    /// THIS group. Unix only; Windows polls [`ProcessGroup::try_wait`] instead.
    #[cfg(unix)]
    pub(crate) fn leader_exited(&self) -> std::io::Result<bool> {
        platform::leader_exited(self.pid())
    }

    /// Poll the child WITHOUT blocking. A `Some(status)` consumes the exit
    /// status, so the lifecycle moves `Live` → `Reaped(status)` in this call:
    /// the kernel released the pid and nothing may signal it again. On a
    /// handle that is already `Reaped`, the recorded status is reported again
    /// (the handle is never polled), exactly as [`Child::try_wait`] reports a
    /// status it already consumed.
    pub(crate) fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        if let ChildLifecycle::Reaped(status) = self.lifecycle {
            return Ok(Some(status));
        }
        match self.child.try_wait()? {
            Some(status) => {
                self.lifecycle = ChildLifecycle::Reaped(status);
                Ok(Some(status))
            }
            None => Ok(None),
        }
    }

    /// Reap the leader — the SINGLE blocking reap (on an already-exited zombie
    /// it returns immediately with its status) — and record the status in the
    /// lifecycle: from here on nothing may signal anything, because the pid is
    /// released by this call. On a handle that is already `Reaped`, the
    /// recorded status is returned and the child is NOT waited on again.
    pub(crate) fn wait(&mut self) -> std::io::Result<ExitStatus> {
        if let ChildLifecycle::Reaped(status) = self.lifecycle {
            return Ok(status);
        }
        let status = self.child.wait()?;
        self.lifecycle = ChildLifecycle::Reaped(status);
        Ok(status)
    }

    /// Whether the single reap already happened (the lifecycle is `Reaped`).
    pub(crate) fn is_reaped(&self) -> bool {
        matches!(self.lifecycle, ChildLifecycle::Reaped(_))
    }

    /// Signal the WHOLE group.
    pub fn signal(&self, sig: GroupSignal) -> std::io::Result<()> {
        self.kill.kill_group(self, sig)
    }

    /// Signal the OWNED leader alone — the last-resort rung, and the only signal
    /// this type sends through the held handle. There is no TERM here: a leader
    /// that escaped its group is signalled to die, not asked to.
    pub fn signal_leader(&mut self) -> std::io::Result<()> {
        let kill = self.kill.clone();
        kill.kill_owned(self)
    }

    /// The LIVE members of the group, the un-reaped leader excluded (judged by
    /// each platform's ONE liveness predicate, see the module docs). A settled
    /// group has none by its own recorded fact.
    pub fn live_members(&self) -> Vec<i32> {
        if self.settled {
            return Vec::new();
        }
        platform::live_group_members(self.pid() as i32, self.pid())
    }

    /// Sweep the group until it is PROVEN empty, bounded by `bound`; returns the
    /// members still live at the bound (empty, and recorded, once settled).
    ///
    /// Iteration is what makes it a proof: `fork(2)` records a new child in its
    /// parent's group (`copy_process` in `kernel/fork.c`), so a descendant can
    /// join AFTER a sweep has passed — it inherits the group but not the signal
    /// — and a signalled process keeps a non-live-but-not-yet-`do_exit` state,
    /// so one enumeration can still list the sweep's own victims mid-death.
    pub fn settle(&mut self, bound: Duration) -> Vec<i32> {
        let deadline = Instant::now() + bound;
        loop {
            let live = self.live_members();
            if live.is_empty() {
                self.settled = true;
                return live;
            }
            if Instant::now() >= deadline {
                return live;
            }
            let _ = self.signal(GroupSignal::Kill);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Collect the leader — the SINGLE reap — CONSUMING the capability: the pgid
    /// is released by this call, so nothing can signal it afterwards, not even
    /// this value's own drop backstop.
    pub(crate) fn reap(mut self) -> std::io::Result<ReapedGroup> {
        let stdout = self.child.stdout.take();
        let stderr = self.child.stderr.take();
        let status = self.wait()?;
        Ok(ReapedGroup {
            status,
            stdout,
            stderr,
        })
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if self.is_reaped() {
            return;
        }
        // Final backstop: signal the whole group (a no-op on Windows), then the
        // owned leader, then wait (bounded) for the collection. Under an
        // injected inert signal the bound expires and the child is left to the
        // caller's own cleanup (the fault is exactly the signal not working).
        let _ = self.signal(GroupSignal::Kill);
        let _ = self.signal_leader();
        let budget = Instant::now() + DROP_REAP_BOUND;
        while Instant::now() < budget {
            // `try_wait` records the status it consumes, so a successful
            // backstop collection leaves the lifecycle `Reaped` as well.
            if let Ok(Some(_)) = self.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

/// A collected group: the leader's exit status and the pipes it left behind. No
/// longer the authority — its pgid is released, so this reports the outcome and
/// drains the remaining output, but cannot signal anything.
pub(crate) struct ReapedGroup {
    status: ExitStatus,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
}

impl ReapedGroup {
    /// The leader's exit status, consumed by the single reap.
    pub(crate) fn status(&self) -> ExitStatus {
        self.status
    }

    /// The stdout read end, for the bounded post-exit drain.
    pub(crate) fn stdout_mut(&mut self) -> &mut Option<ChildStdout> {
        &mut self.stdout
    }

    /// The stderr read end, for the bounded post-exit drain.
    pub(crate) fn stderr_mut(&mut self) -> &mut Option<ChildStderr> {
        &mut self.stderr
    }
}

/// Collect the leader (the SINGLE reap) and notify the observer exactly once.
/// Every path that escapes with a collected child goes through here, so no path
/// reaps twice and none skips it. Consuming the capability is the point: the
/// caller cannot signal the group after this call.
fn collect(
    group: ProcessGroup,
    observer: Option<&Arc<dyn Fn(u32) + Send + Sync>>,
    pid: u32,
    argv: &[String],
) -> std::result::Result<ReapedGroup, RunError> {
    let reaped = group
        .reap()
        .map_err(|e| RunError::Wait(format!("wait {argv:?}: {e}")))?;
    if let Some(observer) = observer {
        observer(pid);
    }
    Ok(reaped)
}

/// The runner's policy knobs: termination timing, the reap bound, the kill
/// seam, and (tests only) the spawn/reap observers that record the lifecycle
/// in the parent. Construct via [`RunnerConfig::production`]; tests build
/// their own with injected faults.
pub struct RunnerConfig {
    /// Grace between the group SIGTERM and the escalated group SIGKILL.
    pub term_to_kill_grace: Duration,
    /// Bound on the post-termination reap: if the child is still alive this
    /// long after the timeout fired, the termination is ineffective and the
    /// runner reports a reap failure (never a fake timeout success).
    pub reap_bound: Duration,
    /// The kill seam (production: [`RealKill`]; tests: injected faults).
    pub kill: Arc<dyn KillSeam>,
    /// Spawn observer: called synchronously in the parent right
    /// after a successful spawn with the child's pid — before the timeout
    /// clock starts — so a test can assert the pid is gone afterwards without
    /// any child-written pidfile (which would race the deadline kill).
    pub spawn_observer: Option<Arc<dyn Fn(u32) + Send + Sync>>,
    /// Reap observer: called exactly once, at the single reap.
    pub reap_observer: Option<Arc<dyn Fn(u32) + Send + Sync>>,
}

impl RunnerConfig {
    /// The production configuration: 200ms TERM→KILL grace, a 2s reap bound,
    /// and the real kill seam.
    pub fn production() -> Self {
        RunnerConfig {
            term_to_kill_grace: TERM_TO_KILL_GRACE,
            reap_bound: KILL_REAP_BOUND,
            kill: Arc::new(RealKill),
            spawn_observer: None,
            reap_observer: None,
        }
    }
}

/// How a runner invocation ended, before the transport maps it to its own
/// outcome shape. The timeout variant exists ONLY after the child (and its
/// group) was proven dead and reaped.
#[derive(Debug)]
pub enum RunOutcome {
    /// The child exited (or was killed by a signal) before the timeout fired.
    Exited {
        exit_code: i32,
        stdout: String,
        stderr: String,
    },
    /// The timeout fired; the child (and its group on Unix) was terminated
    /// AND reaped.
    TimedOut { stderr: String },
}

/// How a runner invocation failed. An error path does not return while the
/// runner still holds a live child: it kills and reaps where it can, and the
/// drop backstop's bounded kill-and-wait covers the paths that cannot.
#[derive(Debug)]
pub enum RunError {
    /// The child could not be spawned.
    Spawn(String),
    /// Waiting on the child failed (wait error, pipe read error).
    Wait(String),
    /// A foreground-only CONTRACT VIOLATION: the command left a process
    /// behind. Two conditions produce this variant.
    ///
    /// (1) A live member of the child's process group survives the child's
    /// exit. Unix only, where the runner enumerates the group: the group
    /// termination is attempted (TERM → grace → KILL, then iterated to an
    /// empty enumeration), the leader is collected, and the violation is then
    /// reported.
    /// (2) A pipe is still open when the bounded post-exit drain expires.
    /// Unix and Windows: a descendant holds the inherited stdout, or the
    /// inherited stderr, write end. On Unix the group check has already
    /// passed, so the holder is not a live in-group member — it is a
    /// descendant that escaped the group (a `setsid` pipe-holder) while
    /// keeping the pipes. NO group termination is attempted on this path;
    /// the drain bound expiring IS the detection.
    ///
    /// Commands are FOREGROUND-ONLY: leaving a process behind is a violation
    /// under either condition, never a successful outcome.
    Background(String),
    /// A timeout-termination signal could not be delivered (kill failure).
    Kill(String),
    /// The child was not collected within the reap bound after termination.
    Reap(String),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::Spawn(m) => write!(f, "{m}"),
            RunError::Wait(m) => write!(f, "{m}"),
            RunError::Background(m) => write!(f, "{m}"),
            RunError::Kill(m) => write!(f, "kill failure: {m}"),
            RunError::Reap(m) => write!(f, "reap failure: {m}"),
        }
    }
}

/// THE shared bounded child-runner for local command execution: spawn the
/// child (into its OWN process group on Unix), wait with the caller's
/// timeout, on timeout terminate (TERM, grace, KILL on Unix; the owned
/// child on Windows), and return a success or timeout outcome only after the
/// child was REAPED. A timeout-kill or reap failure is an ERROR, never a
/// successful timeout outcome.
///
/// The runner is per-exec and owns nothing between calls: the child lives
/// inside the platform exec and is REAPED before the call returns, and the
/// wait runs on the calling thread rather than a spawned one, so no child is
/// leaked and the lifecycle is bounded. The ONE named exception is the Windows
/// pipe-holding case: a descendant that keeps the inherited output pipes open
/// past the drain bound leaves a pipe reader thread blocked in `read_to_end`,
/// so the call returns (with [`RunError::Background`]) while that thread is
/// still alive, left to EOF when the descendant dies. On Unix the drain holds
/// no such thread, so nothing outlives the call there.
pub struct ChildRunner {
    /// The child environment snapshot: every spawned child receives THIS
    /// snapshot as its ENTIRE environment ([`SysEnv::apply_to_command`]:
    /// `env_clear` first, then the snapshot's variables) — deterministic and
    /// hermetic, never whatever the parent env looks like at spawn time.
    env: SysEnv,
    /// The child's working directory (the transport root).
    cwd: PathBuf,
    config: RunnerConfig,
}

impl ChildRunner {
    /// Build a runner that spawns children with the environment snapshot
    /// `env` in working directory `cwd` under the policy `config`.
    pub fn new(env: &SysEnv, cwd: PathBuf, config: RunnerConfig) -> Self {
        ChildRunner {
            env: env.clone(),
            cwd,
            config,
        }
    }

    /// Execute `argv` (no shell) bounding the CHILD by `timeout`. Returns
    /// [`RunOutcome::Exited`] when the child finishes in time (exit code +
    /// captured stdout/stderr) AND (on Unix) left no members of its process
    /// group behind (commands are FOREGROUND-ONLY), [`RunOutcome::TimedOut`]
    /// ONLY after the child (and its group on Unix) was terminated AND the
    /// child was reaped, or an error when the spawn, the wait, the
    /// termination kill, or the reap failed — a failed timeout kill never
    /// yields a successful timeout outcome, and a command that exited but
    /// left background processes in its group is a violation, never a
    /// successful outcome. `timeout` is ADDITIVE with the termination and
    /// bounded-drain tail (TERM grace + the bounded post-exit drain of each of
    /// the two pipes), so a timed-out call returns up to about `timeout +
    /// 4.2 s` later, never at `timeout` exactly; a command that exits inside
    /// `timeout` still reports its real result (a pipe-holding leftover is
    /// [`RunError::Background`], never a deadline flip). The platform-specific
    /// lifecycle (process groups, the foreground-only check, the pipe drain)
    /// lives in the [`unix`] / `windows` submodules.
    pub fn exec(
        &self,
        argv: &[String],
        timeout: Duration,
    ) -> std::result::Result<RunOutcome, RunError> {
        platform::exec(&self.env, &self.cwd, &self.config, argv, timeout)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Instant;

    /// How long the oracle polls for a process to disappear. The runner's own
    /// reap/kill bound is seconds, so this must outlast a correct run while
    /// staying finite: a genuinely live leftover keeps the assertion failing
    /// after the budget instead of hanging the suite.
    const GONE_BUDGET: Duration = Duration::from_secs(5);

    /// The identity of a process: its pid plus the kernel's start-time token.
    /// The token is fixed for a process's whole life and distinct between any
    /// two processes that ever held the same pid, so a pid the OS recycled to
    /// an unrelated process can never be mistaken for the tracked one.
    #[derive(Clone, Copy, Debug)]
    struct ProcessId {
        pid: u32,
        start: u64,
    }

    /// The kernel's view of a pid. A ZOMBIE has exited but is not yet reaped.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ProcessState {
        Live,
        Zombie,
    }

    impl ProcessState {
        /// Classify a `/proc/<pid>/stat` state field through the SAME Linux
        /// [`is_live_state`] predicate production's Linux arm uses. The
        /// oracle does not re-derive the live/gone rule, so an
        /// oracle/production divergence is impossible by construction. (The
        /// macOS arm shares `unix::macos_status_is_not_live` the same way.)
        fn of_proc_state(state: &str) -> Self {
            if is_live_state(state) {
                ProcessState::Live
            } else {
                ProcessState::Zombie
            }
        }
    }

    /// The kernel's `(state, start-time token)` for `pid`, or `None` when the
    /// pid names no process at all (reaped, or never existed). A ZOMBIE is
    /// reported with the start token it had while alive — which is what lets
    /// [`ProcessId::capture`] record a child that exited before the observer
    /// ran — but is never reported as live. Both arms share production's ONE
    /// liveness decision for their platform rather than agreeing with
    /// `kill(pid, 0)` (which a zombie still answers): on Linux,
    /// [`is_live_state`] over the `/proc` state character (with `X` =
    /// `EXIT_DEAD` also not live); on macOS, `macos_status_is_not_live` over
    /// the `p_stat` status this accessor reads.
    #[cfg(target_os = "macos")]
    fn probe_process(pid: u32) -> Option<(ProcessState, u64)> {
        // `KERN_PROC_PID` fills a `struct kinfo_proc`: `p_starttime` is two
        // u64 halves at offset 0 and `p_stat` at offset 36 — and, unlike
        // `proc_pidinfo(PROC_PIDTBSDINFO)` (which returns nothing for a
        // zombie), it ALSO describes zombies, the property the identity
        // capture relies on. The buffer is DELIBERATELY larger than today's
        // 648-byte struct (whose prefix layout is the stable part) so a future
        // growth still returns a readable entry rather than a short write. A
        // reaped pid yields a zero-length result (rc == 0, len == 0).
        const P_STAT_OFFSET: usize = 36;
        let mut buf = [0u8; 1024];
        let mut len = buf.len();
        let mut mib = [
            libc::CTL_KERN,
            libc::KERN_PROC,
            libc::KERN_PROC_PID,
            pid as i32,
        ];
        // SAFETY: `sysctl` writes the kernel's process entry into `buf`; the
        // mib is the documented `KERN_PROC_PID` selector and `len` starts as
        // the buffer size and is updated by the kernel to the bytes written.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as u32,
                buf.as_mut_ptr().cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 || len < P_STAT_OFFSET + 1 {
            return None;
        }
        let sec = u64::from_ne_bytes(buf[0..8].try_into().unwrap());
        let usec = u64::from_ne_bytes(buf[8..16].try_into().unwrap());
        let state = if unix::macos_status_is_not_live(u32::from(buf[P_STAT_OFFSET])) {
            ProcessState::Zombie
        } else {
            ProcessState::Live
        };
        Some((state, sec * 1_000_000 + usec))
    }

    /// Linux: `/proc/<pid>/stat` — `pid (comm) state ppid ...`, with
    /// `starttime` as field 22 (19 fields after `state`). A zombie is still
    /// listed (it holds the pid) with its original start token; a reaped pid
    /// is unreadable and reads as absent.
    #[cfg(target_os = "linux")]
    fn probe_process(pid: u32) -> Option<(ProcessState, u64)> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // `comm` may contain spaces AND ')' — anchor on the LAST ')'.
        let rest = stat.rsplit_once(')')?.1;
        let mut fields = rest.split_whitespace();
        let state = fields.next()?;
        // `state` is field 3; `starttime` is field 22, i.e. `nth(18)` of the
        // fields that follow `state` (field 4 is `nth(0)`).
        let start: u64 = fields.nth(18)?.parse().ok()?;
        let state = ProcessState::of_proc_state(state);
        Some((state, start))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn probe_process(_pid: u32) -> Option<(ProcessState, u64)> {
        compile_error!("the process oracle is implemented for Linux and macOS only");
    }

    /// The bounded poll shared by the oracles: `true` once the pid names
    /// nothing, a ZOMBIE, or a process whose start token differs (the pid was
    /// recycled and the tracked process is gone); `false` only while the SAME
    /// live process is still there when the budget expires.
    fn wait_until<F>(pid: u32, budget: Duration, gone: F) -> bool
    where
        F: Fn(ProcessState, u64) -> bool,
    {
        let deadline = Instant::now() + budget;
        loop {
            match probe_process(pid) {
                None => return true,
                Some((state, start)) if gone(state, start) => return true,
                Some(_) => {}
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    impl ProcessId {
        /// Record `pid`'s identity while it is known to be the tracked
        /// process. `None` only when the pid already names nothing; a zombie
        /// still yields its token, so a child that exited before the observer
        /// ran is still captured.
        fn capture(pid: u32) -> Option<Self> {
            probe_process(pid).map(|(_, start)| ProcessId { pid, start })
        }

        /// Bounded wait for THIS process to be GONE by the runner's own
        /// definition of alive: `live_group_members` excludes zombies, so a
        /// zombie is dead, and a recycled pid carries a different start token
        /// so it is a different process. `false` means the same live process
        /// outlived the budget — a genuine leftover, which must fail the test.
        fn wait_until_gone(self, budget: Duration) -> bool {
            wait_until(self.pid, budget, |state, start| {
                state == ProcessState::Zombie || start != self.start
            })
        }
    }

    /// The token-less fallback, used only where an identity could not be
    /// captured before the kill (a starved capture poll): gone means the pid
    /// names nothing or a ZOMBIE. A live process still fails, so a genuine
    /// leftover is never missed — only the pid-reuse guard is unavailable.
    fn wait_until_not_live(pid: u32, budget: Duration) -> bool {
        wait_until(pid, budget, |state, _| state == ProcessState::Zombie)
    }

    /// Poll `marker` for the pid the shell writes, then capture that process's
    /// identity WHILE IT IS STILL THE ORIGINAL — before the runner's timeout
    /// kill re-parents it and the init process reaps it, when the pid could be
    /// recycled. `None` when the marker never appeared or the pid was already
    /// reaped; the caller then falls back to the token-less check.
    fn capture_grandchild(marker: &std::path::Path, budget: Duration) -> Option<ProcessId> {
        let deadline = Instant::now() + budget;
        loop {
            // Require the trailing newline `echo` writes: a marker seen
            // without it is a PARTIAL write, and parsing a truncated pid would
            // track an unrelated process.
            if let Ok(text) = std::fs::read_to_string(marker)
                && text.ends_with('\n')
                && let Ok(pid) = text.trim().parse::<u32>()
            {
                return ProcessId::capture(pid);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn fixture_runner(cwd: &std::path::Path, config: RunnerConfig) -> ChildRunner {
        ChildRunner::new(&SysEnv::from_process(), cwd.to_path_buf(), config)
    }

    /// A promptly-completing child returns its exit code and captured output,
    /// the single reap is observed (never a zombie), and the recorded pid is
    /// gone after the call.
    #[test]
    fn quick_child_is_reaped_and_output_captured() {
        let pid_slot: Arc<Mutex<Option<ProcessId>>> = Arc::new(Mutex::new(None));
        let reaped = Arc::new(AtomicBool::new(false));
        let config = RunnerConfig {
            spawn_observer: Some({
                let slot = pid_slot.clone();
                Arc::new(move |pid: u32| *slot.lock().unwrap() = ProcessId::capture(pid))
            }),
            reap_observer: Some({
                let reaped = reaped.clone();
                Arc::new(move |_pid: u32| reaped.store(true, Ordering::SeqCst))
            }),
            ..RunnerConfig::production()
        };
        let env = SysEnv::from_process();
        let runner = fixture_runner(&env.temp_dir(), config);
        let out = runner
            .exec(
                &["sh".into(), "-c".into(), "printf ok; exit 3".into()],
                Duration::from_secs(5),
            )
            .expect("a promptly-completing child must succeed");
        match out {
            RunOutcome::Exited {
                exit_code, stdout, ..
            } => {
                assert_eq!(exit_code, 3);
                assert_eq!(stdout, "ok");
            }
            other => panic!("expected Exited, got {other:?}"),
        }
        let pid = pid_slot
            .lock()
            .unwrap()
            .expect("the spawn observer must record the pid at spawn time");
        assert!(
            pid.wait_until_gone(GONE_BUDGET),
            "child {} must be reaped, not signalled-and-left",
            pid.pid
        );
        assert!(
            reaped.load(Ordering::SeqCst),
            "the reap observer must fire exactly once"
        );
    }

    /// A child that outlives the timeout is killed WITH ITS WHOLE PROCESS
    /// GROUP (the backgrounded grandchild dies too) and the outcome is
    /// `TimedOut` only after the reap.
    #[test]
    fn timeout_kills_the_whole_process_group() {
        let env = SysEnv::from_process();
        let dir = tempfile::Builder::new()
            .tempdir_in(env.temp_dir())
            .expect("tempdir");
        let marker = dir.path().join("grandchild.pid");
        let config = RunnerConfig {
            term_to_kill_grace: Duration::from_millis(50),
            reap_bound: Duration::from_secs(5),
            ..RunnerConfig::production()
        };
        let runner = fixture_runner(dir.path(), config);
        let script = format!("sleep 30 & echo $! > {}; wait", marker.display());
        // Capture the grandchild's identity CONCURRENTLY with the run, while
        // it is still the original process: after the group kill it is
        // re-parented and reaped asynchronously, and only a token taken before
        // that can tell it apart from a recycled pid.
        let captured = {
            let marker = marker.clone();
            let reader = std::thread::spawn(move || capture_grandchild(&marker, GONE_BUDGET));
            let out = runner
                .exec(
                    &["sh".into(), "-c".into(), script],
                    Duration::from_millis(500),
                )
                .expect("the timeout path must terminate and reap the group");
            assert!(
                matches!(out, RunOutcome::TimedOut { .. }),
                "expected TimedOut, got {out:?}"
            );
            reader
                .join()
                .expect("the identity-capture thread must not panic")
        };
        let grandchild: u32 = std::fs::read_to_string(&marker)
            .expect("the child writes the grandchild pid immediately")
            .trim()
            .parse()
            .expect("pid");
        match captured {
            Some(id) => assert!(
                id.wait_until_gone(GONE_BUDGET),
                "the grandchild {} must die with the group (not merely be signalled, and not linger as a zombie)",
                id.pid
            ),
            None => assert!(
                wait_until_not_live(grandchild, GONE_BUDGET),
                "the grandchild {grandchild} must die with the group"
            ),
        }
    }

    /// The foreground-only contract: a child that exits but leaves a live
    /// member of its process group behind is reported as an ERROR (the group
    /// is terminated), never a successful outcome.
    #[test]
    fn exiting_child_that_leaves_a_background_process_is_an_error() {
        let env = SysEnv::from_process();
        let dir = tempfile::Builder::new()
            .tempdir_in(env.temp_dir())
            .expect("tempdir");
        let marker = dir.path().join("leftover.pid");
        let config = RunnerConfig {
            term_to_kill_grace: Duration::from_millis(50),
            reap_bound: Duration::from_secs(5),
            ..RunnerConfig::production()
        };
        let runner = fixture_runner(dir.path(), config);
        let script = format!("sleep 30 & echo $! > {}; exit 0", marker.display());
        // Same concurrent identity capture as the timeout test: the leftover
        // is killed by the foreground-only check and reaped asynchronously.
        let captured = {
            let marker = marker.clone();
            let reader = std::thread::spawn(move || capture_grandchild(&marker, GONE_BUDGET));
            let err = runner
                .exec(&["sh".into(), "-c".into(), script], Duration::from_secs(5))
                .expect_err("a command that leaves background processes must error");
            assert!(
                matches!(err, RunError::Background(_)),
                "expected Background, got {err:?}"
            );
            reader
                .join()
                .expect("the identity-capture thread must not panic")
        };
        let leftover: u32 = std::fs::read_to_string(&marker)
            .expect("the child writes the leftover pid immediately")
            .trim()
            .parse()
            .expect("pid");
        match captured {
            Some(id) => assert!(
                id.wait_until_gone(GONE_BUDGET),
                "the leftover {} must be terminated, not merely signalled and left",
                id.pid
            ),
            None => assert!(
                wait_until_not_live(leftover, GONE_BUDGET),
                "the leftover {leftover} must be terminated"
            ),
        }
    }

    /// BUG 1 REGRESSION: a TIMED-OUT group is swept to a PROVEN-EMPTY
    /// enumeration, never sampled once.
    ///
    /// The child exits on its own AFTER the deadline but WELL BEFORE the
    /// ladder's escalated KILL is due, leaving one TERM-immune helper in its
    /// group. That fixes the group's state by construction instead of by
    /// kernel timing: at the moment the wait loop breaks, exactly one live
    /// member exists and the ladder's TERM (which the helper ignores) has
    /// already passed it, so
    ///
    /// * one enumeration reports a live member — the runner's own successful
    ///   termination misread as a foreground-only violation;
    /// * an iterated sweep re-signals and re-enumerates to empty — a clean
    ///   timeout.
    ///
    /// The assertion is the CONSEQUENCE: `RunOutcome::TimedOut` (reachable only
    /// through an empty enumeration) and the helper — which nothing but the
    /// sweep could kill, since the ladder's KILL was never dispatched and the
    /// leader exited without signalling it — is gone.
    #[test]
    fn a_timed_out_group_is_swept_to_a_proven_empty_group() {
        let env = SysEnv::from_process();
        let dir = tempfile::Builder::new()
            .tempdir_in(env.temp_dir())
            .expect("tempdir");
        let marker = dir.path().join("helper.pid");
        let config = RunnerConfig {
            term_to_kill_grace: Duration::from_millis(200),
            reap_bound: Duration::from_secs(2),
            ..RunnerConfig::production()
        };
        let runner = fixture_runner(dir.path(), config);
        // `sleep 0.12` either finishes or is killed by the group TERM at the
        // 100ms deadline; either way the leader exits inside the grace window,
        // before the escalated KILL is due. The helper ignores TERM, so it is
        // live and untouched when the leader goes.
        let script = format!(
            "trap '' TERM; ( trap '' TERM; sleep 30 ) & echo $! > {}; sleep 0.12; exit 0",
            marker.display()
        );
        let captured = {
            let marker = marker.clone();
            let reader = std::thread::spawn(move || capture_grandchild(&marker, GONE_BUDGET));
            let out = runner
                .exec(
                    &["sh".into(), "-c".into(), script],
                    Duration::from_millis(100),
                )
                .expect(
                    "a timed-out group swept to empty must return the timeout outcome, never \
                     report its own termination as a foreground-only violation",
                );
            assert!(
                matches!(out, RunOutcome::TimedOut { .. }),
                "a swept-to-empty timed-out group must yield TimedOut, got {out:?}"
            );
            reader
                .join()
                .expect("the identity-capture thread must not panic")
        };
        let helper: u32 = std::fs::read_to_string(&marker)
            .expect("the leader writes the helper pid immediately")
            .trim()
            .parse()
            .expect("pid");
        match captured {
            Some(id) => assert!(
                id.wait_until_gone(GONE_BUDGET),
                "the TERM-immune helper {} must be terminated by the sweep, not left live",
                id.pid
            ),
            None => assert!(
                wait_until_not_live(helper, GONE_BUDGET),
                "the TERM-immune helper {helper} must be terminated by the sweep"
            ),
        }
    }

    /// A timed-out group that is STILL live at the reap bound is a termination
    /// that did not take effect — never a clean timeout. The seam's group signal
    /// is RECORDED and never delivered, so every member survives the sweep by
    /// construction and the verdict can be asserted without a race. The leader
    /// exits on its own (well inside the ladder's own bound), so the loop
    /// breaks with the leader a zombie while its helper is still live: the
    /// `settle` arm — not the loop's "leader still alive" arm — is the one
    /// under test.
    #[test]
    fn an_unsettled_timed_out_group_is_a_reap_failure() {
        let env = SysEnv::from_process();
        let dir = tempfile::Builder::new()
            .tempdir_in(env.temp_dir())
            .expect("tempdir");
        let reaped = Arc::new(AtomicUsize::new(0));
        let config = RunnerConfig {
            term_to_kill_grace: Duration::from_millis(20),
            reap_bound: Duration::from_millis(300),
            kill: RecordingKill::delivering(Delivery::Recorded),
            reap_observer: Some({
                let reaped = reaped.clone();
                Arc::new(move |_pid: u32| {
                    reaped.fetch_add(1, Ordering::SeqCst);
                })
            }),
            ..RunnerConfig::production()
        };
        let runner = fixture_runner(dir.path(), config);
        let script = "sleep 1 & sleep 0.2";
        let err = runner
            .exec(
                &["sh".into(), "-c".into(), script.into()],
                Duration::from_millis(100),
            )
            .expect_err("a group still live at the bound must not yield a timeout outcome");
        assert!(
            matches!(err, RunError::Reap(_)),
            "an unsettled group with no refused signal must be a Reap failure, got {err:?}"
        );
        assert_eq!(
            reaped.load(Ordering::SeqCst),
            1,
            "the leader must be reaped exactly once before the unsettled verdict escapes"
        );
    }

    /// A group whose ladder recorded a REFUSED signal is a `RunError::Kill`,
    /// never a clean `TimedOut` and never the `Reap` an unexplained leftover
    /// would give.
    ///
    /// The seam delivers TERM and refuses the GROUP KILL; every member ignores
    /// TERM, so the group cannot empty and only the last-resort owned-leader
    /// signal ends the leader. The recorded refusal is what decides the
    /// verdict: without it the same live group is a plain `Reap`.
    #[test]
    fn an_unsettled_group_with_a_refused_kill_is_a_kill_failure() {
        let env = SysEnv::from_process();
        let dir = tempfile::Builder::new()
            .tempdir_in(env.temp_dir())
            .expect("tempdir");
        let reaped = Arc::new(AtomicUsize::new(0));
        let config = RunnerConfig {
            term_to_kill_grace: Duration::from_millis(20),
            reap_bound: Duration::from_millis(500),
            kill: RecordingKill::delivering(Delivery::RefuseKill),
            reap_observer: Some({
                let reaped = reaped.clone();
                Arc::new(move |_pid: u32| {
                    reaped.fetch_add(1, Ordering::SeqCst);
                })
            }),
            ..RunnerConfig::production()
        };
        let runner = fixture_runner(dir.path(), config);
        let script = "trap '' TERM; ( trap '' TERM; sleep 30 ) & sleep 0.2; exit 0";
        let err = runner
            .exec(
                &["sh".into(), "-c".into(), script.into()],
                Duration::from_millis(100),
            )
            .expect_err("a refused termination signal must not yield a timeout outcome");
        assert!(
            matches!(err, RunError::Kill(_)),
            "a group whose KILL was refused must be a Kill failure, got {err:?}"
        );
        assert_eq!(
            reaped.load(Ordering::SeqCst),
            1,
            "the leader must be reaped exactly once before the Kill verdict escapes"
        );
    }

    /// On the paths where the reap succeeds, every returned outcome happens
    /// only after the child was reaped: a timeout kill failure is an error.
    /// An error path that CANNOT reap drops the handle, and the drop
    /// backstop's bounded kill-and-wait is the final resort. This test drives
    /// the success path, so the timed-out child is gone when the call returns.
    #[test]
    fn timed_out_child_is_never_left_live() {
        let pid_slot: Arc<Mutex<Option<ProcessId>>> = Arc::new(Mutex::new(None));
        let config = RunnerConfig {
            term_to_kill_grace: Duration::from_millis(50),
            reap_bound: Duration::from_secs(5),
            spawn_observer: Some({
                let slot = pid_slot.clone();
                Arc::new(move |pid: u32| *slot.lock().unwrap() = ProcessId::capture(pid))
            }),
            ..RunnerConfig::production()
        };
        let env = SysEnv::from_process();
        let runner = fixture_runner(&env.temp_dir(), config);
        let out = runner
            .exec(
                &["sh".into(), "-c".into(), "exec sleep 30".into()],
                Duration::from_millis(200),
            )
            .expect("the timeout path must terminate and reap");
        assert!(matches!(out, RunOutcome::TimedOut { .. }));
        let pid = pid_slot.lock().unwrap().expect("spawn observer");
        assert!(
            pid.wait_until_gone(GONE_BUDGET),
            "child {} must be reaped, not left live",
            pid.pid
        );
    }

    /// The shared LINUX predicate's mapping over every state the kernel can
    /// emit, plus the defensive lowercase and unknown cases. Pinned so the
    /// ONE Linux definition cannot drift: production's Linux enumeration and
    /// the oracle both read this mapping.
    #[test]
    fn is_live_state_classifies_kernel_states() {
        // LIVE: the non-exit states `task_state_array` can report.
        for state in ["R", "S", "D", "T", "t", "P", "I"] {
            assert!(is_live_state(state), "{state} is a live kernel state");
        }
        // DEAD: `X` (EXIT_DEAD) and `Z` (EXIT_ZOMBIE), plus the lowercase
        // spellings a kernel never emits but a defensive reader accepts.
        for state in ["X", "Z", "x", "z"] {
            assert!(!is_live_state(state), "{state} is a dead kernel state");
        }
        // Unknown or malformed input conservatively counts as LIVE, so a
        // leftover can never hide behind an unparsed state.
        for state in ["", "Q", "?", "RS", " Z", "r"] {
            assert!(
                is_live_state(state),
                "{state:?} is not a kernel state and must default to live"
            );
        }
    }

    /// The oracle's own semantics, pinned against the kernel's state table so
    /// it can never silently diverge from production. The shared rule is PER
    /// PLATFORM: on Linux the oracle classifies a `/proc` state character
    /// through the SAME `is_live_state` predicate production uses, and the
    /// state-table loop below pins that mapping — so a regression that counts
    /// `X` (EXIT_DEAD) as live fails here. On macOS the oracle classifies the
    /// BSD status through the SAME `unix::macos_status_is_not_live` production
    /// uses (pinned by `macos_status_classifies_bsd_process_states` in
    /// `unix`), and the live-child and un-reaped-zombie probes below exercise
    /// that shared decision end to end. On top of the table: a LIVE process is
    /// not gone, a ZOMBIE is gone, and a pid whose start token moved is a
    /// DIFFERENT process, so the tracked one is gone even though the number
    /// still answers.
    #[test]
    fn process_oracle_matches_the_live_definition() {
        // The Linux kernel state table, checked through the shared
        // `is_live_state` predicate AND the oracle's derivation from it.
        // Pinning the shared rule pins both sides at once (the oracle does
        // not re-derive it).
        for state in ["R", "S", "D", "T", "t", "P", "I"] {
            assert!(
                is_live_state(state),
                "{state} is a live kernel state; production and the oracle must agree it is live"
            );
            assert_eq!(
                ProcessState::of_proc_state(state),
                ProcessState::Live,
                "the oracle must classify {state} as live, matching production"
            );
        }
        for state in ["X", "Z", "x", "z"] {
            assert!(
                !is_live_state(state),
                "{state} is a dead kernel state (X = EXIT_DEAD, Z = EXIT_ZOMBIE); \
                 production must not count it as a live member"
            );
            assert_eq!(
                ProcessState::of_proc_state(state),
                ProcessState::Zombie,
                "the oracle must classify {state} as gone, matching production"
            );
        }

        // A live child is NOT gone, even after the budget elapses.
        let mut live = std::process::Command::new("sleep")
            .arg("30")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn a live child");
        let live_id = ProcessId::capture(live.id()).expect("a live child is capturable");
        assert!(
            !live_id.wait_until_gone(Duration::from_millis(50)),
            "a live process must not read as gone: the oracle would be vacuous"
        );
        live.kill().expect("kill the live child");
        live.wait().expect("reap the live child");
        assert!(
            live_id.wait_until_gone(GONE_BUDGET),
            "a killed-and-reaped child must read as gone"
        );

        // A child left as an un-reaped ZOMBIE is gone: it still holds its pid,
        // so `kill(pid, 0)` succeeds, but the runner's `live_group_members`
        // excludes zombies — the exact definition the oracle must share.
        let mut zombie = std::process::Command::new("sh")
            .args(["-c", "exit 0"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn a promptly-exiting child");
        let zombie_id = {
            let deadline = Instant::now() + GONE_BUDGET;
            loop {
                match probe_process(zombie.id()) {
                    Some((ProcessState::Zombie, start)) => {
                        break ProcessId {
                            pid: zombie.id(),
                            start,
                        };
                    }
                    Some((ProcessState::Live, _)) => {}
                    None => panic!("the child vanished before it could be observed as a zombie"),
                }
                assert!(Instant::now() < deadline, "the child never became a zombie");
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        assert!(
            zombie_id.wait_until_gone(GONE_BUDGET),
            "an un-reaped zombie must read as gone (the runner counts only live members)"
        );
        zombie.wait().expect("reap the zombie");

        // PID REUSE: the same pid with a moved start token is a different
        // process, so the tracked process is gone.
        let self_id = ProcessId::capture(std::process::id()).expect("this test process is live");
        let forged = ProcessId {
            start: self_id.start.wrapping_add(1),
            ..self_id
        };
        assert!(
            forged.wait_until_gone(Duration::from_millis(50)),
            "a pid whose start token moved must read as the tracked process being gone"
        );
    }

    /// How the recording seam DELIVERS a signal — the state of the group is then
    /// known BY CONSTRUCTION rather than by winning a race against the kernel.
    enum Delivery {
        /// Really signal, the production behaviour.
        Real,
        /// Record and report success WITHOUT signalling anything.
        Recorded,
        /// Deliver TERM, refuse the GROUP KILL with a permission error (the fault
        /// a group that may not be signalled reports).
        RefuseKill,
    }

    /// A [`KillSeam`] that RECORDS every call, so a test can assert DIRECTLY
    /// whether the drop backstop fired, and injects a delivery fault where a
    /// verdict needs the group's state fixed.
    struct RecordingKill {
        delivery: Delivery,
        group_calls: Mutex<Vec<(i32, GroupSignal)>>,
        owned_calls: AtomicUsize,
    }

    impl RecordingKill {
        fn recording(terminate: bool) -> Arc<Self> {
            Self::delivering(if terminate {
                Delivery::Real
            } else {
                Delivery::Recorded
            })
        }

        fn delivering(delivery: Delivery) -> Arc<Self> {
            Arc::new(RecordingKill {
                delivery,
                group_calls: Mutex::new(Vec::new()),
                owned_calls: AtomicUsize::new(0),
            })
        }

        fn group_calls(&self) -> Vec<(i32, GroupSignal)> {
            self.group_calls.lock().unwrap().clone()
        }

        fn owned_calls(&self) -> usize {
            self.owned_calls.load(Ordering::SeqCst)
        }
    }

    impl KillSeam for RecordingKill {
        fn kill_group(&self, group: &ProcessGroup, sig: GroupSignal) -> std::io::Result<()> {
            self.group_calls
                .lock()
                .unwrap()
                .push((group.pid() as i32, sig));
            match self.delivery {
                Delivery::Real => RealKill.kill_group(group, sig),
                Delivery::Recorded => Ok(()),
                Delivery::RefuseKill if sig == GroupSignal::Kill => Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "simulated refusal to signal the group",
                )),
                Delivery::RefuseKill => RealKill.kill_group(group, sig),
            }
        }

        fn kill_owned(&self, group: &mut ProcessGroup) -> std::io::Result<()> {
            self.owned_calls.fetch_add(1, Ordering::SeqCst);
            match self.delivery {
                Delivery::Real | Delivery::RefuseKill => RealKill.kill_owned(group),
                Delivery::Recorded => Ok(()),
            }
        }
    }

    /// The bare `sh -c script` command the capability tests spawn.
    fn group_command(script: &str) -> std::process::Command {
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg(script);
        cmd
    }

    /// WITNESS for the `if self.is_reaped() { return; }` guard in
    /// [`ProcessGroup::drop`].
    ///
    /// The guard protects against signalling a pid the OS RECYCLED after the
    /// reap: [`ProcessGroup::wait`] (like [`ProcessGroup::try_wait`]) moves the
    /// lifecycle to [`ChildLifecycle::Reaped`] and releases the pid, and a
    /// `killpg`/`kill` on the released number could land on an unrelated
    /// process; this test pins the guard DIRECTLY, through the module's own
    /// reap path, and asserts the drop backstop does NOT signal.
    ///
    /// The REAPED arm calls [`ProcessGroup::wait`] (it consumes the exit
    /// status, so the pid is really reaped, not merely declared so) and drops
    /// the capability: the recording seam must record NO group signal and NO
    /// direct signal. The CONTROL arm drops an UN-REAPED capability with the
    /// same fixture: it must record exactly one group signal (`Kill`, the drop
    /// signal) and one direct signal, proving the silence above is the guard's
    /// doing and not the seam's. Deleting the guard makes the REAPED arm's
    /// assertions fail.
    #[test]
    fn a_reaped_owned_child_is_never_signalled_by_the_drop_backstop() {
        // REAPED: `wait` consumes the exit status and moves the lifecycle to
        // `Reaped`, so the drop backstop must return before it touches the
        // seam. The seam is INERT (records without signalling) precisely
        // because the pid under test is released: even with the guard deleted
        // this witness cannot signal a recycled pid.
        let reaped_seam = RecordingKill::recording(false);
        {
            let mut group =
                ProcessGroup::spawn_configured(group_command("exit 0"), reaped_seam.clone())
                    .expect("spawn a promptly-exiting child");
            group.wait().expect("a promptly-exiting child must reap");
        }
        assert_eq!(
            reaped_seam.group_calls(),
            Vec::new(),
            "a reaped child's group must never be signalled by the drop backstop"
        );
        assert_eq!(
            reaped_seam.owned_calls(),
            0,
            "a reaped child must never be signalled directly by the drop backstop"
        );

        // CONTROL: the same fixture, but the child is NOT reaped, so the drop
        // backstop must fire. The seam TERMINATES (delegates to `RealKill`) so
        // the control's child is really collected by the backstop's own bounded
        // collection — no child is left behind.
        let control_seam = RecordingKill::recording(true);
        let pid = {
            let group = ProcessGroup::spawn_configured(
                group_command("exec sleep 30"),
                control_seam.clone(),
            )
            .expect("spawn a long-lived child");
            let pid = group.pid();
            drop(group);
            pid
        };
        assert_eq!(
            control_seam.group_calls(),
            vec![(pid as i32, GroupSignal::Kill)],
            "the control's un-reaped child must be signalled as a group by the drop backstop"
        );
        assert_eq!(
            control_seam.owned_calls(),
            1,
            "the control's un-reaped child must also be signalled through the owned leader"
        );
        assert!(
            wait_until_not_live(pid, GONE_BUDGET),
            "the control's child {pid} must be collected by the drop backstop, not left live"
        );
    }

    /// The lifecycle as a VALUE: the handle is reachable only while the child
    /// is LIVE, the single reap records the exit status it consumed, and the
    /// drop backstop of a reaped capability signals nothing (the pid was
    /// released by the reap, so a signal could land on a recycled pid).
    ///
    /// The seam is INERT (records without signalling) for the same reason as
    /// the control-arm witness above: the child under test is already reaped,
    /// so a deleted guard could otherwise signal a recycled pid.
    #[test]
    fn a_reaped_child_has_no_live_handle_and_is_never_signalled() {
        let seam = RecordingKill::recording(false);
        let mut group = ProcessGroup::spawn_configured(group_command("exit 0"), seam.clone())
            .expect("spawn a promptly-exiting child");
        assert!(!group.is_reaped(), "a freshly spawned child is live");
        assert!(
            group.live_child().is_some(),
            "a live child must expose its handle"
        );

        let status = group.wait().expect("a promptly-exiting child must reap");
        assert_eq!(status.code(), Some(0), "the exit status must be preserved");
        assert!(
            group.is_reaped(),
            "the reap must move the lifecycle to Reaped"
        );
        assert!(
            group.live_child().is_none(),
            "a reaped child must not expose its handle: a signal on the released pid is unrepresentable"
        );
        // The reap is SINGLE: `try_wait` reports the status it already
        // consumed instead of polling the released pid again.
        assert_eq!(
            group
                .try_wait()
                .expect("a reaped child still reports its status"),
            Some(status),
            "the consumed exit status must be the recorded one"
        );

        drop(group);
        assert_eq!(
            seam.group_calls(),
            Vec::new(),
            "the drop backstop must not signal a reaped child's group"
        );
        assert_eq!(
            seam.owned_calls(),
            0,
            "the drop backstop must not signal a reaped child directly"
        );
    }
}
