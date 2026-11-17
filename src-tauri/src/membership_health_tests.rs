// Tests for the membership grant-signer readiness read (the 2026-11-12..11-17 grant outage).

use super::*;

/// The shape core-membership#86 serves (trimmed to the fields this reads, plus neighbours).
fn body(readiness: Option<&str>) -> String {
    let chain = match readiness {
        Some(r) => format!(
            r#"{{"chainId":40204,"vaultCode":"live","sbtCode":"live","signerMode":"droplet","signerReadiness":"{r}","signerAddress":null}}"#
        ),
        None => r#"{"chainId":40204,"vaultCode":"live","sbtCode":"live","signerMode":"droplet"}"#
            .to_string(),
    };
    format!(r#"{{"service":"core-membership","phase":"D","status":"degraded","seams":{{}},"chain":{chain}}}"#)
}

#[test]
fn ready_is_ready() {
    let h = parse_signer_health(200, body(Some("ready")).as_bytes()).expect("health");
    assert_eq!(h.signer_ready, Some(true));
    assert_eq!(h.signer_state.as_deref(), Some("ready"));
    let j = serde_json::to_value(&h).unwrap();
    assert_eq!(j["signerReady"], true);
    assert_eq!(j["signerState"], "ready");
}

#[test]
fn every_named_not_ready_state_on_a_503_is_not_ready() {
    for s in NOT_READY {
        let h = parse_signer_health(503, body(Some(s)).as_bytes()).expect("health");
        assert_eq!(h.signer_ready, Some(false), "{s}");
        assert_eq!(h.signer_state.as_deref(), Some(*s));
    }
}

#[test]
fn an_absent_field_or_local_env_or_an_unknown_value_makes_no_claim() {
    for b in [body(None), body(Some("local-env")), body(Some("totally-fine"))] {
        let h = parse_signer_health(503, b.as_bytes()).expect("health");
        assert_eq!(
            h,
            MembershipSignerHealth {
                signer_ready: None,
                signer_state: None
            }
        );
    }
}

#[test]
fn any_other_status_is_an_error() {
    for code in [500u16, 502, 404, 302, 401] {
        let e = parse_signer_health(code, body(Some("ready")).as_bytes()).expect_err("unavailable");
        assert!(e.contains(&code.to_string()), "{e}");
    }
}

#[test]
fn a_body_that_is_not_core_membership_health_is_an_error() {
    assert!(parse_signer_health(200, b"<html>502 Bad Gateway</html>").is_err());
    assert!(parse_signer_health(
        200,
        br#"{"service":"other","chain":{"signerReadiness":"ready"}}"#
    )
    .is_err());
    assert!(parse_signer_health(200, br#"{"chain":{"signerReadiness":"ready"}}"#).is_err());
}
