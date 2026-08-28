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
