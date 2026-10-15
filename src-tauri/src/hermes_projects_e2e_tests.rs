// HGW-W3, end to end with the REAL sidecar and the REAL renderer (env-gated; test-only).
//
// `CITRATE_E2E_PROJECTS_SIDECAR_BIN` names a built `citrate-agent-sidecar` from the runtime stack
// that has `project_create` (citrate-agent-runtime #79-#83); `CITRATE_E2E_TEMPLATES_BIN` names a
// built `citrate-templates` (scripts/build-citrate-templates.sh). Without both this test says so
// and passes without running, like the other live harnesses. Everything runs in a scratch HOME
// under the temp folder; nothing touches the member's data.
//
// The model is a scripted OpenAI-compatible server (a test double that only decides which tool to
// call). Everything else is real: the env core passes at spawn (this module's `sidecar_env`), the
// sidecar's `project_create` offer, its approval card (`/sessions/:id/approvals/pending`), the
// member's decision, the renderer run in the OS sandbox, the marker, core keeping the project,
// and the next session accepting it back as `projectRoots`.
use super::*;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BIN_ENV: &str = "CITRATE_E2E_PROJECTS_SIDECAR_BIN";
const RENDERER_ENV: &str = "CITRATE_E2E_TEMPLATES_BIN";

type Seen = Arc<Mutex<Vec<serde_json::Value>>>;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("free port")
}

fn tool_names(req: &serde_json::Value) -> Vec<String> {
    req["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|t| {
            t["function"]["name"]
                .as_str()
                .or_else(|| t["name"].as_str())
                .map(str::to_string)
        })
        .collect()
}

/// Offered `project_create`, the model asks for the erc20 project `demo`; given a tool result,
/// it answers with it.
fn reply(req: &serde_json::Value) -> serde_json::Value {
    let msgs = req["messages"].as_array().cloned().unwrap_or_default();
    let last = msgs.last().cloned().unwrap_or_default();
    let message = if last["role"] == "tool" {
        let content = last["content"].as_str().unwrap_or_default();
        serde_json::json!({ "role": "assistant", "content": format!("Result: {content}") })
    } else if tool_names(req).iter().any(|n| n == "project_create") {
        let args = serde_json::json!({
            "name": "demo",
            "template": "erc20",
            "params": { "name": "Demo", "symbol": "DMO", "owner": "0x1111111111111111111111111111111111111111" }
        });
        serde_json::json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{
                "id": format!("call_proj_{}", msgs.len()),
                "type": "function",
                "function": { "name": "project_create", "arguments": args.to_string() }
            }]
        })
    } else {
        serde_json::json!({ "role": "assistant", "content": "No project tool in this conversation." })
    };
    serde_json::json!({ "choices": [{ "index": 0, "message": message, "finish_reason": "stop" }] })
}

fn serve_one(mut s: TcpStream, seen: &Seen) -> Result<(), String> {
    let mut r = BufReader::new(s.try_clone().map_err(|e| e.to_string())?);
    let mut request_line = String::new();
    r.read_line(&mut request_line).map_err(|e| e.to_string())?;
    let mut len = 0usize;
    loop {
        let mut line = String::new();
        r.read_line(&mut line).map_err(|e| e.to_string())?;
        let l = line.trim();
        if l.is_empty() {
            break;
        }
        if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse().map_err(|_| "bad content-length")?;
        }
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).map_err(|e| e.to_string())?;
    if !request_line.contains("/chat/completions") {
        return write!(
            s,
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .map_err(|e| e.to_string());
    }
    let req: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    let out = reply(&req).to_string();
    seen.lock().unwrap_or_else(|e| e.into_inner()).push(req);
    write!(
        s,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
        out.len()
    )
    .map_err(|e| e.to_string())
}

fn start_model(seen: Seen) -> String {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind model");
    let port = l.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let _ = serve_one(s, &seen);
        }
    });
    format!("http://127.0.0.1:{port}/v1")
}

fn wait_until<T>(what: &str, limit: Duration, mut f: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(start.elapsed() < limit, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// The `path` of a `project_create` result (its JSON report, possibly after a prefix).
fn created_path(content: &str) -> Option<String> {
    let start = content.find('{')?;
    let v: serde_json::Value = serde_json::from_str(&content[start..]).ok()?;
    (v["ok"] == true).then(|| v["path"].as_str().map(str::to_string))?
}

#[test]
fn project_create_live_with_the_real_sidecar_and_renderer() {
    let (Ok(bin), Ok(renderer)) = (std::env::var(BIN_ENV), std::env::var(RENDERER_ENV)) else {
        eprintln!("skipped: set {BIN_ENV} and {RENDERER_ENV} to run the live project_create e2e");
        return;
    };
    let base = std::env::temp_dir().join(format!("hermes-proj-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("home")).expect("home");
    let base = std::fs::canonicalize(&base).expect("canon");
    let home = base.join("home");
    let hermes_dir = base.join("data/hermes");
    std::fs::create_dir_all(&hermes_dir).expect("hermes dir");
    let grants = crate::agent_grants::GrantStore::new(base.join("data/agent"), &home);
    let projects = ProjectStore::new(&hermes_dir, &home);

    // The workspace card: allowed, so the folder is created and granted read + write.
    workspace_enable(&projects, &grants.view(1), |root| {
        grants
            .add_folder(root, true, true, 1)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .expect("workspace");
    assert!(projects.view(&grants.view(2)).workspace_ready);

    let templates = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../templates");
    let templates = std::fs::canonicalize(templates).expect("templates");
    let r = Renderer {
        program: PathBuf::from(&renderer),
        templates,
        tier: "T0".into(),
    };
    let env_home = home.clone();
    let m = crate::hermes::HermesManager::new(
        PathBuf::from(&bin),
        hermes_dir.join("bearer.token"),
        hermes_dir.join("crashes.log"),
    )
    .with_control_addr(&format!("127.0.0.1:{}", free_port()))
    .with_health_interval(Duration::from_millis(200))
    .with_env_source(Arc::new(move || {
        sidecar_env(
            &env_home,
            Some(&r),
            valid_identity("Scratch Member", "scratch@example.org"),
        )
    }));
    m.start().expect("start the sidecar");
    let health = format!("{}/health", m.control_url());
    wait_until(
        "the sidecar to answer /health",
        Duration::from_secs(30),
        || crate::serve::http_health_ok(&health).then_some(()),
    );

    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let model_url = start_model(seen.clone());
    let open = |roots: &[String]| -> Result<String, String> {
        let body = crate::hermes::build_session_body(
            "You are Hermes.",
            "[]",
            &model_url,
            "",
            "scripted",
            8192,
        )?;
        let doc = grants.document_for_agent();
        let body = crate::agent_grants::attach_grants(&body, &doc)?;
        let body = attach_project_roots(&body, roots)?;
        m.session_open(&body).map_err(|e| e.to_string())
    };

    // A conversation with the workspace shared is offered project_create, which waits on a card.
    let s1 = open(&[]).expect("session");
    m.session_send(&s1, "create an erc20 project called demo")
        .expect("send");
    let first = wait_until("the model's first request", Duration::from_secs(30), || {
        seen.lock()
            .unwrap_or_else(|e| e.into_inner())
            .first()
            .cloned()
    });
    assert!(
        tool_names(&first).iter().any(|n| n == "project_create"),
        "projects on: project_create is offered: {:?}",
        tool_names(&first)
    );
    let card = wait_until("the project_create card", Duration::from_secs(60), || {
        let resp = m
            .control_get(&format!("/sessions/{s1}/approvals/pending"))
            .ok()?;
        let v: serde_json::Value = serde_json::from_str(&resp.body).ok()?;
        v["pending"].as_array().and_then(|a| a.first().cloned())
    });
    assert_eq!(card["tool"], "project_create", "{card}");
    assert_eq!(card["hic"], "required");
    let demo = projects_root(&home).join("demo");
    assert!(card["summary"]
        .as_str()
        .unwrap_or_default()
        .contains(&demo.display().to_string()));
    assert!(
        !demo.exists(),
        "nothing is created before the member decides"
    );
    let aid = card["id"].as_str().expect("id").to_string();
    let subject = card["subject"].as_str().expect("subject").to_string();
    let resp = m
        .control_post(
            &format!("/sessions/{s1}/approvals/{aid}/decide"),
            &serde_json::json!({ "allow": true, "subject": subject }).to_string(),
        )
        .expect("decide");
    assert_eq!(resp.status, 200, "{}", resp.body);

    // The renderer ran: the project, its marker and the rendered sources exist.
    let result = wait_until(
        "the tool result at the model",
        Duration::from_secs(90),
        || {
            let g = seen.lock().unwrap_or_else(|e| e.into_inner());
            g.iter().find_map(|r| {
                let last = r["messages"].as_array()?.last()?.clone();
                (last["role"] == "tool")
                    .then(|| last["content"].as_str().unwrap_or_default().to_string())
            })
        },
    );
    let made =
        created_path(&result).unwrap_or_else(|| panic!("project_create returned a path: {result}"));
    assert_eq!(PathBuf::from(&made), demo);
    assert!(
        demo.join(MARKER_FILE).is_file(),
        "the runtime wrote its marker"
    );
    assert!(
        std::fs::read_dir(&demo).expect("ls").count() > 1,
        "the erc20 template was rendered into the project"
    );
    eprintln!(
        "project_create result: {}",
        result.chars().take(400).collect::<String>()
    );

    // Core keeps it, and the next conversation is accepted with it as a project root.
    projects.record_created(&made, 3).expect("record");
    let roots = projects.live_roots(&grants.view(4));
    assert_eq!(roots, vec![made.clone()]);
    let s2 = open(&roots).expect("the runtime accepts core's projectRoots");
    let _ = m.session_close(&s1);
    let _ = m.session_close(&s2);
    m.stop();
    let _ = std::fs::remove_dir_all(&base);
}
