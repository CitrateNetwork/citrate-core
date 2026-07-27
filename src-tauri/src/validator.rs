//! Proposer identity (W1.1 — WP-11-aligned).
//!
//! A Citrate node's block-signing (proposer) key is a PERSISTED SECRET: a 32-byte
//! ed25519 seed the node mints on first start at `<data_dir>/proposer.key` (0600,
//! preserved across wipes — see citrate-chain `node/src/main.rs
//! ::load_or_generate_proposer_key`, WP-11 / PR #114). It is NO LONGER derived from
//! the public coinbase — that was a security hole (anyone could reconstruct a
//! node's signing key from its on-chain coinbase and equivocate). So the app READS
//! the node's real proposer pubkey from that seed file; it does NOT compute it from
//! the coinbase.
//!
//! The pubkey is the value registered in `ValidatorRegistry` and the key for
//! `validatorInfo(pubkey)` / reward reads. It exists only after the node has
//! started once (mint-on-first-run), so reads before then honestly error.

use ed25519_dalek::SigningKey;
use zeroize::Zeroize as _;

/// The node's proposer-key file, relative to its data dir (citrate-chain WP-11).
pub const PROPOSER_KEY_FILE: &str = "proposer.key";

/// Derive the ed25519 proposer public key (32 bytes) from a node's 32-byte
/// `proposer.key` seed — exactly what the node registers + signs blocks with.
pub fn proposer_pubkey_from_seed(seed: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(seed).verifying_key().to_bytes()
}

/// Read `<data_dir>/proposer.key` (the node's minted 32-byte ed25519 seed) and
/// return the `0x`-prefixed proposer pubkey hex. Honest errors: the file is absent
/// (node not started / key not minted yet) or the wrong length. The seed copy is
/// zeroized before returning — only the public value leaves.
pub fn read_proposer_pubkey(data_dir: &std::path::Path) -> Result<String, String> {
    let path = data_dir.join(PROPOSER_KEY_FILE);
    let mut bytes = std::fs::read(&path)
        .map_err(|e| format!("proposer key not available yet ({}): {e}", path.display()))?;
    if bytes.len() != 32 {
        bytes.zeroize();
        return Err(format!(
            "proposer.key is {} bytes, expected a 32-byte ed25519 seed",
            bytes.len()
        ));
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&bytes);
    bytes.zeroize();
    let pubkey = proposer_pubkey_from_seed(&seed);
    seed.zeroize();
    Ok(format!("0x{}", hex::encode(pubkey)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden vectors: a 32-byte ed25519 seed → its verifying (public) key. If these
    /// break, the app's read has drifted from the node's mint (both are plain
    /// ed25519 `SigningKey::from_bytes(seed).verifying_key()`).
    #[test]
    fn proposer_pubkey_from_seed_matches_ed25519_vectors() {
        let cases = [
            ([0x01u8; 32], "8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c"),
            ([0x2au8; 32], "197f6b23e16c8532c6abc838facd5ea789be0c76b2920334039bfa8b3d368d61"),
        ];
        for (seed, want) in cases {
            assert_eq!(hex::encode(proposer_pubkey_from_seed(&seed)), want);
        }
    }

    #[test]
    fn read_proposer_pubkey_reads_the_minted_seed_file() {
        let dir = std::env::temp_dir().join(format!("citrate-proposer-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(PROPOSER_KEY_FILE), [0x01u8; 32]).unwrap();
        let got = read_proposer_pubkey(&dir).unwrap();
        assert_eq!(
            got,
            "0x8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_proposer_pubkey_errors_honestly_before_the_node_mints_it() {
        let missing = std::path::Path::new("/no/such/citrate/data/dir/xyz");
        assert!(read_proposer_pubkey(missing).is_err());
    }

    #[test]
    fn distinct_seeds_produce_distinct_pubkeys() {
        // Each node's minted seed is unique → a distinct registered identity (WP-11
        // one-staker-one-pubkey). Sanity that the derivation is not collapsing.
        assert_ne!(
            proposer_pubkey_from_seed(&[0x01; 32]),
            proposer_pubkey_from_seed(&[0x02; 32]),
        );
    }

    #[test]
    fn read_proposer_pubkey_is_0x_prefixed_32_byte_hex() {
        let dir = std::env::temp_dir().join(format!("citrate-proposer-fmt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(PROPOSER_KEY_FILE), [0x2au8; 32]).unwrap();
        let got = read_proposer_pubkey(&dir).unwrap();
        assert!(got.starts_with("0x"));
        assert_eq!(got.len(), 2 + 64, "0x + 32-byte ed25519 pubkey");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_proposer_pubkey_rejects_a_wrong_length_seed() {
        let dir = std::env::temp_dir().join(format!("citrate-proposer-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(PROPOSER_KEY_FILE), [0u8; 16]).unwrap(); // too short
        assert!(read_proposer_pubkey(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
