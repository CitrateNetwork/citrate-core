// 0.6.0 L6-WP02: the Connections & Marketplace lane's characterization suite, Connections half
// (L6-CH-1 and the registry check). Included into `connections::tests`, so it reaches the module's
// private test seams (`fresh_vault`, `hf_mgr`) and calls the module tests it wraps instead of
// copying them. Select the whole lane suite with `cargo test -p citrate-core --lib l6_char`.
//
// Each test pins a guarantee the lane holds at citrate-core a826e441, before any L6 work package
// changes the code around it. They are green on main; one goes red only when a later change
// regresses the guarantee it names. A later L6 PR that changes a pinned behavior names the test it
// supersedes and replaces the assertion only with a stricter one (ci/characterization.d/l6.json).
//
// Data source (Rule 7): the current code only. Fixture tokens and secrets are made here; nothing
// touches the network or the member's keychain.

mod l6_char {
    use super::*;
    use std::collections::BTreeSet;

    /// Keys that would carry a credential if they ever appeared in a serialized status.
    const TOKEN_KEYS: &[&str] = &[
        "access_token",
        "refresh_token",
        "client_secret",
        "accessToken",
        "refreshToken",
        "clientSecret",
    ];

    /// Every fixture credential the suite seals starts with this, so one substring check covers
    /// them all.
    const FIXTURE_MARK: &str = "FIXTURE-L6-";
    const FIXTURE_CLIENT_SECRET: &str = "FIXTURE-L6-CLIENT-SECRET";

    /// A provider response carrying both tokens, as Google returns them.
    const FULL_TOKEN_RESPONSE: &str = r#"{"access_token":"FIXTURE-L6-ACCESS","refresh_token":"FIXTURE-L6-REFRESH","token_type":"bearer","scope":"repo","expires_in":3600}"#;

    /// An HTTP seam that fails the test if any request is made.
    struct NoNetwork;

    impl crate::oidc::HttpClient for NoNetwork {
        fn get(
            &self,
            url: &str,
            _bearer: Option<&str>,
        ) -> std::result::Result<String, crate::oidc::AuthError> {
            panic!("no request may be made here, got GET {url}")
        }
        fn post_form(
            &self,
            url: &str,
            _form: &[(&str, &str)],
        ) -> std::result::Result<String, crate::oidc::AuthError> {
            panic!("no request may be made here, got POST {url}")
        }
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let n: u64 = rand::random();
        std::env::temp_dir().join(format!(
            "citrate-core-l6-char-{tag}-{}-{n:016x}",
            std::process::id()
        ))
    }

    /// Run the real code exchange (`exchange_and_store`) over a scripted provider `response` and
    /// return the status the `connection_start` command would carry.
    fn exchange(service: Service, response: &str, vault: &CustodyVault) -> ConnectionStatus {
        let creds = ClientCreds {
            client_id: "fixture-l6-client".to_string(),
            client_secret: Zeroizing::new(FIXTURE_CLIENT_SECRET.to_string()),
        };
        super::hf_mgr(response)
            .exchange_and_store(service, &creds, vault, "fixture-code", "fixture-verifier")
            .expect("the exchange seals the fixture token")
    }

    /// Every object key in `v`, at any depth.
    fn all_keys(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(m) => {
                for (k, child) in m {
                    out.push(k.clone());
                    all_keys(child, out);
                }
            }
            serde_json::Value::Array(a) => a.iter().for_each(|c| all_keys(c, out)),
            _ => {}
        }
    }

    /// Fails if a serialized status names a token key or carries a fixture credential.
    fn assert_claim_free(what: &str, statuses: &[ConnectionStatus]) {
        let v = serde_json::to_value(statuses).expect("statuses serialize");
        let mut keys = Vec::new();
        all_keys(&v, &mut keys);
        for k in &keys {
            assert!(!TOKEN_KEYS.contains(&k.as_str()), "{what}: key {k} in {v}");
        }
        let text = v.to_string();
        assert!(!text.contains(FIXTURE_MARK), "{what}: credential in {text}");
    }

    /// The return types of every `#[tauri::command]` in `src` (the attribute on a line of its
    /// own, so a mention in a doc comment is not a command), as written after `->`.
    fn command_return_types(src: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for chunk in src.split("\n#[tauri::command]\n").skip(1) {
            let Some(fn_at) = chunk.find("fn ") else {
                continue;
            };
            let sig = &chunk[fn_at + 3..];
            let name: String = sig
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let Some(body_at) = sig.find('{') else {
                continue;
            };
            let head = &sig[..body_at];
            let ret = head
                .rfind("->")
                .map(|i| head[i + 2..].split_whitespace().collect::<String>())
                .unwrap_or_default();
            out.push((name, ret));
        }
        out
    }

    /// The only return types a connections command may have: a claim-free status, or nothing.
    const ALLOWED_CONNECTION_RETURNS: &[&str] = &[
        "std::result::Result<ConnectionStatus,String>",
        "std::result::Result<Vec<ConnectionStatus>,String>",
        "std::result::Result<(),String>",
    ];

    const CONNECTIONS_SRC: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/connections.rs"
    ));

    #[test]
    fn char_no_command_returns_a_token() {
        // Every command in connections.rs returns a status or nothing.
        let prod = CONNECTIONS_SRC
            .split("\n#[cfg(test)]\nmod tests {")
            .next()
            .unwrap_or(CONNECTIONS_SRC);
        assert!(prod.len() < CONNECTIONS_SRC.len(), "test module marker not found");
        let cmds = command_return_types(prod);
        let names: BTreeSet<&str> = cmds.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            BTreeSet::from(["connection_start", "connection_status", "connection_disconnect"]),
            "{cmds:?}"
        );
        for (name, ret) in &cmds {
            assert!(
                ALLOWED_CONNECTION_RETURNS.contains(&ret.as_str()),
                "{name} returns {ret}"
            );
        }
        // Negative control: the scanner flags a command that returns the token response.
        let bad = "\n#[tauri::command]\npub async fn leak(x: u8) -> std::result::Result<TokenResponse, String> {\n}\n";
        let flagged = command_return_types(bad);
        assert_eq!(flagged.len(), 1);
        assert!(!ALLOWED_CONNECTION_RETURNS.contains(&flagged[0].1.as_str()));

        // The serialized status, on every path that builds one, names no token and carries none.
        let vault = fresh_vault();
        let mgr = ConnectionManager::new(Box::new(NoNetwork), tmp_dir("status").join("none"));
        assert_claim_free("before connect", &mgr.status(&vault));
        let started: Vec<ConnectionStatus> = ALL_SERVICES
            .iter()
            .map(|svc| exchange(*svc, FULL_TOKEN_RESPONSE, &vault))
            .collect();
        assert!(started.iter().all(|s| s.connected), "{started:?}");
        assert_claim_free("connection_start result", &started);
        let all = mgr.status(&vault);
        assert!(all.iter().all(|s| s.connected), "{all:?}");
        assert_claim_free("connection_status, all connected", &all);
        mgr.disconnect(Service::GitHub, &vault).expect("disconnect");
        assert_claim_free("after disconnect", &mgr.status(&vault));
        vault.lock();
        let locked = mgr.status(&vault);
        assert!(locked.iter().all(|s| !s.connected), "{locked:?}");
        assert_claim_free("locked vault", &locked);
    }

    #[test]
    fn char_release_without_creds_reports_not_configured() {
        // A release build ships no oauth.dev.json: the path is missing, or the file has no keys.
        let dir = tmp_dir("nocreds");
        std::fs::create_dir_all(&dir).expect("mk tmpdir");
        let empty = dir.join("empty.dev.json");
        std::fs::write(&empty, "# no client credentials in a release build\n").expect("write");
        // Hold the callback port: a flow that bound it before checking credentials would report
        // PortInUse here instead of NotConfigured. If something else holds it, it is held all
        // the same.
        let _held = TcpListener::bind((Ipv4Addr::LOCALHOST, OAUTH_LOOPBACK_PORT));
        let vault = fresh_vault();
        for creds_path in [dir.join("oauth.dev.json"), empty] {
            let mgr = ConnectionManager::new(Box::new(NoNetwork), creds_path.clone());
            for svc in ALL_SERVICES {
                assert!(!mgr.is_configured(svc), "{svc:?} at {creds_path:?}");
                let opened = std::cell::Cell::new(0u32);
                let err = mgr
                    .connect(svc, &vault, |_| {
                        opened.set(opened.get() + 1);
                        Ok(())
                    })
                    .expect_err("no credentials, no flow");
                assert_eq!(err, ConnError::NotConfigured, "{svc:?} at {creds_path:?}");
                assert_eq!(opened.get(), 0, "no browser opened for {svc:?}");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn char_state_mismatch_fails_closed() {
        super::pba_l7b_009_drive_by_hits_do_not_burn_the_sign_in();
        super::parse_callback_target_missing_param_fails_closed();
    }

    #[test]
    fn char_slot_names_are_connection_prefixed() {
        let slots: Vec<String> = ALL_SERVICES.iter().map(|s| slot(*s)).collect();
        assert_eq!(
            slots,
            [
                "connection-github",
                "connection-gdrive",
                "connection-notion",
                "connection-hf",
                "connection-gsheets",
                "connection-gcal",
            ]
        );
        for (svc, s) in ALL_SERVICES.iter().zip(&slots) {
            assert_eq!(*s, format!("connection-{}", svc.id()));
            // Only the backend writes it: the invoke boundary's custody put refuses the prefix.
            assert!(crate::custody::is_backend_reserved_slot(s), "{s}");
        }
        let distinct: BTreeSet<&String> = slots.iter().collect();
        assert_eq!(distinct.len(), slots.len());
    }

    #[test]
    fn char_expired_token_never_sent() {
        super::hup_s0_3b_sealed_access_token_is_none_when_absent_locked_or_expired();
        super::hup_s0_3b_sealed_access_token_is_none_for_an_empty_token();
    }

    /// The `#[test] fn char_*` names declared in a suite file.
    fn declared_char_tests(src: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut after_test_attr = false;
        for line in src.lines().map(str::trim) {
            if line == "#[test]" {
                after_test_attr = true;
                continue;
            }
            if after_test_attr {
                if let Some(rest) = line.strip_prefix("fn char_") {
                    if let Some(name) = rest.split('(').next() {
                        out.insert(format!("char_{name}"));
                    }
                }
            }
            after_test_attr = false;
        }
        out
    }

    /// The lane's suite files, by registry suite id.
    const SUITE_FILES: &[(&str, &str)] = &[
        (
            "L6-CH-1",
            "src-tauri/src/lanes/l6/characterization/connections_char_tests.rs",
        ),
        (
            "L6-CH-2",
            "src-tauri/src/lanes/l6/characterization/mcp_servers_char_tests.rs",
        ),
        (
            "L6-CH-3",
            "src-tauri/src/lanes/l6/characterization/store_char_tests.rs",
        ),
    ];

    fn repo_file(rel: &str) -> String {
        let p = format!("{}/../{rel}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{rel} must exist ({e})"))
    }

    /// LQ runs this suite on every L6 merge from ci/characterization.d/l6.json, and members of the
    /// lane read the same list in docs/MCP_MARKETPLACE.md. The registry, the doc and the suite
    /// files must name the same tests.
    #[test]
    fn char_suite_is_listed_for_lq() {
        let mut declared = BTreeSet::new();
        for (id, file) in SUITE_FILES {
            let here = declared_char_tests(&repo_file(file));
            assert!(!here.is_empty(), "{id}: {file} declares no char_ test");
            declared.extend(here);
        }
        assert!(declared.len() >= 13, "{declared:?}");

        let reg: serde_json::Value =
            serde_json::from_str(&repo_file("ci/characterization.d/l6.json"))
                .expect("l6.json parses");
        assert_eq!(reg["owner"], "L6", "{reg}");
        let suites = reg["suites"].as_array().cloned().unwrap_or_default();
        let mut registered = BTreeSet::new();
        for (id, file) in SUITE_FILES {
            let suite = suites
                .iter()
                .find(|s| s["id"] == *id)
                .unwrap_or_else(|| panic!("{id} missing from l6.json"));
            assert_eq!(suite["runner"], "cargo", "{id}");
            let files: Vec<&str> = suite["files"]
                .as_array()
                .map(|a| a.iter().filter_map(|f| f.as_str()).collect())
                .unwrap_or_default();
            assert_eq!(files, [*file], "{id}");
            let listed: BTreeSet<String> = suite["tests"]
                .as_array()
                .map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect())
                .unwrap_or_default();
            assert_eq!(listed, declared_char_tests(&repo_file(file)), "{id} in l6.json");
            registered.extend(listed);
        }
        assert_eq!(registered, declared, "l6.json and the suite files differ");

        let doc = repo_file("docs/MCP_MARKETPLACE.md");
        assert!(
            doc.contains("cargo test -p citrate-core --lib l6_char"),
            "docs/MCP_MARKETPLACE.md must carry the suite's run command"
        );
        let in_doc: BTreeSet<String> = doc
            .lines()
            .filter_map(|l| l.trim_start().strip_prefix("- `char_"))
            .filter_map(|rest| rest.split('`').next())
            .map(|n| format!("char_{n}"))
            .collect();
        assert_eq!(in_doc, declared, "docs/MCP_MARKETPLACE.md and the suite differ");

        // Negative control: the scanner reads only `#[test]` functions named char_*.
        let sample = "#[test]\nfn char_a() {}\nfn char_b() {}\n#[test]\nfn other() {}\n";
        assert_eq!(declared_char_tests(sample), BTreeSet::from(["char_a".to_string()]));
    }
}
