//! Startup cleanup barrier before node admission (SCL-S8.5a, US-7.3 AC1, red-team RT-11).
//!
//! At launch, core first cleans up sidecar processes left by an earlier run (the startup
//! cleanup in `lib.rs` `setup`), then may start the node. The node's chain database reset on a
//! genesis change ([`crate::node_genesis`]) depends on that order: an orphaned node from the
//! earlier run would otherwise still hold the database. Until now the order held only because
//! the cleanup happened to be called earlier in `setup`. This barrier makes it a rule: no node
//! spawn is admitted until the startup cleanup has returned.
//!
//! - [`StartupBarrier::run_cleanup`] runs the cleanup and opens the barrier when it returns. It
//!   wraps the cleanup function without depending on what the cleanup does.
//! - `NodeManager::start` waits (bounded) for the barrier before it touches the data dir. A
//!   start that times out is refused with [`crate::node::NodeError::StartupCleanupPending`].
//! - If the cleanup panics the barrier stays closed: no node is admitted (fail closed).
//!
//! The barrier is per process. A crash restarts it closed on the next launch.

use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Admission gate for node spawns: closed until the startup cleanup has finished.
#[derive(Debug, Default)]
pub struct StartupBarrier {
    open: Mutex<bool>,
    cv: Condvar,
}

impl StartupBarrier {
    /// A closed barrier.
    pub fn new() -> Self {
        StartupBarrier::default()
    }

    /// The process-wide barrier the app's startup cleanup opens.
    pub fn global() -> Arc<StartupBarrier> {
        static GLOBAL: OnceLock<Arc<StartupBarrier>> = OnceLock::new();
        GLOBAL
            .get_or_init(|| Arc::new(StartupBarrier::new()))
            .clone()
    }

    /// Run the startup `cleanup`, then open the barrier. A panic in `cleanup` propagates and
    /// leaves the barrier closed.
    pub fn run_cleanup(&self, cleanup: impl FnOnce()) {
        cleanup();
        self.open();
    }

    fn open(&self) {
        *self.open.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.cv.notify_all();
    }

    /// True once the startup cleanup has finished.
    #[cfg(test)]
    pub fn is_open(&self) -> bool {
        *self.open.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Wait up to `timeout` for the barrier to open. Returns whether it is open.
    pub fn wait_open(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut open = self.open.lock().unwrap_or_else(|e| e.into_inner());
        while !*open {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            open = match self.cv.wait_timeout(open, deadline - now) {
                Ok((guard, _)) => guard,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_until_the_cleanup_returns() {
        let b = Arc::new(StartupBarrier::new());
        assert!(!b.is_open());
        assert!(!b.wait_open(Duration::from_millis(20)));
        let seen = Arc::new(Mutex::new(None));
        let (b2, seen2) = (b.clone(), seen.clone());
        b.run_cleanup(move || {
            *seen2.lock().unwrap() = Some(b2.is_open());
        });
        assert_eq!(
            *seen.lock().unwrap(),
            Some(false),
            "closed while cleanup runs"
        );
        assert!(b.is_open());
        assert!(b.wait_open(Duration::ZERO));
    }

    #[test]
    fn a_waiter_is_released_when_the_cleanup_finishes() {
        let b = Arc::new(StartupBarrier::new());
        let b2 = b.clone();
        let waiter = std::thread::spawn(move || b2.wait_open(Duration::from_secs(10)));
        std::thread::sleep(Duration::from_millis(50));
        b.run_cleanup(|| {});
        assert!(waiter.join().unwrap());
    }

    #[test]
    fn a_panicking_cleanup_leaves_the_barrier_closed() {
        let b = StartupBarrier::new();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            b.run_cleanup(|| panic!("cleanup failed"));
        }));
        assert!(r.is_err());
        assert!(!b.is_open(), "fail closed: no node admitted");
    }

    /// Tripwire: the app's startup cleanup runs through the barrier, before the node state is
    /// built. (Checks the call shape only, not which cleanup function is passed.)
    #[test]
    fn setup_runs_the_startup_cleanup_through_the_barrier() {
        let src = include_str!("lib.rs");
        let setup = src.find(".setup(|app| {").expect("setup closure");
        let barrier = src[setup..]
            .find("StartupBarrier::global().run_cleanup(")
            .map(|i| i + setup)
            .expect("the startup cleanup runs through StartupBarrier::run_cleanup");
        let node = src[setup..]
            .find("node::build_node_state(")
            .map(|i| i + setup)
            .expect("node state is built in setup");
        assert!(barrier < node, "cleanup barrier precedes node admission");
    }
}
