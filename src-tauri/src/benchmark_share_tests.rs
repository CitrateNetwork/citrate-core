//! HUP-S7.5 (US-7.3 AC3): core's recheck of the sidecar's BenchmarkRegistry payload.
//!
//! The fixture is the real sidecar's `POST /metering/benchmark` answer for a seeded day (agent id
//! 7, registry `0x…b1`), captured 2026-10-01 from citrate-agent-runtime `hup/n5-anchor-rest`.
//! Selector checked with `cast sig "record(uint256,bytes32,bytes32,uint256)"` = `0xfce25138`.

use super::*;

const FIXTURE: &str = include_str!("../tests/fixtures/anchor/sidecar-benchmark-payload.json");
const REGISTRY: &str = "0x00000000000000000000000000000000000000b1";
const DAY: &str = "2026-10-01";

fn payload() -> Payload {
    serde_json::from_str(FIXTURE).expect("fixture")
}

#[test]
fn selector_and_capsule_label_match_cast() {
    assert_eq!(hex::encode(record_selector()), "fce25138");
    assert_eq!(
        hex::encode(hermes_capsule_id()),
        "9bf1a6295bf6cad36e6ad3a5defce3c880372fa75b99e42b64ad8c530c2f8850"
    );
}

#[test]
fn the_real_payload_is_accepted_and_rebuilt_call_for_call() {
    let calls = validate(&payload(), REGISTRY, 7, DAY).expect("valid");
    assert_eq!(calls.len(), 15);
    assert_eq!(calls[0].metric, "hermes.daily.turns");
    assert_eq!(calls[0].value, "3");
    assert_eq!(calls[0].data.len(), 4 + 4 * 32);
    // The registry address is matched without regard to case.
    assert!(validate(
        &payload(),
        &REGISTRY.to_ascii_uppercase().replace("0X", "0x"),
        7,
        DAY
    )
    .is_ok());
}

fn refused(p: &Payload, registry: &str, agent: u128, day: &str, why: &str) {
    match validate(p, registry, agent, day) {
        Err(e) => assert!(e.contains(why), "{e} should mention {why}"),
        Ok(_) => panic!("expected a refusal mentioning {why}"),
    }
}

#[test]
fn the_envelope_must_be_the_one_core_expects() {
    let p = payload();
    refused(
        &p,
        "0x00000000000000000000000000000000000000b2",
        7,
        DAY,
        "pinned BenchmarkRegistry",
    );
    refused(&p, REGISTRY, 8, DAY, "another agent id");
    refused(&p, REGISTRY, 7, "2026-09-30", "another day");
    let mut q = p.clone();
    q.chain_id = 1;
    refused(&q, REGISTRY, 7, DAY, "chain");
    let mut q = p.clone();
    q.value = "1".into();
    refused(&q, REGISTRY, 7, DAY, "no value");
    let mut q = p.clone();
    q.sent = true;
    refused(&q, REGISTRY, 7, DAY, "already sent");
    let mut q = p.clone();
    q.function = "approve(address,uint256)".into();
    refused(&q, REGISTRY, 7, DAY, "unexpected function");
    let mut q = p.clone();
    q.capsule_id = format!("0x{}", "00".repeat(32));
    refused(&q, REGISTRY, 7, DAY, "capsule");
    let mut q = p.clone();
    q.calls.clear();
    refused(&q, REGISTRY, 7, DAY, "no calls");
    let mut q = p.clone();
    while q.calls.len() <= MAX_CALLS {
        let mut c = q.calls[0].clone();
        c.metric = format!("hermes.daily.x{}", q.calls.len());
        q.calls.push(c);
    }
    refused(&q, REGISTRY, 7, DAY, "more than");
}

#[test]
fn each_call_is_rebuilt_and_must_match_byte_for_byte() {
    let p = payload();
    // Calldata that differs from core's own build (another value hidden in the bytes).
    let mut q = p.clone();
    let mut data = q.calls[0].data.clone();
    data.replace_range(data.len() - 1.., "9");
    q.calls[0].data = data;
    refused(&q, REGISTRY, 7, DAY, "not the record call");
    // A value core cannot read.
    let mut q = p.clone();
    q.calls[0].value = "-1".into();
    refused(&q, REGISTRY, 7, DAY, "whole number");
    // A metric outside the Hermes namespace, or with an odd name.
    let mut q = p.clone();
    q.calls[0].metric = "other.metric".into();
    refused(&q, REGISTRY, 7, DAY, "unexpected metric");
    let mut q = p.clone();
    q.calls[0].metric = "hermes.daily.Turns".into();
    refused(&q, REGISTRY, 7, DAY, "unexpected metric");
    // A metric whose hash is not its name's.
    let mut q = p.clone();
    q.calls[0].metric_name = q.calls[1].metric_name.clone();
    refused(&q, REGISTRY, 7, DAY, "does not match its name");
    // The same metric twice.
    let mut q = p.clone();
    let dup = q.calls[0].clone();
    q.calls.push(dup);
    refused(&q, REGISTRY, 7, DAY, "appears twice");
}

#[test]
fn record_calldata_is_selector_then_four_words() {
    let cd = record_calldata(7, &[1u8; 32], &[2u8; 32], 5);
    assert_eq!(&cd[..4], &record_selector());
    assert_eq!(cd[4 + 31], 7);
    assert_eq!(&cd[36..68], &[1u8; 32]);
    assert_eq!(&cd[68..100], &[2u8; 32]);
    assert_eq!(cd[100 + 31], 5);
}

#[test]
fn the_agent_id_is_the_first_agent_sbt_and_none_means_no_sharing() {
    let mut st = crate::agent_sbt::AgentSbtStatus {
        contract: None,
        member: "0x1111111111111111111111111111111111111111".into(),
        did: None,
        parent_org_id: "0".into(),
        balance: None,
        tokens: None,
        tokens_note: None,
        state: crate::agent_sbt::MintState::NotInBook,
        available: false,
        message: String::new(),
    };
    assert!(agent_id_of(&st).unwrap_err().contains("AgentSBT"));
    st.tokens = Some(vec![]);
    assert!(agent_id_of(&st).is_err());
    st.tokens = Some(vec![crate::agent_sbt::AgentToken {
        token_id: "12".into(),
        parent_org_id: "0".into(),
        did: String::new(),
        pubkey_fingerprint: String::new(),
        quarantined: false,
    }]);
    assert_eq!(agent_id_of(&st), Ok(12));
}

#[test]
fn a_day_is_prepared_once_per_session_unless_released() {
    let day = "1999-01-02";
    assert!(reserve_day(day));
    assert!(!reserve_day(day));
    release_day(day);
    assert!(reserve_day(day));
    release_day(day);
}
