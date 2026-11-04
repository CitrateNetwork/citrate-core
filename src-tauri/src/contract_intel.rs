//! CORE-A1 / CORE-A2 (r1007 planset 03 CORE-A): red. The tests below describe the read-only
//! contract tools (address book lookup, get_code, identify, ABI resolution, signature subset,
//! tx fetch); the implementation lands in the next commit.

#[cfg(test)]
mod tests {
    include!("contract_intel_tests.rs");
}
