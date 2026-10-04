// Hermes P3 / WP3.2 — contract-deploy (creation-tx) tests. Pure — no signing, no chain.
use super::*;

#[test]
fn initcode_is_bytecode_then_constructor_args() {
    let code = [0x60, 0x80, 0x60, 0x40]; // a bytecode prefix
    let args = [0xaa; 32]; // one ABI word
    let ic = deploy_initcode(&code, &args);
    assert_eq!(&ic[..4], &code);
    assert_eq!(&ic[4..], &args);
    // No args → init code is exactly the bytecode.
    assert_eq!(deploy_initcode(&code, &[]), code);
}

#[test]
fn parse_hex_accepts_prefixed_bare_and_empty_and_rejects_junk() {
    assert_eq!(parse_hex("0xdeadbeef", "b").unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
    assert_eq!(parse_hex("deadbeef", "b").unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
    assert!(parse_hex("", "b").unwrap().is_empty());
    assert!(parse_hex("0xnothex", "b").is_err());
}

#[test]
fn deploy_tx_json_is_a_to_less_creation_the_decoder_renders_honestly() {
    // A real (tiny) init code. The produced tx JSON must decode as a contract creation
    // (to == None) so the ceremony shows the honest "contract creation" action — and,
    // because the action is recognized, it is approvable (not stuck behind a raw-ack).
    let initcode = hex::decode("6080604052").unwrap();
    let raw = encode_deploy_tx_json("0x1111111111111111111111111111111111111111", &initcode, 0, 2_000_000);

    // No recipient in the JSON.
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(v.get("to").is_none(), "a creation tx carries no `to`");
    assert_eq!(v["data"], "0x6080604052");
    assert_eq!(v["chainId"], "0x9d0c"); // 40204

    // The shared decoder agrees: to == None, action == "contract creation".
    let (parsed, display) = crate::txdecode::decode_transaction(&raw).expect("decodes");
    assert!(parsed.to.is_none(), "decoded as contract creation");
    assert!(
        display.action.to_lowercase().contains("contract"),
        "human sees a contract deploy/creation, got {:?}",
        display.action
    );
}

#[test]
fn value_defaults_to_zero_and_encodes_as_hex() {
    let raw = encode_deploy_tx_json("0xabc", &[0x00], 0, 21000);
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["value"], "0x0");
    let raw2 = encode_deploy_tx_json("0xabc", &[0x00], 1_000_000_000_000_000_000, 21000);
    let v2: serde_json::Value = serde_json::from_str(&raw2).unwrap();
    assert_eq!(v2["value"], "0xde0b6b3a7640000"); // 1 SALT in wei
}

// ---------------------------------------------------------------- HUP-S6.4 D-4 deploy gate

/// The deploy body consults the gate BEFORE it touches the wallet or opens a ceremony, and it
/// checks the gate against the same `initcode` bytes the creation tx carries (no second parse).
#[test]
fn contract_deploy_requires_a_ready_gate_before_any_ceremony() {
    let src = include_str!("contract_deploy.rs");
    // The command body delegates to `propose_deploy` (the one deploy body, also driven by the
    // hello-mint end-to-end run), and holds no deploy logic of its own.
    let command = src
        .split("pub fn contract_deploy_sync(")
        .nth(1)
        .and_then(|b| b.split("pub fn propose_deploy(").next())
        .expect("contract_deploy_sync exists");
    assert!(command.contains("propose_deploy("), "the command delegates");
    assert!(!command.contains(".request("), "no second ceremony path in the command");
    let body = src
        .split("pub fn propose_deploy(")
        .nth(1)
        .and_then(|b| b.split("#[cfg(test)]").next())
        .expect("propose_deploy exists");
    let gate_at = body
        .find(".require_ready(&initcode)")
        .expect("the deploy body checks the D-4 gate on the initcode");
    let wallet_at = body.find("address_auto_unlocked").expect("wallet read");
    let cer_at = body.find(".request(intent)").expect("ceremony request");
    let encode_at = body.find("encode_deploy_tx_json(").expect("tx encode");
    assert!(
        gate_at < wallet_at && gate_at < cer_at && gate_at < encode_at,
        "gate first"
    );
    // The ceremony is opened INSIDE the gate store (re-checked under its lock, and remembered so
    // a later NOT READY for this hash rejects it).
    let open_at = body
        .find(".open_ceremony(")
        .expect("ceremony opened through the gate store");
    assert!(
        body[open_at..]
            .trim_start_matches(".open_ceremony(")
            .trim_start()
            .starts_with("&initcode,"),
        "the gate store opens the ceremony for the gated initcode"
    );
    assert!(open_at < cer_at, "request happens inside open_ceremony");
    assert_eq!(
        body.matches(".request(").count(),
        1,
        "exactly one ceremony request"
    );
    // The tx is encoded from the very `initcode` the gate checked.
    assert!(
        body[encode_at..].contains("&initcode"),
        "the ceremony carries the gated bytes"
    );
}

#[test]
fn deploy_proposal_flattens_the_ceremony_view_and_adds_the_gate() {
    let view =
        crate::ceremony::SignatureCeremony::new().request(crate::ceremony::SignatureIntent {
            origin: "local-user".into(),
            kind: crate::ceremony::IntentKind::Transaction,
            chain_id: 40204,
            raw: encode_deploy_tx_json(
                "0x1111111111111111111111111111111111111111",
                &[0x60, 0x00],
                0,
                21000,
            ),
        });
    let gate = crate::deploy_gate::GateRecord {
        initcode_hash: "0xaa".into(),
        binding_hash: "0xbb".into(),
        compiler: crate::deploy_gate::CompilerSettings {
            solc_version: "0.8.28".into(),
            optimizer: true,
            optimizer_runs: 200,
            evm_version: "cancun".into(),
            via_ir: false,
        },
        verdict: crate::deploy_gate::Verdict::Ready,
        items: vec![],
        evaluated_at_ms: 1,
    };
    let p = DeployProposal {
        ceremony: view.clone(),
        gate,
    };
    let v = serde_json::to_value(&p).expect("serializes");
    // A superset of CeremonyView: existing callers keep reading id / chainId / decoded.
    assert_eq!(v["id"], view.id);
    assert_eq!(v["chainId"], 40204);
    assert!(v["decoded"].is_object());
    assert_eq!(v["gate"]["verdict"], "READY");
    assert_eq!(v["gate"]["initcodeHash"], "0xaa");
}
