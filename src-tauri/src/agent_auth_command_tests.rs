// L1-WP14b (planset v060): the agent runtime sign-in commands. Only an agent the reviewed lock
// names is accepted; status reads metadata only and creates nothing; sign-out revokes the agent's
// routes and empties its home; removal is the uninstall cleanup; a path-shaped or unknown id is
// refused before the file system is touched.

use super::*;
use crate::model_proxy::tests::Rig;
use crate::model_provider::Vendor;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

struct Fx(PathBuf);

impl Fx {
    fn new() -> Fx {
        let p = std::env::temp_dir().join(format!(
            "core-agent-auth-cmd-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
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

#[test]
fn only_agents_the_lock_names_are_accepted() {
    for id in ["hermes", "codex", "claude", "gemini", "goose", "grok", "muse"] {
        assert!(locked_agent(id).is_ok(), "{id}");
    }
    for id in ["unknown-agent", "openclaw", "", "../codex", "codex/..", "Codex", "codex\n"] {
        assert!(matches!(locked_agent(id), Err(AuthError::BadId(_))), "{id:?}");
    }
}

#[test]
fn a_refused_id_touches_nothing() {
    let fx = Fx::new();
    let h = fx.homes();
    assert!(auth_status_at(&h, "../../etc").is_err());
    assert!(sign_out_at(&h, None, "unknown-agent").is_err());
    assert!(remove_at(&h, None, "..").is_err());
    assert!(!h.home(&id_for_test("codex")).exists());
    assert!(!fx.0.join(AGENTS_DIR).exists(), "no folder is created for a refused id");
}

fn id_for_test(s: &str) -> AgentId {
    AgentId::parse(s).unwrap()
}

#[test]
fn status_command_creates_nothing_and_reports_the_offer() {
    let fx = Fx::new();
    let h = fx.homes();
    let s = auth_status_at(&h, "codex").unwrap();
    assert!(!s.home_exists);
    assert_eq!(s.own_login_present, Some(false));
    assert!(s.own_login_offered);
    assert!(!fx.0.join(AGENTS_DIR).exists());
    let c = auth_status_at(&h, "claude").unwrap();
    assert!(!c.own_login_offered, "Claude Agent signs in only through the proxy (L1-Q3)");
}

#[test]
fn sign_out_command_revokes_routes_and_empties_the_home() {
    let fx = Fx::new();
    let rig = Rig::new("agent-auth-cmd-signout");
    let h = fx.homes();
    let codex = id_for_test("codex");
    let home = h.ensure(&codex).unwrap();
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    std::fs::write(home.join(".codex/auth.json"), "{}").unwrap();
    let g = mint_agent_route(
        &rig.proxy,
        &codex,
        ProviderId::Vendor(Vendor::OpenAi),
        "gpt-known",
        rig.gate(),
    )
    .unwrap();
    let s = sign_out_at(&h, Some(&*rig.proxy), "codex").unwrap();
    assert!(s.home_exists);
    assert_eq!(s.own_login_present, Some(false));
    assert!(rig.proxy.route_for(g.bearer.expose()).is_none());
}

#[test]
fn sign_out_without_a_running_proxy_still_empties_the_home() {
    let fx = Fx::new();
    let h = fx.homes();
    let home = h.ensure(&id_for_test("gemini")).unwrap();
    std::fs::create_dir_all(home.join(".gemini")).unwrap();
    std::fs::write(home.join(".gemini/oauth_creds.json"), "{}").unwrap();
    let s = sign_out_at(&h, None, "gemini").unwrap();
    assert_eq!(s.own_login_present, Some(false));
}

#[test]
fn remove_command_is_the_uninstall_cleanup() {
    let fx = Fx::new();
    let h = fx.homes();
    h.ensure(&id_for_test("goose")).unwrap();
    h.ensure(&id_for_test("grok")).unwrap();
    assert!(remove_at(&h, None, "goose").unwrap());
    assert!(!h.agent_dir(&id_for_test("goose")).exists());
    assert!(h.agent_dir(&id_for_test("grok")).exists());
    assert!(!remove_at(&h, None, "goose").unwrap());
}

#[test]
fn command_answers_carry_no_secret() {
    let fx = Fx::new();
    let h = fx.homes();
    let home = h.ensure(&id_for_test("codex")).unwrap();
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    std::fs::write(
        home.join(".codex/auth.json"),
        format!("{{\"token\":\"{}\"}}", crate::provider_fixtures_tests::fake_key("sk-")),
    )
    .unwrap();
    let json = serde_json::to_string(&auth_status_at(&h, "codex").unwrap()).unwrap();
    assert!(!crate::telemetry::has_key_shape(&json));
    assert!(!json.contains("token"));
}
