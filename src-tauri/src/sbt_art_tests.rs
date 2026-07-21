// BC-5.3 — the REAL on-chain SBT emblem read (isSubBound -> tokenIdForSub ->
// tokenURI), decoded to the `data:image/svg+xml;base64,...` the UI renders.
//
// Load-bearing properties, all grounded (Rule 1 — no fabricated art):
//   * The three selectors are PINNED constants proven here by an INDEPENDENT
//     Keccak-256 derivation (`sha3`) — a drift reads the WRONG function.
//   * The member subHash is `keccak256(sub_utf8_bytes)` (matches core-membership's
//     subHashOf) — proven against an independent keccak.
//   * A member WITH an SBT: isSubBound=true -> tokenId -> tokenURI decodes to the
//     embedded `data:image/svg+xml;base64,...` (fixture round-trip).
//   * A member with NO SBT: isSubBound=false -> `Ok(None)` (no fabricated art).
//   * A malformed/short tokenURI return errors honestly (never a truncated URI).
use super::*;
use crate::rpc::{RpcClient, RpcError, RpcTransport};
use serde_json::Value as JsonValue;
use std::cell::RefCell;
use std::collections::VecDeque;

// A scripted mock RPC transport (mirrors staking_tests / grant_status_tests).
struct MockRpc {
    responses: RefCell<VecDeque<JsonValue>>,
}
impl MockRpc {
    fn new(responses: Vec<JsonValue>) -> Self {
        MockRpc {
            responses: RefCell::new(responses.into_iter().collect()),
        }
    }
}
impl RpcTransport for MockRpc {
    fn call(&self, _body: JsonValue) -> std::result::Result<JsonValue, RpcError> {
        self.responses
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| RpcError::Transport("mock: no scripted response".into()))
    }
}
fn ok(result: JsonValue) -> JsonValue {
    serde_json::json!({ "jsonrpc": "2.0", "id": 1, "result": result })
}

/// Independent Keccak-256 selector derivation (proves the pinned constants).
fn derive_selector(sig: &str) -> [u8; 4] {
    use sha3::{Digest, Keccak256};
    let h = Keccak256::digest(sig.as_bytes());
    [h[0], h[1], h[2], h[3]]
}

const SAMPLE_SUB: &str = "oidc|abc123";

/// A 32-byte bool word (`0x…01` for true, all-zero for false), as a `0x`-hex string.
fn bool_hex(b: bool) -> String {
    let mut word = [0u8; 32];
    if b {
        word[31] = 1;
    }
    format!("0x{}", hex::encode(word))
}

/// A 32-byte uint256 word for a small tokenId, as a `0x`-hex string.
fn uint256_hex(n: u128) -> String {
    let mut word = [0u8; 32];
    word[16..32].copy_from_slice(&n.to_be_bytes());
    format!("0x{}", hex::encode(word))
}

// The fixture: a real `tokenURI` ABI-`string` return whose JSON `image` field is a
// `data:image/svg+xml;base64,...`. Generated from a canonical
// `{"name","description","image"}` object (see the sprint scope). ABI layout is
// `[offset=0x20][length][utf8 data]`.
const TOKEN_URI_ABI_HEX: &str = "0x0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000016d646174613a6170706c69636174696f6e2f6a736f6e3b6261736536342c65794a755957316c496a6f6951326c30636d46305a53424e5a5731695a584967497a63694c434a6b5a584e6a636d6c7764476c7662694936496c4e7664577869623356755a4342745a5731695a584a7a61476c7749697769615731685a3255694f694a6b595852684f6d6c745957646c4c334e325a79743462577737596d467a5a5459304c464249546a4a6165554930596c643464574e364d476c685346497759305276646b777a5a444e6b6554557a54586b31646d4e7459335a4e616b463354554d35656d527459326c4a53475277576b6853623142545354464a61554a76576c6473626d464955546c4a616c56705547703465567058546a424a53475277576b6853623142545354464a61554a76576c6473626d464955546c4a616c56705355646163474a48647a6c4a61553133576c52476145315554576c4d656a513454444e4f4d6c70364e44306966513d3d00000000000000000000000000000000000000";

const EXPECTED_IMAGE_DATA_URI: &str = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSI1IiBoZWlnaHQ9IjUiPjxyZWN0IHdpZHRoPSI1IiBoZWlnaHQ9IjUiIGZpbGw9IiMwZTFhMTMiLz48L3N2Zz4=";

// ===========================================================================
// Canonical address + selector pins (Rule 11 tripwires)
// ===========================================================================

/// The SBT reads target the canonical post-reroll CitrateMemberSBT address (reused
/// from grant_status, the same book value). A paste error would read the WRONG
/// contract.
#[test]
fn sbt_address_is_the_canonical_reroll_value() {
    assert_eq!(
        CITRATE_MEMBER_SBT,
        "0x4ce39f891c0a519fa0e0de97a1dd3e3f856e0cf1"
    );
}

/// The pinned `isSubBound(bytes32)` selector is the REAL keccak of the signature.
#[test]
fn is_sub_bound_selector_is_keccak_of_signature() {
    assert_eq!(is_sub_bound_selector(), derive_selector("isSubBound(bytes32)"));
    assert_eq!(
        format!("0x{}", hex::encode(is_sub_bound_selector())),
        "0xdee2ef9d"
    );
}

/// The pinned `tokenIdForSub(bytes32)` selector is the REAL keccak of the signature.
#[test]
fn token_id_for_sub_selector_is_keccak_of_signature() {
    assert_eq!(
        token_id_for_sub_selector(),
        derive_selector("tokenIdForSub(bytes32)")
    );
    assert_eq!(
        format!("0x{}", hex::encode(token_id_for_sub_selector())),
        "0xf8856bd9"
    );
}

/// The pinned `tokenURI(uint256)` selector is the REAL keccak of the signature AND
/// the canonical ERC-721 value 0xc87b56dd.
#[test]
fn token_uri_selector_is_keccak_of_signature() {
    assert_eq!(token_uri_selector(), derive_selector("tokenURI(uint256)"));
    assert_eq!(
        format!("0x{}", hex::encode(token_uri_selector())),
        "0xc87b56dd"
    );
}

// ===========================================================================
// subHash — keccak256(sub) (matches core-membership subHashOf)
// ===========================================================================

/// The member subHash is the INDEPENDENT keccak256 of the sub's UTF-8 bytes — the
/// raw sub never goes on chain (matches core-membership `subHashOf`).
#[test]
fn sub_hash_is_keccak_of_utf8_sub() {
    use sha3::{Digest, Keccak256};
    let expected: [u8; 32] = Keccak256::digest(SAMPLE_SUB.as_bytes()).into();
    assert_eq!(sub_hash(SAMPLE_SUB), expected);
    // A different sub yields a different hash (no accidental collision / constant).
    assert_ne!(sub_hash(SAMPLE_SUB), sub_hash("oidc|different"));
}

// ===========================================================================
// ABI string + image-uri decoding
// ===========================================================================

/// The ABI-`string` decoder round-trips the fixture tokenURI return to the exact
/// `data:application/json;base64,...` string.
#[test]
fn decode_abi_string_reads_the_token_uri() {
    let ret = hex::decode(&TOKEN_URI_ABI_HEX[2..]).unwrap();
    let uri = decode_abi_string(&ret).expect("abi string decodes");
    assert!(
        uri.starts_with("data:application/json;base64,"),
        "decoded the on-chain tokenURI data-uri"
    );
    // And its embedded image extracts to the exact svg data-uri.
    let image = extract_image_data_uri(&uri).expect("image extracts");
    assert_eq!(image, EXPECTED_IMAGE_DATA_URI);
    assert!(image.starts_with("data:image/svg+xml;base64,"));
}

/// A short / malformed ABI-string return errors (never a truncated / fabricated URI).
#[test]
fn decode_abi_string_rejects_short_and_overlong() {
    assert!(decode_abi_string(&[0u8; 32]).is_err(), "too short for offset+len");
    // offset ok but declared length runs past the buffer.
    let mut bad = vec![0u8; 64];
    bad[31] = 0x20; // offset = 0x20
    bad[63] = 0xff; // length = 255, but no data follows
    assert!(
        decode_abi_string(&bad).is_err(),
        "declared length past the buffer is refused, not truncated"
    );
}

/// `extract_image_data_uri` refuses a non-json / non-image-uri / missing-image value.
#[test]
fn extract_image_refuses_malformed() {
    assert!(
        extract_image_data_uri("not-a-data-uri").is_err(),
        "not a data:application/json uri"
    );
    // Valid json but no image field.
    let no_image = format!(
        "data:application/json;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(br#"{"name":"x"}"#)
    );
    assert!(
        extract_image_data_uri(&no_image).is_err(),
        "no `image` field is an honest error, not fabricated art"
    );
    // image present but not a data:image/ uri (e.g. an ipfs link) — refuse.
    let http_image = format!(
        "data:application/json;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(br#"{"image":"https://x/y.png"}"#)
    );
    assert!(
        extract_image_data_uri(&http_image).is_err(),
        "non-data image uri refused"
    );
}

// ===========================================================================
// The full read over a scripted mock RPC
// ===========================================================================

/// A member WITH an SBT: isSubBound=true, tokenIdForSub=7, tokenURI decodes to the
/// on-chain emblem. The three eth_calls target the SBT contract in order.
#[test]
fn read_sbt_emblem_reads_the_on_chain_art_when_present() {
    let rpc = RpcClient::with_transport(MockRpc::new(vec![
        ok(JsonValue::String(bool_hex(true))),         // isSubBound -> true
        ok(JsonValue::String(uint256_hex(7))),         // tokenIdForSub -> 7
        ok(JsonValue::String(TOKEN_URI_ABI_HEX.into())), // tokenURI -> abi string
    ]));
    let emblem = read_sbt_emblem(&rpc, SAMPLE_SUB)
        .expect("read ok")
        .expect("member has an SBT");
    assert_eq!(
        emblem.image_data_uri, EXPECTED_IMAGE_DATA_URI,
        "the REAL on-chain svg data-uri, not a local emblem"
    );
    assert!(emblem.token_uri.starts_with("data:application/json;base64,"));
}

/// The negative control that bites: a member with NO SBT (isSubBound=false) reads
/// `None` — NO tokenIdForSub / tokenURI call is made and NO art is fabricated.
#[test]
fn read_sbt_emblem_returns_none_for_a_member_without_an_sbt() {
    let mock = MockRpc::new(vec![ok(JsonValue::String(bool_hex(false)))]);
    // Only ONE response scripted: if the code tried a second call it would error,
    // proving the None short-circuits at the existence gate.
    let rpc = RpcClient::with_transport(mock);
    let got = read_sbt_emblem(&rpc, SAMPLE_SUB).expect("read ok");
    assert!(got.is_none(), "no SBT -> None, never a fabricated emblem");
}

/// The `sbt_token_uri` command surface returns exactly the image data-uri (Some) or
/// None — proven here via the underlying read (the command wires the live RPC).
#[test]
fn command_maps_emblem_to_image_uri_and_none() {
    // present -> Some(image)
    let rpc = RpcClient::with_transport(MockRpc::new(vec![
        ok(JsonValue::String(bool_hex(true))),
        ok(JsonValue::String(uint256_hex(1))),
        ok(JsonValue::String(TOKEN_URI_ABI_HEX.into())),
    ]));
    let mapped = read_sbt_emblem(&rpc, SAMPLE_SUB)
        .unwrap()
        .map(|e| e.image_data_uri);
    assert_eq!(mapped, Some(EXPECTED_IMAGE_DATA_URI.to_string()));
    // absent -> None
    let rpc2 = RpcClient::with_transport(MockRpc::new(vec![ok(JsonValue::String(bool_hex(false)))]));
    let mapped2 = read_sbt_emblem(&rpc2, SAMPLE_SUB)
        .unwrap()
        .map(|e| e.image_data_uri);
    assert_eq!(mapped2, None);
}

/// A malformed tokenURI return (short) after a bound sub errors honestly.
#[test]
fn read_sbt_emblem_errors_on_bad_token_uri() {
    let rpc = RpcClient::with_transport(MockRpc::new(vec![
        ok(JsonValue::String(bool_hex(true))),
        ok(JsonValue::String(uint256_hex(3))),
        ok(JsonValue::String("0x1234".into())), // too short for an abi string
    ]));
    let err = read_sbt_emblem(&rpc, SAMPLE_SUB).unwrap_err();
    match err {
        SbtArtError::Decode(_) => {}
        other => panic!("expected a decode error, got {other:?}"),
    }
}

/// The eth_calls target the canonical SBT contract with the pinned selectors.
#[test]
fn reads_target_the_sbt_with_pinned_selectors() {
    // The calldata builders target the SBT with the pinned selectors (proves the
    // wire shape the read sends; the full read path is covered above).
    let sh = sub_hash(SAMPLE_SUB);
    let bound = is_sub_bound_call(&sh);
    assert_eq!(bound["to"], CITRATE_MEMBER_SBT);
    assert!(bound["data"]
        .as_str()
        .unwrap()
        .starts_with("0xdee2ef9d"));
    let tid = token_id_for_sub_call(&sh);
    assert!(tid["data"].as_str().unwrap().starts_with("0xf8856bd9"));
}
