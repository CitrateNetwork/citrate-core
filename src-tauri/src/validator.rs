//! Proposer identity (W1.1).
//!
//! A Citrate node's block-signing (proposer) key is a deterministic ed25519 key
//! derived from its 20-byte coinbase (staker/reward) address. To read
//! `ValidatorRegistry.validatorInfo(pubkey)` and drive registration, the app must
//! derive the SAME public value the node signs blocks with.
//!
//! This mirrors the ONE canonical derivation in citrate-chain
//! `core/consensus/src/crypto.rs::derive_block_signing_key`:
//!
//! ```text
//! coinbase32      = coinbase20 ‖ [0u8; 12]                        (right zero-pad)
//! proposer_seed   = Sha3_256(b"citrate-block-signing-key-v1" ‖ coinbase32)
//! proposer_priv   = Ed25519SigningKey::from_bytes(proposer_seed)
//! proposer_pubkey = proposer_priv.verifying_key().to_bytes()      (the REGISTERED value)
//! ```
//!
//! DRIFT TRIPWIRE: `proposer_pubkey_matches_canonical_vectors` pins two golden
//! vectors generated from the canonical crate. If citrate-chain ever changes the
//! domain string, the padding, or the algorithm, this test fails and forces a
//! conscious re-sync rather than silent divergence between the app and the node.

use ed25519_dalek::SigningKey;
use sha3::{Digest as _, Sha3_256};

/// Canonical domain separator — byte-for-byte with `BLOCK_SIGNING_KEY_DOMAIN`.
const BLOCK_SIGNING_KEY_DOMAIN: &[u8] = b"citrate-block-signing-key-v1";

/// Right-zero-pad a 20-byte coinbase address to the 32-byte buffer the block
/// producer hashes (the shape `derive_block_signing_key` expects).
fn coinbase32(coinbase20: &[u8; 20]) -> [u8; 32] {
    let mut buf = [0u8; 32];
    buf[..20].copy_from_slice(coinbase20);
    buf
}

/// Derive the node's ed25519 proposer public key (the value registered in
/// `ValidatorRegistry` and used to sign proposed blocks) from its 20-byte
/// coinbase address.
pub fn derive_proposer_pubkey(coinbase20: &[u8; 20]) -> [u8; 32] {
    let mut hasher = Sha3_256::new();
    hasher.update(BLOCK_SIGNING_KEY_DOMAIN);
    hasher.update(coinbase32(coinbase20));
    let seed = hasher.finalize();
    let mut seed_bytes = [0u8; 32];
    seed_bytes.copy_from_slice(&seed);
    let signing_key = SigningKey::from_bytes(&seed_bytes);
    signing_key.verifying_key().to_bytes()
}

/// Parse a `0x`-prefixed 20-byte address into raw bytes.
pub fn parse_address_20(addr: &str) -> Result<[u8; 20], String> {
    let raw = addr.strip_prefix("0x").unwrap_or(addr);
    let bytes = hex::decode(raw).map_err(|e| format!("bad coinbase hex: {e}"))?;
    if bytes.len() != 20 {
        return Err(format!(
            "coinbase must be 20 bytes, got {} ({addr})",
            bytes.len()
        ));
    }
    let mut a = [0u8; 20];
    a.copy_from_slice(&bytes);
    Ok(a)
}

/// Convenience: coinbase address string (`0x…`) → `0x`-prefixed proposer pubkey hex.
/// This is the value the app shows as the node's on-chain proposer identity and
/// keys the `validatorInfo` read.
pub fn proposer_pubkey_hex(coinbase_addr: &str) -> Result<String, String> {
    let coinbase = parse_address_20(coinbase_addr)?;
    Ok(format!("0x{}", hex::encode(derive_proposer_pubkey(&coinbase))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(hex_str: &str) -> [u8; 20] {
        parse_address_20(hex_str).expect("valid 20-byte address")
    }

    /// Golden vectors generated from the canonical citrate-chain crate
    /// `core/consensus/src/crypto.rs::derive_block_signing_key`. If these break,
    /// the app's derivation has drifted from the node's — DO NOT "fix" by editing
    /// the expected values; re-verify against the canonical source first.
    #[test]
    fn proposer_pubkey_matches_canonical_vectors() {
        let cases = [
            (
                "0x0000000000000000000000000000000000000001",
                "30e8b8269043376ffe9ca86ec95d344d61a1b90cce11386b870f46fc821636d5",
            ),
            (
                "0xd12c00c377eb4615a7ae934df509c903a29ecb6c",
                "818ff2ad161b1f822394873e1a98e4c8135f56733ae1a28326dfcf1f1b9aba48",
            ),
        ];
        for (coinbase, want) in cases {
            let got = hex::encode(derive_proposer_pubkey(&addr(coinbase)));
            assert_eq!(got, want, "proposer pubkey for coinbase {coinbase}");
        }
    }

    #[test]
    fn coinbase32_right_zero_pads() {
        let buf = coinbase32(&addr("0x0000000000000000000000000000000000000001"));
        assert_eq!(buf[19], 1, "last address byte preserved");
        assert_eq!(&buf[20..], &[0u8; 12], "12 trailing zero bytes");
    }

    #[test]
    fn derivation_is_deterministic() {
        let a = addr("0xd12c00c377eb4615a7ae934df509c903a29ecb6c");
        assert_eq!(derive_proposer_pubkey(&a), derive_proposer_pubkey(&a));
    }

    #[test]
    fn proposer_pubkey_hex_round_trips_through_the_string_api() {
        let got = proposer_pubkey_hex("0xd12c00c377eb4615a7ae934df509c903a29ecb6c").unwrap();
        assert_eq!(
            got,
            "0x818ff2ad161b1f822394873e1a98e4c8135f56733ae1a28326dfcf1f1b9aba48"
        );
    }

    #[test]
    fn rejects_wrong_length_coinbase() {
        assert!(parse_address_20("0x1234").is_err());
        assert!(proposer_pubkey_hex("0xnothex").is_err());
    }
}
