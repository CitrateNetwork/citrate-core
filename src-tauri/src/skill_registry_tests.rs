// Hermes P2 — SkillRegistry ABI decoder tests, by round-trip (build a valid encoding, assert
// the decoder extracts the right fields). Pure — no network.
use super::*;

fn word(n: usize) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[24..32].copy_from_slice(&(n as u64).to_be_bytes());
    w
}

fn enc_string_tail(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = word(b.len()).to_vec();
    out.extend_from_slice(b);
    let pad = (32 - b.len() % 32) % 32;
    out.extend(std::iter::repeat(0u8).take(pad));
    out
}

/// Encode `getSkill`'s return: 6 head words (owner, name-off, version-off, manifestCID-off,
/// description-off, isActive) then the string tails in order.
fn encode_get_skill_return(owner: [u8; 20], name: &str, version: &str, cid: &str, desc: &str) -> Vec<u8> {
    let head = 6 * 32;
    let name_t = enc_string_tail(name);
    let ver_t = enc_string_tail(version);
    let cid_t = enc_string_tail(cid);
    let desc_t = enc_string_tail(desc);
    let name_off = head;
    let ver_off = name_off + name_t.len();
    let cid_off = ver_off + ver_t.len();
    let desc_off = cid_off + cid_t.len();

    let mut w0 = [0u8; 32];
    w0[12..32].copy_from_slice(&owner);
    let mut out = Vec::new();
    out.extend_from_slice(&w0);
    out.extend_from_slice(&word(name_off));
    out.extend_from_slice(&word(ver_off));
    out.extend_from_slice(&word(cid_off));
    out.extend_from_slice(&word(desc_off));
    out.extend_from_slice(&word(1)); // isActive
    out.extend_from_slice(&name_t);
    out.extend_from_slice(&ver_t);
    out.extend_from_slice(&cid_t);
    out.extend_from_slice(&desc_t);
    out
}

#[test]
fn decodes_get_skill_owner_name_version_cid_desc() {
    let owner = [0x4fu8; 20];
    // Mirrors the live "hello" starter skill: empty manifestCID, real description.
    let enc = encode_get_skill_return(owner, "hello", "1.0.0", "", "Starter capsule: hello-world.");
    let (got_owner, name, version, cid, desc) = decode_get_skill(&enc).unwrap();
    assert_eq!(got_owner, format!("0x{}", hex::encode(owner)));
    assert_eq!(name, "hello");
    assert_eq!(version, "1.0.0");
    assert_eq!(cid, "");
    assert_eq!(desc, "Starter capsule: hello-world.");
}

#[test]
fn decodes_get_skill_with_manifest_cid() {
    let owner = [0x01u8; 20];
    let enc = encode_get_skill_return(owner, "hf-model-register", "1.0.0", "QmSkillManifestCID", "Pull + register an HF model.");
    let (_, name, _, cid, _) = decode_get_skill(&enc).unwrap();
    assert_eq!(name, "hf-model-register");
    assert_eq!(cid, "QmSkillManifestCID");
}

#[test]
fn get_skill_short_return_errors() {
    assert!(decode_get_skill(&[0u8; 32 * 3]).is_err()); // fewer than 6 head words
}
