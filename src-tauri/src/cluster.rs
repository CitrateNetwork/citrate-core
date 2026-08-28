//! CX-S4 (lane s4) — group private P2P cluster host commands (C-20).
//!
//! ## S4.1 — roster → allowed-peers derivation (the RBAC→network boundary)
//! A group's cluster is a private P2P mesh among its members. The authorization boundary is the
//! group roster: the cluster admits a peer connection ONLY from an address in the roster. [`allowed_peers`]
//! is that derivation — canonical, de-duplicated, stable across nodes — and is the set the hybrid
//! Noise identities are minted from in S4.2 (D-24: Noise-identity + libp2p-gossipsub). S4.1 ships +
//! TESTS the derivation; the live transport (dialing, gossipsub, connectivity, shared files) is S4.2,
//! so the `cluster_*` commands below stay honest `not wired` until then (Rule 1 — never fake a peer).
//!
//! The S4.1 cluster VIEW (who your peers WOULD be) is composed on the frontend from the existing
//! `groups_roster` command; this module owns the canonical authorization algorithm the transport
//! will enforce. Names frozen; registered in lib.rs.

/// Derive the cluster's allowed-peer set from a group roster (CX-S4.1). The cluster admits P2P
/// connections ONLY from addresses in this set — the RBAC boundary the S4.2 transport enforces when
/// minting per-peer Noise identities. Canonicalizes each address (strip `0x`, lowercase), drops
/// anything that is not a 20-byte hex address, de-duplicates, and sorts — so two nodes computing the
/// set from the same roster get byte-identical results (a stable mesh membership).
// Proven by cluster_tests in S4.1; its live consumer (per-peer Noise identity minting + gossipsub
// admission) lands in S4.2, so it reads as unused for exactly one WP (the S3.1 primitive pattern).
#[allow(dead_code)]
pub(crate) fn allowed_peers(roster: &[String]) -> Vec<String> {
    let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for raw in roster {
        if let Some(addr) = canonical_address(raw) {
            set.insert(addr);
        }
    }
    set.into_iter().collect()
}

/// Canonical form of an EVM address for the peer set: lowercase, no `0x`, exactly 40 hex chars.
/// Returns `None` for anything that is not a well-formed 20-byte address (dropped from the set).
fn canonical_address(raw: &str) -> Option<String> {
    let h = raw.trim().trim_start_matches("0x").to_ascii_lowercase();
    if h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(h)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// CX-S4.2 — admission gate + membership lifecycle (the RBAC→network safety core).
//
// S4.1 answered "who COULD be a peer" (the roster). S4.2 answers "who IS in the mesh, and can we
// prove no unauthorized peer ever is". Admission is role-gated (D-24: role >= Member — a guest is in
// the group's conversation but NOT its compute/file mesh), and membership is reconciled against the
// live roster so an offboard/role-drop EVICTS the peer in the same step (the offboard safety
// property). The libp2p wire that dials/gossips over this membership is S4.3 (a sidecar reusing
// citrate-compute-pool/training-worker/libp2p_transport.rs — src-tauri stays lean, no libp2p link,
// mirroring the comms member-daemon). Formal model: src-tauri/formal/ClusterAdmission.tla.
// ---------------------------------------------------------------------------

/// The minimum group role admitted to a cluster (D-24). Guests/agents below this are group members
/// but not mesh peers. Unknown roles rank lowest (fail closed).
const MIN_CLUSTER_RANK: u8 = 2; // Member

/// Rank the group role vocabulary (matches comms Role) so admission can compare by threshold.
fn role_rank(role: &str) -> u8 {
    match role {
        "owner" => 5,
        "admin" => 4,
        "partner" => 3,
        "member" => 2,
        "agent" => 1,
        "guest" => 0,
        _ => 0, // unknown → lowest (fail closed — never admit on an unrecognized role)
    }
}

/// The role-gated allowed set from a (address, role) roster: canonical addresses whose role is
/// >= Member. This is the S4.2 refinement of S4.1's `allowed_peers` (which is address-only).
fn allowed_set(roster: &[(String, String)]) -> std::collections::BTreeSet<String> {
    let mut set = std::collections::BTreeSet::new();
    for (addr, role) in roster {
        if role_rank(role) >= MIN_CLUSTER_RANK {
            if let Some(a) = canonical_address(addr) {
                set.insert(a);
            }
        }
    }
    set
}

/// Admission policy: a candidate is admitted IFF its (canonical) address is in the role-gated
/// allowed set. The transport enforces this before dialing / accepting a topic subscription.
#[allow(dead_code)] // consumed by the S4.3 transport; exercised by cluster_tests
pub(crate) fn admit(address: &str, allowed: &std::collections::BTreeSet<String>) -> bool {
    canonical_address(address)
        .map(|a| allowed.contains(&a))
        .unwrap_or(false)
}

/// The cluster's live membership: the role-gated allowed set (from the roster) and the set of
/// currently-admitted peers. INVARIANT (formal: ClusterAdmission): `admitted ⊆ allowed` at all times
/// — no unauthorized peer is ever in the mesh, and an offboard/role-drop evicts in the same step.
#[allow(dead_code)] // the state machine the S4.3 libp2p transport drives; proven by cluster_tests
pub(crate) struct ClusterMembership {
    allowed: std::collections::BTreeSet<String>,
    admitted: std::collections::BTreeSet<String>,
}

#[allow(dead_code)]
impl ClusterMembership {
    /// Open a membership over a group roster (address, role). No peer is admitted until it joins.
    pub fn new(roster: &[(String, String)]) -> Self {
        ClusterMembership {
            allowed: allowed_set(roster),
            admitted: std::collections::BTreeSet::new(),
        }
    }

    /// A candidate presents itself (a valid, owner-signed RoleAssertion for this group was verified
    /// upstream by the comms layer → we get its (address, role) here). Admit IFF the policy passes;
    /// returns whether it was admitted. Idempotent.
    pub fn join(&mut self, address: &str, role: &str) -> bool {
        if role_rank(role) < MIN_CLUSTER_RANK {
            return false;
        }
        match canonical_address(address) {
            Some(a) if self.allowed.contains(&a) => {
                self.admitted.insert(a);
                true
            }
            _ => false,
        }
    }

    /// A peer leaves (voluntarily or dropped). Idempotent.
    pub fn leave(&mut self, address: &str) {
        if let Some(a) = canonical_address(address) {
            self.admitted.remove(&a);
        }
    }

    /// Reconcile against a new roster (offboard / role change): recompute the allowed set and EVICT
    /// any admitted peer no longer allowed (the offboard safety property — one step, no window where
    /// a removed member is still meshed). Returns the evicted peers (the transport disconnects them).
    pub fn reconcile(&mut self, roster: &[(String, String)]) -> Vec<String> {
        self.allowed = allowed_set(roster);
        let evicted: Vec<String> = self
            .admitted
            .iter()
            .filter(|a| !self.allowed.contains(*a))
            .cloned()
            .collect();
        for e in &evicted {
            self.admitted.remove(e);
        }
        evicted
    }

    /// The currently-admitted peers (canonical addresses), sorted.
    pub fn admitted(&self) -> Vec<String> {
        self.admitted.iter().cloned().collect()
    }

    /// Whether `address` is currently admitted.
    pub fn is_admitted(&self, address: &str) -> bool {
        canonical_address(address)
            .map(|a| self.admitted.contains(&a))
            .unwrap_or(false)
    }

    /// The safety invariant, checkable at runtime + asserted in tests: admitted ⊆ allowed.
    pub fn invariant_holds(&self) -> bool {
        self.admitted.is_subset(&self.allowed)
    }
}

fn not_wired(cmd: &str) -> Result<(), String> {
    Err(format!("cluster::{cmd} is not wired yet (CX-S4.3 libp2p transport)"))
}

#[tauri::command]
pub fn cluster_status() -> Result<(), String> {
    not_wired("status")
}

#[tauri::command]
pub fn cluster_join() -> Result<(), String> {
    not_wired("join")
}

#[tauri::command]
pub fn cluster_peers() -> Result<(), String> {
    not_wired("peers")
}

#[tauri::command]
pub fn cluster_share_file() -> Result<(), String> {
    not_wired("share_file")
}

#[tauri::command]
pub fn cluster_leave() -> Result<(), String> {
    not_wired("leave")
}

#[cfg(test)]
mod tests {
    include!("cluster_tests.rs");
}
