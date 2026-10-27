//! C1-V3: what an agent or an MCP client is told about a payment that was submitted but whose
//! outcome is unknown. (Red stub: nothing is recognised yet.)

pub fn is_tx_hash(_s: &str) -> bool {
    false
}

pub fn submitted_tx_hash(_err: &str) -> Option<String> {
    None
}

pub fn agent_message(_tx_hash: Option<&str>) -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    include!("broadcast_outcome_tests.rs");
}
