//! L1-WP03c (v060) — the ACP host inside Core: one [`AcpHost`] for the app, the live sessions, and
//! the launch of a pinned agent in its own home with at most one route. Planset v060
//! `lanes/L1_AGENT_RUNTIME.md` WP03 (session manager in the app) and WP14 step 2 (the route written
//! into the agent's own config inside its home).
//!
//! Data sources (Rule 7): `release/agents.lock.json` (launch rows), the agent's home under
//! `<app data>/agents/<id>/home` (`agent_auth::AgentHomes`), the in-process model proxy (route
//! tokens, `agent_auth::mint_agent_route`), and the agent process's own ACP stdio.
//!
//! Not wired to a Tauri command yet: an agent is runnable only once the component updater installs
//! it (WP13; "not installable in this build" until the signing ceremony), so the `agent_session_*`
//! commands land with the install path. Sign-out and removal already stop the agent's sessions.
//! Until then only sign-out and removal call into this module outside its tests, hence the
//! dead-code allowance below (the same one `agent_auth.rs` carries for the same reason).
#![cfg_attr(not(test), allow(dead_code))]

use crate::hermes::agent_auth::{self, AgentHomes, AgentId, ROUTE_TOKEN_ENV};
use crate::model_proxy::ProxyGrant;
use citrate_acp_host::permission::DenyByDefault;
use citrate_acp_host::registry::{locked_launch, AgentSpec};
use citrate_acp_host::{AcpHost, HostConfig, Session, SessionStart, StartFailure};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// The reviewed agents lock, compiled in (the same file `agent_auth::locked_agent` reads).
pub const AGENTS_LOCK_JSON: &str = include_str!("../../release/agents.lock.json");

/// The command Claude Code runs to read its key: it prints the route token from the environment,
/// so the token sits in one variable and in no file.
pub const PRINTENV: &str = "/usr/bin/printenv";

/// Where an installed agent's program is (WP13 installs it; the pin is checked again at spawn).
#[derive(Debug, Clone)]
pub struct Installed {
    /// An absolute program path (the agent's own binary, or the pinned Node runtime).
    pub command: PathBuf,
    /// Arguments before the lock row's (a Node agent's entry script).
    pub args_prefix: Vec<String>,
    pub pinned_file: PathBuf,
    pub sha256: [u8; 32],
}

/// The model route of one session: the proxy grant and the model it is bound to.
pub struct Route<'a> {
    pub grant: &'a ProxyGrant,
    pub model: &'a str,
}

/// Everything a session needs to start.
#[derive(Debug)]
pub struct Launch {
    pub spec: AgentSpec,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
}

/// The host configuration Core runs with: Core's vendor-key check on every outbound frame, prompt,
/// argument and env value; `CITRATE_ROUTE_TOKEN` as the one route variable (guarded by exact match
/// per session); every permission request denied until L2's HIC policy lands (WP06).
pub fn host_config() -> HostConfig {
    HostConfig {
        client_name: "citrate-core".into(),
        client_version: env!("CARGO_PKG_VERSION").into(),
        scan: Arc::new(crate::telemetry::has_vendor_key_shape),
        policy: Arc::new(DenyByDefault),
        tap: None,
        init_timeout: Duration::from_secs(90),
        cancel_grace: Duration::from_secs(5),
        route_env: Some(ROUTE_TOKEN_ENV.into()),
    }
}

/// Write `text` to `<home>/<dirs...>/<file>`, refusing any link on the way (the agent may have
/// planted one to make Core write outside its home); folders are created owner-only.
fn write_in_home(home: &Path, dirs: &[&str], file: &str, text: &str) -> Result<(), String> {
    let refused = |p: &Path| {
        format!(
            "refused: {} is a link or not a folder; Core writes only inside the agent's home",
            p.display()
        )
    };
    let mut cur = home.to_path_buf();
    for d in dirs {
        cur.push(d);
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() || !m.is_dir() => return Err(refused(&cur)),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let mut b = std::fs::DirBuilder::new();
                #[cfg(unix)]
                std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
                b.create(&cur)
                    .map_err(|e| format!("{}: {e}", cur.display()))?;
            }
            Err(e) => return Err(format!("{}: {e}", cur.display())),
        }
    }
    let path = cur.join(file);
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(refused(&path));
    }
    crate::atomic_file::replace(&path, text.as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// The route written into the agent's own config inside its home (never the token itself), and the
/// extra variables that point the agent at the proxy.
fn wire_route(home: &Path, id: &str, route: &Route<'_>) -> Result<Vec<(String, String)>, String> {
    match id {
        "claude" => {
            let path = home.join(".claude").join("settings.json");
            let mut s: Value = std::fs::symlink_metadata(&path)
                .ok()
                .filter(|m| m.is_file())
                .and_then(|_| std::fs::read_to_string(&path).ok())
                .and_then(|t| serde_json::from_str(&t).ok())
                .filter(Value::is_object)
                .unwrap_or_else(|| json!({}));
            s["apiKeyHelper"] = json!(format!("{PRINTENV} {ROUTE_TOKEN_ENV}"));
            let text = serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?;
            write_in_home(home, &[".claude"], "settings.json", &text)?;
            let base = route.grant.base_url.trim_end_matches("/v1").to_string();
            let mut env = vec![("ANTHROPIC_BASE_URL".to_string(), base)];
            for k in [
                "ANTHROPIC_MODEL",
                "ANTHROPIC_SMALL_FAST_MODEL",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            ] {
                env.push((k.to_string(), route.model.to_string()));
            }
            Ok(env)
        }
        "codex" => {
            let cfg = format!(
                "# Written by Citrate Core: Codex reaches its model through the Core proxy.\nmodel = {m}\nmodel_provider = \"citrate\"\n\n[model_providers.citrate]\nname = \"Citrate Core\"\nbase_url = {b}\nenv_key = \"{ROUTE_TOKEN_ENV}\"\nwire_api = \"responses\"\n",
                m = toml_str(route.model),
                b = toml_str(&route.grant.base_url),
            );
            write_in_home(home, &[".codex"], "config.toml", &cfg)?;
            Ok(vec![])
        }
        other => Err(format!(
            "{other} has no proxy wiring yet; it can use only its own sign-in"
        )),
    }
}

fn toml_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The launch of `id` from its lock row: its home-only environment (`AgentHomes::launch_env`), the
/// lock row's fixed variables, and, with a route, the route wired into the agent's own config.
pub fn prepare_launch(
    homes: &AgentHomes,
    id: &str,
    installed: &Installed,
    lock_json: &str,
    inherited: impl Iterator<Item = (String, String)>,
    route: Option<Route<'_>>,
) -> Result<Launch, String> {
    let row = locked_launch(lock_json, id)?;
    let agent = AgentId::parse(id).map_err(|e| e.to_string())?;
    let mut env = homes
        .launch_env(&agent, inherited, route.as_ref().map(|r| r.grant))
        .map_err(|e| e.to_string())?;
    let home = homes.home(&agent);
    let mut set = |k: String, v: String| {
        env.retain(|(n, _)| !n.eq_ignore_ascii_case(&k));
        env.push((k, v));
    };
    for (k, v) in row.env {
        set(k, v);
    }
    if let Some(r) = &route {
        for (k, v) in wire_route(&home, id, r)? {
            set(k, v);
        }
    }
    agent_auth::check_env(&env).map_err(|e| e.to_string())?;
    let mut args = installed.args_prefix.clone();
    args.extend(row.args);
    agent_auth::check_argv(&args).map_err(|e| e.to_string())?;
    let spec = AgentSpec {
        id: id.to_string(),
        command: installed.command.clone(),
        args,
        pinned_file: installed.pinned_file.clone(),
        sha256: installed.sha256,
    };
    Ok(Launch {
        spec,
        env,
        cwd: home,
    })
}

struct Registry {
    host: AcpHost,
    live: Mutex<HashMap<String, Arc<Session>>>,
}

static REGISTRY: OnceLock<Result<Registry, String>> = OnceLock::new();

fn registry() -> Result<&'static Registry, String> {
    REGISTRY
        .get_or_init(|| {
            Ok(Registry {
                host: AcpHost::new(host_config()).map_err(|e| format!("acp host: {e}"))?,
                live: Mutex::new(HashMap::new()),
            })
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn live() -> Option<&'static Mutex<HashMap<String, Arc<Session>>>> {
    REGISTRY
        .get()
        .and_then(|r| r.as_ref().ok())
        .map(|r| &r.live)
}

/// Start a session; answers its key (`<agent id>:<ACP session id>`). Blocks until the agent has
/// answered `initialize` and `session/new`, so it runs off the main thread.
pub fn start_session(l: Launch) -> Result<String, Box<StartFailure>> {
    let reg = registry().map_err(|reason| {
        Box::new(StartFailure {
            reason,
            initialize: None,
            auth_required: false,
            stderr_tail: String::new(),
        })
    })?;
    let s = reg.host.start(SessionStart {
        spec: l.spec,
        env: l.env,
        cwd: l.cwd,
        mcp_servers: vec![],
    })?;
    let key = format!("{}:{}", s.runtime_id(), s.acp_session_id());
    reg.live
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(key.clone(), Arc::new(s));
    Ok(key)
}

pub fn session(key: &str) -> Option<Arc<Session>> {
    live()?
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(key)
        .cloned()
}

/// Stop every session of one agent (sign-out, removal). Answers how many were stopped.
pub fn close_agent_sessions(id: &str) -> usize {
    let Some(m) = live() else { return 0 };
    let prefix = format!("{id}:");
    let gone: Vec<Arc<Session>> = {
        let mut map = m.lock().unwrap_or_else(|p| p.into_inner());
        let keys: Vec<String> = map
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        keys.iter().filter_map(|k| map.remove(k)).collect()
    };
    for s in &gone {
        s.shutdown();
    }
    gone.len()
}

#[cfg(test)]
mod tests {
    include!("agent_runtime_ipc_tests.rs");
}
