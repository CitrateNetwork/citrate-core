// Tests for the C1-V3 outcome-unknown wording. Included into `broadcast_outcome::tests`.

use super::*;

const HASH: &str = "0xabababababababababababababababababababababababababababababababab";

#[test]
fn the_ceremony_error_carries_the_hash() {
    let e = format!("ceremony: submitted, outcome unknown (tx {HASH}): rpc transport error: down");
    assert_eq!(submitted_tx_hash(&e), Some(HASH.to_string()));
}

#[test]
fn a_failure_before_submission_is_not_an_unknown_outcome() {
    assert_eq!(submitted_tx_hash("not submitted: the vault is locked"), None);
    assert_eq!(submitted_tx_hash("kubo is not running"), None);
    // A malformed hash is not trusted as one.
    assert_eq!(
        submitted_tx_hash("ceremony: submitted, outcome unknown (tx 0x1234): down"),
        None
    );
}

#[test]
fn the_agent_message_never_says_rejected() {
    let with = agent_message(Some(HASH));
    assert_eq!(
        with,
        format!("submitted, outcome unknown, tx {HASH}, check before retrying")
    );
    let without = agent_message(None);
    assert_eq!(
        without,
        "submitted, outcome unknown, check recent transactions before retrying"
    );
    for m in [with, without] {
        assert!(!m.contains("rejected") && !m.contains("failed"));
    }
}

#[test]
fn tx_hashes_are_0x_and_64_hex_digits() {
    assert!(is_tx_hash(HASH));
    assert!(!is_tx_hash("0x"));
    assert!(!is_tx_hash(&HASH[2..]));
    assert!(!is_tx_hash(&format!("{HASH}00")));
    assert!(!is_tx_hash(&HASH.replace('a', "g")));
}

// Contract v1.1 §1 (#347): an ambiguous node error ("already known", DuplicateNonce, -32603) or
// any error on a re-send now reaches core as an unknown outcome with the hash, so an agent payment
// is reported as submitted; only a definitive first-send refusal stays a failure.
#[test]
fn ambiguous_send_errors_reach_the_agent_as_an_unknown_outcome() {
    let h = format!("0x{}", "ab".repeat(32));
    for reason in [
        "rpc node error: already known",
        "rpc node error: Failed to submit transaction: DuplicateNonce { nonce: 0 }",
        "rpc node error: Failed to submit transaction: NonceTooLow { expected: 3, got: 0 }",
    ] {
        let err = format!("ceremony: submitted, outcome unknown (tx {h}): {reason}");
        assert_eq!(submitted_tx_hash(&err).as_deref(), Some(h.as_str()), "{reason}");
    }
    let refused = "not submitted: ceremony: broadcast failed: rpc node error: Failed to submit transaction: Full";
    assert_eq!(submitted_tx_hash(refused), None);
}
