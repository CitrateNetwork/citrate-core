// G-SEC PAY-14: the payouts status the webview reads, from the shipped profile and the chain.

struct CodeRpc {
    code: &'static str,
    calls: std::cell::Cell<usize>,
}

impl crate::rpc::RpcTransport for CodeRpc {
    fn call(&self, body: serde_json::Value) -> Result<serde_json::Value, crate::rpc::RpcError> {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(body["method"], "eth_getCode");
        Ok(serde_json::json!({"jsonrpc": "2.0", "id": 1, "result": self.code}))
    }
}

fn r1007() -> &'static crate::network_profile::NetworkProfile {
    crate::network_profile::profile("r1007").expect("r1007")
}

#[test]
fn the_shipped_network_reports_payouts_dark_without_reading_the_chain() {
    let rpc = crate::rpc::RpcClient::with_transport(CodeRpc { code: "0x", calls: Default::default() });
    let s = status_with(&r1007().payouts_policy(), &rpc);
    assert!(!s.open && !s.payouts_open_flag);
    assert_eq!(s.network, "r1007");
    assert_eq!(
        s.reason.as_deref(),
        Some("FL payouts are not open on network \"r1007\" (payoutsOpen is false)")
    );
    assert_eq!(rpc.transport().calls.get(), 0);
    let wm = s
        .contracts
        .iter()
        .find(|c| c.contract == crate::payouts_gate::FlContract::WorkMarket)
        .expect("WorkMarket");
    assert_eq!(wm.address, None);
}

#[test]
fn the_flag_alone_does_not_open_payouts_on_r1007() {
    // r1007 with `payoutsOpen: true` still has no WorkMarket: not deployed means not open.
    let mut p = r1007().clone();
    p.payouts_open = true;
    let rpc = crate::rpc::RpcClient::with_transport(CodeRpc { code: "0x6080", calls: Default::default() });
    let s = status_with(&p.payouts_policy(), &rpc);
    assert!(s.payouts_open_flag && !s.open);
    assert!(
        s.reason.as_deref().is_some_and(|r| r.starts_with("WorkMarket is not pinned on this network")),
        "{:?}",
        s.reason
    );
    // The pinned ledgers were read, and "0x6080" is not their build.
    let frl = s
        .contracts
        .iter()
        .find(|c| c.contract == crate::payouts_gate::FlContract::FederatedRoundLedger)
        .expect("FRL");
    assert_eq!(frl.code_hash_matches, Some(false));
}
