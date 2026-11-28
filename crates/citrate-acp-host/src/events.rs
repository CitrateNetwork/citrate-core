//! The per-session event log behind `agent_session_events` (L1-WP03 step 4): a bounded ring of
//! sequenced events with a blocking long-poll, the same contract as `hermes_session_events`.
//! `session/update` notifications become events in arrival order; the host adds `permission`,
//! `stopped`, `error` and `exited`.

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// Events kept per session; older ones fall off (a reader that far behind sees a gap in `seq`).
pub const EVENT_CAP: usize = 4096;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AgentEvent {
    pub seq: u64,
    pub kind: String,
    pub body: Value,
}

#[derive(Default)]
pub struct EventLog {
    inner: Mutex<(VecDeque<AgentEvent>, u64)>,
    cv: Condvar,
}

impl EventLog {
    pub fn push(&self, kind: &str, body: Value) -> u64 {
        let mut g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        g.1 += 1;
        let seq = g.1;
        if g.0.len() == EVENT_CAP {
            g.0.pop_front();
        }
        g.0.push_back(AgentEvent {
            seq,
            kind: kind.to_string(),
            body,
        });
        drop(g);
        self.cv.notify_all();
        seq
    }

    /// One `session/update` (its JSON form, tagged by `sessionUpdate`) as an event.
    pub fn push_update(&self, update: Value) -> u64 {
        let tag = update["sessionUpdate"].as_str().unwrap_or("").to_string();
        let (kind, role) = match tag.as_str() {
            "agent_message_chunk" => ("message_chunk", Some("agent")),
            "user_message_chunk" => ("message_chunk", Some("user")),
            "agent_thought_chunk" => ("thought_chunk", None),
            "tool_call" => ("tool_call", None),
            "tool_call_update" => ("tool_call_update", None),
            "plan" => ("plan", None),
            "current_mode_update" => ("mode", None),
            "available_commands_update" => ("commands", None),
            _ => ("update", None),
        };
        let mut body = update;
        if let (Some(r), Some(m)) = (role, body.as_object_mut()) {
            m.insert("role".into(), json!(r));
        }
        self.push(kind, body)
    }

    /// The events after `after` (and the newest seq). Waits up to `wait` when there are none yet.
    pub fn after(&self, after: u64, wait: Duration) -> (Vec<AgentEvent>, u64) {
        let end = Instant::now() + wait;
        let mut g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        while g.1 <= after {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            g = self
                .cv
                .wait_timeout(g, left)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
        let evs = g.0.iter().filter(|e| e.seq > after).cloned().collect();
        (evs, g.1)
    }
}
