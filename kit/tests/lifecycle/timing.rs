// created: 2026-10-18 | branch: beta/k1a-gate0-harness | author: Larry Klosowski + Claude Opus 5.5 (MAC lane)
// status: active (SCL-S1.3 fault-injection harness)
//
// Monotonic timing helpers. Every bound here is measured on `Instant` (monotonic), never as a
// sum of requested sleeps and never on the wall clock, so a slow CI host stretches a wait but
// cannot shorten an observation window (SPEC-001: "actual monotonic elapsed time").

#![allow(dead_code)]

use std::time::{Duration, Instant};

/// An absolute monotonic deadline.
#[derive(Debug, Clone, Copy)]
pub struct Deadline {
    start: Instant,
    end: Instant,
}

impl Deadline {
    pub fn after(d: Duration) -> Self {
        let start = Instant::now();
        Deadline {
            start,
            end: start + d,
        }
    }

    pub fn expired(&self) -> bool {
        Instant::now() >= self.end
    }

    pub fn remaining(&self) -> Duration {
        self.end.saturating_duration_since(Instant::now())
    }

    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

/// A wait that ran to its deadline without the condition holding.
#[derive(Debug, Clone, Copy)]
pub struct Elapsed {
    pub waited: Duration,
}

/// Poll `f` every `poll` until it returns `Some`, or until `timeout` of monotonic time has
/// passed. Returns the value and how long it took, or how long it waited.
pub fn wait_for<T>(
    timeout: Duration,
    poll: Duration,
    mut f: impl FnMut() -> Option<T>,
) -> Result<(T, Duration), Elapsed> {
    let deadline = Deadline::after(timeout);
    loop {
        if let Some(v) = f() {
            return Ok((v, deadline.elapsed()));
        }
        if deadline.expired() {
            return Err(Elapsed {
                waited: deadline.elapsed(),
            });
        }
        std::thread::sleep(poll.min(deadline.remaining()).max(Duration::from_millis(1)));
    }
}

/// Sleep until at least `window` of monotonic time has passed (a sleep can wake early on some
/// platforms; this loops until the window is really over). Returns the actual elapsed time.
pub fn hold_for(window: Duration) -> Duration {
    let deadline = Deadline::after(window);
    while !deadline.expired() {
        std::thread::sleep(deadline.remaining());
    }
    deadline.elapsed()
}

/// Sample `f` every `every` for a full `window`, returning `(offset, value)` pairs, the first
/// taken at once and the last at or after the end of the window. Used for "nothing new appears
/// over N seconds" observations, where the window must not end early.
pub fn sample_over<T>(
    window: Duration,
    every: Duration,
    mut f: impl FnMut() -> T,
) -> Vec<(Duration, T)> {
    let deadline = Deadline::after(window);
    let mut out = Vec::new();
    loop {
        out.push((deadline.elapsed(), f()));
        if deadline.expired() {
            return out;
        }
        std::thread::sleep(
            every
                .min(deadline.remaining())
                .max(Duration::from_millis(1)),
        );
    }
}
