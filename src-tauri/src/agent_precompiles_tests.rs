// HUP-S7.2 follow-up (US-7.5 AC1): core's agent precompile encoders and decoders, pinned against
// the vectors citrate-chain generates with its own Rust encoders and precompile functions
// (citrate-chain `core/execution/tests/agent_precompile_vectors.rs`). Every input core builds must
// be byte-identical to the chain's, and every chain answer must decode to the pinned meaning.

use super::*;
use crate::anchor_proof::BatchHeader;
use serde_json::{json, Value};

const VECTORS: &str = include_str!("../tests/fixtures/precompiles/agent_precompile_vectors.json");

fn vectors() -> Value {
    serde_json::from_str(VECTORS).unwrap_or_else(|e| panic!("vectors are JSON: {e}"))
}

fn arr<'a>(v: &'a Value, key: &str) -> &'a Vec<Value> {
    v[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} is an array"))
}

fn tensor(v: &Value) -> Q16Tensor {
    serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("tensor {v}: {e}"))
}

fn unhex(s: &str) -> Vec<u8> {
    hex::decode(s.trim_start_matches("0x")).unwrap_or_else(|e| panic!("hex {s}: {e}"))
}

fn s(v: &Value) -> String {
    v.as_str()
        .unwrap_or_else(|| panic!("{v} is a string"))
        .to_string()
}

fn u(v: &Value) -> u64 {
    v.as_u64().unwrap_or_else(|| panic!("{v} is a u64"))
}

fn proof_of(x: &Value) -> AnchorProof {
    AnchorProof {
        header: BatchHeader {
            v: u(&x["v"]) as u32,
            day: u(&x["day"]),
            first_seq: u(&x["first_seq"]),
            last_seq: u(&x["last_seq"]),
            count: u(&x["count"]),
            tree_root: s(&x["tree_root"]),
        },
        seq: u(&x["seq"]),
        leaf_index: u(&x["leaf_index"]),
        record_hash: s(&x["record_hash"]),
        path: x["path"]
            .as_array()
            .unwrap_or_else(|| panic!("path"))
            .iter()
            .map(s)
            .collect(),
    }
}

fn link_of(x: &Value) -> DeviceLinkWire {
    DeviceLinkWire {
        member: s(&x["member"]),
        device: s(&x["device"]),
        wallet: s(&x["wallet"]),
        index: u(&x["index"]) as u32,
        label: s(&x["label"]),
        issued_at: u(&x["issued_at"]),
        member_sig: format!("0x{}", s(&x["member_sig"])),
        device_sig: s(&x["device_sig"]),
        wallet_sig: s(&x["wallet_sig"]),
    }
}

fn revocation_of(x: &Value) -> RevocationWire {
    RevocationWire {
        member: format!("0x{}", s(&x["member"])),
        device: s(&x["device"]),
        revoked_at: u(&x["revoked_at"]),
        member_sig: s(&x["member_sig"]),
    }
}

#[test]
fn vectors_are_the_chain_generated_set() {
    let v = vectors();
    assert_eq!(v["version"], json!(1));
    assert_eq!(
        v["generator"],
        json!("citrate-chain core/execution/tests/agent_precompile_vectors.rs")
    );
    for (name, short) in [
        ("LORA_APPLY", LORA_APPLY),
        ("LORA_MERGE", LORA_MERGE),
        ("MEMORY_ANCHOR_VERIFY", MEMORY_ANCHOR_VERIFY),
        ("AGENT_OPS", AGENT_OPS),
    ] {
        assert_eq!(v["addresses"][name], json!(address(short)), "{name}");
    }
    for key in [
        "lora_apply",
        "lora_merge",
        "memory_anchor",
        "device_link",
        "device_revocation",
    ] {
        assert!(arr(&v, key).len() >= 2, "{key} has at least two vectors");
    }
}

#[test]
fn lora_apply_inputs_match_the_chain_encoder_and_outputs_decode() {
    for x in arr(&vectors(), "lora_apply") {
        let alpha = x["alpha"].as_i64().unwrap_or_else(|| panic!("alpha"));
        let (input, gas) =
            lora_apply_input(&tensor(&x["w"]), &tensor(&x["b"]), &tensor(&x["a"]), alpha)
                .unwrap_or_else(|e| panic!("{}: {e}", x["name"]));
        assert_eq!(hex::encode(&input), s(&x["input"]), "{}", x["name"]);
        assert_eq!(gas, u(&x["gas"]), "{} gas", x["name"]);
        let out = decode_lora_output(&unhex(&s(&x["output"]))).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(out, tensor(&x["out"]), "{}", x["name"]);
        // Round trip: the decoded answer re-encodes to the chain's bytes.
        assert_eq!(
            hex::encode(out.encode().unwrap_or_default()),
            s(&x["output"])
        );
    }
}

#[test]
fn lora_merge_inputs_match_the_chain_encoder_and_outputs_decode() {
    for x in arr(&vectors(), "lora_merge") {
        let adapters: Vec<LoraAdapter> =
            serde_json::from_value(x["adapters"].clone()).unwrap_or_else(|e| panic!("{e}"));
        let (input, gas) =
            lora_merge_input(&adapters).unwrap_or_else(|e| panic!("{}: {e}", x["name"]));
        assert_eq!(hex::encode(&input), s(&x["input"]), "{}", x["name"]);
        assert_eq!(gas, u(&x["gas"]), "{} gas", x["name"]);
        let out = decode_lora_output(&unhex(&s(&x["output"]))).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(out, tensor(&x["out"]), "{}", x["name"]);
    }
}

#[test]
fn memory_anchor_inputs_match_and_core_agrees_with_the_chain_answer() {
    for x in arr(&vectors(), "memory_anchor") {
        let p = proof_of(x);
        let input = memory_anchor_input(&p).unwrap_or_else(|e| panic!("{}: {e}", x["name"]));
        assert_eq!(hex::encode(&input), s(&x["input"]), "{}", x["name"]);
        assert_eq!(
            memory_anchor_gas(p.path.len()),
            u(&x["gas"]),
            "{} gas",
            x["name"]
        );
        let answer =
            decode_memory_anchor_output(&unhex(&s(&x["output"]))).unwrap_or_else(|e| panic!("{e}"));
        // Core's own verifier (anchor_proof, an independent copy) reaches the chain's verdict.
        match (answer, anchor_proof::check_inclusion(&p)) {
            (AnchorAnswer::Commitment(c), Ok(mine)) => {
                assert_eq!(c, mine, "{}", x["name"]);
                assert_eq!(hex::encode(c), s(&x["commitment"]));
                assert_eq!(x["valid"], json!(true));
            }
            (AnchorAnswer::Invalid, Err(_)) => {
                assert_eq!(x["valid"], json!(false), "{}", x["name"])
            }
            (a, m) => panic!("{}: chain {a:?}, core {m:?}", x["name"]),
        }
    }
}

#[test]
fn device_link_inputs_match_including_the_signed_text() {
    for x in arr(&vectors(), "device_link") {
        let e = device_link_input(&link_of(x)).unwrap_or_else(|e| panic!("{}: {e}", x["name"]));
        assert_eq!(hex::encode(&e.input), s(&x["input"]), "{}", x["name"]);
        assert_eq!(e.input[0], OP_DEVICE_LINK_VERIFY);
        assert_eq!(e.body(), &e.input[1..]);
        assert_eq!(e.gas, u(&x["gas"]), "{} gas", x["name"]);
        assert_eq!(e.message, s(&x["message"]), "{}", x["name"]);
        let valid =
            decode_agent_ops_output(&unhex(&s(&x["output"]))).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(json!(valid), x["valid"], "{}", x["name"]);
    }
}

#[test]
fn device_revocation_inputs_match_including_the_signed_text() {
    for x in arr(&vectors(), "device_revocation") {
        let e = device_revocation_input(&revocation_of(x))
            .unwrap_or_else(|e| panic!("{}: {e}", x["name"]));
        assert_eq!(hex::encode(&e.input), s(&x["input"]), "{}", x["name"]);
        assert_eq!(e.input[0], OP_DEVICE_REVOCATION_VERIFY);
        assert_eq!(e.gas, u(&x["gas"]), "{} gas", x["name"]);
        assert_eq!(e.message, s(&x["message"]), "{}", x["name"]);
        let valid =
            decode_agent_ops_output(&unhex(&s(&x["output"]))).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(json!(valid), x["valid"], "{}", x["name"]);
    }
}

#[test]
fn encoders_refuse_what_the_chain_refuses() {
    let m = |shape: &[u32], n: usize| Q16Tensor {
        shape: shape.to_vec(),
        q16: vec![0; n],
    };
    // Shapes must agree, ranks 1..=64, tiles at most 256 x 256, tensors must be matrices.
    assert!(lora_apply_input(&m(&[2, 2], 4), &m(&[2, 1], 2), &m(&[1, 2], 2), 1).is_ok());
    assert!(lora_apply_input(&m(&[2, 2], 4), &m(&[3, 1], 3), &m(&[1, 2], 2), 1).is_err());
    assert!(lora_apply_input(&m(&[2, 2], 4), &m(&[2, 65], 130), &m(&[65, 2], 130), 1).is_err());
    assert!(lora_apply_input(&m(&[257, 1], 257), &m(&[257, 1], 257), &m(&[1, 1], 1), 1).is_err());
    assert!(lora_apply_input(&m(&[4], 4), &m(&[2, 1], 2), &m(&[1, 2], 2), 1).is_err());
    // An element count that does not fit its shape.
    assert!(lora_apply_input(&m(&[2, 2], 3), &m(&[2, 1], 2), &m(&[1, 2], 2), 1).is_err());
    // Merge: 1..=16 adapters of one tile size.
    let ad = |d: u32, r: u32, k: u32| LoraAdapter {
        b: m(&[d, r], (d * r) as usize),
        a: m(&[r, k], (r * k) as usize),
        alpha: 1,
        weight: 1,
    };
    assert!(lora_merge_input(&[]).is_err());
    assert!(lora_merge_input(&vec![ad(1, 1, 1); 17]).is_err());
    assert!(lora_merge_input(&vec![ad(1, 1, 1); 16]).is_ok());
    assert!(lora_merge_input(&[ad(2, 1, 2), ad(2, 3, 2)]).is_ok());
    assert!(lora_merge_input(&[ad(2, 1, 2), ad(2, 1, 3)]).is_err());
    // Anchor: at most 64 path hashes, 32-byte hashes.
    let v = vectors();
    let x = &arr(&v, "memory_anchor")[0];
    let mut p = proof_of(x);
    p.path = vec![p.path[0].clone(); ANCHOR_MAX_PATH + 1];
    assert!(memory_anchor_input(&p).is_err());
    let mut p = proof_of(x);
    p.record_hash = "ab".into();
    assert!(memory_anchor_input(&p).is_err());
    // Device link: label rule, index cap, a device key of its own, 65-byte signatures.
    let l = &arr(&v, "device_link")[0];
    let mut w = link_of(l);
    w.label = " Linux box".into();
    assert!(device_link_input(&w).is_err());
    let mut w = link_of(l);
    w.index = 1024;
    assert!(device_link_input(&w).is_err());
    let mut w = link_of(l);
    w.device = w.member.clone();
    assert!(device_link_input(&w).is_err());
    let mut w = link_of(l);
    w.wallet_sig = format!("0x{}", "11".repeat(64));
    assert!(device_link_input(&w).is_err());
    let mut r = revocation_of(&arr(&v, "device_revocation")[0]);
    r.member = "0x1234".into();
    assert!(device_revocation_input(&r).is_err());
}

#[test]
fn decoders_never_read_empty_or_odd_answers_as_results() {
    // An inactive precompile (below the fork) gives empty data: an error, not "invalid".
    assert!(decode_memory_anchor_output(&[]).is_err());
    assert!(decode_agent_ops_output(&[]).is_err());
    assert!(decode_lora_output(&[]).is_err());
    let mut two = [0u8; 32];
    two[31] = 2;
    assert!(decode_agent_ops_output(&two).is_err());
    let mut high = [0u8; 32];
    high[0] = 1;
    high[31] = 1;
    assert!(decode_agent_ops_output(&high).is_err());
    assert_eq!(
        decode_memory_anchor_output(&[0u8; 32]),
        Ok(AnchorAnswer::Invalid)
    );
    // A LoRA answer is a Q16.16 matrix; a scalar, a field tensor or trailing bytes are refused.
    assert!(decode_lora_output(&Q16Tensor::scalar(5).encode().unwrap_or_default()).is_err());
    let mut field = Q16Tensor {
        shape: vec![1, 1],
        q16: vec![1],
    }
    .encode()
    .unwrap_or_default();
    field[9] = 0x02;
    assert!(decode_lora_output(&field).is_err());
    let mut trailing = Q16Tensor {
        shape: vec![1, 1],
        q16: vec![1],
    }
    .encode()
    .unwrap_or_default();
    trailing.push(0);
    assert!(decode_lora_output(&trailing).is_err());
}

#[test]
fn json_entry_points_encode_and_decode_every_operation() {
    let v = vectors();
    let ap = &arr(&v, "lora_apply")[1];
    let e = encode_json(
        "LORA_APPLY",
        &json!({"w": ap["w"], "b": ap["b"], "a": ap["a"], "alpha": ap["alpha"]}),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(e["address"], json!(address(LORA_APPLY)));
    assert_eq!(e["input"], json!(format!("0x{}", s(&ap["input"]))));
    assert_eq!(e["gas"], ap["gas"]);
    let t = &e["tensors"];
    let joined = [&t["w"], &t["b"], &t["a"], &t["alpha"]]
        .iter()
        .map(|x| s(x).trim_start_matches("0x").to_string())
        .collect::<String>();
    assert_eq!(joined, s(&ap["input"]));
    // Reroll 2026-10-05 (owner decision 2026-10-04): active from genesis on 40204, no gate.
    assert!(s(&e["note"]).contains("from genesis on chain 40204"));
    assert!(!s(&e["note"]).contains("not scheduled"));
    let d = decode_json("LORA_APPLY", &format!("0x{}", s(&ap["output"])))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(d["q16"], ap["out"]["q16"]);
    assert_eq!(d["values"][0], json!(-4.5));

    let mg = &arr(&v, "lora_merge")[0];
    let e = encode_json("LORA_MERGE", &json!({"adapters": mg["adapters"]}))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(e["input"], json!(format!("0x{}", s(&mg["input"]))));
    assert_eq!(e["gas"], mg["gas"]);

    let an = &arr(&v, "memory_anchor")[0];
    let e = encode_json(
        "MEMORY_ANCHOR_VERIFY",
        &json!({"proof": serde_json::to_value(proof_of(an)).unwrap_or_default()}),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(e["input"], json!(format!("0x{}", s(&an["input"]))));
    assert_eq!(
        e["expected"]["commitment"],
        json!(format!("0x{}", s(&an["commitment"])))
    );
    let d = decode_json("MEMORY_ANCHOR_VERIFY", &format!("0x{}", s(&an["output"])))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(d["valid"], json!(true));
    let bad = &arr(&v, "memory_anchor")[1];
    let e = encode_json(
        "MEMORY_ANCHOR_VERIFY",
        &json!({"proof": serde_json::to_value(proof_of(bad)).unwrap_or_default()}),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(e["expected"]["valid"], json!(false));

    let dl = &arr(&v, "device_link")[0];
    let e = encode_json(
        "DEVICE_LINK_VERIFY",
        &serde_json::to_value(link_of(dl)).unwrap_or_default(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(e["address"], json!(address(AGENT_OPS)));
    assert_eq!(e["body"], json!(format!("0x{}", &s(&dl["input"])[2..])));
    assert_eq!(e["message"], dl["message"]);
    let d = decode_json("DEVICE_LINK_VERIFY", &format!("0x{}", s(&dl["output"])))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(d["valid"], json!(true));

    let dr = &arr(&v, "device_revocation")[1];
    let e = encode_json(
        "DEVICE_REVOCATION_VERIFY",
        &serde_json::to_value(revocation_of(dr)).unwrap_or_default(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(e["input"], json!(format!("0x{}", s(&dr["input"]))));
    let d = decode_json(
        "DEVICE_REVOCATION_VERIFY",
        &format!("0x{}", s(&dr["output"])),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(d["valid"], json!(false));

    assert!(encode_json("LORA_TRAIN", &json!({})).is_err());
    assert!(encode_json("LORA_APPLY", &json!({"w": ap["w"]})).is_err());
    assert!(encode_json(
        "LORA_APPLY",
        &json!({"w": ap["w"], "b": ap["b"], "a": ap["a"], "alpha": 1, "extra": 1})
    )
    .is_err());
    assert!(decode_json("LORA_APPLY", "00").is_err());
    assert!(decode_json("AGENT_OPS", "0x").is_err());
}
