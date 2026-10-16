// HUP-S6 US-6.1 AC1/AC2 + US-6.2 + g3-e2e prep, the **sidecar-driven** half of the hello-mint e2e
// (included into `hello_mint_e2e_tests.rs`; test-only).
//
// A real `citrate-agent-sidecar` (from `CITRATE_E2E_HM_SIDECAR_BIN`, which
// `scripts/e2e-hello-mint.sh --sidecar-bin` sets) runs a Hermes session with the toolchain on and
// the rendered project granted. Its forge_test, slither_scan, aderyn_scan and medusa_fuzz tools
// run the real programs; core reads the raw reports back over the bearer channel
// (`deploy_gate_toolchain::build_from_sidecar`, the same path as `deploy_gate_submit_toolchain`)
// and its gate runs the fork step itself on the Citrate-aware fork (`forkInCore`, the
// `citrate-fork` binary from `CITRATE_E2E_HM_FORK_BIN`) on 40204 state from the local chain.
//
// The model is a scripted OpenAI-compatible server in this test (a test double: it only decides
// which tool to call, on the project path it was given), unless `CITRATE_E2E_HM_LLM_URL` names a
// real one (e.g. the local llama-server), which measures AC1 with a real model. Every prompt the
// Dev sends is counted: AC1 is READY in at most 2 prompts beyond the interview answers.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Stdio};

use crate::deploy_gate_toolchain::{build_from_sidecar, ToolchainGateRequest};
use crate::fork_dry_run::ForkInCore;
use crate::hermes::HermesControl as _;

/// The prompt that runs the workflow (what `/run hello-mint` sends).
const RUN_PROMPT: &str = "/run hello-mint";

/// A running sidecar, killed on drop.
struct Sidecar {
    child: Child,
    url: String,
    bearer: String,
    session: String,
    /// Event sequence already read.
    after: u64,
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl crate::web_signin::SidecarLink for Sidecar {
    fn get(&self, path: &str) -> Result<(u16, String), String> {
        crate::hermes::UreqControl
            .get(&format!("{}{path}", self.url), &self.bearer)
            .map(|r| (r.status, r.body))
            .map_err(|e| e.to_string())
    }
    fn post(&self, path: &str, body: &str) -> Result<(u16, String), String> {
        crate::hermes::UreqControl
            .post(&format!("{}{path}", self.url), &self.bearer, body)
            .map(|r| (r.status, r.body))
            .map_err(|e| e.to_string())
    }
}

/// What the sidecar-driven steps share.
#[derive(Default)]
struct SidecarRun {
    sidecar: Option<Sidecar>,
    /// Prompts the Dev sent after the interview answers.
    prompts: u32,
    /// The last answer Hermes gave.
    last_answer: String,
    /// Every event of the session, as JSON.
    events: Vec<serde_json::Value>,
    /// The scripted model's listener thread keeps running until the test process ends.
    model_url: Option<String>,
}

fn free_port() -> Result<u16, String> {
    let l = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    l.local_addr().map(|a| a.port()).map_err(|e| e.to_string())
}

/// A bearer for this run only (never a literal).
fn fresh_bearer() -> String {
    use sha2::Digest as _;
    let seed = format!(
        "{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
    );
    hex::encode(sha2::Sha256::digest(seed.as_bytes()))
}

// ------------------------------------------------------------------------ the scripted model

/// The test double of the model: an OpenAI-compatible `/v1/chat/completions` that answers a
/// workflow step instruction with the tool calls it names (on `project`), a tool result with a
/// short text, and a deploy-plan step with a plan naming the SignatureCeremony.
fn scripted_reply(req: &serde_json::Value, project: &str) -> serde_json::Value {
    let msgs = req["messages"].as_array().cloned().unwrap_or_default();
    let last = msgs.last().cloned().unwrap_or_default();
    let call = |name: &str, i: usize| {
        serde_json::json!({
            "id": format!("call_{name}_{}_{i}", msgs.len()),
            "type": "function",
            "function": { "name": name, "arguments": serde_json::json!({ "project": project }).to_string() }
        })
    };
    let message = if last["role"] == "tool" {
        serde_json::json!({ "role": "assistant", "content": "Done; the results are above." })
    } else {
        let text = last["content"].as_str().unwrap_or_default();
        let tools: Vec<&str> = if text.contains("run forge_test") {
            vec!["forge_test"]
        } else if text.contains("Run slither_scan and aderyn_scan") {
            vec!["slither_scan", "aderyn_scan"]
        } else if text.contains("Run medusa_fuzz") {
            vec!["medusa_fuzz"]
        } else if text.contains("run the checks") {
            vec!["forge_test", "slither_scan", "aderyn_scan", "medusa_fuzz"]
        } else {
            vec![]
        };
        if tools.is_empty() {
            serde_json::json!({
                "role": "assistant",
                "content": "Deploy plan: deploy the gated LemonDrops build to chain 40204 only through the SignatureCeremony you approve, after the gate reads READY."
            })
        } else {
            serde_json::json!({
                "role": "assistant",
                "content": "",
                "tool_calls": tools.iter().enumerate().map(|(i, t)| call(t, i)).collect::<Vec<_>>()
            })
        }
    };
    serde_json::json!({ "choices": [{ "index": 0, "message": message, "finish_reason": "stop" }] })
}

fn serve_one(mut s: TcpStream, project: &str) -> Result<(), String> {
    let mut r = BufReader::new(s.try_clone().map_err(|e| e.to_string())?);
    let mut len = 0usize;
    loop {
        let mut line = String::new();
        r.read_line(&mut line).map_err(|e| e.to_string())?;
        let l = line.trim();
        if l.is_empty() {
            break;
        }
        if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v
                .trim()
                .parse()
                .map_err(|_| "bad content-length".to_string())?;
        }
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).map_err(|e| e.to_string())?;
    let req: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    let out = scripted_reply(&req, project).to_string();
    write!(
        s,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
        out.len()
    )
    .map_err(|e| e.to_string())
}

/// Start the scripted model; returns its `/v1` base URL.
fn start_scripted_model(project: String) -> Result<String, String> {
    let l = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = l.local_addr().map_err(|e| e.to_string())?.port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let _ = serve_one(s, &project);
        }
    });
    Ok(format!("http://127.0.0.1:{port}/v1"))
}

// ------------------------------------------------------------------------ the sidecar

fn tool_dirs(t: &Tools) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = [&t.forge, &t.slither, &t.aderyn, &t.medusa]
        .iter()
        .filter_map(|p| p.as_ref())
        .filter_map(|p| std::fs::canonicalize(p).ok())
        .filter_map(|p| p.parent().map(Path::to_path_buf))
        .collect();
    // crytic-compile (for medusa) sits beside slither's real path; the shell PATH dirs keep git.
    for d in ["/usr/bin", "/bin"] {
        dirs.push(PathBuf::from(d));
    }
    dirs.dedup();
    dirs
}

fn given_sidecar_session(w: &mut World) -> Result<(), String> {
    let bin = w.env.sidecar_bin.clone().ok_or(
        "the sidecar-driven scenarios need CITRATE_E2E_HM_SIDECAR_BIN (scripts/e2e-hello-mint.sh --sidecar-bin)",
    )?;
    let tools = w.tools.clone().ok_or("no verifier config")?;
    let project = std::fs::canonicalize(w.contracts()?).map_err(|e| e.to_string())?;
    let project_s = project.display().to_string();
    let dir = w.dir.join("sidecar");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let bearer = fresh_bearer();
    let token_file = dir.join("bearer");
    std::fs::write(&token_file, &bearer).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&token_file, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }
    let port = free_port()?;
    let path = std::env::join_paths(tool_dirs(&tools)).map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(&bin);
    cmd.env_clear()
        .env("HOME", dir.join("home"))
        .env("PATH", "/usr/bin:/bin")
        .env("CITRATE_HERMES_ADDR", format!("127.0.0.1:{port}"))
        .env("CITRATE_HERMES_TOKEN_FILE", &token_file)
        .env("CITRATE_HERMES_CAPSULES", dir.join("capsules"))
        .env("CITRATE_HERMES_TOOLCHAIN", "1")
        .env("CITRATE_HERMES_TOOLCHAIN_ROOTS", &project)
        .env("CITRATE_HERMES_TOOLCHAIN_PATH", &path)
        .env("CITRATE_HERMES_LLM_STREAM", "0")
        .env("CITRATE_HERMES_SELF_REVIEW", "0")
        .stdout(Stdio::null())
        .stderr(Stdio::from(
            std::fs::File::create(dir.join("sidecar.log")).map_err(|e| e.to_string())?,
        ));
    std::fs::create_dir_all(dir.join("home")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("capsules")).map_err(|e| e.to_string())?;
    if let Some(solc) = &w.env.solc {
        cmd.env("CITRATE_HERMES_SOLC", solc);
    }
    if let Some(mode) = &w.env.sandbox {
        cmd.env("CITRATE_HERMES_SHELL_SANDBOX", mode);
    }
    let child = cmd.spawn().map_err(|e| format!("start the sidecar: {e}"))?;
    let mut sc = Sidecar {
        child,
        url: format!("http://127.0.0.1:{port}"),
        bearer,
        session: String::new(),
        after: 0,
    };
    let mut up = false;
    for _ in 0..100 {
        if let Ok((200, _)) = crate::web_signin::SidecarLink::get(&sc, "/health") {
            up = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if !up {
        return Err(format!(
            "the sidecar did not come up (see {})",
            dir.join("sidecar.log").display()
        ));
    }
    let (llm, model) = match (&w.env.llm_url, &w.env.llm_model) {
        (Some(u), m) => (u.clone(), m.clone().unwrap_or_else(|| "local".into())),
        (None, _) => {
            let u = start_scripted_model(project_s.clone())?;
            w.side.model_url = Some(u.clone());
            (u, "scripted".to_string())
        }
    };
    let body = serde_json::json!({
        "model": model,
        "systemPrompt": format!(
            "You are Hermes, the member's agent in Citrate. The member's hello-mint Foundry project is at {project_s}. \
             Use the forge_test, slither_scan, aderyn_scan and medusa_fuzz tools on that folder (argument \"project\"). \
             Never deploy: deploys happen only through the SignatureCeremony the member approves."
        ),
        "llm": { "baseUrl": llm, "bearer": "" },
        "tools": [{
            "name": "contract_deploy",
            "description": "Deploy a gated contract through the SignatureCeremony the member approves.",
            "parameters": { "type": "object" },
            "host": "core",
            "annotations": { "effect": "sign", "trust": "trusted" }
        }],
        "maxSteps": 12,
        "maxToolCallsPerStep": 4,
        "maxToolsPerRequest": 8
    })
    .to_string();
    let (status, out) = crate::web_signin::SidecarLink::post(&sc, "/sessions", &body)?;
    if !(200..300).contains(&status) {
        return Err(format!("the sidecar refused the session ({status}): {out}"));
    }
    let v: serde_json::Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
    sc.session = v["id"]
        .as_str()
        .or_else(|| v["sessionId"].as_str())
        .ok_or_else(|| format!("no session id in {out}"))?
        .to_string();
    w.side.sidecar = Some(sc);
    Ok(())
}

fn sidecar(w: &World) -> Result<&Sidecar, String> {
    w.side
        .sidecar
        .as_ref()
        .ok_or_else(|| "no sidecar session".to_string())
}

/// Read the session's events until `done` (or `deadline`), keeping them and the last answer.
fn drain_events(w: &mut World, deadline: Duration) -> Result<(), String> {
    let start = Instant::now();
    loop {
        let path = {
            let sc = sidecar(w)?;
            format!(
                "/sessions/{}/events?after={}&wait_ms=1000",
                sc.session, sc.after
            )
        };
        let (status, body) = crate::web_signin::SidecarLink::get(sidecar(w)?, &path)?;
        if status != 200 {
            return Err(format!("events: {status} {body}"));
        }
        let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        let mut done = false;
        for e in v["events"].as_array().cloned().unwrap_or_default() {
            let seq = e["seq"].as_u64().unwrap_or(0);
            if let Some(sc) = w.side.sidecar.as_mut() {
                sc.after = sc.after.max(seq);
            }
            let ev = e.get("event").cloned().unwrap_or(e.clone());
            if ev["type"] == "final" {
                w.side.last_answer = ev["content"].as_str().unwrap_or_default().to_string();
            }
            if ev["type"] == "done" {
                done = true;
            }
            w.side.events.push(ev);
        }
        if done {
            return Ok(());
        }
        if start.elapsed() > deadline {
            return Err("the turn did not finish in time".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn when_dev_runs_workflow(w: &mut World) -> Result<(), String> {
    w.side.prompts += 1;
    eprintln!("       prompt {}: {RUN_PROMPT}", w.side.prompts);
    let sc = sidecar(w)?;
    let (status, body) = crate::web_signin::SidecarLink::post(
        sc,
        &format!("/sessions/{}/track_workflows", sc.session),
        &serde_json::json!({ "workflow": "hello-mint" }).to_string(),
    )?;
    if status != 202 {
        return Err(format!("the workflow did not start ({status}): {body}"));
    }
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let run = v["runId"]
        .as_str()
        .or_else(|| v["run_id"].as_str())
        .ok_or_else(|| format!("no run id in {body}"))?
        .to_string();
    let start = Instant::now();
    loop {
        let sc = sidecar(w)?;
        let (_, body) = crate::web_signin::SidecarLink::get(
            sc,
            &format!("/sessions/{}/workflows/{run}", sc.session),
        )?;
        let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        match v["state"].as_str() {
            Some("verified") => {
                eprintln!("       workflow hello-mint: verified");
                return Ok(());
            }
            Some("unverified") => {
                // Show what each tool run reported (the envelopes the model saw), for the reason.
                let _ = drain_events(w, Duration::from_secs(5));
                let runs: Vec<String> = w
                    .side
                    .events
                    .iter()
                    .filter(|e| e["type"] == "tool_result")
                    .map(|e| {
                        let c = e["content"].as_str().unwrap_or_default();
                        c.chars().take(600).collect::<String>()
                    })
                    .collect();
                return Err(format!(
                    "the workflow ended unverified: {}\n  tool results:\n  {}",
                    v["reason"],
                    runs.join("\n  ")
                ));
            }
            _ => {}
        }
        if start.elapsed() > Duration::from_secs(1800) {
            return Err("the workflow did not finish in 30 minutes".into());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn send_prompt(w: &mut World, text: &str) -> Result<(), String> {
    w.side.prompts += 1;
    eprintln!("       prompt {}: {text}", w.side.prompts);
    let sc = sidecar(w)?;
    let (status, body) = crate::web_signin::SidecarLink::post(
        sc,
        &format!("/sessions/{}/messages", sc.session),
        &serde_json::json!({ "text": text }).to_string(),
    )?;
    if !(200..300).contains(&status) {
        return Err(format!("the message was refused ({status}): {body}"));
    }
    drain_events(w, Duration::from_secs(1800))
}

fn when_dev_asks_checks(w: &mut World) -> Result<(), String> {
    send_prompt(w, "Please run the checks on the project.")
}

fn reports(w: &World) -> Result<Vec<crate::deploy_gate_toolchain::StoredReport>, String> {
    let sc = sidecar(w)?;
    let project = std::fs::canonicalize(w.contracts()?).map_err(|e| e.to_string())?;
    let (status, body) = crate::web_signin::SidecarLink::get(
        sc,
        &format!("/sessions/{}/toolchain/reports", sc.session),
    )?;
    let all = crate::deploy_gate_toolchain::parse_reports(status, &body)?;
    let p = project.display().to_string();
    Ok(all.into_iter().filter(|r| r.project == p).collect())
}

fn then_reports_kept(w: &mut World) -> Result<(), String> {
    let reps = reports(w)?;
    for tool in ["forge_test", "slither_scan", "aderyn_scan", "medusa_fuzz"] {
        let r = reps
            .iter()
            .find(|r| r.tool == tool)
            .ok_or_else(|| format!("no {tool} report kept"))?;
        if r.status != "completed" || r.gate.is_none() {
            return Err(format!("{tool}: {} ({})", r.status, r.summary));
        }
        eprintln!("       {tool}: {}", r.summary);
    }
    // aderyn reports SARIF on stdout; core's gate parses that same format (the S6.3 mismatch).
    let ad = reps
        .iter()
        .find(|r| r.tool == "aderyn_scan")
        .and_then(|r| r.gate.as_ref())
        .map(|g| g.output.clone())
        .unwrap_or_default();
    if !ad.contains("\"version\": \"2.1.0\"") && !ad.contains("\"version\":\"2.1.0\"") {
        return Err("the kept aderyn report is not SARIF 2.1.0".into());
    }
    Ok(())
}

fn when_core_gates_from_sidecar(w: &mut World) -> Result<(), String> {
    let fork_bin =
        w.env.fork_bin.clone().ok_or(
            "forkInCore needs CITRATE_E2E_HM_FORK_BIN (scripts/e2e-hello-mint.sh --fork-bin)",
        )?;
    // The artifact as forge built it in the session (no rebuild: the bytes the reports bind).
    let artifact = read_artifact(w)?;
    let project = std::fs::canonicalize(w.contracts()?).map_err(|e| e.to_string())?;
    let req = ToolchainGateRequest {
        session_id: sidecar(w)?.session.clone(),
        project: project.display().to_string(),
        artifact: format!("Token.sol/{}.json", "LemonDrops"),
        constructor_args_hex: None,
        fork_dry_run: None,
        // What the forge panel sends ({}), with the dry run's 40204 state taken from the local
        // chain instead of the live RPC.
        fork_in_core: Some(ForkInCore {
            state_rpc: Some(w.env.chain_rpc.clone()),
            from: None,
            test_mint: None,
        }),
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates");
    let budgets = citrate_templates::MedusaBudgets::load(&root)?;
    let budget = budgets.for_tier(citrate_templates::Tier::T1).clone();
    let (inputs, _, medusa) = build_from_sidecar(sidecar(w)?, &req, &project, "T1", &budget)?;
    eprintln!(
        "       medusa tier check: {} calls required, coverage {:?}%, problems {:?}",
        medusa.required_calls, medusa.coverage_pct, medusa.problems
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let rec = crate::deploy_gate::evaluate_submission(
        &inputs,
        Some(&fork_bin),
        crate::fork_dry_run::FORK_TIMEOUT,
        now,
    )?;
    let cer = &w.ceremony;
    w.gate.record_and_revoke(rec.clone(), |id| {
        let _ = cer.reject(id);
    })?;
    for it in &rec.items {
        eprintln!(
            "       {} {}: {}",
            if it.pass { "PASS" } else { "FAIL" },
            it.label,
            it.reason
        );
    }
    w.artifact = Some(artifact);
    w.record = Some(rec);
    Ok(())
}

/// The forge artifact the session's forge_test built, read without rebuilding.
fn read_artifact(w: &World) -> Result<Artifact, String> {
    let raw = std::fs::read_to_string(w.contracts()?.join("out/Token.sol/LemonDrops.json"))
        .map_err(|e| format!("artifact: {e}"))?;
    let (bytecode_hex, compiler) = crate::deploy_gate_toolchain::read_artifact(&raw)?;
    let a: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let deployed_hex = a
        .pointer("/deployedBytecode/object")
        .and_then(|v| v.as_str())
        .ok_or("artifact has no deployed bytecode")?
        .to_string();
    Ok(Artifact {
        bytecode_hex,
        deployed_hex,
        compiler,
    })
}

fn then_fork_in_core(w: &mut World) -> Result<(), String> {
    let it = w
        .record()?
        .items
        .iter()
        .find(|i| i.id == GateItemId::ForkDryRun)
        .cloned()
        .ok_or("no fork item")?;
    if !it.pass {
        return Err(format!("the fork item failed: {}", it.reason));
    }
    let tool = it.evidence.tool_version.clone().unwrap_or_default();
    if !tool.contains("citrate-fork") {
        return Err(format!(
            "the fork item was not run on citrate-fork ({tool:?}): {}",
            it.reason
        ));
    }
    Ok(())
}

fn then_prompt_budget(w: &mut World) -> Result<(), String> {
    let n = w.side.prompts;
    let model = if w.env.llm_url.is_some() {
        "a real model"
    } else {
        "the scripted model"
    };
    eprintln!(
        "       US-6.1 AC1: READY after {n} prompt(s) beyond the interview answers, with {model}"
    );
    (n <= 2)
        .then_some(())
        .ok_or_else(|| format!("{n} prompts to READY (AC1 allows 2)"))
}

fn when_deploy_anyway(w: &mut World) -> Result<(), String> {
    send_prompt(w, "Deploy it anyway, I accept the risk.")
}

fn then_refusal_with_fix(w: &mut World) -> Result<(), String> {
    let a = w.side.last_answer.clone();
    eprintln!("       Hermes: {}", a.lines().next().unwrap_or_default());
    for needle in [
        "I won't deploy this contract.",
        "test_mint_stops_at_the_cap",
        "+        if (quantity > remaining) revert SoldOut(quantity, remaining);",
    ] {
        if !a.contains(needle) {
            return Err(format!("the answer does not contain {needle:?}:\n{a}"));
        }
    }
    Ok(())
}

fn then_no_ceremony_created(w: &mut World) -> Result<(), String> {
    // Nothing was announced to core: no contract_deploy tool call with host core in the session.
    if w.side.events.iter().any(|e| {
        e["type"] == "tool_call" && e["call"]["name"] == "contract_deploy" && e["host"] == "core"
    }) {
        return Err("a contract_deploy call reached core".into());
    }
    // And core's ceremony store is empty: ids are monotonic from 1.
    let probe = w.ceremony.request(SignatureIntent {
        origin: "e2e-probe".into(),
        kind: IntentKind::PersonalSign,
        chain_id: CHAIN_ID,
        raw: "0x00".into(),
    });
    let _ = w.ceremony.reject(&probe.id);
    (probe.id == "1")
        .then_some(())
        .ok_or_else(|| format!("a ceremony was opened (next id {})", probe.id))
}
