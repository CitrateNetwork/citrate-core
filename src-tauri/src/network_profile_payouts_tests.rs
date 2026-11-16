// G-SEC PAY-14 / PAY-11: the typed `payoutsOpen` flag and the FL code-hash pins in the network
// profile, and the payouts policy the signer is given from them.

const R1007_PAYOUTS: &str = "r1007";

fn r1007_profile() -> &'static NetworkProfile {
    profile(R1007_PAYOUTS).expect("the r1007 profile exists")
}

/// The r1007 root book's code hashes (citrate-chain origin/main
/// `contracts/addresses/r1007/40204.provenance.json`, keccak256 of `eth_getCode` at the probe
/// block; re-checked against the live chain at block 158248 on 2026-11-15).
const R1007_PINS: [(&str, &str, &str); 3] = [
    (
        "FederatedRoundLedger",
        "0x25fc02fbfad9589d0dde84f858d0962bfa0253c5",
        "0x542a2cb3ea0febdf15ecd02774af4d7765d7e6c3e9558cb9b00305fb3bc28455",
    ),
    (
        "PatronageLedger",
        "0x41f68b9e710585b064459a0ec706148fbf503659",
        "0xd59fb0af578034a325bf2c3f01c5496ae4af03f77180fd98c96ad2f7dc4834f9",
    ),
    (
        "LoRAFactory",
        "0x62e9aabec26288e60db2503f518eacf9cdcc8890",
        "0x56e3c1a10501f054f0667ffc606f42d05e1a6548252743d085c3aedf75121697",
    ),
];

#[test]
fn r1007_ships_with_payouts_dark() {
    let p = r1007_profile();
    assert!(!p.payouts_open, "0.5.4 ships payouts dark (owner 2026-11-16)");
    let policy = p.payouts_policy();
    assert_eq!(policy.network, "r1007");
    assert!(!policy.payouts_open);
}

#[test]
fn r1007_pins_the_three_deployed_fl_contracts_from_the_root_book() {
    let p = r1007_profile();
    let names: Vec<&str> = p.fl_code_pins.iter().map(|x| x.name).collect();
    assert_eq!(names, FL_CONTRACT_NAMES);
    for (name, address, hash) in R1007_PINS {
        let pin = p.fl_code_pins.iter().find(|x| x.name == name).expect(name);
        assert_eq!(pin.address.as_deref(), Some(address), "{name}");
        assert_eq!(pin.code_hash.as_deref(), Some(hash), "{name}");
    }
    for name in ["WorkMarket", "FLRoundVerifier", "DatasetRegistry"] {
        let pin = p.fl_code_pins.iter().find(|x| x.name == name).expect(name);
        assert_eq!((pin.address.as_deref(), pin.code_hash.as_deref()), (None, None), "{name} is not on r1007");
        assert!(pin.pending.as_deref().is_some_and(|w| w.contains("not deployed on r1007")), "{name}");
    }
    let policy = p.payouts_policy();
    let wm = policy
        .pins
        .iter()
        .find(|x| x.contract == crate::payouts_gate::FlContract::WorkMarket)
        .expect("WorkMarket is named");
    assert_eq!((wm.address, wm.code_hash), (None, None));
}

/// The pinned addresses are the address book's, so the gate guards the contracts the app calls.
#[test]
fn fl_pins_agree_with_the_address_book() {
    let book: serde_json::Value =
        serde_json::from_str(include_str!("../addresses/40204.json")).expect("book");
    for pin in &r1007_profile().fl_code_pins {
        let in_book = book["addresses"][pin.name].as_str().map(str::to_ascii_lowercase);
        assert_eq!(pin.address, in_book, "{}", pin.name);
    }
}

#[test]
fn no_other_profile_opens_payouts() {
    for p in profiles().expect("profiles") {
        assert!(!p.payouts_open, "{} opens payouts", p.id);
        assert!(!p.payouts_policy().payouts_open, "{}", p.id);
    }
}

/// The shipped file with r1007 edited by `edit`, parsed; the r1007 profile from it.
fn r1007_with(edit: impl Fn(&mut serde_json::Value)) -> Result<NetworkProfile, ProfileError> {
    let mut v: serde_json::Value = serde_json::from_str(PROFILES_JSON).expect("json");
    edit(&mut v["profiles"]["r1007"]);
    let (_, all) = parse(&v.to_string())?;
    Ok(all.into_iter().find(|p| p.id == "r1007").expect("r1007"))
}

#[test]
fn payouts_open_is_a_typed_bool() {
    let p = r1007_with(|r| r["payoutsOpen"] = serde_json::json!(true)).expect("parses");
    assert!(p.payouts_open);
    assert!(p.payouts_policy().payouts_open, "the flag reaches the signer's policy");
    // Not a bool: refused, never read as true or false.
    assert!(matches!(
        r1007_with(|r| r["payoutsOpen"] = serde_json::json!("true")),
        Err(ProfileError::Malformed(_))
    ));
    // Absent: false.
    let p = r1007_with(|r| {
        r.as_object_mut().expect("object").remove("payoutsOpen");
    })
    .expect("parses");
    assert!(!p.payouts_open);
}

#[test]
fn an_unknown_fl_contract_or_pin_key_is_refused() {
    assert!(matches!(
        r1007_with(|r| r["flCodePins"]["WorkMarketV2"] = serde_json::json!({"address": null, "codeHash": null, "pending": "x"})),
        Err(ProfileError::Malformed(_))
    ));
    assert!(matches!(
        r1007_with(|r| r["flCodePins"]["LoRAFactory"]["codehash"] = serde_json::json!("0x00")),
        Err(ProfileError::Malformed(_))
    ));
    assert!(matches!(
        r1007_with(|r| {
            r["flCodePins"].as_object_mut().expect("object").remove("DatasetRegistry");
        }),
        Err(ProfileError::Malformed(_))
    ));
}

fn invalid_pins(p: &NetworkProfile) -> bool {
    matches!(p.ready(), Err(ProfileError::Invalid { field: "flCodePins", .. }))
}

#[test]
fn a_malformed_pin_makes_the_profile_not_ready_and_the_policy_closed() {
    let cases: [(&str, serde_json::Value); 5] = [
        ("an address without its code hash", serde_json::json!({"address": "0x62e9aabec26288e60db2503f518eacf9cdcc8890", "codeHash": null, "pending": "x"})),
        ("a code hash without its address", serde_json::json!({"address": null, "codeHash": R1007_PINS[2].2})),
        ("a short code hash", serde_json::json!({"address": R1007_PINS[2].1, "codeHash": "0x1234"})),
        ("a zero address", serde_json::json!({"address": format!("0x{}", "0".repeat(40)), "codeHash": R1007_PINS[2].2})),
        ("unpinned with no reason", serde_json::json!({"address": null, "codeHash": null})),
    ];
    for (what, pin) in cases {
        let p = r1007_with(|r| {
            r["payoutsOpen"] = serde_json::json!(true);
            r["flCodePins"]["LoRAFactory"] = pin.clone();
        })
        .expect("parses");
        assert!(invalid_pins(&p), "{what}: {:?}", p.ready());
        let policy = p.payouts_policy();
        assert!(!policy.payouts_open && policy.pins.is_empty(), "{what}: closed");
    }
}

#[test]
fn a_ready_profile_hands_the_checked_pins_to_the_signer() {
    let r = r1007_profile().ready().expect("ready");
    let lora = r
        .payouts
        .pins
        .iter()
        .find(|x| x.contract == crate::payouts_gate::FlContract::LoraFactory)
        .expect("LoRAFactory");
    assert_eq!(lora.address.map(hex::encode).as_deref(), Some(&R1007_PINS[2].1[2..]));
    assert_eq!(lora.code_hash.map(hex::encode).as_deref(), Some(&R1007_PINS[2].2[2..]));
}
