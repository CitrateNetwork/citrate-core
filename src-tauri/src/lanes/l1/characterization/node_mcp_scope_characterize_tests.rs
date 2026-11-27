// L1-WP01 CH-8 (characterization, citrate-core a826e441). Included into `node_mcp::tests`.
//
// Pins today's node MCP scope for Hermes: the built-in Hermes entry gets a read-only connect
// token (`HERMES_NODE_WRITE_TOOLS` is off), and a read-only token is offered exactly the Read
// tools and is refused every Signature, Action and Budgeted tool before anything is proposed,
// queued or run, whatever the arguments. It is a property over the whole catalog
// (`node_mcp_tools::all_tools`), so a tool added later is covered too. WP18 (node write tools for
// agents behind HIC) supersedes it and names CH-8.

mod lq_ch8_node_mcp_scope {
    use super::*;
    use crate::node_mcp_tools::{all_tools, ToolKind};
    use serde_json::json;
    use std::sync::Arc;

    fn write_tools() -> Vec<&'static crate::node_mcp_tools::ToolDef> {
        all_tools().filter(|t| t.kind != ToolKind::Read).collect()
    }

    #[test]
    fn lq_ch8_hermes_gets_a_read_only_token() {
        const { assert!(!crate::hermes_mcp::HERMES_NODE_WRITE_TOOLS) };
        let sv = serve();
        let _ = sv.state.hermes_token(None).expect("issue");
        let label = crate::hermes_mcp::HERMES_NODE_TOKEN_LABEL;
        let hermes: Vec<_> = sv
            .state
            .status()
            .tokens
            .into_iter()
            .filter(|t| t.label == label)
            .collect();
        assert_eq!(hermes.len(), 1);
        assert!(hermes[0].read_only);
        sv.state.stop();
    }

    #[test]
    fn lq_ch8_the_catalog_has_every_write_kind() {
        // The refusal property below is not vacuous: each non-read kind has at least one tool.
        for kind in [ToolKind::Signature, ToolKind::Action, ToolKind::Budgeted] {
            assert!(
                all_tools().any(|t| t.kind == kind),
                "no {kind:?} tool in the catalog"
            );
        }
    }

    #[test]
    fn lq_ch8_a_read_only_token_is_offered_exactly_the_read_tools() {
        let core = core_with(Arc::new(Fixture::default()));
        let r = rpc(&core, &read_only_ctx("h"), "tools/list", json!({}));
        let offered: Vec<String> = r["result"]["tools"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|t| t["name"].as_str().map(str::to_string))
            .collect();
        let reads: Vec<String> = all_tools()
            .filter(|t| t.kind == ToolKind::Read)
            .map(|t| t.name.to_string())
            .collect();
        assert!(!reads.is_empty());
        assert_eq!(offered, reads, "same tools, same order");
    }

    #[test]
    fn lq_ch8_every_write_tool_is_refused_for_a_read_only_token_and_nothing_happens() {
        let f = Arc::new(Fixture::default());
        let core = core_with(f.clone());
        let c = read_only_ctx("h");
        let writes = write_tools();
        assert!(!writes.is_empty());
        for t in &writes {
            for args in [json!({}), json!({"anything": "goes"}), json!(null)] {
                for reply in [
                    call(&core, &c, t.name, args.clone()),
                    call_meta(&core, &c, t.name, args.clone(), tasks_meta()),
                ] {
                    assert!(is_tool_error(&reply), "{}: {reply}", t.name);
                    assert!(
                        reply["result"]["content"][0]["text"]
                            .as_str()
                            .unwrap_or_default()
                            .contains("limited to the read tools"),
                        "{}: {reply}",
                        t.name
                    );
                    assert!(reply["result"].get("taskId").is_none(), "{}", t.name);
                }
            }
        }
        assert!(f
            .proposals
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty());
        assert!(f.deploys.lock().unwrap_or_else(|e| e.into_inner()).is_empty());
        assert!(core.inbox().list(1).0.is_empty(), "no request was queued");
    }

    #[test]
    fn lq_ch8_a_full_token_still_sees_every_tool() {
        let core = core_with(Arc::new(Fixture::default()));
        let full = rpc(&core, &ctx("m"), "tools/list", json!({}));
        assert_eq!(
            full["result"]["tools"].as_array().map(Vec::len),
            Some(all_tools().count())
        );
    }
}
