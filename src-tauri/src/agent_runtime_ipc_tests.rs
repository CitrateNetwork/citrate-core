// L1-WP03c (planset v060): the ACP host inside Core. A pinned agent starts from its lock row, in
// its own home (HOME, XDG, TMPDIR and its config folder inside `<app data>/agents/<id>/home`), with
// no vendor key anywhere (env, argv, files Core writes, ACP frames), and with at most one route
// token, in CITRATE_ROUTE_TOKEN only. Its model calls reach the proxy on that route and are
// recorded under the agent's label. Sign-out and removal stop its sessions first.

use super::*;
use crate::hermes::agent_auth::{mint_agent_route, AgentHomes, AgentId, ROUTE_TOKEN_ENV};
use crate::model_provider::{ProviderId, Vendor};
use crate::model_proxy::tests::Rig;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

struct Fx(PathBuf);

impl Fx {
    fn new() -> Fx {
        let p = std::env::temp_dir().join(format!("core-acp-ipc-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Fx(p.canonicalize().unwrap())
    }
    fn homes(&self) -> AgentHomes {
        AgentHomes::new(&self.0)
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const VENDOR_KEY: &str = "sk-ant-api03-THISISAFAKEKEYFORTESTSONLY0000";

fn inherited() -> Vec<(String, String)> {
    vec![
        ("PATH".into(), "/usr/bin:/bin".into()),
        ("HOME".into(), "/Users/member".into()),
        ("LANG".into(), "C".into()),
        ("TMPDIR".into(), "/var/folders/member".into()),
        ("ANTHROPIC_API_KEY".into(), VENDOR_KEY.into()),
        ("SOME_TOOL".into(), format!("token={VENDOR_KEY}")),
        ("SSH_AUTH_SOCK".into(), "/tmp/ssh.sock".into()),
    ]
}

fn installed(dir: &Path) -> Installed {
    let prog = dir.join("agent-entry.js");
    std::fs::write(&prog, b"// pinned entry\n").unwrap();
    Installed {
        command: PathBuf::from("/usr/bin/node"),
        args_prefix: vec![prog.display().to_string()],
        pinned_file: prog.clone(),
        sha256: citrate_acp_host::registry::sha256_file(&prog).unwrap(),
    }
}

fn all_files(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(all_files(&p));
        } else {
            out.push((p.clone(), std::fs::read_to_string(&p).unwrap_or_default()));
        }
    }
    out
}

#[test]
fn agent_home_isolation_in_the_launch() {
    let fx = Fx::new();
    let homes = fx.homes();
    let l = prepare_launch(&homes, "claude", &installed(&fx.0), AGENTS_LOCK_JSON, inherited().into_iter(), None).unwrap();
    let home = homes.home(&AgentId::parse("claude").unwrap());
    let get = |k: &str| l.env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    for k in ["HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "TMPDIR", "CLAUDE_CONFIG_DIR"] {
        let v = get(k).unwrap_or_else(|| panic!("{k} set"));
        assert!(Path::new(&v).starts_with(&home), "{k}={v} inside the agent's home");
    }
    assert_eq!(get("PATH").as_deref(), Some("/usr/bin:/bin"));
    for k in ["ANTHROPIC_API_KEY", "SOME_TOOL", "SSH_AUTH_SOCK", ROUTE_TOKEN_ENV] {
        assert!(get(k).is_none(), "{k} never reaches the agent");
    }
    assert_eq!(l.spec.command, PathBuf::from("/usr/bin/node"));
    assert_eq!(l.spec.id, "claude");
    assert_eq!(l.cwd, home, "until the workspace grant (WP21) the session runs in the agent's home");
}

#[test]
fn spawned_env_is_exactly_the_prepared_env() {
    let fx = Fx::new();
    let homes = fx.homes();
    let l = prepare_launch(&homes, "codex", &installed(&fx.0), AGENTS_LOCK_JSON, inherited().into_iter(), None).unwrap();
    let out = fx.0.join("env.txt");
    let script = format!("/usr/bin/env > {}", out.display());
    // The host's own spawn (cleared environment, new process group), on Tauri's tokio runtime.
    tauri::async_runtime::block_on(async {
        let mut child = citrate_acp_host::process::spawn(Path::new("/bin/sh"), &["-c".into(), script], &l.env, &fx.0).unwrap();
        child.wait().await.unwrap();
    });
    let seen = std::fs::read_to_string(&out).unwrap();
    let mut names: Vec<&str> = seen.lines().filter_map(|x| x.split('=').next()).filter(|n| !["PWD", "SHLVL", "_"].contains(n)).collect();
    let mut want: Vec<&str> = l.env.iter().map(|(k, _)| k.as_str()).collect();
    names.sort();
    want.sort();
    assert_eq!(names, want);
    assert!(!seen.contains(VENDOR_KEY) && !seen.contains("/Users/member"));
}

#[test]
fn claude_route_rides_in_citrate_route_token_and_its_config_holds_no_secret() {
    let rig = Rig::new("acp-claude-route");
    let fx = Fx::new();
    let homes = fx.homes();
    let id = AgentId::parse("claude").unwrap();
    let g = mint_agent_route(&rig.proxy, &id, ProviderId::Vendor(Vendor::OpenAi), "gpt-known", rig.gate()).unwrap();
    let l = prepare_launch(&homes, "claude", &installed(&fx.0), AGENTS_LOCK_JSON, inherited().into_iter(), Some(Route { grant: &g, model: "gpt-known" })).unwrap();
    let token = g.bearer.expose();
    let holders: Vec<&str> = l.env.iter().filter(|(_, v)| v.contains(token)).map(|(k, _)| k.as_str()).collect();
    assert_eq!(holders, vec![ROUTE_TOKEN_ENV], "the token is in one variable");
    let get = |k: &str| l.env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    assert_eq!(get("ANTHROPIC_BASE_URL"), Some(g.base_url.trim_end_matches("/v1").to_string()));
    for k in ["ANTHROPIC_MODEL", "ANTHROPIC_SMALL_FAST_MODEL", "ANTHROPIC_DEFAULT_HAIKU_MODEL"] {
        assert_eq!(get(k).as_deref(), Some("gpt-known"), "{k}: the route binds the model");
    }
    let settings: Value = serde_json::from_str(&std::fs::read_to_string(homes.home(&id).join(".claude/settings.json")).unwrap()).unwrap();
    assert_eq!(settings["apiKeyHelper"], format!("{PRINTENV} {ROUTE_TOKEN_ENV}"));
    for (p, text) in all_files(&homes.home(&id)) {
        assert!(!text.contains(token) && !text.contains(VENDOR_KEY), "{} holds no secret", p.display());
    }
    assert!(l.spec.args.iter().all(|a| !a.contains(token)), "argv holds no route");
}

#[test]
fn codex_route_uses_the_responses_wire_through_citrate_route_token() {
    let rig = Rig::new("acp-codex-route");
    let fx = Fx::new();
    let homes = fx.homes();
    let id = AgentId::parse("codex").unwrap();
    let g = mint_agent_route(&rig.proxy, &id, ProviderId::Vendor(Vendor::OpenAi), "gpt-known", rig.gate()).unwrap();
    prepare_launch(&homes, "codex", &installed(&fx.0), AGENTS_LOCK_JSON, inherited().into_iter(), Some(Route { grant: &g, model: "gpt-known" })).unwrap();
    let cfg = std::fs::read_to_string(homes.home(&id).join(".codex/config.toml")).unwrap();
    let t: toml::Value = toml::from_str(&cfg).unwrap();
    assert_eq!(t["model"].as_str(), Some("gpt-known"));
    assert_eq!(t["model_provider"].as_str(), Some("citrate"));
    let p = &t["model_providers"]["citrate"];
    assert_eq!(p["base_url"].as_str(), Some(g.base_url.as_str()));
    assert_eq!(p["env_key"].as_str(), Some(ROUTE_TOKEN_ENV));
    assert_eq!(p["wire_api"].as_str(), Some("responses"));
    assert!(!cfg.contains(g.bearer.expose()));
}

#[cfg(unix)]
#[test]
fn route_config_write_refuses_a_link_in_the_home() {
    let rig = Rig::new("acp-link");
    let fx = Fx::new();
    let homes = fx.homes();
    let id = AgentId::parse("claude").unwrap();
    let home = homes.ensure_home(&id).unwrap();
    let outside = fx.0.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, home.join(".claude")).unwrap();
    let g = mint_agent_route(&rig.proxy, &id, ProviderId::Vendor(Vendor::OpenAi), "gpt-known", rig.gate()).unwrap();
    let e = prepare_launch(&homes, "claude", &installed(&fx.0), AGENTS_LOCK_JSON, inherited().into_iter(), Some(Route { grant: &g, model: "gpt-known" })).unwrap_err();
    assert!(e.contains("refused"), "{e}");
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0, "nothing written outside the home");
    std::fs::remove_file(home.join(".claude")).unwrap();
    std::fs::create_dir(home.join(".claude")).unwrap();
    std::os::unix::fs::symlink(outside.join("victim.json"), home.join(".claude/settings.json")).unwrap();
    assert!(prepare_launch(&homes, "claude", &installed(&fx.0), AGENTS_LOCK_JSON, inherited().into_iter(), Some(Route { grant: &g, model: "gpt-known" })).is_err());
    assert!(!outside.join("victim.json").exists());
}

#[test]
fn an_agent_without_proxy_wiring_refuses_a_route() {
    let rig = Rig::new("acp-noroute");
    let fx = Fx::new();
    let id = AgentId::parse("gemini").unwrap();
    let g = mint_agent_route(&rig.proxy, &id, ProviderId::Vendor(Vendor::OpenAi), "gpt-known", rig.gate()).unwrap();
    let e = prepare_launch(&fx.homes(), "gemini", &installed(&fx.0), AGENTS_LOCK_JSON, inherited().into_iter(), Some(Route { grant: &g, model: "gpt-known" })).unwrap_err();
    assert!(e.contains("no proxy wiring"), "{e}");
    assert!(prepare_launch(&fx.homes(), "nope", &installed(&fx.0), AGENTS_LOCK_JSON, inherited().into_iter(), None).is_err());
}

#[test]
fn lock_launch_args_and_env_follow_the_installed_entry() {
    let fx = Fx::new();
    let l = prepare_launch(&fx.homes(), "gemini", &installed(&fx.0), AGENTS_LOCK_JSON, inherited().into_iter(), None).unwrap();
    assert_eq!(l.spec.args.last().map(String::as_str), Some("--acp"));
    assert!(l.spec.args[0].ends_with("agent-entry.js"));
    let m = prepare_launch(&fx.homes(), "muse", &installed(&fx.0), AGENTS_LOCK_JSON, inherited().into_iter(), None).unwrap();
    assert!(m.env.iter().any(|(k, v)| k == "MUSE_EXPERIMENTAL_ACP" && v == "on"));
}

#[test]
fn the_host_scans_frames_with_cores_vendor_key_check() {
    let cfg = host_config();
    assert!((cfg.scan)(VENDOR_KEY));
    assert!(!(cfg.scan)("cmp_00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff"), "route tokens are guarded by exact match per session");
    assert_eq!(cfg.route_env.as_deref(), Some(ROUTE_TOKEN_ENV));
    assert_eq!(cfg.client_name, "citrate-core");
}

#[test]
fn provider_routing_records_spend_under_the_agent() {
    // The agent's model call: the route token from its CITRATE_ROUTE_TOKEN, to its base URL. The
    // usage log attributes the call to `ag:claude:<hex>`; revoking the agent's routes ends it.
    use crate::model_proxy_http::{serve_one, HttpReq, CHAT_PATH};
    let rig = Rig::new("acp-attr");
    let fx = Fx::new();
    let id = AgentId::parse("claude").unwrap();
    let g = mint_agent_route(&rig.proxy, &id, ProviderId::Vendor(Vendor::OpenAi), "gpt-known", rig.gate()).unwrap();
    let l = prepare_launch(&fx.homes(), "claude", &installed(&fx.0), AGENTS_LOCK_JSON, inherited().into_iter(), Some(Route { grant: &g, model: "gpt-known" })).unwrap();
    let token = l.env.iter().find(|(k, _)| k == ROUTE_TOKEN_ENV).map(|(_, v)| v.clone()).unwrap();
    let call = |t: &str| {
        let mut out = Vec::new();
        let req = HttpReq {
            method: "POST".into(),
            path: CHAT_PATH.into(),
            headers: vec![("host".into(), "127.0.0.1:1".into()), ("authorization".into(), format!("Bearer {t}"))],
            body: json!({"model": "gpt-known", "messages": [{"role": "user", "content": "hi"}]}).to_string().into_bytes(),
        };
        serve_one(&rig.proxy, &req, 1, &mut out);
        String::from_utf8(out).unwrap()
    };
    assert!(call(&token).starts_with("HTTP/1.1 200"));
    let end = rig.log_text().lines().rev().find(|l| l.contains("\"end\"")).unwrap().to_string();
    assert!(end.contains("\"session\":\"ag:claude:"), "{end}");
    crate::hermes::agent_auth::revoke_agent_routes(Some(&rig.proxy), &id);
    assert!(call(&token).starts_with("HTTP/1.1 401"));
}

#[test]
fn sign_out_and_remove_close_the_agents_sessions_first() {
    // No session is live here (a live one needs an installed agent: see the ignored live test);
    // the registry answers how many it closed, and an agent with none closes zero.
    assert_eq!(close_agent_sessions("claude"), 0);
    let fx = Fx::new();
    let homes = fx.homes();
    homes.ensure_home(&AgentId::parse("codex").unwrap()).unwrap();
    assert!(crate::hermes::agent_auth::remove_at(&homes, None, "codex").unwrap());
}

/// Runtime evidence on the Linux build host (never on a member's machine): the real, pinned
/// claude-agent-acp through Core's host and Core's model proxy, with no key in the keychain.
/// `ACP_LIVE_NODE` = a Node 22 binary, `ACP_LIVE_CLAUDE_ENTRY` = the installed dist/index.js.
#[test]
#[ignore = "needs an installed claude-agent-acp; run on the build host"]
fn live_claude_agent_through_core_host_and_proxy() {
    use crate::provider_fixtures_tests::MemKeyring;
    let node = std::env::var("ACP_LIVE_NODE").expect("ACP_LIVE_NODE");
    let entry = PathBuf::from(std::env::var("ACP_LIVE_CLAUDE_ENTRY").expect("ACP_LIVE_CLAUDE_ENTRY"));
    let fx = Fx::new();
    let pdir = fx.0.join("providers");
    std::fs::create_dir_all(&pdir).unwrap();
    // No vendor key is stored: the proxy must refuse at its key step, after the begin record.
    let proxy = Arc::new(crate::model_proxy::ModelProxy::new(pdir.clone(), Arc::new(MemKeyring::default()), Arc::new(crate::provider_egress::PinnedHttp)));
    crate::model_proxy_http::start(proxy.clone()).unwrap();
    let id = AgentId::parse("claude").unwrap();
    let model = "claude-sonnet-5-5";
    let g = mint_agent_route(&proxy, &id, ProviderId::Vendor(Vendor::Anthropic), model, fx.0.join("route")).unwrap();
    let homes = fx.homes();
    let inst = Installed { command: PathBuf::from(&node), args_prefix: vec![entry.display().to_string()], pinned_file: entry.clone(), sha256: citrate_acp_host::registry::sha256_file(&entry).unwrap() };
    let node_dir = Path::new(&node).parent().unwrap().display().to_string();
    let inh = vec![("PATH".to_string(), format!("{node_dir}:/usr/bin:/bin")), ("LANG".into(), "C.UTF-8".into())];
    let l = prepare_launch(&homes, "claude", &inst, AGENTS_LOCK_JSON, inh.into_iter(), Some(Route { grant: &g, model })).unwrap();
    let key = start_session(l).unwrap_or_else(|e| panic!("start: {} / {}", e.reason, e.stderr_tail));
    let s = session(&key).unwrap();
    let pid = s.pid();
    let s2 = s.clone();
    let turn = std::thread::spawn(move || s2.prompt(vec![json!({"type": "text", "text": "Reply with the single word: hello"})]));
    let t0 = std::time::Instant::now();
    while !turn.is_finished() && t0.elapsed() < std::time::Duration::from_secs(45) {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    if !turn.is_finished() {
        s.cancel().unwrap();
    }
    let result = turn.join().unwrap();
    let log = std::fs::read_to_string(crate::provider_usage_log::log_path(&pdir)).unwrap_or_default();
    let mine: Vec<&str> = log.lines().filter(|l| l.contains("\"session\":\"ag:claude:")).collect();
    println!("turn: {result:?} after {:?}", t0.elapsed());
    println!("usage records under ag:claude: {}", mine.len());
    fn find<'a>(v: &'a Value, k: &str) -> Option<&'a Value> {
        match v {
            Value::Object(m) => m.get(k).or_else(|| m.values().find_map(|x| find(x, k))),
            _ => None,
        }
    }
    for r in &mine {
        let v: Value = serde_json::from_str(r).unwrap();
        let f = |k: &str| find(&v, k).cloned().unwrap_or(Value::Null);
        println!("  kind={} provider={} model={} session={} outcome={}", f("kind"), f("provider"), f("model"), f("session"), f("outcome"));
    }
    let events = s.events(0, std::time::Duration::ZERO).0;
    println!("event kinds: {:?}", events.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>());
    drop(s);
    assert!(!mine.is_empty(), "the agent's model calls reached the proxy under its label");
    assert!(!log.contains(g.bearer.expose()), "the route token is not in the log");
    // Removal: sessions closed (process group gone), routes revoked, home deleted.
    assert!(crate::hermes::agent_auth::remove_at(&homes, Some(&proxy), "claude").unwrap());
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(session(&key).is_none());
    #[cfg(unix)]
    assert_ne!(unsafe { libc::killpg(pid as libc::pid_t, 0) }, 0, "no process left in the agent's group");
    assert!(proxy.route_for(g.bearer.expose()).is_none(), "route revoked");
    assert!(!homes.home(&id).exists());
}
