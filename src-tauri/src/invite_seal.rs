//! CONNECT-S1 — sealing a group-invite claim to the invite's ephemeral key, so it can ride the relay's
//! **server-blind** claims-inbox: the invitee seals `{group, token, address}` to the ephemeral public
//! key carried in the invite link, submits the ciphertext, and only the owner (who holds the matching
//! private key in their `PendingInvite`) can open it. The relay stores opaque bytes.
//!
//! Construction: ECIES over secp256k1 — a fresh ephemeral keypair per seal, ECDH against the recipient
//! key, a domain-separated SHA-256 of the shared secret as the AES-256-GCM key, and a random 96-bit
//! nonce. Wire layout: `eph_pubkey(33, compressed SEC1) ‖ nonce(12) ‖ AES-GCM ciphertext`.
use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit};
use k256::ecdh::diffie_hellman;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::{PublicKey, SecretKey};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};

const EPH_LEN: usize = 33; // compressed SEC1 public key
const NONCE_LEN: usize = 12;
const KDF_INFO: &[u8] = b"citrate-connect-s1-claim-v1";

/// Domain-separated KDF from the ECDH shared secret to the AES key (a one-shot; the ephemeral key is
/// never reused, so a single SHA-256 with a context tag is sufficient).
fn derive_key(shared: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(KDF_INFO);
    h.update(shared);
    let out = h.finalize();
    let mut k = [0u8; 32];
    k.copy_from_slice(&out);
    k
}

/// Generate a fresh invite ephemeral keypair. Returns `(private_key_hex, public_key_hex)` — the private
/// key is persisted with the owner's `PendingInvite`; the public key is embedded in the invite link.
pub fn new_invite_keypair() -> (String, String) {
    let sk = SecretKey::random(&mut OsRng);
    let pk = sk.public_key();
    let priv_hex = hex::encode(sk.to_bytes());
    let pub_hex = hex::encode(pk.to_encoded_point(true).as_bytes());
    (priv_hex, pub_hex)
}

/// Seal `plaintext` to the invite's public key (hex, compressed SEC1). Invitee side.
pub fn seal_to(recipient_pub_hex: &str, plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let pub_bytes = hex::decode(recipient_pub_hex.trim()).map_err(|_| "invite key is not hex")?;
    let recipient = PublicKey::from_sec1_bytes(&pub_bytes).map_err(|_| "invite key is not a valid point")?;
    let eph = SecretKey::random(&mut OsRng);
    let shared = diffie_hellman(eph.to_nonzero_scalar(), recipient.as_affine());
    let key = derive_key(shared.raw_secret_bytes());
    let cipher = Aes256Gcm::new(GenericArray::from_slice(&key));
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(GenericArray::from_slice(&nonce), plaintext)
        .map_err(|_| "seal: encrypt failed")?;
    let eph_pub = eph.public_key().to_encoded_point(true);
    let mut out = Vec::with_capacity(EPH_LEN + NONCE_LEN + ct.len());
    out.extend_from_slice(eph_pub.as_bytes());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Open a sealed claim with the invite's private key (hex). Owner side. Fails closed on any tamper.
pub fn open_with(recipient_priv_hex: &str, sealed: &[u8]) -> Result<Vec<u8>, String> {
    if sealed.len() < EPH_LEN + NONCE_LEN {
        return Err("sealed claim is too short".into());
    }
    let priv_bytes = hex::decode(recipient_priv_hex.trim()).map_err(|_| "invite priv is not hex")?;
    let sk = SecretKey::from_slice(&priv_bytes).map_err(|_| "invite priv is not a valid scalar")?;
    let eph = PublicKey::from_sec1_bytes(&sealed[..EPH_LEN]).map_err(|_| "bad ephemeral key")?;
    let nonce = &sealed[EPH_LEN..EPH_LEN + NONCE_LEN];
    let ct = &sealed[EPH_LEN + NONCE_LEN..];
    let shared = diffie_hellman(sk.to_nonzero_scalar(), eph.as_affine());
    let key = derive_key(shared.raw_secret_bytes());
    let cipher = Aes256Gcm::new(GenericArray::from_slice(&key));
    cipher
        .decrypt(GenericArray::from_slice(nonce), ct)
        .map_err(|_| "open: decrypt failed (wrong key or tampered)".into())
}

/// The relay inbox key for an invite token: SHA-256 of the token. The token itself never reaches the
/// relay; both the invitee (submit) and the owner (poll) compute this over the same token.
pub fn token_hash(token: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    let out = h.finalize();
    let mut k = [0u8; 32];
    k.copy_from_slice(&out);
    k
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_roundtrips_the_claim() {
        let (priv_hex, pub_hex) = new_invite_keypair();
        let claim = br#"{"group":"g1","token":"abc","address":"0xAA"}"#;
        let sealed = seal_to(&pub_hex, claim).unwrap();
        // The sealed blob is not the plaintext and carries the ephemeral pubkey + nonce prefix.
        assert!(sealed.len() > EPH_LEN + NONCE_LEN);
        assert_ne!(&sealed[EPH_LEN + NONCE_LEN..], &claim[..]);
        let opened = open_with(&priv_hex, &sealed).unwrap();
        assert_eq!(opened, claim);
    }

    #[test]
    fn open_fails_with_the_wrong_key() {
        let (_p1, pub1) = new_invite_keypair();
        let (priv2, _pub2) = new_invite_keypair(); // a different invite's key
        let sealed = seal_to(&pub1, b"secret claim").unwrap();
        assert!(open_with(&priv2, &sealed).is_err()); // wrong key → fail closed
    }

    #[test]
    fn open_fails_on_tamper() {
        let (priv_hex, pub_hex) = new_invite_keypair();
        let mut sealed = seal_to(&pub_hex, b"secret claim").unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01; // flip a ciphertext bit
        assert!(open_with(&priv_hex, &sealed).is_err()); // AEAD auth fails
    }

    #[test]
    fn token_hash_is_stable_and_matches_both_sides() {
        assert_eq!(token_hash("tok-123"), token_hash("tok-123"));
        assert_ne!(token_hash("tok-123"), token_hash("tok-124"));
    }
}
