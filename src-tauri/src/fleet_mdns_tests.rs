// HUP-S8.2 — opt-in mDNS discovery: packet codec tests (no network).
// Written red-first, then brought green.

use super::*;

fn advert() -> Advert {
    Advert {
        id: "a1b2c3d4e5f60718".into(),
        label: "Linux box".into(),
        tier: Some("T1".into()),
        role: "worker".into(),
        port: 0,
    }
}

#[test]
fn a_query_asks_for_the_citrate_service_ptr() {
    let q = encode_query();
    let m = parse(&q).unwrap();
    assert!(!m.response);
    assert!(is_service_query(&m));
    assert_eq!(m.questions.len(), 1);
    assert_eq!(m.questions[0].name, SERVICE);
    assert_eq!(m.questions[0].qtype, TYPE_PTR);
}

#[test]
fn a_response_round_trips_to_the_same_advert() {
    let r = encode_response(&advert()).unwrap();
    let m = parse(&r).unwrap();
    assert!(m.response);
    assert!(!is_service_query(&m));
    assert_eq!(adverts(&m), vec![advert()]);
}

#[test]
fn an_advert_carries_no_wallet_or_host_identity() {
    let r = encode_response(&advert()).unwrap();
    let text = String::from_utf8_lossy(&r).to_lowercase();
    for k in ["0x", "addr", "wallet", "host"] {
        assert!(!text.contains(k), "{k}");
    }
}

#[test]
fn the_service_name_matches_case_insensitively() {
    let mut q = encode_query();
    // Upper-case the first label's letters in place.
    for b in q.iter_mut().skip(12).take(14) {
        b.make_ascii_uppercase();
    }
    assert!(is_service_query(&parse(&q).unwrap()));
}

#[test]
fn compressed_names_are_followed() {
    // Response: PTR record whose owner name is a pointer to the question name at offset 12.
    let mut p = vec![0, 0, 0x84, 0, 0, 1, 0, 1, 0, 0, 0, 0];
    p.extend(encode_name(SERVICE));
    p.extend([0, 12, 0, 1]); // QTYPE PTR, QCLASS IN
    p.extend([0xC0, 12]); // owner = pointer to offset 12
    p.extend([0, 12, 0, 1, 0, 0, 0, 120]);
    let target = [&[3u8][..], b"abc", &[0xC0, 12]].concat();
    p.extend((target.len() as u16).to_be_bytes());
    p.extend(&target);
    let m = parse(&p).unwrap();
    assert_eq!(m.records.len(), 1);
    assert_eq!(m.records[0].name, SERVICE);
    assert_eq!(m.records[0].data, RData::Ptr(format!("abc.{SERVICE}")));
}

#[test]
fn pointer_loops_and_truncation_are_rejected() {
    // A name that points at itself.
    let mut p = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    p.extend([0xC0, 12, 0, 12, 0, 1]);
    assert!(parse(&p).is_err());
    // Truncated in the header, in a name, and in an rdata length.
    assert!(parse(&[0, 0, 0]).is_err());
    let q = encode_query();
    assert!(parse(&q[..q.len() - 3]).is_err());
    let r = encode_response(&advert()).unwrap();
    assert!(parse(&r[..r.len() - 2]).is_err());
}

#[test]
fn absurd_record_counts_do_not_allocate_or_panic() {
    let p = vec![
        0, 0, 0x84, 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    ];
    assert!(parse(&p).is_err());
}

#[test]
fn other_services_are_ignored() {
    let r = encode_response(&advert()).unwrap();
    let m = parse(&r).unwrap();
    let mut other = m.clone();
    for rec in other.records.iter_mut() {
        rec.name = rec.name.replace("_citrate-core", "_airplay");
        if let RData::Ptr(t) = &mut rec.data {
            *t = t.replace("_citrate-core", "_airplay");
        }
    }
    assert!(adverts(&other).is_empty());
}

#[test]
fn adverts_with_bad_fields_are_dropped() {
    for bad in [
        Advert {
            id: "".into(),
            ..advert()
        },
        Advert {
            id: "has.dot".into(),
            ..advert()
        },
        Advert {
            label: "x".repeat(65),
            ..advert()
        },
        Advert {
            tier: Some("T9".into()),
            ..advert()
        },
        Advert {
            role: "".into(),
            ..advert()
        },
    ] {
        assert!(encode_response(&bad).is_err(), "{bad:?}");
    }
    // And a TXT that a foreign responder filled with junk yields no advert.
    let r = encode_response(&advert()).unwrap();
    let mut m = parse(&r).unwrap();
    for rec in m.records.iter_mut() {
        if let RData::Txt(kv) = &mut rec.data {
            kv.retain(|s| !s.starts_with("v="));
            kv.push("v=9".into());
        }
    }
    assert!(adverts(&m).is_empty());
}

#[test]
fn a_txt_without_tier_is_still_an_advert() {
    let a = Advert {
        tier: None,
        ..advert()
    };
    let m = parse(&encode_response(&a).unwrap()).unwrap();
    assert_eq!(adverts(&m), vec![a]);
}

#[test]
fn record_counts_over_the_cap_are_refused_up_front() {
    // 65 questions, no body: refused on the header count, not by running out of bytes.
    let p = vec![0, 0, 0, 0, 0, 65, 0, 0, 0, 0, 0, 0];
    assert_eq!(parse(&p), Err(MdnsError::TooMany));
    // 65 answers likewise.
    let p = vec![0, 0, 0x84, 0, 0, 0, 0, 65, 0, 0, 0, 0];
    assert_eq!(parse(&p), Err(MdnsError::TooMany));
}
