//! citrate-core — the node liveness probe (CORE-D2, planset r1007-v060 §CORE-D).
//!
//! The supervisor restarts the node when its process exits. A node that keeps running but stops
//! importing blocks (a wedge) was never restarted. This watch samples the node's own RPC and
//! restarts it, through the supervisor's automated restart, only on strong evidence:
//!
//! - **a sustained stall:** the local head has not moved for [`LivenessPolicy::stall_after`]
//!   (10 minutes by default) while the node has peers **and** the network tip, read from the
//!   public RPC, is ahead of it by more than [`LivenessPolicy::tip_margin`] blocks. A network that
//!   is halted, an unknown tip, no peers, or a silent RPC is never a wedge, and resets the stall
//!   clock where it would otherwise carry a stale stall into a later sample;
//! - **never during initial sync:** an incarnation (one supervisor instance and one spawn) that
//!   has not yet been seen within the margin of the network tip is still syncing, however long it
//!   stands still. Each new incarnation starts in initial sync, so a restarted node has to catch
//!   up before it can be restarted again;
//! - **backoff:** after a restart the next one waits at least [`LivenessPolicy::first_gap`],
//!   doubling after each restart up to [`LivenessPolicy::max_gap`]. The streak ends when the node
//!   is seen caught up more than `max_gap` after the last restart;
//! - **the reason is logged:** every restart (or refused restart) is written to the app log and
//!   kept as the node's note, which `sidecars_health` shows on the node's row.
//!
//! Data sources (Rule 7): the local head and peer count are `eth_blockNumber` and `net_peerCount`
//! on the node's own RPC; the network tip is `eth_blockNumber` on the public RPC (the node
//! manager's 15 s cache); the run identity is the supervisor's lifecycle cell (instance and
//! incarnation). The restart is the supervisor's `request_auto_restart`: the incarnation is torn
//! down gracefully (SIGTERM, then SIGKILL after the stop grace) and respawned under the same retry
//! budget; it is never counted as a crash and never revives a `Failed` node.
//!
//! The policy is pure: it reads time only from the `now_ms` it is given, so tests drive it on a set
//! clock. Overrides: `CITRATE_NODE_LIVENESS=off` (or `0`, `false`) turns the restart off;
//! `CITRATE_NODE_STALL_SECS` sets the stall window (at least [`MIN_STALL_AFTER`]).

use std::time::Duration;

use crate::supervisor::Clock;

/// Turns the liveness restart off when set to `off`, `0` or `false`.
pub const LIVENESS_ENV: &str = "CITRATE_NODE_LIVENESS";
/// Overrides the stall window, in seconds.
pub const STALL_SECS_ENV: &str = "CITRATE_NODE_STALL_SECS";

/// The default stall window: the head unchanged this long, with peers, while the network moves.
pub const DEFAULT_STALL_AFTER: Duration = Duration::from_secs(10 * 60);
/// The shortest stall window an override may set: anything shorter would restart a node that is
/// only slow to import a large batch.
pub const MIN_STALL_AFTER: Duration = Duration::from_secs(3 * 60);

/// When and how the node is restarted for a stall.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivenessPolicy {
    /// Whether a stall restarts the node at all (the watch still samples and notes when off).
    pub enabled: bool,
    /// How often the watch samples the node.
    pub check_every: Duration,
    /// How long the head must stand still, with peers, while the network is ahead.
    pub stall_after: Duration,
    /// The least time after a restart before the next one; doubles after each restart.
    pub first_gap: Duration,
    /// The cap on the gap between restarts.
    pub max_gap: Duration,
    /// Blocks within which the node counts as caught up to the network tip (the mining gate's
    /// margin, [`crate::node::is_caught_up`]).
    pub tip_margin: u64,
}

impl Default for LivenessPolicy {
    fn default() -> Self {
        LivenessPolicy {
            enabled: true,
            check_every: Duration::from_secs(60),
            stall_after: DEFAULT_STALL_AFTER,
            first_gap: Duration::from_secs(20 * 60),
            max_gap: Duration::from_secs(4 * 60 * 60),
            tip_margin: 32,
        }
    }
}

/// The default policy with the environment's overrides applied. A value that cannot be read, or a
/// stall window under [`MIN_STALL_AFTER`], leaves the default in place.
pub fn policy_from_env(get: impl Fn(&str) -> Option<String>) -> LivenessPolicy {
    let mut p = LivenessPolicy::default();
    if let Some(v) = get(LIVENESS_ENV) {
        if matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "false"
        ) {
            p.enabled = false;
        }
    }
    if let Some(secs) = get(STALL_SECS_ENV).and_then(|v| v.trim().parse::<u64>().ok()) {
        let d = Duration::from_secs(secs);
        if d >= MIN_STALL_AFTER {
            p.stall_after = d;
        }
    }
    p
}

/// One sample of the running node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    /// The run it was taken from: (supervisor instance, incarnation). A new pair is a new process.
    pub run: (u64, u64),
    /// The local head (`eth_blockNumber`), `None` when the RPC did not answer.
    pub head: Option<u64>,
    /// Connected peers (`net_peerCount`).
    pub peers: u64,
    /// The network tip from the public RPC, `None` when unknown.
    pub tip: Option<u64>,
}

/// Why the watch is not restarting the node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hold {
    /// The policy is off.
    Disabled,
    /// The node is not running under a supervisor (or is not `Alive`).
    NotRunning,
    /// The node's RPC did not answer.
    RpcUnavailable,
    /// This incarnation has not reached the network tip yet.
    InitialSync,
    /// No peers: the node cannot advance, which is not a wedge.
    NoPeers,
    /// The network tip is unknown.
    TipUnknown,
    /// The network is not ahead of the node (it is at the tip, or the network is halted).
    NetworkNotAhead,
    /// A restart would be due, but the gap since the last one has not passed.
    Backoff { until_ms: u64 },
}

/// What the watch concluded from one sample.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The head moved and the node has reached the tip in this incarnation.
    Healthy,
    /// Not a wedge, or not restartable now.
    Hold(Hold),
    /// The head has stood still since `since_ms` with peers while the network is ahead.
    Stalling { since_ms: u64, for_ms: u64 },
    /// Restart the node, for this reason.
    Restart { reason: String },
}

/// The watch's memory between samples.
#[derive(Debug, Clone)]
pub struct LivenessWatch {
    policy: LivenessPolicy,
    run: Option<(u64, u64)>,
    last_head: Option<u64>,
    caught_up_once: bool,
    stalled_since: Option<u64>,
    restarts: u32,
    last_restart_ms: Option<u64>,
}

impl LivenessWatch {
    pub fn new(policy: LivenessPolicy) -> Self {
        LivenessWatch {
            policy,
            run: None,
            last_head: None,
            caught_up_once: false,
            stalled_since: None,
            restarts: 0,
            last_restart_ms: None,
        }
    }

    /// Restarts in the current streak.
    pub fn restarts(&self) -> u32 {
        self.restarts
    }

    /// The earliest time the next restart is allowed, while a streak is open.
    pub fn next_allowed_ms(&self) -> Option<u64> {
        let last = self.last_restart_ms?;
        if self.restarts == 0 {
            return None;
        }
        let exp = self.restarts.saturating_sub(1).min(31);
        let gap = self
            .policy
            .first_gap
            .saturating_mul(1u32 << exp)
            .min(self.policy.max_gap);
        Some(last.saturating_add(u64::try_from(gap.as_millis()).unwrap_or(u64::MAX)))
    }

    /// Record a restart that the supervisor accepted at `now_ms`.
    pub fn restarted(&mut self, now_ms: u64) {
        self.restarts = self.restarts.saturating_add(1);
        self.last_restart_ms = Some(now_ms);
        self.stalled_since = None;
    }

    fn ms(d: Duration) -> u64 {
        u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
    }

    /// Judge one sample taken at `now_ms` (`None`: the node is not running).
    pub fn observe(&mut self, now_ms: u64, sample: Option<Sample>) -> Verdict {
        if !self.policy.enabled {
            return Verdict::Hold(Hold::Disabled);
        }
        let Some(s) = sample else {
            self.run = None;
            self.stalled_since = None;
            return Verdict::Hold(Hold::NotRunning);
        };
        if self.run != Some(s.run) {
            // A new process: its sync starts over.
            self.run = Some(s.run);
            self.last_head = None;
            self.caught_up_once = false;
            self.stalled_since = None;
        }
        let Some(head) = s.head else {
            self.stalled_since = None;
            return Verdict::Hold(Hold::RpcUnavailable);
        };
        let moved = self.last_head != Some(head);
        self.last_head = Some(head);
        let at_tip = s
            .tip
            .is_some_and(|tip| head.saturating_add(self.policy.tip_margin) >= tip);
        if at_tip {
            self.caught_up_once = true;
            // A node seen at the tip long after its last restart has recovered: the streak ends.
            if let Some(last) = self.last_restart_ms {
                if now_ms.saturating_sub(last) >= Self::ms(self.policy.max_gap) {
                    self.restarts = 0;
                    self.last_restart_ms = None;
                }
            }
        }
        if !self.caught_up_once {
            self.stalled_since = None;
            return Verdict::Hold(Hold::InitialSync);
        }
        if moved {
            self.stalled_since = None;
            return Verdict::Healthy;
        }
        if s.peers == 0 {
            self.stalled_since = None;
            return Verdict::Hold(Hold::NoPeers);
        }
        let Some(tip) = s.tip else {
            self.stalled_since = None;
            return Verdict::Hold(Hold::TipUnknown);
        };
        if at_tip {
            self.stalled_since = None;
            return Verdict::Hold(Hold::NetworkNotAhead);
        }
        let since = *self.stalled_since.get_or_insert(now_ms);
        let for_ms = now_ms.saturating_sub(since);
        if for_ms < Self::ms(self.policy.stall_after) {
            return Verdict::Stalling {
                since_ms: since,
                for_ms,
            };
        }
        if let Some(until_ms) = self.next_allowed_ms() {
            if now_ms < until_ms {
                return Verdict::Hold(Hold::Backoff { until_ms });
            }
        }
        Verdict::Restart {
            reason: format!(
                "head stuck at {head} for {}s with {} peers while the network tip is {tip}",
                for_ms / 1_000,
                s.peers
            ),
        }
    }
}

/// What the watch reads from and acts on: the node manager in production, a recording fake in
/// tests.
pub trait LivenessProbe {
    /// A sample of the running node, or `None` when it is not running (`Alive`).
    fn sample(&self) -> Option<Sample>;
    /// Ask the supervisor for an automated restart. `Err` carries why it was refused.
    fn restart(&self, reason: &str) -> std::result::Result<(), String>;
    /// Keep a line for the member (the node's note on Node › Services) and the app log.
    fn note(&self, text: String);
}

/// One turn of the watch: sample, judge, and restart when the verdict says so. Returns the verdict.
pub fn tick(watch: &mut LivenessWatch, clock: &dyn Clock, probe: &dyn LivenessProbe) -> Verdict {
    let now = clock.now_unix_ms();
    let verdict = watch.observe(now, probe.sample());
    if let Verdict::Restart { reason } = &verdict {
        match probe.restart(reason) {
            Ok(()) => {
                watch.restarted(now);
                probe.note(format!(
                    "liveness restart #{} at {now}: {reason}",
                    watch.restarts()
                ));
            }
            Err(e) => {
                probe.note(format!("liveness restart refused at {now}: {e} ({reason})"));
            }
        }
    }
    verdict
}

#[cfg(test)]
mod tests {
    include!("node_liveness_tests.rs");
}
