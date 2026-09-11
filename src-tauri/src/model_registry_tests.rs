// Hermes WP0.2b — ModelRegistry ABI decoder tests. The dynamic-ABI decode is the risky part,
// so we test it by ROUND-TRIP: build a valid ABI encoding, then assert the decoder extracts the
// right fields. (No network — pure decoders.)
use super::*;

/// A 32-byte big-endian word from a usize.
fn word(n: usize) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[24..32].copy_from_slice(&(n as u64).to_be_bytes());
    w
}

/// Encode an ABI `bytes32[]` return (single dynamic return: offset 0x20, length, hashes).
fn encode_bytes32_array(hashes: &[[u8; 32]]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&word(0x20)); // offset to the array
    out.extend_from_slice(&word(hashes.len())); // length
    for h in hashes {
        out.extend_from_slice(h);
    }
    out
}

/// Encode a dynamic `string` tail (len word + data padded to 32).
fn enc_string_tail(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = word(b.len()).to_vec();
    out.extend_from_slice(b);
    let pad = (32 - b.len() % 32) % 32;
    out.extend(std::iter::repeat(0u8).take(pad));
    out
}

/// Encode `getModel`'s return: 8 head words (owner + 4 string offsets + price + infer + active)
/// then the string tails in order (name, framework, version, ipfsCID).
fn encode_get_model_return(owner: [u8; 20], name: &str, framework: &str, version: &str, cid: &str) -> Vec<u8> {
    let head = 8 * 32;
    let name_tail = enc_string_tail(name);
    let fw_tail = enc_string_tail(framework);
    let ver_tail = enc_string_tail(version);
    let cid_tail = enc_string_tail(cid);
    let name_off = head;
    let fw_off = name_off + name_tail.len();
    let ver_off = fw_off + fw_tail.len();
    let cid_off = ver_off + ver_tail.len();

    let mut w0 = [0u8; 32];
    w0[12..32].copy_from_slice(&owner); // address in the low 20 bytes
    let mut out = Vec::new();
    out.extend_from_slice(&w0); // owner
    out.extend_from_slice(&word(name_off));
    out.extend_from_slice(&word(fw_off));
    out.extend_from_slice(&word(ver_off));
    out.extend_from_slice(&word(cid_off));
    out.extend_from_slice(&word(0)); // inferencePrice
    out.extend_from_slice(&word(0)); // totalInferences
    out.extend_from_slice(&word(1)); // isActive = true
    out.extend_from_slice(&name_tail);
    out.extend_from_slice(&fw_tail);
    out.extend_from_slice(&ver_tail);
    out.extend_from_slice(&cid_tail);
    out
}

#[test]
fn selector_is_first_4_bytes_of_keccak() {
    // Sanity: selector() yields a 4-byte prefix (the storage.rs runtime-keccak convention).
    assert_eq!(selector("getAllModelHashes()").len(), 4);
    assert_ne!(selector("getAllModelHashes()"), selector("getModel(bytes32)"));
}

#[test]
fn decodes_bytes32_array_roundtrip() {
    let a = [0x11u8; 32];
    let b = [0x22u8; 32];
    let enc = encode_bytes32_array(&[a, b]);
    assert_eq!(decode_bytes32_array(&enc).unwrap(), vec![a, b]);
    // empty
    assert_eq!(decode_bytes32_array(&encode_bytes32_array(&[])).unwrap(), Vec::<[u8; 32]>::new());
}

#[test]
fn bytes32_array_short_return_errors() {
    assert!(decode_bytes32_array(&[0u8; 8]).is_err());
}

#[test]
fn decodes_get_model_owner_name_cid() {
    let owner = [0xabu8; 20];
    let enc = encode_get_model_return(owner, "gemma-4-E4B-it", "llama.cpp", "1.0", "QmModelWeightsCID123");
    let (got_owner, name, cid) = decode_get_model(&enc).unwrap();
    assert_eq!(got_owner, format!("0x{}", hex::encode(owner)));
    assert_eq!(name, "gemma-4-E4B-it");
    assert_eq!(cid, "QmModelWeightsCID123");
}

#[test]
fn get_model_handles_empty_and_long_strings() {
    let owner = [0x01u8; 20];
    let long = "a".repeat(70); // spans 3 data words — exercises padding math
    let enc = encode_get_model_return(owner, "", "fw", "v", &long);
    let (_, name, cid) = decode_get_model(&enc).unwrap();
    assert_eq!(name, "");
    assert_eq!(cid, long);
}

#[test]
fn get_model_short_return_errors() {
    assert!(decode_get_model(&[0u8; 32 * 3]).is_err()); // fewer than 8 head words
}

// --- Adversarial F2: hostile eth_call returns must be an honest Err, never a panic ---

/// A word whose value is 0xFFFF…FFFF in the low 8 bytes (max u64) — the offset that
/// wraps `off + 32` on a 64-bit usize if the add is unchecked.
fn max_u64_word() -> [u8; 32] {
    let mut w = [0u8; 32];
    w[24..32].copy_from_slice(&u64::MAX.to_be_bytes());
    w
}

#[test]
fn read_string_at_rejects_a_wrapping_offset_without_panicking() {
    // 64 bytes of data; an offset of u64::MAX would wrap to a small number if unchecked.
    let ret = vec![0u8; 64];
    let off = u64::MAX as usize;
    assert!(read_string_at(&ret, off).is_err(), "wrapping offset → honest Err, not panic");
}

#[test]
fn decode_bytes32_array_rejects_a_wrapping_offset() {
    // The head offset word is u64::MAX → offset+32 must not wrap past the length guard.
    let ret = max_u64_word().to_vec(); // 32 bytes, offset = u64::MAX
    let mut padded = ret;
    padded.extend_from_slice(&[0u8; 32]); // 64 bytes so the initial length check passes
    assert!(decode_bytes32_array(&padded).is_err(), "wrapping array offset → Err");
}

#[test]
fn decode_bytes32_array_rejects_a_length_that_would_overflow() {
    // Valid offset (0x20) but a length of u64::MAX → start + len*32 must not wrap.
    let mut ret = word(0x20).to_vec();
    ret.extend_from_slice(&max_u64_word()); // length = u64::MAX at the array head
    assert!(decode_bytes32_array(&ret).is_err(), "overflowing length → Err, not OOM/panic");
}

#[test]
fn read_string_at_rejects_a_length_past_the_end() {
    // Offset 0 points at a length word claiming a huge string the buffer can't hold.
    let mut ret = word(1_000_000).to_vec(); // len = 1e6 at offset 0
    ret.extend_from_slice(&[0u8; 32]); // only 64 bytes total
    assert!(read_string_at(&ret, 0).is_err(), "length past end → Err");
}
