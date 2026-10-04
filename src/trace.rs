//! Verbose step tracing.
//!
//! A tiny, dependency-free tracer: when verbose is enabled, each traced
//! step is emitted to STDERR as a `[trace]` line carrying the step name,
//! the time spent since the previous step (and since the trace start), and
//! a free-form detail line. Stderr keeps the report on stdout
//! machine-parseable. A disabled tracer is a zero-cost no-op.
//!
//! The tracer is the debugging surface for the multi-step operations in
//! this crate: a caller records each step (parse, resolve, transfer) so a
//! future agent can see exactly what an operation did, in what order, and
//! how long each step took — `grep '\[trace\]'` on the captured stderr.

use std::time::Instant;

/// A verbose step tracer. `enabled` gates every emission; a disabled tracer
/// records nothing and prints nothing.
pub struct Tracer {
    enabled: bool,
    start: Instant,
    last: Instant,
}

impl Tracer {
    /// A tracer gated on `enabled`.
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            start: Instant::now(),
            last: Instant::now(),
        }
    }

    /// Record a step: `[trace] +<since-last> (+<since-start>) <name>: <detail>`.
    /// A no-op when verbose is off.
    pub fn step(&mut self, name: &str, detail: impl std::fmt::Display) {
        if !self.enabled {
            return;
        }
        let now = Instant::now();
        let since_last = now.duration_since(self.last);
        let since_start = now.duration_since(self.start);
        eprintln!("[trace] +{since_last:?} (+{since_start:?}) {name}: {detail}");
        self.last = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A disabled tracer RECORDS nothing: `step` returns before it touches the
    /// bookkeeping, so a `Tracer::new(false)` cannot print and cannot move
    /// `last`. Asserted on the observable it has (the bookkeeping), because a
    /// test that only calls `step` and checks for panic cannot fail.
    #[test]
    fn disabled_tracer_is_a_noop() {
        let mut t = Tracer::new(false);
        let before = t.last;
        std::thread::sleep(std::time::Duration::from_millis(2));
        t.step("ref.parse", "token=\"@-\" -> @-");
        assert_eq!(
            t.last, before,
            "a disabled tracer must not record the step (the gate returns first)"
        );
    }

    /// An enabled tracer RECORDS a step: `last` moves to the step's own instant,
    /// so the elapsed time since the trace start covers the sleep before it.
    /// This is what fails if `step` stops recording (a total no-op leaves
    /// `last == start`), which is the property the name states.
    #[test]
    fn enabled_tracer_records_steps() {
        let mut t = Tracer::new(true);
        std::thread::sleep(std::time::Duration::from_millis(2));
        t.step("ref.parse", "token=\"@-\" -> @-");
        let el = t.last.duration_since(t.start);
        assert!(
            el >= std::time::Duration::from_millis(2),
            "an enabled tracer must record the step AFTER the sleep: elapsed since start = {el:?}"
        );
        // The emission itself goes to stderr (the documented sink); the
        // bookkeeping above is the in-process observable, and the second step
        // must not move `last` BACKWARDS (the clock is monotonic).
        let after_first = t.last;
        t.step("ref.resolve", "target=\"production\" expr=@-");
        assert!(
            t.last >= after_first,
            "the step clock must not go backwards"
        );
    }
}
