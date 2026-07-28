//! The 40204 address book — ONE source for every pin in this app.
//!
//! # Why this module exists
//!
//! The same contract addresses were hardcoded in two places here
//! (`grant_status.rs` and `node.rs`), and in three more outside this repo (the
//! droplet signer env, Vercel, core-membership's constants). A reroll moves them,
//! and on 2026-07-28 a stale `CitrateMemberSBT` pin cost most of a day: the address
//! had no code, so the grant's uniqueness probe threw before the treasury signer
//! was ever reached, and every paid order silently bounced back to `paid`.
//!
//! The book here is GENERATED from `citrate-chain/contracts/addresses/40204.json`
//! by `scripts/sync-addresses.py`. It is embedded at compile time, so a packaged
//! desktop build carries the pins it was built with — no network read, no drift
//! between what the app shows and what it was shipped with.
//!
//! **Do not hand-edit `addresses/40204.json`.** Re-run the sync script; the
//! `generated_book_is_not_hand_edited` test below is the tripwire.

use std::sync::OnceLock;

/// The generated book, embedded at compile time.
const BOOK_JSON: &str = include_str!("../addresses/40204.json");

#[derive(serde::Deserialize)]
struct Book {
    #[serde(rename = "chainId")]
    chain_id: u64,
    addresses: Addresses,
}

#[derive(serde::Deserialize)]
struct Addresses {
    #[serde(rename = "CitrateMemberSBT")]
    citrate_member_sbt: String,
    #[serde(rename = "MembershipStakeVault")]
    membership_stake_vault: String,
    #[serde(rename = "ValidatorRegistry")]
    validator_registry: String,
    #[serde(rename = "CitrateWalletFactory")]
    citrate_wallet_factory: String,
    #[serde(rename = "LiquidStakingPool")]
    liquid_staking_pool: String,
}

fn book() -> &'static Book {
    static BOOK: OnceLock<Book> = OnceLock::new();
    BOOK.get_or_init(|| {
        // Addresses are LOWERCASED on load. The canonical book stores the EIP-55
        // checksummed form (right for humans and for eyeballing a diff), but every
        // consumer here compares and emits lowercase — `eth_call` `to` fields, test
        // assertions, the node env. Normalising once here means the two
        // representations can never drift into a false mismatch.
        // A malformed embedded book is a BUILD defect, not a runtime condition —
        // the file is generated and compiled in. Panicking here fails the app
        // loudly at first use rather than serving a wrong address, which is the
        // failure this module exists to prevent.
        let mut b: Book =
            serde_json::from_str(BOOK_JSON).expect("embedded 40204 address book is malformed");
        b.addresses.citrate_member_sbt = b.addresses.citrate_member_sbt.to_ascii_lowercase();
        b.addresses.membership_stake_vault = b.addresses.membership_stake_vault.to_ascii_lowercase();
        b.addresses.validator_registry = b.addresses.validator_registry.to_ascii_lowercase();
        b.addresses.citrate_wallet_factory = b.addresses.citrate_wallet_factory.to_ascii_lowercase();
        b.addresses.liquid_staking_pool = b.addresses.liquid_staking_pool.to_ascii_lowercase();
        b
    })
}

/// The chain id the book was generated for.
pub fn chain_id() -> u64 {
    book().chain_id
}

/// `CitrateMemberSBT` — the soulbound membership token.
pub fn citrate_member_sbt() -> &'static str {
    &book().addresses.citrate_member_sbt
}

/// `MembershipStakeVault` — the pre-bond-fund grant target. Still read so a member
/// granted under the old model reports honestly.
pub fn membership_stake_vault() -> &'static str {
    &book().addresses.membership_stake_vault
}

/// `ValidatorRegistry` — where a member's bonded stake lives under bond-fund.
pub fn validator_registry() -> &'static str {
    &book().addresses.validator_registry
}

/// `CitrateWalletFactory` — the AA smart-wallet factory.
#[allow(dead_code)]
pub fn citrate_wallet_factory() -> &'static str {
    &book().addresses.citrate_wallet_factory
}

/// `LiquidStakingPool` — self-stake / withdrawal queue.
#[allow(dead_code)]
pub fn liquid_staking_pool() -> &'static str {
    &book().addresses.liquid_staking_pool
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_address(s: &str) -> bool {
        s.len() == 42
            && s.starts_with("0x")
            && s[2..].chars().all(|c| c.is_ascii_hexdigit())
    }

    #[test]
    fn every_pin_is_a_well_formed_address_and_none_is_zero() {
        for (name, addr) in [
            ("CitrateMemberSBT", citrate_member_sbt()),
            ("MembershipStakeVault", membership_stake_vault()),
            ("ValidatorRegistry", validator_registry()),
            ("CitrateWalletFactory", citrate_wallet_factory()),
            ("LiquidStakingPool", liquid_staking_pool()),
        ] {
            assert!(is_address(addr), "{name} is not a 20-byte 0x address: {addr}");
            assert_ne!(
                addr.to_ascii_lowercase(),
                format!("0x{}", "0".repeat(40)),
                "{name} is the zero address — a book that pins zero is worse than none"
            );
        }
    }

    /// Consumers compare and emit lowercase; the canonical book is checksummed.
    /// Normalising on load is what keeps those two from drifting into a false
    /// mismatch, so pin it.
    #[test]
    fn accessors_return_lowercase() {
        for addr in [
            citrate_member_sbt(),
            membership_stake_vault(),
            validator_registry(),
            citrate_wallet_factory(),
            liquid_staking_pool(),
        ] {
            assert_eq!(addr, &addr.to_ascii_lowercase(), "accessors must return lowercase");
        }
    }

    #[test]
    fn the_book_is_for_40204() {
        assert_eq!(chain_id(), 40204);
    }

    /// The book is GENERATED. If someone hand-edits it, the marker goes away and
    /// the next `sync-addresses.py` run silently overwrites their change — so fail
    /// here instead, while the edit is still in front of them.
    #[test]
    fn generated_book_is_not_hand_edited() {
        assert!(
            BOOK_JSON.contains("GENERATED by scripts/sync-addresses.py"),
            "addresses/40204.json lost its generated marker — re-run the sync script \
             rather than editing it by hand"
        );
    }

    /// No duplicate pins: two different contracts sharing an address means the book
    /// was generated from a partial/garbled source.
    #[test]
    fn pins_are_distinct() {
        let all = [
            citrate_member_sbt(),
            membership_stake_vault(),
            validator_registry(),
            citrate_wallet_factory(),
            liquid_staking_pool(),
        ];
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                assert_ne!(
                    all[i].to_ascii_lowercase(),
                    all[j].to_ascii_lowercase(),
                    "two pins share an address — the book is garbled"
                );
            }
        }
    }
}
