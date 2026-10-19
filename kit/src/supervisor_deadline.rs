// citrate-core — the reserved deadline publisher (SCL-S2.4, SPEC-001 "Ownership, intent and
// generations"). Included into `supervisor.rs`.
//
// One app-owned std thread observes a fixed overlay of deadline records. A record is armed in a
// control's own admission path, before the monitor is woken, with an absolute monotonic deadline.
// The publisher marks a record expired once that deadline passes. It takes no lifecycle, owner or
// detail lock, does no I/O, joins nothing, serializes nothing and emits no events: it only flips
// one atomic per record. It also never stops anything and never authorizes an action.
//
// A receipt read compares the monotonic deadline itself, so expiry is reported even when the
// publisher has not run (or has died). Records are read through a per-record version (a seqlock):
// a read that cannot establish the current identity yields Unknown, never another record's
// expiry.
//
// If the publisher thread cannot be created, or ends, deadline admission is unavailable. Nothing
// ever creates a replacement thread.

/// The number of deadline records in the overlay. SPEC-001 has nine (eight services plus app
/// close); SCL counts the embed server as its own owner (02 §2), so ten.
pub const DEADLINE_RECORDS: usize = 10;

/// The publisher's longest wait between observations (SPEC-001: initial maximum 50 ms).
pub const DEADLINE_CADENCE: Duration = Duration::from_millis(50);

const REC_FREE: u8 = 0;
const REC_CLAIMED: u8 = 1;
const REC_ARMED: u8 = 2;

/// What the overlay holds for one control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeadlineState {
    /// Armed; the publisher has not marked it expired (its deadline may still have passed).
    Armed,
    /// The publisher observed the deadline pass.
    Expired,
}

/// One fixed record. Identity and deadline are written only while `version` is odd.
#[derive(Default)]
struct DeadlineRecord {
    version: AtomicU64,
    state: AtomicU8,
    owner: AtomicU64,
    epoch: AtomicU64,
    seq: AtomicU64,
    deadline_ns: AtomicU64,
    /// The record version the publisher saw expire. A mark is valid only while it equals the
    /// current version, so a mark made just before the record was rewritten never applies to
    /// the new control.
    expired_version: AtomicU64,
}

/// What a lock-free lookup found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lookup {
    /// No record for this control.
    None,
    /// A record, with its state and absolute deadline (ns since the publisher's base).
    Found { expired_mark: bool, deadline_ns: u64 },
    /// The record was being rewritten throughout the bounded read.
    Torn,
}

/// The body a spawner must run on the reserved thread.
pub type PublisherBody = Box<dyn FnOnce() + Send + 'static>;

/// The reserved deadline publisher. One per app ([`DeadlinePublisher::global`]); tests may build
/// their own instance and hand it to a supervisor through [`SupervisorConfig::deadlines`].
pub struct DeadlinePublisher {
    base: std::time::Instant,
    records: [DeadlineRecord; DEADLINE_RECORDS],
    cadence: Duration,
    thread: Mutex<Option<JoinHandle<()>>>,
    init_error: std::sync::OnceLock<String>,
}

impl std::fmt::Debug for DeadlinePublisher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeadlinePublisher")
            .field("cadence", &self.cadence)
            .field("armed", &self.armed_records())
            .field("init_error", &self.init_error.get())
            .finish_non_exhaustive()
    }
}

impl DeadlinePublisher {
    /// The app-wide publisher, created once on first use with [`DEADLINE_CADENCE`]. A creation
    /// failure is kept (see [`DeadlinePublisher::init_error`]) and is never retried.
    pub fn global() -> Arc<DeadlinePublisher> {
        static GLOBAL: std::sync::OnceLock<Arc<DeadlinePublisher>> = std::sync::OnceLock::new();
        GLOBAL
            .get_or_init(|| DeadlinePublisher::start(DEADLINE_CADENCE))
            .clone()
    }

    /// A publisher on its own std thread, observing at most every `cadence`.
    pub fn start(cadence: Duration) -> Arc<DeadlinePublisher> {
        DeadlinePublisher::start_with_spawner(cadence, |body| {
            std::thread::Builder::new()
                .name("supervisor:deadlines".into())
                .spawn(body)
        })
    }

    /// A publisher whose thread is created by `spawn`, called exactly once. The fault fixtures use
    /// this to make creation fail or the thread end; production uses [`DeadlinePublisher::start`].
    pub fn start_with_spawner(
        cadence: Duration,
        spawn: impl FnOnce(PublisherBody) -> std::io::Result<JoinHandle<()>>,
    ) -> Arc<DeadlinePublisher> {
        let publisher = Arc::new(DeadlinePublisher {
            base: std::time::Instant::now(),
            records: Default::default(),
            cadence: cadence.max(Duration::from_millis(1)),
            thread: Mutex::new(None),
            init_error: std::sync::OnceLock::new(),
        });
        let weak = Arc::downgrade(&publisher);
        match spawn(Box::new(move || publisher_loop(weak))) {
            Ok(handle) => {
                *publisher.thread.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);
            }
            Err(e) => {
                let _ = publisher
                    .init_error
                    .set(format!("the deadline publisher thread could not be created: {e}"));
            }
        }
        publisher
    }

    /// Why the publisher could not be created, if it could not.
    pub fn init_error(&self) -> Option<&str> {
        self.init_error.get().map(String::as_str)
    }

    /// Whether the publisher thread exists and has not ended.
    pub fn is_running(&self) -> bool {
        self.init_error.get().is_none()
            && self
                .thread
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .is_some_and(|h| !h.is_finished())
    }

    /// Why deadline admission is unavailable, or `None` if it is available.
    fn unavailable(&self) -> Option<String> {
        if let Some(e) = self.init_error.get() {
            return Some(e.clone());
        }
        if !self.is_running() {
            return Some(
                "the deadline publisher thread has ended; deadline admission stays unavailable for \
                 this app run"
                    .to_string(),
            );
        }
        None
    }

    /// Records currently claimed or armed.
    pub fn armed_records(&self) -> usize {
        self.records
            .iter()
            .filter(|r| r.state.load(Ordering::Acquire) != REC_FREE)
            .count()
    }

    /// What the overlay holds for control `token` of supervisor `owner`.
    pub fn record_state(&self, owner: u64, token: ControlToken) -> Option<DeadlineState> {
        match self.lookup(owner, token.app_epoch, token.intent_sequence) {
            Lookup::Found {
                expired_mark: true, ..
            } => Some(DeadlineState::Expired),
            Lookup::Found { .. } => Some(DeadlineState::Armed),
            _ => None,
        }
    }

    fn now_ns(&self) -> u64 {
        u64::try_from(self.base.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }

    /// Arm a record for (owner, epoch, seq) expiring `within` from now. `false` if every record is
    /// in use (the caller commits nothing and answers Busy).
    fn arm(&self, owner: u64, epoch: u64, seq: u64, within: Duration) -> bool {
        let deadline_ns = self
            .now_ns()
            .saturating_add(u64::try_from(within.as_nanos()).unwrap_or(u64::MAX));
        for r in &self.records {
            if r.state
                .compare_exchange(REC_FREE, REC_CLAIMED, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                r.version.fetch_add(1, Ordering::AcqRel); // odd: writing
                r.owner.store(owner, Ordering::Release);
                r.epoch.store(epoch, Ordering::Release);
                r.seq.store(seq, Ordering::Release);
                r.deadline_ns.store(deadline_ns, Ordering::Release);
                r.state.store(REC_ARMED, Ordering::Release);
                r.version.fetch_add(1, Ordering::AcqRel); // even: stable
                self.wake();
                return true;
            }
        }
        false
    }

    /// Arm (owner, epoch, seq), or, if it already has a record, move its deadline earlier when
    /// `within` is tighter. A deadline is never moved later. `false` if a new record was needed
    /// and every record is in use.
    fn arm_or_tighten(&self, owner: u64, epoch: u64, seq: u64, within: Duration) -> bool {
        let deadline_ns = self
            .now_ns()
            .saturating_add(u64::try_from(within.as_nanos()).unwrap_or(u64::MAX));
        for r in &self.records {
            if r.state.load(Ordering::Acquire) == REC_ARMED
                && r.owner.load(Ordering::Acquire) == owner
                && r.epoch.load(Ordering::Acquire) == epoch
                && r.seq.load(Ordering::Acquire) == seq
            {
                if deadline_ns < r.deadline_ns.load(Ordering::Acquire) {
                    r.version.fetch_add(1, Ordering::AcqRel);
                    r.deadline_ns.store(deadline_ns, Ordering::Release);
                    r.version.fetch_add(1, Ordering::AcqRel);
                    self.wake();
                }
                return true;
            }
        }
        self.arm(owner, epoch, seq, within)
    }

    /// Free every record held by `owner` (a supervisor that is being dropped).
    fn release_owner(&self, owner: u64) {
        for r in &self.records {
            if r.state.load(Ordering::Acquire) == REC_ARMED
                && r.owner.load(Ordering::Acquire) == owner
            {
                let epoch = r.epoch.load(Ordering::Acquire);
                let seq = r.seq.load(Ordering::Acquire);
                self.release(owner, epoch, seq);
            }
        }
    }

    /// Free the record of (owner, epoch, seq), if there is one.
    fn release(&self, owner: u64, epoch: u64, seq: u64) {
        for r in &self.records {
            let st = r.state.load(Ordering::Acquire);
            if st == REC_ARMED
                && r.owner.load(Ordering::Acquire) == owner
                && r.epoch.load(Ordering::Acquire) == epoch
                && r.seq.load(Ordering::Acquire) == seq
            {
                r.version.fetch_add(1, Ordering::AcqRel);
                r.owner.store(0, Ordering::Release);
                r.seq.store(0, Ordering::Release);
                r.deadline_ns.store(0, Ordering::Release);
                r.state.store(REC_FREE, Ordering::Release);
                r.version.fetch_add(1, Ordering::AcqRel);
            }
        }
    }

    /// Find the record of (owner, epoch, seq) without any lock. Each record is read under its
    /// version; a record that stays mid-write for every bounded retry makes the lookup Torn.
    fn lookup(&self, owner: u64, epoch: u64, seq: u64) -> Lookup {
        let mut torn = false;
        for r in &self.records {
            let mut settled = false;
            for _ in 0..8 {
                let v1 = r.version.load(Ordering::Acquire);
                if v1 % 2 == 1 {
                    std::hint::spin_loop();
                    continue;
                }
                let st = r.state.load(Ordering::Acquire);
                let o = r.owner.load(Ordering::Acquire);
                let e = r.epoch.load(Ordering::Acquire);
                let s = r.seq.load(Ordering::Acquire);
                let d = r.deadline_ns.load(Ordering::Acquire);
                let mark = r.expired_version.load(Ordering::Acquire);
                if r.version.load(Ordering::Acquire) != v1 {
                    continue;
                }
                settled = true;
                if st == REC_ARMED && o == owner && e == epoch && s == seq {
                    return Lookup::Found {
                        expired_mark: mark == v1,
                        deadline_ns: d,
                    };
                }
                break;
            }
            torn |= !settled;
        }
        if torn {
            Lookup::Torn
        } else {
            Lookup::None
        }
    }

    /// Whether control (owner, epoch, seq) has a record whose deadline has passed, by the
    /// publisher's mark or by comparing the monotonic clock directly. `None` if the read was torn.
    fn expired(&self, owner: u64, epoch: u64, seq: u64) -> Option<bool> {
        match self.lookup(owner, epoch, seq) {
            Lookup::None => Some(false),
            Lookup::Found {
                expired_mark,
                deadline_ns,
            } => Some(expired_mark || self.now_ns() >= deadline_ns),
            Lookup::Torn => None,
        }
    }

    fn wake(&self) {
        if let Some(h) = self
            .thread
            .try_lock()
            .ok()
            .and_then(|g| g.as_ref().map(|h| h.thread().clone()))
        {
            h.unpark();
        }
    }

    /// One observation: mark every armed record whose deadline passed; return how long until the
    /// next deadline, capped at the cadence.
    fn observe(&self) -> Duration {
        let now = self.now_ns();
        let mut next = self.cadence;
        for r in &self.records {
            let v1 = r.version.load(Ordering::Acquire);
            if v1 % 2 == 1 || r.state.load(Ordering::Acquire) != REC_ARMED {
                continue;
            }
            let d = r.deadline_ns.load(Ordering::Acquire);
            if r.version.load(Ordering::Acquire) != v1 {
                continue;
            }
            if now >= d {
                r.expired_version.store(v1, Ordering::Release);
            } else {
                next = next.min(Duration::from_nanos(d - now));
            }
        }
        next
    }
}

/// The reserved thread's loop. Holds only a weak reference, so a dropped (test) publisher ends
/// it. Rechecks every absolute deadline after each wakeup; a wakeup never restarts a budget.
fn publisher_loop(publisher: std::sync::Weak<DeadlinePublisher>) {
    loop {
        let wait = match publisher.upgrade() {
            Some(p) => p.observe(),
            None => return,
        };
        std::thread::park_timeout(wait);
    }
}
