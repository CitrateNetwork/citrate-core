// CX-S4.1 — cluster allowed-peers derivation tests. Included into `cluster::tests`.
//
// The derivation is the RBAC→network boundary + the input to S4.2 Noise-identity minting, so it must
// be canonical (case-insensitive), de-duplicated, stably ordered, and reject non-addresses.

use super::*;

const A: &str = "00000000000000000000000000000000000000aa";
const B: &str = "00000000000000000000000000000000000000bb";

#[test]
fn empty_roster_yields_no_peers() {
    assert!(allowed_peers(&[]).is_empty());
}

#[test]
fn addresses_are_canonicalized_lowercase_no_prefix() {
    // Mixed 0x prefix + upper/lower — all collapse to the one canonical form.
    let roster = vec![format!("0x{}", A.to_uppercase()), A.to_string()];
    assert_eq!(allowed_peers(&roster), vec![A.to_string()]);
}

#[test]
fn duplicates_are_removed_and_the_set_is_sorted() {
    let roster = vec![B.to_string(), A.to_string(), B.to_string(), format!("0x{A}")];
    // Sorted (A before B), each once.
    assert_eq!(allowed_peers(&roster), vec![A.to_string(), B.to_string()]);
}

#[test]
fn non_addresses_are_dropped_not_admitted() {
    let roster = vec![
        A.to_string(),
        "not-an-address".to_string(),
        "0x1234".to_string(),          // too short
        "".to_string(),
        format!("{A}zz"),               // non-hex tail / too long
    ];
    assert_eq!(allowed_peers(&roster), vec![A.to_string()]);
}

#[test]
fn two_nodes_derive_the_identical_set_regardless_of_input_order() {
    let node1 = allowed_peers(&[format!("0x{A}"), B.to_string()]);
    let node2 = allowed_peers(&[B.to_uppercase(), A.to_string()]);
    assert_eq!(node1, node2, "the mesh membership must be byte-identical across nodes");
}

// ---- CX-S4.2: admission gate + membership lifecycle ----

const C: &str = "00000000000000000000000000000000000000cc";

fn roster(entries: &[(&str, &str)]) -> Vec<(String, String)> {
    entries.iter().map(|(a, r)| (a.to_string(), r.to_string())).collect()
}

#[test]
fn allowed_set_is_role_gated_at_member() {
    // owner/admin/member admitted; guest/agent below Member excluded.
    let r = roster(&[(A, "owner"), (B, "member"), (C, "guest")]);
    let allowed = allowed_set(&r);
    assert!(allowed.contains(A) && allowed.contains(B));
    assert!(!allowed.contains(C), "a guest is in the group but not the mesh");
}

#[test]
fn join_admits_an_allowed_member_and_rejects_guest_and_stranger() {
    let mut m = ClusterMembership::new(&roster(&[(A, "member"), (C, "guest")]));
    assert!(m.join(A, "member"), "an allowed member is admitted");
    assert!(!m.join(C, "guest"), "a guest is rejected (role < Member)");
    assert!(!m.join(B, "member"), "a stranger not in the roster is rejected");
    assert!(m.is_admitted(A) && !m.is_admitted(C) && !m.is_admitted(B));
    assert!(m.invariant_holds());
}

#[test]
fn a_role_assertion_below_member_is_never_admitted_even_if_in_roster() {
    // The address is in the roster but presents a guest role → the mesh must refuse.
    let mut m = ClusterMembership::new(&roster(&[(A, "owner")]));
    assert!(!m.join(A, "guest"), "presented role gates admission, fail closed");
    assert!(m.admitted().is_empty());
}

#[test]
fn leave_removes_and_is_idempotent() {
    let mut m = ClusterMembership::new(&roster(&[(A, "member")]));
    assert!(m.join(A, "member"));
    m.leave(A);
    m.leave(A); // idempotent
    assert!(!m.is_admitted(A) && m.admitted().is_empty());
}

#[test]
fn reconcile_evicts_an_offboarded_member_in_one_step() {
    // The offboard safety property: a removed member is evicted from the mesh with no window.
    let mut m = ClusterMembership::new(&roster(&[(A, "member"), (B, "member")]));
    assert!(m.join(A, "member") && m.join(B, "member"));
    let evicted = m.reconcile(&roster(&[(A, "member")])); // B offboarded
    assert_eq!(evicted, vec![B.to_string()]);
    assert!(m.is_admitted(A) && !m.is_admitted(B));
    assert!(m.invariant_holds(), "admitted ⊆ allowed after eviction");
}

#[test]
fn reconcile_evicts_on_a_role_drop_below_member() {
    let mut m = ClusterMembership::new(&roster(&[(A, "admin")]));
    assert!(m.join(A, "admin"));
    let evicted = m.reconcile(&roster(&[(A, "guest")])); // demoted below Member
    assert_eq!(evicted, vec![A.to_string()]);
    assert!(!m.is_admitted(A) && m.invariant_holds());
}

#[test]
fn the_invariant_holds_across_a_join_reconcile_rejoin_sequence() {
    let mut m = ClusterMembership::new(&roster(&[(A, "member"), (B, "member")]));
    m.join(A, "member");
    m.join(B, "member");
    assert!(m.invariant_holds());
    m.reconcile(&roster(&[(A, "member")])); // B out
    assert!(m.invariant_holds());
    assert!(!m.join(B, "member"), "B no longer allowed cannot rejoin");
    m.reconcile(&roster(&[(A, "member"), (B, "member")])); // B back in roster
    assert!(m.join(B, "member"), "B re-added can rejoin");
    assert!(m.invariant_holds());
}

// ---- CX-S4.3 (lean): the transport seam + membership-driven connection lifecycle ----

/// An in-process transport double: records who is "connected" so we can assert the wire tracks the
/// admitted set. Stands in for the S4.3 libp2p sidecar (which opens real Noise/gossipsub sessions).
struct FakeTransport {
    connected: std::collections::BTreeSet<String>,
}
impl FakeTransport {
    fn new() -> Self {
        FakeTransport { connected: std::collections::BTreeSet::new() }
    }
}
impl ClusterTransport for FakeTransport {
    fn dial(&mut self, peer: &str) {
        self.connected.insert(peer.to_string());
    }
    fn disconnect(&mut self, peer: &str) {
        self.connected.remove(peer);
    }
    fn connected(&self) -> Vec<String> {
        self.connected.iter().cloned().collect()
    }
}

#[test]
fn admitting_a_peer_dials_it_rejecting_does_not() {
    let mut s = ClusterSession::new(&roster(&[(A, "member"), (C, "guest")]), FakeTransport::new());
    assert!(s.join(A, "member"));
    assert!(!s.join(C, "guest"));       // guest rejected
    assert!(!s.join(B, "member"));      // stranger rejected
    assert_eq!(s.transport.connected(), vec![A.to_string()], "only the admitted peer is dialed");
    assert!(s.wire_tracks_admitted());
}

#[test]
fn leaving_disconnects_the_wire() {
    let mut s = ClusterSession::new(&roster(&[(A, "member")]), FakeTransport::new());
    s.join(A, "member");
    s.leave(A);
    assert!(s.transport.connected().is_empty());
    assert!(s.wire_tracks_admitted());
}

#[test]
fn offboard_reconcile_tears_down_the_evicted_peers_wire_in_one_step() {
    let mut s = ClusterSession::new(&roster(&[(A, "member"), (B, "member")]), FakeTransport::new());
    s.join(A, "member");
    s.join(B, "member");
    assert_eq!(s.transport.connected().len(), 2);
    let evicted = s.reconcile(&roster(&[(A, "member")])); // B offboarded
    assert_eq!(evicted, vec![B.to_string()]);
    assert_eq!(s.transport.connected(), vec![A.to_string()], "B's wire is torn down, A stays");
    assert!(s.wire_tracks_admitted(), "the wire never outruns the RBAC gate");
}

#[test]
fn the_wire_never_contains_an_unadmitted_peer_across_a_sequence() {
    let mut s = ClusterSession::new(&roster(&[(A, "admin"), (B, "member")]), FakeTransport::new());
    s.join(A, "admin");
    s.join(B, "member");
    s.reconcile(&roster(&[(A, "guest")])); // A demoted below Member -> evicted; B no longer in roster -> evicted
    assert!(s.transport.connected().is_empty(), "both evicted, wire empty");
    assert!(s.wire_tracks_admitted());
    assert!(s.membership().admitted().is_empty());
}
