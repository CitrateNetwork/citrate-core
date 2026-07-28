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

// ---------------------------------------------------------------------------
// W1.3 — validator registration (register the node as a block producer).
//
// The member's smart wallet calls `registerValidator{value: 32k SALT}(pubkey, sig)`.
// `staker = msg.sender` (the wallet) becomes the validator owner; `bondedStake =
// msg.value` is the 32k bond. `sig` is the ed25519 proposer key signing the
// EIP-712-style register digest — proof of proposer-key control, binding the
// pubkey to the staker. All three builders mirror citrate-chain
// `core/consensus/src/crypto.rs` + `ValidatorRegistry.sol` byte-for-byte (golden
// vectors below); a drift there fails these tests rather than the on-chain revert.
// ---------------------------------------------------------------------------

use sha3::{Digest as _, Keccak256};

/// `registerValidator(bytes32,bytes)` selector (`keccak256(sig)[..4]`).
pub const REGISTER_VALIDATOR_SELECTOR: [u8; 4] = [0x10, 0xf5, 0x3b, 0xa1];
/// `registrationNonce(address)` selector — the eth_call that reads the staker's
/// current nonce for the digest.
pub const REGISTRATION_NONCE_SELECTOR: [u8; 4] = [0x7c, 0x36, 0x0a, 0x1d];
/// The EIP-712-ish type string the contract's `REGISTER_TYPEHASH` hashes.
const REGISTER_TYPE: &[u8] =
    b"Register(uint256 chainId,address registry,address staker,bytes32 proposerPubkey,uint256 nonce)";

fn keccak256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(bytes);
    h.finalize().into()
}

/// A `u64` as a 32-byte big-endian ABI word.
fn word_u64(n: u64) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[24..].copy_from_slice(&n.to_be_bytes());
    w
}

/// A 20-byte address right-aligned in a 32-byte ABI word.
fn word_addr(addr: &[u8; 20]) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(addr);
    w
}

/// The registration digest the ed25519 proposer key signs — exactly what
/// `ValidatorRegistry.registerValidator` reconstructs:
/// `keccak256(abi.encode(REGISTER_TYPEHASH, chainId, registry, staker, pubkey, nonce))`.
pub fn registration_digest(
    chain_id: u64,
    registry: &[u8; 20],
    staker: &[u8; 20],
    proposer_pubkey: &[u8; 32],
    nonce: u64,
) -> [u8; 32] {
    let mut enc = Vec::with_capacity(6 * 32);
    enc.extend_from_slice(&keccak256(REGISTER_TYPE));
    enc.extend_from_slice(&word_u64(chain_id));
    enc.extend_from_slice(&word_addr(registry));
    enc.extend_from_slice(&word_addr(staker));
    enc.extend_from_slice(proposer_pubkey);
    enc.extend_from_slice(&word_u64(nonce));
    keccak256(&enc)
}

/// Sign the registration digest with the node's proposer key (its 32-byte
/// `proposer.key` seed). The contract verifies `abi.encodePacked(digest)` = the
/// 32-byte digest, so we sign exactly those 32 bytes (canonical → `verify_strict`).
/// Returns the 64-byte ed25519 signature.
pub fn sign_registration(
    proposer_seed: &[u8; 32],
    chain_id: u64,
    registry: &[u8; 20],
    staker: &[u8; 20],
    nonce: u64,
) -> [u8; 64] {
    use ed25519_dalek::Signer as _;
    let signing_key = SigningKey::from_bytes(proposer_seed);
    let proposer_pubkey = signing_key.verifying_key().to_bytes();
    let digest = registration_digest(chain_id, registry, staker, &proposer_pubkey, nonce);
    signing_key.sign(&digest).to_bytes()
}

/// ABI-encode the `registerValidator(bytes32 proposerPubkey, bytes ed25519Sig)`
/// calldata (selector + head + tail). The 64-byte sig is already 32-aligned.
pub fn register_validator_calldata(proposer_pubkey: &[u8; 32], ed25519_sig: &[u8; 64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 32 * 3 + 64);
    out.extend_from_slice(&REGISTER_VALIDATOR_SELECTOR);
    out.extend_from_slice(proposer_pubkey); // head word 1: bytes32
    out.extend_from_slice(&word_u64(0x40)); // head word 2: offset to the bytes arg
    out.extend_from_slice(&word_u64(64)); // tail: byte length
    out.extend_from_slice(ed25519_sig); // tail: the 64 sig bytes (32-aligned)
    out
}

/// ABI-encode the `registrationNonce(address staker)` eth_call input.
pub fn registration_nonce_calldata(staker: &[u8; 20]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 32);
    out.extend_from_slice(&REGISTRATION_NONCE_SELECTOR);
    out.extend_from_slice(&word_addr(staker));
    out
}

/// Parse a `0x`-prefixed 20-byte address into raw bytes (registry / staker).
pub fn parse_address_20(addr: &str) -> Result<[u8; 20], String> {
    let raw = addr.strip_prefix("0x").unwrap_or(addr);
    let bytes = hex::decode(raw).map_err(|e| format!("bad address hex: {e}"))?;
    if bytes.len() != 20 {
        return Err(format!("address must be 20 bytes, got {}", bytes.len()));
    }
    let mut a = [0u8; 20];
    a.copy_from_slice(&bytes);
    Ok(a)
}

/// Read `ValidatorRegistry.registrationNonce(staker)` via a live eth_call — the
/// replay-guard nonce the register digest must carry. Real read or honest error
/// (Rule 1). The nonce is small; the top 24 bytes of the word are ignored.
pub fn read_registration_nonce<T: crate::rpc::RpcTransport>(
    rpc: &crate::rpc::RpcClient<T>,
    registry: &str,
    staker: &[u8; 20],
) -> Result<u64, String> {
    let call = serde_json::json!({
        "to": registry,
        "data": format!("0x{}", hex::encode(registration_nonce_calldata(staker))),
    });
    let ret = rpc.eth_call(call).map_err(|e| e.to_string())?;
    if ret.len() < 32 {
        return Err(format!("registrationNonce returned {} bytes, expected 32", ret.len()));
    }
    let mut b = [0u8; 8];
    b.copy_from_slice(&ret[24..32]);
    Ok(u64::from_be_bytes(b))
}

/// `rewardsOf(bytes32)` selector (`keccak256("rewardsOf(bytes32)")[..4]`).
const REWARDS_OF_SELECTOR: [u8; 4] = [0x18, 0x7e, 0x9c, 0x41];

/// Low 16 bytes of a 32-byte ABI word as a `u128` (SALT wei exceeds u64).
fn u128_from_word(word: &[u8]) -> u128 {
    let mut b = [0u8; 16];
    b.copy_from_slice(&word[16..32]);
    u128::from_be_bytes(b)
}

/// Read the node's REAL validator earnings from `ValidatorRegistry.rewardsOf(pubkey)`
/// via a live eth_call — `(total, claimableNow)` in wei of SALT. This is the CORRECT
/// earnings source for a block-producing validator (the block subsidy accrues here,
/// keyed by the proposer pubkey), replacing the wrong `ContributionAccounting.claimable`
/// read (W1.4). Real read or honest error (Rule 1); an unregistered pubkey returns
/// `(0, 0)`.
pub fn read_validator_rewards<T: crate::rpc::RpcTransport>(
    rpc: &crate::rpc::RpcClient<T>,
    registry: &str,
    pubkey: &[u8; 32],
) -> Result<(u128, u128), String> {
    let mut data = Vec::with_capacity(36);
    data.extend_from_slice(&REWARDS_OF_SELECTOR);
    data.extend_from_slice(pubkey); // bytes32 pubkey is already a 32-byte ABI word
    let call = serde_json::json!({
        "to": registry,
        "data": format!("0x{}", hex::encode(&data)),
    });
    let ret = rpc.eth_call(call).map_err(|e| e.to_string())?;
    if ret.len() < 64 {
        return Err(format!("rewardsOf returned {} bytes, expected 64", ret.len()));
    }
    Ok((u128_from_word(&ret[0..32]), u128_from_word(&ret[32..64])))
}

/// Read the node's `proposer.key`, sign the registration digest with it, and return
/// `(proposer_pubkey, ed25519_sig)` — the two values `registerValidator` needs. The
/// 32-byte seed is read, used, and zeroized here; it never leaves this function.
pub fn sign_registration_from_data_dir(
    data_dir: &std::path::Path,
    chain_id: u64,
    registry: &[u8; 20],
    staker: &[u8; 20],
    nonce: u64,
) -> Result<([u8; 32], [u8; 64]), String> {
    let path = data_dir.join(PROPOSER_KEY_FILE);
    let mut bytes = std::fs::read(&path)
        .map_err(|e| format!("proposer key not available yet ({}): {e}", path.display()))?;
    if bytes.len() != 32 {
        bytes.zeroize();
        return Err(format!("proposer.key is {} bytes, expected 32", bytes.len()));
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&bytes);
    bytes.zeroize();
    let pubkey = proposer_pubkey_from_seed(&seed);
    let sig = sign_registration(&seed, chain_id, registry, staker, nonce);
    seed.zeroize();
    Ok((pubkey, sig))
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

    // ---- W1.3 registration (golden vectors from the canonical crate + contract) ----

    fn addr20(hex_str: &str) -> [u8; 20] {
        let raw = hex_str.strip_prefix("0x").unwrap_or(hex_str);
        let b = hex::decode(raw).unwrap();
        let mut a = [0u8; 20];
        a.copy_from_slice(&b);
        a
    }
    const REGISTRY: &str = "0x61d44d8a14443646b756905410be951e6ece95a6";
    const STAKER: &str = "0x00000000000000000000000000000000000000aa";

    #[test]
    fn selectors_match_the_contract_signatures() {
        assert_eq!(REGISTER_VALIDATOR_SELECTOR, keccak256(b"registerValidator(bytes32,bytes)")[..4]);
        assert_eq!(REGISTRATION_NONCE_SELECTOR, keccak256(b"registrationNonce(address)")[..4]);
    }

    #[test]
    fn registration_digest_matches_canonical_golden() {
        let pubkey = proposer_pubkey_from_seed(&[0x01; 32]);
        let digest = registration_digest(40204, &addr20(REGISTRY), &addr20(STAKER), &pubkey, 0);
        assert_eq!(
            hex::encode(digest),
            "dede65b456a15ed489052ce3bc2ee40421c8a8e9f9de087d931898921d7525d9"
        );
    }

    #[test]
    fn sign_registration_matches_canonical_golden() {
        // ed25519 is deterministic (RFC 8032) → the sig is a stable golden. This is
        // the proof-of-proposer-key-control the contract's _ed25519Verify checks.
        let sig = sign_registration(&[0x01; 32], 40204, &addr20(REGISTRY), &addr20(STAKER), 0);
        assert_eq!(
            hex::encode(sig),
            "67b6447494e64f7afc3a324771f52ef6b77f5a1b8b32b703656692c7a655e07590726236c5a47555cd8fa6e8cc1564c5f1e376a4c5531eed85f63c24c7ec3c00"
        );
    }

    #[test]
    fn register_validator_calldata_is_abi_encoded_bytes32_bytes() {
        let pubkey = proposer_pubkey_from_seed(&[0x01; 32]);
        let sig = sign_registration(&[0x01; 32], 40204, &addr20(REGISTRY), &addr20(STAKER), 0);
        let cd = register_validator_calldata(&pubkey, &sig);
        assert_eq!(&cd[..4], &REGISTER_VALIDATOR_SELECTOR, "selector");
        assert_eq!(&cd[4..36], &pubkey, "bytes32 pubkey head");
        // offset to the dynamic bytes = 0x40 (two head words)
        assert_eq!(cd[67], 0x40);
        // byte length = 64
        assert_eq!(cd[99], 64);
        assert_eq!(&cd[100..164], &sig, "the 64 sig bytes");
        assert_eq!(cd.len(), 164, "4 + 32 + 32 + 32 + 64");
    }

    #[test]
    fn registration_nonce_calldata_is_selector_plus_padded_address() {
        let cd = registration_nonce_calldata(&addr20(STAKER));
        assert_eq!(&cd[..4], &REGISTRATION_NONCE_SELECTOR);
        assert_eq!(cd.len(), 36);
        assert_eq!(&cd[16..36], &addr20(STAKER), "address right-aligned in the word");
        assert_eq!(&cd[4..16], &[0u8; 12], "left-padded with zeros");
    }

    #[test]
    fn parse_address_20_validates_length_and_hex() {
        assert!(parse_address_20(REGISTRY).is_ok());
        assert!(parse_address_20("0x1234").is_err());
        assert!(parse_address_20("0xZZ44d8a14443646b756905410be951e6ece95a6").is_err());
    }

    #[test]
    fn sign_registration_from_data_dir_reads_the_seed_and_matches_golden() {
        let dir = std::env::temp_dir().join(format!("citrate-reg-sign-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(PROPOSER_KEY_FILE), [0x01u8; 32]).unwrap();
        let (pubkey, sig) =
            sign_registration_from_data_dir(&dir, 40204, &addr20(REGISTRY), &addr20(STAKER), 0).unwrap();
        assert_eq!(
            hex::encode(pubkey),
            "8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c"
        );
        assert_eq!(
            hex::encode(sig),
            "67b6447494e64f7afc3a324771f52ef6b77f5a1b8b32b703656692c7a655e07590726236c5a47555cd8fa6e8cc1564c5f1e376a4c5531eed85f63c24c7ec3c00"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_registration_nonce_decodes_the_uint256_word() {
        use crate::rpc::{RpcClient, RpcError, RpcTransport};
        struct M(u64);
        impl RpcTransport for M {
            fn call(&self, _body: serde_json::Value) -> std::result::Result<serde_json::Value, RpcError> {
                let mut word = [0u8; 32];
                word[24..].copy_from_slice(&self.0.to_be_bytes());
                Ok(serde_json::json!({"jsonrpc":"2.0","id":1,"result": format!("0x{}", hex::encode(word))}))
            }
        }
        let rpc = RpcClient::with_transport(M(7));
        assert_eq!(read_registration_nonce(&rpc, REGISTRY, &addr20(STAKER)).unwrap(), 7);
    }

    #[test]
    fn read_validator_rewards_decodes_total_and_claimable() {
        use crate::rpc::{RpcClient, RpcError, RpcTransport};
        // rewardsOf returns two uint256 words: (total, claimableNow).
        struct M(u128, u128);
        impl RpcTransport for M {
            fn call(&self, _body: serde_json::Value) -> std::result::Result<serde_json::Value, RpcError> {
                let mut buf = [0u8; 64];
                buf[16..32].copy_from_slice(&self.0.to_be_bytes());
                buf[48..64].copy_from_slice(&self.1.to_be_bytes());
                Ok(serde_json::json!({"jsonrpc":"2.0","id":1,"result": format!("0x{}", hex::encode(buf))}))
            }
        }
        let rpc = RpcClient::with_transport(M(9_410_000_000_000_000_000u128, 4_000_000_000_000_000_000u128));
        let pubkey = [0x11u8; 32];
        let (total, claimable) = read_validator_rewards(&rpc, REGISTRY, &pubkey).unwrap();
        assert_eq!(total, 9_410_000_000_000_000_000u128);
        assert_eq!(claimable, 4_000_000_000_000_000_000u128);
    }
}
