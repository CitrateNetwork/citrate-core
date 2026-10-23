//! C1-V3: what an agent or an MCP client is told about a payment that was submitted but whose
//! outcome is unknown.
//!
//! Data source (Rule 7): the signature ceremony's typed error
//! (`CeremonyError::SubmittedOutcomeUnknown`, C1-V #347), which reaches core as the string
//! `"ceremony: submitted, outcome unknown (tx <hash>): <reason>"`. Every failure established before
//! the node could have received the transaction is prefixed `"not submitted: "` instead, and stays
//! a failure. An unknown outcome is never "rejected" (the member approved it) and never "failed"
//! (it may be in the mempool), so the caller is told to check the transaction before retrying.

/// The marker the ceremony puts before the hash of a submitted transaction with no known outcome.
const MARKER: &str = "submitted, outcome unknown (tx ";

/// True for `0x` followed by exactly 64 hex digits.
pub fn is_tx_hash(s: &str) -> bool {
    s.strip_prefix("0x")
        .is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The hash of a submitted transaction whose outcome is unknown, read from the ceremony's error;
/// `None` for any other error (including a malformed hash, which is not trusted as one).
pub fn submitted_tx_hash(err: &str) -> Option<String> {
    let rest = &err[err.find(MARKER)? + MARKER.len()..];
    let hash = &rest[..rest.find(')')?];
    is_tx_hash(hash).then(|| hash.to_string())
}

/// The words an agent or MCP client gets for an unknown outcome: submitted, with the hash when
/// known, and to check before retrying.
pub fn agent_message(tx_hash: Option<&str>) -> String {
    match tx_hash {
        Some(h) => format!("submitted, outcome unknown, tx {h}, check before retrying"),
        None => "submitted, outcome unknown, check recent transactions before retrying".to_string(),
    }
}

#[cfg(test)]
mod tests {
    include!("broadcast_outcome_tests.rs");
}
