// L1-WP01 CH-10 (characterization, citrate-core a826e441). Included into `hermes::tests`
// (lane L1's module; hermes_headless.rs is L0's), so it reaches the dispatcher only through its
// public API.
//
// Pins today's headless dispatcher rule (hermes_headless.rs module doc): a core-hosted call on a
// session the view is not watching is answered here only when it is one of the two read tools
// core serves itself; every other tool is declined with the "needs the member" reason, a call the
// sidecar marked `hic: "required"` is declined whatever the tool, and nothing is proposed, queued
// or approved on the member's behalf. The backend below would answer anything and open any
// proposal, so the refusal is the dispatcher's, not the backend's.

mod lq_ch10_headless_never_approves {
    use crate::hermes_headless::*;
    use crate::node_mcp_protocol::{NodeBackend, ProposedSignature, RpcRead};
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Permissive {
        reads: AtomicUsize,
        proposals: AtomicUsize,
    }

    impl Permissive {
        fn read(&self) -> Result<Value, String> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"ok": true}))
        }
        fn propose(&self) -> Result<ProposedSignature, String> {
            self.proposals.fetch_add(1, Ordering::SeqCst);
            Ok(ProposedSignature {
                ceremony_id: "c-1".to_string(),
                ceremony: json!({}),
            })
        }
    }

    impl NodeBackend for Permissive {
        fn node_status(&self) -> Result<Value, String> {
            self.read()
        }
        fn rpc_read(&self, _method: &str, _params: Value) -> Result<RpcRead, String> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(RpcRead {
                value: json!("0x1"),
                source: "local-node".to_string(),
            })
        }
        fn wallet_address(&self) -> Result<String, String> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok("0x0000000000000000000000000000000000000001".to_string())
        }
        fn memory_search(&self, _tenant: &str, _query: &str, _limit: usize) -> Result<Value, String> {
            self.read()
        }
        fn groups(&self) -> Result<Value, String> {
            self.read()
        }
        fn cluster_status(&self, _group: &str) -> Result<Value, String> {
            self.read()
        }
        fn cluster_peers(&self, _group: &str) -> Result<Value, String> {
            self.read()
        }
        fn invites(&self, _group: &str) -> Result<Value, String> {
            self.read()
        }
        fn propose_transaction(
            &self,
            _origin: &str,
            _to: &str,
            _value_wei: u128,
            _data: &str,
        ) -> Result<ProposedSignature, String> {
            self.propose()
        }
        fn close_ceremony(&self, _ceremony_id: &str) {}
        fn faucet_request(&self, _origin: &str, _initcode_hash: &str) -> Result<Value, String> {
            self.proposals.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"sent": true}))
        }
        fn devices(&self) -> Result<Value, String> {
            self.read()
        }
        fn pins(&self) -> Result<Value, String> {
            self.read()
        }
        fn propose_deploy(
            &self,
            _origin: &str,
            _bytecode: &str,
            _constructor_args: &str,
            _value_wei: u128,
            _gas: Option<u64>,
        ) -> Result<ProposedSignature, String> {
            self.propose()
        }
        fn anchor_ready(&self) -> Result<(), String> {
            Ok(())
        }
        fn contract_abi(&self, _address: &str) -> Result<Value, String> {
            self.read()
        }
    }

    fn claimed(name: &str, arguments: Value, hic_required: bool) -> Claimed {
        Claimed {
            session: "s-1".to_string(),
            seq: 1,
            call_id: "call_0".to_string(),
            name: name.to_string(),
            arguments,
            hic_required,
        }
    }

    /// Every node MCP tool, the Hermes session tools, and effectful chat tools a sidecar may host.
    fn names() -> Vec<String> {
        let mut v: Vec<String> = crate::node_mcp_tools::all_tools()
            .map(|t| t.name.to_string())
            .collect();
        for n in [
            "shell_run",
            "file_write",
            "contract_deploy",
            "send_salt",
            "approve_latest",
            "wallet_sign",
            "",
        ] {
            v.push(n.to_string());
        }
        v
    }

    #[test]
    fn lq_ch10_only_the_two_read_tools_are_answered_here() {
        assert_eq!(HEADLESS_TOOLS, &["node_status", "groups_list"]);
        for n in HEADLESS_TOOLS {
            let def = crate::node_mcp_tools::tool(n).expect("a node MCP tool");
            assert_eq!(def.kind, crate::node_mcp_tools::ToolKind::Read, "{n}");
        }
        let backend = std::sync::Arc::new(Permissive::default());
        let host = NodeBackendTools(backend.clone());
        for n in names() {
            let (status, content) = answer(&host, &claimed(&n, json!({}), false));
            if HEADLESS_TOOLS.contains(&n.as_str()) {
                assert_eq!(status, "ok", "{n}: {content}");
            } else {
                assert_eq!(status, "denied", "{n}");
                assert_eq!(content, needs_member(&n), "{n}");
            }
        }
        assert_eq!(backend.reads.load(Ordering::SeqCst), HEADLESS_TOOLS.len());
        assert_eq!(backend.proposals.load(Ordering::SeqCst), 0, "nothing was proposed");
    }

    #[test]
    fn lq_ch10_a_hic_required_call_is_declined_whatever_the_tool() {
        let backend = std::sync::Arc::new(Permissive::default());
        let host = NodeBackendTools(backend.clone());
        for n in names() {
            let (status, content) = answer(&host, &claimed(&n, json!({}), true));
            assert_eq!(status, "denied", "{n}");
            assert_eq!(content, needs_hic(&n), "{n}");
        }
        assert_eq!(backend.reads.load(Ordering::SeqCst), 0, "nothing was run");
        assert_eq!(backend.proposals.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn lq_ch10_arguments_that_are_not_an_object_run_nothing() {
        let backend = std::sync::Arc::new(Permissive::default());
        let host = NodeBackendTools(backend.clone());
        for args in [json!(null), json!([]), json!("{}"), json!(1)] {
            let (status, _) = answer(&host, &claimed("node_status", args.clone(), false));
            assert_eq!(status, "error", "{args}");
        }
        assert_eq!(backend.reads.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn lq_ch10_the_reasons_say_nothing_was_done() {
        for n in ["tx_propose", "deploy_propose", "faucet_request"] {
            assert!(needs_member(n).contains("Nothing was done"), "{n}");
            assert!(needs_hic(n).contains("Nothing was done"), "{n}");
            assert!(needs_member(n).starts_with(n));
        }
    }
}
