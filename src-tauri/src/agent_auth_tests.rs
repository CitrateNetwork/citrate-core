// L1-WP14a (planset v060): per-agent homes. Each agent's home is owner-only at every level and
// never a symlink; the launch environment points HOME, XDG and the agent's own config variables
// into it, drops every vendor key variable and key-shaped value, and carries at most one Citrate
// route token; arguments and ACP authenticate requests never carry a key; sign-out and removal
// revoke only that agent's routes and never follow a link out; one agent cannot resolve a path in
// another agent's home; the home and its sign-in survive a restart.

use super::*;
use crate::model_provider::{ProviderId, Vendor};
use crate::model_proxy::tests::Rig;
use crate::provider_fixtures_tests::fake_key;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

/// A fresh app data folder (canonical, so path comparisons are exact).
struct Fx {
    base: PathBuf,
}

impl Fx {
    fn new() -> Fx {
        let base = std::env::temp_dir().join(format!(
            "core-agent-auth-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("appdata")).unwrap();
        Fx {
            base: base.canonicalize().unwrap(),
        }
    }
    fn appdata(&self) -> PathBuf {
        self.base.join("appdata")
    }
    fn homes(&self) -> AgentHomes {
        AgentHomes::new(&self.appdata())
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Undo any 0000 a test left so the cleanup can descend.
            for e in walk(&self.base) {
                let _ = std::fs::set_permissions(&e, std::fs::Permissions::from_mode(0o700));
            }
        }
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn walk(p: &Path) -> Vec<PathBuf> {
    let mut out = vec![p.to_path_buf()];
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let ft = e.file_type().ok();
            if ft.is_some_and(|t| t.is_dir()) {
                out.extend(walk(&e.path()));
            } else {
                out.push(e.path());
            }
        }
    }
    out
}

fn id(s: &str) -> AgentId {
    AgentId::parse(s).unwrap()
}

fn get<'a>(env: &'a [(String, String)], k: &str) -> Option<&'a str> {
    env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

#[cfg(unix)]
fn mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::symlink_metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn agent_ids_are_validated() {
    for ok in [
        "hermes", "codex", "claude", "gemini", "goose", "grok", "muse", "a", "codex-2",
    ] {
        assert!(AgentId::parse(ok).is_ok(), "{ok}");
    }
    let long = "a".repeat(MAX_ID_LEN + 1);
    for bad in [
        "",
        "..",
        ".",
        "a/b",
        "../x",
        "Codex",
        "-x",
        "x-",
        "2x",
        "a b",
        "a:b",
        "a\0b",
        long.as_str(),
    ] {
        assert!(
            matches!(AgentId::parse(bad), Err(AuthError::BadId(_))),
            "{bad:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn every_level_of_a_new_home_is_owner_only() {
    let fx = Fx::new();
    let h = fx.homes();
    let home = h.ensure_home(&id("codex")).unwrap();
    assert_eq!(home, fx.appdata().join("agents/codex/home"));
    for p in [
        fx.appdata().join("agents"),
        fx.appdata().join("agents/codex"),
        home,
    ] {
        assert_eq!(mode(&p), 0o700, "{}", p.display());
    }
}

#[cfg(unix)]
#[test]
fn a_loose_existing_home_is_narrowed_to_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fx::new();
    let home = fx.appdata().join("agents/gemini/home");
    std::fs::create_dir_all(&home).unwrap();
    for p in [
        fx.appdata().join("agents"),
        fx.appdata().join("agents/gemini"),
        home.clone(),
    ] {
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    fx.homes().ensure_home(&id("gemini")).unwrap();
    assert_eq!(mode(&home), 0o700);
    assert_eq!(mode(&fx.appdata().join("agents")), 0o700);
}

#[cfg(unix)]
#[test]
fn a_symlinked_home_or_agent_folder_is_refused() {
    let fx = Fx::new();
    let elsewhere = fx.base.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::create_dir_all(fx.appdata().join("agents/codex")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, fx.appdata().join("agents/codex/home")).unwrap();
    assert!(matches!(
        fx.homes().ensure_home(&id("codex")),
        Err(AuthError::Refused(_))
    ));
    std::os::unix::fs::symlink(&elsewhere, fx.appdata().join("agents/goose")).unwrap();
    assert!(matches!(
        fx.homes().ensure_home(&id("goose")),
        Err(AuthError::Refused(_))
    ));
    assert!(matches!(
        fx.homes().remove_agent_dir(&id("goose"), None),
        Err(AuthError::Refused(_))
    ));
    assert!(elsewhere.exists(), "a refused removal deletes nothing");
}

#[test]
fn launch_env_points_home_xdg_and_the_agents_own_variables_into_its_home() {
    let fx = Fx::new();
    let h = fx.homes();
    let env = h
        .launch_env(&id("codex"), std::iter::empty(), None)
        .unwrap();
    let home = h.home(&id("codex"));
    assert_eq!(get(&env, "HOME"), Some(home.to_str().unwrap()));
    assert_eq!(
        get(&env, "CODEX_HOME"),
        Some(home.join(".codex").to_str().unwrap())
    );
    for k in [
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
        "USERPROFILE",
    ] {
        assert!(Path::new(get(&env, k).unwrap()).starts_with(&home), "{k}");
    }
    let g = h
        .launch_env(&id("gemini"), std::iter::empty(), None)
        .unwrap();
    assert_eq!(
        get(&g, "GEMINI_CLI_HOME"),
        Some(h.home(&id("gemini")).to_str().unwrap())
    );
    assert_eq!(
        get(&g, "GEMINI_FORCE_FILE_STORAGE"),
        Some("true"),
        "tokens stay in a home file"
    );
    let c = h
        .launch_env(&id("claude"), std::iter::empty(), None)
        .unwrap();
    assert_eq!(
        get(&c, "CLAUDE_CONFIG_DIR"),
        Some(h.home(&id("claude")).join(".claude").to_str().unwrap())
    );
    // Every path variable of every agent stays inside that agent's own home.
    for a in [
        "hermes", "codex", "claude", "gemini", "goose", "grok", "muse",
    ] {
        let env = h.launch_env(&id(a), std::iter::empty(), None).unwrap();
        for (k, v) in &env {
            if v.starts_with('/') {
                assert!(Path::new(v).starts_with(h.home(&id(a))), "{a}: {k}={v}");
            }
        }
    }
}

#[test]
fn launch_env_drops_vendor_key_variables_key_shaped_values_and_everything_unlisted() {
    let fx = Fx::new();
    let inherited = vec![
        ("PATH".to_string(), "/usr/bin:/bin".to_string()),
        ("LANG".to_string(), "en_US.UTF-8".to_string()),
        ("OPENAI_API_KEY".to_string(), fake_key("sk-")),
        ("ANTHROPIC_API_KEY".to_string(), fake_key("sk-ant-")),
        ("GEMINI_API_KEY".to_string(), fake_key("AIza")),
        ("TERM".to_string(), fake_key("xai-")),
        ("HOME".to_string(), "/Users/member".to_string()),
        ("SSH_AUTH_SOCK".to_string(), "/tmp/ssh.sock".to_string()),
        ("AWS_SECRET_ACCESS_KEY".to_string(), "x".repeat(40)),
        ("CITRATE_DATA_DIR".to_string(), "/core/node".to_string()),
    ];
    let env = fx
        .homes()
        .launch_env(&id("codex"), inherited.into_iter(), None)
        .unwrap();
    assert_eq!(get(&env, "PATH"), Some("/usr/bin:/bin"));
    assert_eq!(get(&env, "LANG"), Some("en_US.UTF-8"));
    assert_eq!(
        get(&env, "TERM"),
        None,
        "a key-shaped value is dropped even under an allowed name"
    );
    assert_ne!(
        get(&env, "HOME"),
        Some("/Users/member"),
        "the member's home never crosses"
    );
    for gone in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "GEMINI_API_KEY",
        "SSH_AUTH_SOCK",
        "AWS_SECRET_ACCESS_KEY",
        "CITRATE_DATA_DIR",
    ] {
        assert_eq!(get(&env, gone), None, "{gone}");
    }
    for (k, v) in &env {
        assert!(
            !crate::telemetry::has_key_shape(v),
            "{k} carries a key shape"
        );
    }
}

#[test]
fn a_proxy_agent_gets_only_its_scoped_route_token_never_the_vendor_key() {
    let fx = Fx::new();
    let rig = Rig::new("agent-auth-route");
    let codex = id("codex");
    let grant = mint_agent_route(
        &rig.proxy,
        &codex,
        ProviderId::Vendor(Vendor::OpenAi),
        "gpt-known",
        rig.gate(),
    )
    .unwrap();
    let env = fx
        .homes()
        .launch_env(&codex, std::iter::empty(), Some(&grant))
        .unwrap();
    let token = get(&env, ROUTE_TOKEN_ENV).unwrap();
    assert!(is_route_token(token));
    assert_eq!(get(&env, ROUTE_BASE_ENV), Some("http://127.0.0.1:1/v1"));
    // The token is bound to exactly this provider, model and agent.
    let route = rig.proxy.route_for(token).unwrap();
    assert_eq!(route.provider.as_str(), "openai");
    assert_eq!(route.model, "gpt-known");
    assert!(
        route.session_id.starts_with("ag:codex:"),
        "{}",
        route.session_id
    );
    // The vendor key appears nowhere in the environment, and nothing else is credential-shaped.
    let all: String = env.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
    assert!(
        !all.contains(&rig.key),
        "the vendor key never reaches an agent"
    );
    let creds: Vec<_> = env
        .iter()
        .filter(|(_, v)| crate::telemetry::has_key_shape(v))
        .collect();
    assert_eq!(creds.len(), 1, "exactly one credential: the route token");
    assert_eq!(creds[0].0, ROUTE_TOKEN_ENV);
}

#[test]
fn a_grant_that_is_not_a_route_token_is_refused() {
    let fx = Fx::new();
    let fake = ProxyGrant {
        base_url: "http://127.0.0.1:1/v1".into(),
        bearer: crate::secret::SecretString::new(fake_key("sk-")),
    };
    let r = fx
        .homes()
        .launch_env(&id("codex"), std::iter::empty(), Some(&fake));
    assert!(matches!(r, Err(AuthError::Refused(_))));
}

#[test]
fn check_env_refuses_vendor_key_names_shapes_and_misplaced_route_tokens() {
    let token = format!("cmp_{}", "ab".repeat(32));
    assert!(check_env(&[(ROUTE_TOKEN_ENV.into(), token.clone())]).is_ok());
    assert!(
        check_env(&[("OPENAI_API_KEY".into(), token.clone())]).is_err(),
        "a vendor key name, whatever it holds"
    );
    assert!(
        check_env(&[("openai_api_key".into(), "x".into())]).is_err(),
        "names match case-insensitively"
    );
    assert!(
        check_env(&[("OTHER".into(), token)]).is_err(),
        "a route token only in its own variable"
    );
    for p in ["sk-", "sk-ant-", "AIza", "xai-", "cog_", "cgk_", "LLM_"] {
        assert!(check_env(&[("X".into(), fake_key(p))]).is_err(), "{p}");
    }
}

#[test]
fn arguments_never_carry_a_key_or_a_route_token() {
    assert!(check_argv(&["acp".into(), "--model".into(), "gpt-known".into()]).is_ok());
    assert!(check_argv(&["--api-key".into(), fake_key("sk-")]).is_err());
    assert!(
        check_argv(&[format!("--token=cmp_{}", "cd".repeat(32))]).is_err(),
        "route tokens stay out of argv too"
    );
}

#[test]
fn an_acp_authenticate_request_never_carries_a_vendor_key() {
    assert!(check_authenticate(r#"{"methodId":"chat-gpt"}"#).is_ok());
    let with_key = format!(
        r#"{{"methodId":"api-key","_meta":{{"key":"{}"}}}}"#,
        fake_key("sk-")
    );
    assert!(matches!(
        check_authenticate(&with_key),
        Err(AuthError::Refused(_))
    ));
    let gemini = format!(
        r#"{{"methodId":"gemini-api-key","_meta":{{"api-key":"{}"}}}}"#,
        fake_key("AIza")
    );
    assert!(check_authenticate(&gemini).is_err());
}

#[cfg(unix)]
#[test]
fn apply_env_replaces_the_inherited_environment_of_a_real_process() {
    let fx = Fx::new();
    let env = fx
        .homes()
        .launch_env(
            &id("goose"),
            vec![("PATH".to_string(), "/usr/bin:/bin".to_string())].into_iter(),
            None,
        )
        .unwrap();
    let mut cmd = std::process::Command::new("/usr/bin/env");
    apply_env(&mut cmd, &env);
    let out = cmd.output().expect("run env");
    let text = String::from_utf8_lossy(&out.stdout);
    let names: Vec<&str> = text.lines().filter_map(|l| l.split('=').next()).collect();
    let allowed: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
    for n in &names {
        assert!(
            allowed.contains(n),
            "{n} leaked into the agent's environment"
        );
    }
    assert!(names.contains(&"HOME"));
}

#[test]
fn one_agent_cannot_resolve_a_path_in_another_agents_home() {
    let fx = Fx::new();
    let h = fx.homes();
    let codex_home = h.ensure_home(&id("codex")).unwrap();
    let gemini_home = h.ensure_home(&id("gemini")).unwrap();
    std::fs::create_dir_all(gemini_home.join(".gemini")).unwrap();
    std::fs::write(gemini_home.join(".gemini/oauth_creds.json"), "{}").unwrap();
    // Inside its own home (existing or not yet): allowed.
    assert!(h
        .resolve_in_home(&id("codex"), &codex_home.join(".codex/config.toml"))
        .is_ok());
    // Another agent's home: refused, by name.
    let e = h
        .resolve_in_home(&id("codex"), &gemini_home.join(".gemini/oauth_creds.json"))
        .unwrap_err();
    assert!(
        e.to_string().contains("belongs to the agent \"gemini\""),
        "{e}"
    );
    // `..` out of the home, a relative path, and core's own app data: refused.
    assert!(h
        .resolve_in_home(&id("codex"), &codex_home.join("../../gemini/home/x"))
        .is_err());
    assert!(h
        .resolve_in_home(&id("codex"), &codex_home.join("nope/../../../gemini/home"))
        .is_err());
    assert!(h
        .resolve_in_home(&id("codex"), Path::new("relative/x"))
        .is_err());
    assert!(h
        .resolve_in_home(&id("codex"), &fx.appdata().join("agent/agent-grants.json"))
        .is_err());
}

#[cfg(unix)]
#[test]
fn a_symlink_out_of_the_home_is_refused() {
    let fx = Fx::new();
    let h = fx.homes();
    let codex_home = h.ensure_home(&id("codex")).unwrap();
    let gemini_home = h.ensure_home(&id("gemini")).unwrap();
    std::os::unix::fs::symlink(&gemini_home, codex_home.join("peek")).unwrap();
    std::os::unix::fs::symlink(&fx.base, codex_home.join("up")).unwrap();
    assert!(h
        .resolve_in_home(&id("codex"), &codex_home.join("peek/x"))
        .is_err());
    assert!(h
        .resolve_in_home(&id("codex"), &codex_home.join("up"))
        .is_err());
}

#[test]
fn the_home_and_the_agents_own_sign_in_survive_a_restart() {
    let fx = Fx::new();
    let home = fx.homes().ensure_home(&id("codex")).unwrap();
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    std::fs::write(home.join(".codex/auth.json"), "{}").unwrap();
    // A new process: a fresh AgentHomes over the same app data.
    let again = AgentHomes::new(&fx.appdata());
    assert_eq!(again.ensure_home(&id("codex")).unwrap(), home);
    assert!(
        home.join(".codex/auth.json").exists(),
        "ensure never clears an existing home"
    );
    let s = again.home_status(&id("codex"));
    assert!(s.home_exists);
    assert_eq!(s.own_login_present, Some(true));
    assert!(s.own_login_offered);
}

#[cfg(unix)]
#[test]
fn status_reads_metadata_only_never_the_login_file() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fx::new();
    let home = fx.homes().ensure_home(&id("gemini")).unwrap();
    std::fs::create_dir_all(home.join(".gemini")).unwrap();
    let f = home.join(".gemini/oauth_creds.json");
    std::fs::write(&f, "{\"token\":\"secret-value\"}").unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o000)).unwrap();
    let s = fx.homes().home_status(&id("gemini"));
    assert_eq!(
        s.own_login_present,
        Some(true),
        "presence comes from metadata"
    );
    let json = serde_json::to_string(&s).unwrap();
    assert!(!json.contains("secret-value"));
}

#[test]
fn own_sign_in_is_offered_only_where_core_knows_the_agent_keeps_it_in_its_home() {
    let fx = Fx::new();
    let h = fx.homes();
    for (a, offered) in [
        ("codex", true),
        ("gemini", true),
        ("claude", false),
        ("hermes", false),
        ("goose", false),
        ("grok", false),
        ("muse", false),
        ("unknown-agent", false),
    ] {
        let s = h.home_status(&id(a));
        assert_eq!(s.own_login_offered, offered, "{a}");
        assert_eq!(s.own_login_present.is_some(), offered, "{a}");
        assert!(!s.home_exists, "status creates nothing");
    }
}

#[test]
fn sign_out_empties_the_home_and_revokes_only_that_agents_routes() {
    let fx = Fx::new();
    let rig = Rig::new("agent-auth-signout");
    let h = fx.homes();
    let (codex, codex_x) = (id("codex"), id("codex-x"));
    let home = h.ensure_home(&codex).unwrap();
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    std::fs::write(home.join(".codex/auth.json"), "{}").unwrap();
    let other_home = h.ensure_home(&codex_x).unwrap();
    std::fs::write(other_home.join("keep"), "x").unwrap();
    let g1 = mint_agent_route(
        &rig.proxy,
        &codex,
        ProviderId::Vendor(Vendor::OpenAi),
        "gpt-known",
        rig.gate(),
    )
    .unwrap();
    let g2 = mint_agent_route(
        &rig.proxy,
        &codex_x,
        ProviderId::Vendor(Vendor::OpenAi),
        "gpt-known",
        rig.gate(),
    )
    .unwrap();
    h.sign_out_agent(&codex, Some(&*rig.proxy)).unwrap();
    assert!(
        rig.proxy.route_for(g1.bearer.expose()).is_none(),
        "the agent's route is revoked"
    );
    assert!(
        rig.proxy.route_for(g2.bearer.expose()).is_some(),
        "a similarly named agent keeps its route"
    );
    assert!(home.exists(), "the home is recreated");
    assert_eq!(std::fs::read_dir(&home).unwrap().count(), 0, "and empty");
    assert_eq!(h.home_status(&codex).own_login_present, Some(false));
    assert!(
        other_home.join("keep").exists(),
        "another agent's home is untouched"
    );
    #[cfg(unix)]
    assert_eq!(mode(&home), 0o700);
}

#[cfg(unix)]
#[test]
fn removal_deletes_the_agents_folder_never_a_link_target_or_another_agent() {
    let fx = Fx::new();
    let rig = Rig::new("agent-auth-remove");
    let h = fx.homes();
    let outside = fx.base.join("outside.txt");
    std::fs::write(&outside, "member file").unwrap();
    let home = h.ensure_home(&id("goose")).unwrap();
    std::os::unix::fs::symlink(&outside, home.join("link")).unwrap();
    std::os::unix::fs::symlink(&fx.base, home.join("dirlink")).unwrap();
    let other = h.ensure_home(&id("grok")).unwrap();
    let g = mint_agent_route(
        &rig.proxy,
        &id("goose"),
        ProviderId::Vendor(Vendor::OpenAi),
        "gpt-known",
        rig.gate(),
    )
    .unwrap();
    assert!(h.remove_agent_dir(&id("goose"), Some(&*rig.proxy)).unwrap());
    assert!(!h.agent_dir(&id("goose")).exists());
    assert!(rig.proxy.route_for(g.bearer.expose()).is_none());
    assert_eq!(
        std::fs::read_to_string(&outside).unwrap(),
        "member file",
        "a link's target survives"
    );
    assert!(other.exists(), "another agent's folder survives");
    assert!(
        !h.remove_agent_dir(&id("goose"), Some(&*rig.proxy)).unwrap(),
        "removing twice is a no-op"
    );
}
