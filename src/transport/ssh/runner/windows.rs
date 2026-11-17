//! The Windows implementation of the ssh subprocess seam: plain spawn (no
//! process groups), owned-handle termination (`TerminateProcess`), and
//! reader THREADS for the pipe drains (no `poll`/`fcntl` non-blocking
//! pipes). The documented weaker guarantees of the Windows port: a
//! background descendant survives a kill (only the direct child is
//! terminated), and a descendant that keeps the output pipes open keeps the
//! reader threads from EOF, so the buffer collection has no bound. Selected by
//! the single `#[cfg(windows)]` `mod` declaration in [`super`].

use super::*;
use crate::transport::runner::{ProcessGroup, RealKill};
use std::io::Read;
use std::process::Stdio;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

/// The Windows spawn: the child is spawned plain (no process group), the
/// kill terminates the OWNED child (`TerminateProcess`), and the wait
/// drains the pipes via reader THREADS.
pub(crate) fn spawn(
    env: &SysEnv,
    _op: OpKind,
    argv: &[String],
    stdin: Option<Vec<u8>>,
) -> std::io::Result<SpawnedChild> {
    let mut cmd = std::process::Command::new(&argv[0]);
    env.apply_to_command(&mut cmd);
    cmd.args(&argv[1..]);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    // No process group on Windows (`own_process_group` is a no-op there): the
    // owned handle (`TerminateProcess`) is the whole termination path, the
    // documented weaker guarantee of this port.
    let group = ProcessGroup::spawn_configured(cmd, Arc::new(RealKill))?;
    // The pid is read off the capability synchronously at spawn time — the
    // parent's own number, so the child never has to write a pidfile.
    let pid = group.pid();
    // The group is shared EXCLUSIVELY between the runner's deadline path and
    // the wait thread through this ONE slot (the same discipline as the Unix
    // seam): the wait thread polls `try_wait` with the slot locked and CONSUMES
    // it on exit; the deadline path locks the same slot and signals the owned
    // child. Whoever takes the capability consumes it, so a signal after the
    // collect finds `None` and is a no-op by construction.
    //
    // The slot holds the SAME platform-neutral [`ProcessGroup`] the local
    // runner owns, so the Windows SSH seam inherits the SAME drop backstop the
    // Unix seam has (ONE authority for "every error path leaves no uncollected
    // child"): a `try_wait`/reader error that returns early from the wait
    // closure drops the slot — and the capability with it — and the backstop
    // terminates and collects it (`TerminateProcess`; the group signal is a
    // no-op on Windows), where a bare `std::process::Child`'s own `Drop`
    // neither waits nor signals. On Windows the backstop terminates only the
    // DIRECT child — a background descendant survives, the documented weaker
    // guarantee of this port.
    let group: Arc<Mutex<Option<ProcessGroup>>> = Arc::new(Mutex::new(Some(group)));
    let kill_group_cap = group.clone();
    let kill: Box<dyn Fn() -> std::io::Result<()> + Send> = Box::new(move || {
        let mut guard = kill_group_cap.lock().unwrap();
        let Some(group) = guard.as_mut() else {
            // The wait thread already consumed the capability: a signal on it
            // is a NO-OP by construction.
            return Ok(());
        };
        // No process groups on Windows: the owned child (`TerminateProcess`) is
        // the whole termination path, a background descendant survives. The
        // leader is reachable only while it is LIVE: once the wait thread has
        // consumed the exit status `live_child` yields nothing, so a pid the OS
        // recycled after the collect can never be signalled.
        group.signal_leader()
    });
    let wait_group = group.clone();
    // The deadline fact: the wait closure flips it to `Reaped` the instant it
    // consumes the exit status — BEFORE the reader buffers are collected — so
    // a deadline that fires afterwards finds a child that had already exited
    // and returns the closure's real result instead of fabricating a timeout.
    let status: Arc<WaitStatusCell> = Arc::new(WaitStatusCell::new(WaitStatus::Running));
    let wait_status_cell = status.clone();
    let wait: WaitClosure = Box::new(move |tx: mpsc::Sender<WaitResult>| {
        // The body below keeps its early-return shape; EVERY exit still ends
        // in exactly one result delivery.
        let body = move || -> WaitResult {
            use std::io::Write;
            let mut stdin_pipe = wait_group
                .lock()
                .unwrap()
                .as_mut()
                .and_then(|g| g.take_stdin());
            // Write the payload FIRST, saving any error (the same
            // collect-before-surface discipline as the Unix seam).
            let write_res = match (&stdin, stdin_pipe.as_mut()) {
                (Some(data), Some(sin)) => sin.write_all(data),
                _ => Ok(()),
            };
            drop(stdin_pipe);
            // Reader threads own the output pipes (no poll/fcntl on
            // Windows): they read to EOF and send the buffer on a channel.
            let stdout_rx = {
                let mut guard = wait_group.lock().unwrap();
                spawn_reader(guard.as_mut().and_then(|g| g.stdout_mut().take()))
            };
            let stderr_rx = {
                let mut guard = wait_group.lock().unwrap();
                spawn_reader(guard.as_mut().and_then(|g| g.stderr_mut().take()))
            };
            // Poll loop: `try_wait` with the slot locked; when the child
            // exits the slot is consumed (reaped) and the reader buffers
            // collected.
            let wait_res = loop {
                let mut exited: Option<(ProcessGroup, std::process::ExitStatus)> = None;
                {
                    let mut guard = wait_group.lock().unwrap();
                    let g = guard
                        .as_mut()
                        .expect("the wait thread is the sole consumer of the group slot");
                    match g.try_wait() {
                        Ok(Some(status)) => {
                            // `try_wait` consumed the exit status and moved the
                            // handle to `Reaped`: record that in the shared
                            // deadline fact BEFORE the reader buffers are
                            // collected (the bounded drain), so the runner can
                            // tell "the child exited" from "the child was still
                            // running at the deadline".
                            record_reap(&wait_status_cell);
                            exited = guard.take().map(|g| (g, status));
                        }
                        Ok(None) => {}
                        Err(e) => return Err(RunError::Wait(format!("wait: {e}"))),
                    }
                }
                if let Some((group, status)) = exited {
                    // The child is REAPED and the capability cannot signal it;
                    // `Reaped` was announced above. Its handles are closed, so
                    // the reader threads EOF and send their buffers. The collect
                    // CONSUMES the capability, so nothing can signal the
                    // released pid.
                    let mut reaped = group
                        .reap()
                        .map_err(|e| RunError::Wait(format!("wait: {e}")))?;
                    debug_assert_eq!(reaped.status(), status);
                    let stdout = stdout_rx
                        .recv()
                        .map_err(|_| RunError::Wait("stdout reader failed".into()))?;
                    let stderr = stderr_rx
                        .recv()
                        .map_err(|_| RunError::Wait("stderr reader failed".into()))?;
                    break Ok(std::process::Output {
                        status: reaped.status(),
                        stdout,
                        stderr,
                    });
                }
                std::thread::sleep(Duration::from_millis(1));
            };
            // The saved stdin-write error is surfaced only AFTER the child
            // was collected — so the far side's own stderr and exit status
            // are available. Preserve them (the Unix seam does the same), so
            // a failed upload to a full disk is not reported as a bare
            // `Broken pipe` indistinguishable from a dead host. The class
            // stays [`RunError::StdinWrite`].
            match write_res {
                Err(e) => {
                    let far_side = match &wait_res {
                        Ok(out) => String::from_utf8_lossy(&out.stderr).trim().to_string(),
                        Err(_) => String::new(),
                    };
                    let status = match &wait_res {
                        Ok(out) => match out.status.code() {
                            Some(code) => format!("; the far side exited {code}"),
                            None => "; the far side was killed by a signal".to_string(),
                        },
                        Err(_) => String::new(),
                    };
                    let detail = if far_side.is_empty() {
                        status
                    } else {
                        format!("{status}; the far side reported: {far_side}")
                    };
                    Err(RunError::StdinWrite(format!("stdin write: {e}{detail}")))
                }
                Ok(()) => wait_res,
            }
        };
        // Exactly ONE result per wait, whatever the body returned.
        let _ = tx.send(body());
    });
    Ok(SpawnedChild {
        pid,
        status,
        kill,
        wait,
    })
}

/// Spawn a thread that reads `stream` to EOF into a buffer and sends it on
/// a channel.
fn spawn_reader<R: Read + Send + 'static>(stream: Option<R>) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut s) = stream {
            // Best-effort: a read error mid-drain yields the partial buffer.
            let _ = s.read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });
    rx
}
