// 0.6.0 L6-WP02: the Connections & Marketplace lane's characterization suite, user-added MCP
// servers half (L6-CH-2). Included into `mcp_servers::tests` after `mcp_servers_tests.rs`, so it
// calls the module tests it wraps instead of copying them. Select the whole lane suite with
// `cargo test -p citrate-core --lib l6_char`.
//
// Each test pins a guarantee held at citrate-core a826e441, before any L6 work package (the
// Marketplace install path in WP10 and WP12 above all) changes it. A later L6 PR that changes a
// pinned behavior names the test it supersedes and replaces the assertion only with a stricter
// one (ci/characterization.d/l6.json).
//
// Data source (Rule 7): the current code only; the keychain is the module's in-memory double.

mod l6_char {
    use super::*;

    #[test]
    fn char_user_server_saved_disabled_and_needs_probe() {
        super::save_adds_a_disabled_entry_that_needs_review();
        super::enable_requires_a_successful_probe_of_the_entry_as_it_stands();
    }

    #[test]
    fn char_probe_claiming_trusted_tool_refused() {
        super::a_probe_report_claiming_a_trusted_tool_is_refused();
    }

    #[test]
    fn char_env_values_only_in_keychain() {
        super::saved_env_values_live_in_the_keychain_and_never_in_the_file();
    }

    #[test]
    fn char_max_servers_is_16() {
        // The runtime's limit (agent-mcp-host config::MAX_SERVERS), and the registry enforces it.
        assert_eq!(std::hint::black_box(MAX_SERVERS), 16);
        super::duplicate_names_and_too_many_servers_are_refused();
    }

    /// Hermes is offered the node server's read tools only, pending owner sign-off (09 section
    /// 10 item 2). Read through `hermes_mcp`'s public surface; that module belongs to L1.
    #[test]
    fn char_hermes_node_write_tools_off() {
        let write_tools = std::hint::black_box(crate::hermes_mcp::HERMES_NODE_WRITE_TOOLS);
        assert!(!write_tools, "Hermes node write tools are on");

        let settings = crate::hermes_mcp::McpSettings {
            mem: false,
            scan: false,
            node: true,
        };
        let node = crate::hermes_mcp::NodeTarget {
            exe: std::env::temp_dir().join("citrate-core-l6-char-shim"),
            port: 47204,
            token: crate::secret::SecretString::new(format!(
                "{}{}",
                crate::node_mcp_token::TOKEN_PREFIX,
                "cd".repeat(32)
            )),
        };
        let cfg = crate::hermes_mcp::render_config(&settings, None, Some(&node))
            .expect("the node entry renders");
        let servers = cfg["servers"].as_array().cloned().unwrap_or_default();
        let entry = servers
            .iter()
            .find(|s| s["name"] == crate::hermes_mcp::NODE_SERVER_NAME)
            .unwrap_or_else(|| panic!("no node entry in {cfg}"));
        assert_eq!(entry["allow_write_tools"], false, "{entry}");
    }
}
