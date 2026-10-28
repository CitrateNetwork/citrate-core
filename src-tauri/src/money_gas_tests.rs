// citrate-core#501 class: a money send's gas limit is the node's estimate plus a margin, never a
// fixed number. Each builder below is driven against a node that answers `eth_estimateGas`; the
// tripwire at the end fails CI when a fixed gas limit comes back into a transaction core builds.
use super::*;
use crate::rpc::RpcError;
use serde_json::{json, Value};
use std::cell::RefCell;

const FROM: &str = "0x9858effd232b4033e47d90003d41ec34ecaeda94";
const TO: &str = "0x1111111111111111111111111111111111111111";
/// What the test node estimates: above every fixed limit the builders used to send with.
const ESTIMATE: u64 = 960_000;
const WITH_MARGIN: u64 = 1_200_000;

/// A node that answers `eth_estimateGas` (and nothing else) and records each estimate call.
struct GasNode {
    answer: Result<u64, String>,
    calls: RefCell<Vec<Value>>,
}

impl GasNode {
    fn estimating(gas: u64) -> RpcClient<GasNode> {
        RpcClient::with_transport(GasNode {
            answer: Ok(gas),
            calls: RefCell::default(),
        })
    }
    fn refusing(reason: &str) -> RpcClient<GasNode> {
        RpcClient::with_transport(GasNode {
            answer: Err(reason.to_string()),
            calls: RefCell::default(),
        })
    }
}

impl RpcTransport for GasNode {
    fn call(&self, body: Value) -> Result<Value, RpcError> {
        if body["method"] != "eth_estimateGas" {
            return Err(RpcError::Node(format!("unexpected {}", body["method"])));
        }
        self.calls.borrow_mut().push(body["params"][0].clone());
        match &self.answer {
            Ok(g) => Ok(json!({"jsonrpc": "2.0", "id": body["id"], "result": format!("0x{g:x}")})),
            Err(e) => Ok(
                json!({"jsonrpc": "2.0", "id": body["id"], "error": {"code": 3, "message": e}}),
            ),
        }
    }
}

/// Every money send core builds, from `FROM`, as the ceremony's tx JSON.
fn money_sends(rpc: &RpcClient<GasNode>) -> Vec<(&'static str, Result<String, String>)> {
    let activate = vec![0xAB; 164];
    let register = crate::model_register::register_model_calldata(
        "m",
        "gguf",
        "1",
        "bafy",
        1,
        1,
        &crate::model_register::ModelMetadata {
            description: String::new(),
            license: String::new(),
            tags: vec![],
        },
    );
    vec![
        (
            "wallet_send (native transfer)",
            crate::transfer::transfer_intent(rpc, FROM, TO, 1_500_000_000_000_000_000)
                .map(|i| i.raw),
        ),
        (
            "MemberBond.activate",
            crate::node::activate_tx_json(rpc, FROM, TO, &activate),
        ),
        (
            "InferenceRouter.claimRefund",
            crate::inference_router::claim_refund_intent(rpc, FROM, TO).map(|i| i.raw),
        ),
        (
            "ModelRegistry.registerModel",
            crate::model_register::register_model_intent(rpc, FROM, &register).map(|i| i.raw),
        ),
    ]
}

#[test]
fn the_margin_is_a_quarter_over_the_estimate_and_the_cap_refuses_a_runaway() {
    assert_eq!(with_margin(160_500), Ok(200_625), "#501: the pool's real cost");
    assert_eq!(with_margin(21_000), Ok(26_250));
    assert_eq!(
        with_margin(2_800_000),
        Ok(MAX_SEND_GAS),
        "the margin stops at the cap, still above the estimate"
    );
    assert_eq!(with_margin(MAX_SEND_GAS), Ok(MAX_SEND_GAS));
    assert!(
        with_margin(MAX_SEND_GAS + 1).is_err(),
        "an estimate above the cap is refused"
    );
    assert!(with_margin(u64::MAX).is_err(), "no overflow, refused");
    assert!(with_margin(0).is_err(), "a zero estimate is not a gas limit");
}

#[test]
fn every_money_send_carries_the_nodes_estimate_plus_the_margin() {
    let rpc = GasNode::estimating(ESTIMATE);
    let sends = money_sends(&rpc);
    let calls = rpc.transport().calls.borrow().clone();
    assert_eq!(calls.len(), sends.len(), "one estimate per send: {calls:?}");
    for ((name, raw), call) in sends.into_iter().zip(calls) {
        let raw = raw.unwrap_or_else(|e| panic!("{name}: {e}"));
        let (tx, _) = crate::txdecode::decode_transaction(&raw)
            .unwrap_or_else(|| panic!("{name}: the tx JSON decodes"));
        assert_eq!(
            tx.gas_limit,
            Some(WITH_MARGIN),
            "{name}: gas = the estimate plus the margin, not a fixed limit"
        );
        // The estimate was for this exact call: same sender, target, value and calldata.
        let sent: Value = serde_json::from_str(&raw).unwrap_or_default();
        for k in ["from", "to", "value", "data"] {
            assert_eq!(
                call[k].as_str().map(str::to_ascii_lowercase),
                sent[k].as_str().map(str::to_ascii_lowercase),
                "{name}: the estimate's {k}"
            );
        }
    }
}

#[test]
fn a_send_the_node_cannot_estimate_is_not_proposed() {
    let rpc = GasNode::refusing("execution reverted: Too early");
    for (name, raw) in money_sends(&rpc) {
        let err = raw.err().unwrap_or_else(|| panic!("{name}: built a send the node refused"));
        assert!(
            err.contains("execution reverted: Too early"),
            "{name}: the node's reason reaches the member: {err}"
        );
    }
}

#[test]
fn a_send_estimated_above_the_cap_is_not_proposed() {
    let rpc = GasNode::estimating(MAX_SEND_GAS + 1);
    for (name, raw) in money_sends(&rpc) {
        assert!(raw.is_err(), "{name}: signed a runaway gas limit");
    }
}

// ------------------------------------------------------------------------ tripwire

/// Fixed gas limits that are allowed to stay, each with why. An entry whose code is gone is fine.
const FIXED_GAS_ALLOWED: &[(&str, &str, &str)] = &[
    (
        "hermes_learn.rs",
        "REGISTER_SKILL_GAS",
        "a zero-value skill publish, switched off (SKILL_PUBLISH_ENABLED); not a money send",
    ),
    (
        "contract_deploy.rs",
        "DEFAULT_DEPLOY_GAS",
        "a contract creation's default limit, which the caller sets; not a money send",
    ),
];

/// The production part of a source file: up to its first `#[cfg(test)]` module.
fn production(src: &str) -> &str {
    src.find("#[cfg(test)]\nmod ").map_or(src, |i| &src[..i])
}

/// Fixed gas limits in `src`: `const X_GAS: u64 = <literal>` (bounds named MAX_/MIN_ are caps,
/// not limits) and a constant written straight into a tx JSON `"gas"` field.
fn fixed_gas(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in production(src).lines() {
        let t = line.trim_start();
        let t = t.strip_prefix("pub(crate) ").or(t.strip_prefix("pub ")).unwrap_or(t);
        if let Some(rest) = t.strip_prefix("const ") {
            if let Some((name, value)) = rest.split_once(": u64 =") {
                let literal = value.trim().trim_end_matches(';').replace('_', "");
                if (name.ends_with("_GAS") || name.ends_with("_GAS_LIMIT"))
                    && !name.starts_with("MAX_")
                    && !name.starts_with("MIN_")
                    && literal.parse::<u64>().is_ok()
                {
                    out.push(name.to_string());
                }
            }
        }
        if let Some(i) = t.find("\"gas\": format!(\"0x{") {
            let ident: String = t[i + 19..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !ident.is_empty() && ident.chars().all(|c| !c.is_ascii_lowercase()) {
                out.push(ident);
            }
        }
        if let Some(i) = t.find("\"gas\": \"0x") {
            let hex: String = t[i + 10..]
                .chars()
                .take_while(char::is_ascii_hexdigit)
                .collect();
            out.push(format!("literal 0x{hex}"));
        }
    }
    out.sort();
    out.dedup();
    out
}

#[test]
fn tripwire_the_scan_finds_a_fixed_gas_limit() {
    let src = "const SEND_GAS: u64 = 150_000;\nconst MAX_SEND_GAS: u64 = 1;\n\
               fn f() { json!({\"gas\": format!(\"0x{OTHER_GAS:x}\"), \"g\": format!(\"0x{gas:x}\")}) }\n\
               fn g() { json!({\"gas\": \"0x30d40\"}) }\n\
               #[cfg(test)]\nmod tests { const TEST_GAS: u64 = 1; }";
    assert_eq!(fixed_gas(src), vec!["OTHER_GAS", "SEND_GAS", "literal 0x30d40"]);
}

#[test]
fn tripwire_no_transaction_core_builds_has_a_fixed_gas_limit() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap_or_else(|e| panic!("{e}")).path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        if !name.ends_with(".rs") || name.ends_with("_tests.rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        for ident in fixed_gas(&src) {
            if !FIXED_GAS_ALLOWED
                .iter()
                .any(|(f, i, _)| *f == name && *i == ident)
            {
                found.push(format!("{name}: {ident}"));
            }
        }
    }
    found.sort();
    assert!(
        found.is_empty(),
        "a transaction core builds uses a fixed gas limit; estimate it with money_gas::estimate \
         (citrate-core#501): {found:?}"
    );
}
