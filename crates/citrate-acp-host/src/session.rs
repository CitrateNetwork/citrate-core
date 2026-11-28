//! The session manager (L1-WP03 step 3): one agent process per session, driven over ACP from a
//! dedicated runtime (threads named `acp-host`). The caller's side is synchronous (Tauri commands
//! run it off the main thread): `start`, `prompt`, `cancel`, `events`, `close`.

use crate::events::{AgentEvent, EventLog};
use crate::permission::{outcome, PermissionAsk, PermissionPolicy, Verdict};
use crate::process::{self, ScannedWriter, SecretScan, StderrRing, TappedReader, WireTap};
use crate::registry::{verify_pin, AgentSpec};
use agent_client_protocol::v1::schema::{
    AuthenticateRequest, CancelNotification, InitializeRequest, NewSessionRequest, PromptRequest,
    RequestPermissionRequest, RequestPermissionResponse, SessionNotification,
};
use agent_client_protocol::{is_incoming_transport_closed, ByteStreams, Client, Error};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

pub struct HostConfig {
    pub client_name: String,
    pub client_version: String,
    /// Core's credential detector; every outbound frame, prompt, argument and env value is checked.
    pub scan: SecretScan,
    pub policy: Arc<dyn PermissionPolicy>,
    pub tap: Option<WireTap>,
    /// How long `initialize` plus `session/new` may take.
    pub init_timeout: Duration,
    /// How long a cancelled turn may take to end before the agent's process group is killed.
    pub cancel_grace: Duration,
}

pub struct SessionStart {
    pub spec: AgentSpec,
    /// The agent's whole environment (the inherited one is cleared).
    pub env: Vec<(String, String)>,
    /// The session worktree; sent as `session/new` `cwd`.
    pub cwd: PathBuf,
    /// The MCP servers for `session/new` (Core passes only its gateway entry, L1-WP16a).
    pub mcp_servers: Vec<Value>,
}

#[derive(Debug)]
pub struct StartFailure {
    pub reason: String,
    /// The agent's newest stderr bytes, for the caller to show or drop; the host never logs it.
    pub stderr_tail: String,
}

enum Cmd {
    Prompt(Value, oneshot::Sender<Result<String, String>>),
    Authenticate(String, oneshot::Sender<Result<(), String>>),
    Cancel,
}

/// One turn at a time: `active` from `prompt` until the first of the agent's answer, its exit or
/// the cancel kill ends it ([`end_turn`] makes that first ending the only one reported).
#[derive(Default)]
struct TurnState {
    active: AtomicBool,
    cancelling: AtomicBool,
}

fn end_turn(events: &EventLog, t: &TurnState, r: &Result<String, String>, killed: bool) {
    if t.active.swap(false, SeqCst) {
        match r {
            Ok(reason) if killed => {
                events.push("stopped", json!({"stopReason": reason, "killed": true}))
            }
            Ok(reason) => events.push("stopped", json!({"stopReason": reason})),
            Err(m) => events.push("error", json!({"method": "session/prompt", "message": m})),
        };
    }
}

const EXITED_MID_TURN: &str = "the agent exited mid-turn; the turn was not answered";

pub struct AcpHost {
    rt: tokio::runtime::Runtime,
    cfg: Arc<HostConfig>,
}

pub struct Session {
    id: String,
    acp_session: String,
    initialize: Value,
    pid: u32,
    events: Arc<EventLog>,
    cmd: mpsc::UnboundedSender<Cmd>,
    turn: Arc<TurnState>,
    exited: Arc<AtomicBool>,
    rt: tokio::runtime::Handle,
    kill: Mutex<Option<oneshot::Sender<()>>>,
    stderr: Arc<Mutex<StderrRing>>,
    scan: SecretScan,
    cancel_grace: Duration,
}

fn cast<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, Error> {
    serde_json::from_value(v).map_err(Error::into_internal_error)
}

fn to_json<T: serde::Serialize>(v: &T) -> Result<Value, Error> {
    serde_json::to_value(v).map_err(Error::into_internal_error)
}

impl AcpHost {
    pub fn new(cfg: HostConfig) -> std::io::Result<AcpHost> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("acp-host")
            .enable_all()
            .build()?;
        Ok(AcpHost {
            rt,
            cfg: Arc::new(cfg),
        })
    }

    /// Verify the pin, spawn the agent, then `initialize` and `session/new`. Must not be called
    /// from inside an async runtime (it blocks until the session is ready or refused).
    pub fn start(&self, s: SessionStart) -> Result<Session, StartFailure> {
        let fail = |reason: String| StartFailure {
            reason,
            stderr_tail: String::new(),
        };
        verify_pin(&s.spec).map_err(fail)?;
        process::check_launch(&s.env, &s.spec.args, &self.cfg.scan).map_err(fail)?;
        let _rt = self.rt.enter();
        let mut child = process::spawn(&s.spec.command, &s.spec.args, &s.env, &s.cwd)
            .map_err(|e| fail(format!("{}: spawn failed: {e}", s.spec.id)))?;
        let pid = child.id().unwrap_or(0);
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(fail(format!("{}: no stdio pipes", s.spec.id)));
        };
        let ring = Arc::new(Mutex::new(StderrRing::default()));
        self.rt.spawn(process::pump_stderr(stderr, ring.clone()));
        let events = Arc::new(EventLog::default());
        let exited = Arc::new(AtomicBool::new(false));
        let (kill_tx, kill_rx) = oneshot::channel::<()>();
        let (ev, ex) = (events.clone(), exited.clone());
        self.rt.spawn(async move {
            let status = tokio::select! {
                st = child.wait() => st,
                _ = kill_rx => { let _ = child.start_kill(); child.wait().await }
            };
            ex.store(true, SeqCst);
            ev.push("exited", exit_body(status));
        });
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = oneshot::channel();
        let transport = ByteStreams::new(
            ScannedWriter::new(
                stdin.compat_write(),
                self.cfg.scan.clone(),
                self.cfg.tap.clone(),
            ),
            TappedReader::new(stdout.compat(), self.cfg.tap.clone()),
        );
        let turn = Arc::new(TurnState::default());
        let mcp = Value::Array(s.mcp_servers);
        let drive = drive(
            self.cfg.clone(),
            transport,
            s.cwd,
            mcp,
            events.clone(),
            turn.clone(),
            cmd_rx,
            ready_tx,
        );
        self.rt.spawn(drive);
        let mut session = Session {
            id: s.spec.id,
            acp_session: String::new(),
            initialize: Value::Null,
            pid,
            events,
            cmd: cmd_tx,
            turn,
            exited,
            rt: self.rt.handle().clone(),
            kill: Mutex::new(Some(kill_tx)),
            stderr: ring,
            scan: self.cfg.scan.clone(),
            cancel_grace: self.cfg.cancel_grace,
        };
        let ready = self
            .rt
            .block_on(tokio::time::timeout(self.cfg.init_timeout, ready_rx));
        let reason = match ready {
            Ok(Ok(Ok((init, sid)))) => {
                session.initialize = init;
                session.acp_session = sid;
                return Ok(session);
            }
            Ok(Ok(Err(e))) => e,
            Ok(Err(_)) => format!(
                "{}: the agent closed the connection before session/new",
                session.id
            ),
            Err(_) => format!(
                "{}: no session within {:?}",
                session.id, self.cfg.init_timeout
            ),
        };
        std::thread::sleep(Duration::from_millis(100));
        Err(StartFailure {
            reason,
            stderr_tail: session.stderr_tail(),
        })
    }
}

fn exit_body(status: std::io::Result<std::process::ExitStatus>) -> Value {
    match status {
        #[cfg(unix)]
        Ok(st) => {
            json!({"code": st.code(), "signal": std::os::unix::process::ExitStatusExt::signal(&st)})
        }
        #[cfg(not(unix))]
        Ok(st) => json!({"code": st.code()}),
        Err(e) => json!({"error": e.to_string()}),
    }
}

#[allow(clippy::too_many_arguments)]
async fn drive<O, I>(
    cfg: Arc<HostConfig>,
    transport: ByteStreams<O, I>,
    cwd: PathBuf,
    mcp: Value,
    events: Arc<EventLog>,
    turn: Arc<TurnState>,
    mut cmds: mpsc::UnboundedReceiver<Cmd>,
    ready: oneshot::Sender<Result<(Value, String), String>>,
) where
    O: futures::io::AsyncWrite + Send + 'static,
    I: futures::io::AsyncRead + Send + 'static,
{
    let (ev_update, ev_perm, ev_end, policy) = (
        events.clone(),
        events.clone(),
        events.clone(),
        cfg.policy.clone(),
    );
    let result = Client
        .v1()
        .name(cfg.client_name.clone())
        .on_receive_notification(
            async move |n: SessionNotification, _cx| {
                ev_update.push_update(to_json(&n.update)?);
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |req: RequestPermissionRequest, responder, _cx| {
                let ask = PermissionAsk::from_request(&to_json(&req)?);
                let verdict = policy.decide(&ask);
                let out = outcome(&verdict, &ask.options);
                let (word, reason) = match &verdict {
                    Verdict::Allow(r) => ("allow", r),
                    Verdict::Deny(r) => ("deny", r),
                };
                ev_perm.push(
                    "permission",
                    json!({"toolCallId": ask.tool_call_id, "toolKind": ask.tool_kind, "title": ask.title,
                           "verdict": word, "reason": reason, "outcome": out}),
                );
                responder.respond(cast::<RequestPermissionResponse>(json!({"outcome": out}))?)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(transport, async move |cx| {
            let init_req = cast::<InitializeRequest>(json!({
                "protocolVersion": crate::pin::protocol_version(),
                "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
                "clientInfo": {"name": cfg.client_name, "version": cfg.client_version}
            }))?;
            let setup = async {
                let init = cx.send_request(init_req).block_task().await?;
                let new = cx
                    .send_request(cast::<NewSessionRequest>(json!({"cwd": cwd, "mcpServers": mcp}))?)
                    .block_task()
                    .await?;
                Ok::<_, Error>((to_json(&init)?, new.session_id.to_string()))
            };
            let (init, sid) = match setup.await {
                Ok(v) => v,
                Err(e) => {
                    let _ = ready.send(Err(format!("initialize or session/new failed: {}", e.message)));
                    return Err(e);
                }
            };
            let _ = ready.send(Ok((init, sid.clone())));
            while let Some(cmd) = cmds.recv().await {
                match cmd {
                    Cmd::Prompt(blocks, reply) => {
                        let sent = cx.send_request(cast::<PromptRequest>(json!({"sessionId": sid, "prompt": blocks}))?);
                        let (events, turn) = (events.clone(), turn.clone());
                        cx.spawn(async move {
                            let (answer, killed) = match sent.block_task().await {
                                Ok(resp) => (Ok(to_json(&resp.stop_reason)?.as_str().unwrap_or("").to_string()), false),
                                Err(_) if turn.cancelling.load(SeqCst) => (Ok("cancelled".to_string()), true),
                                Err(e) if is_incoming_transport_closed(&e) => (Err(EXITED_MID_TURN.to_string()), false),
                                Err(e) => (Err(format!("session/prompt failed: {}", e.message)), false),
                            };
                            end_turn(&events, &turn, &answer, killed);
                            let _ = reply.send(answer);
                            Ok(())
                        })?;
                    }
                    Cmd::Authenticate(method, reply) => {
                        let sent = cx.send_request(cast::<AuthenticateRequest>(json!({"methodId": method}))?);
                        cx.spawn(async move {
                            let r = sent.block_task().await.map(|_| ()).map_err(|e| e.message);
                            let _ = reply.send(r);
                            Ok(())
                        })?;
                    }
                    Cmd::Cancel => {
                        turn.cancelling.store(true, SeqCst);
                        cx.send_notification(cast::<CancelNotification>(json!({"sessionId": sid}))?)?;
                    }
                }
            }
            Ok(())
        })
        .await;
    if let Err(e) = result {
        ev_end.push(
            "error",
            json!({"method": "connection", "message": e.message}),
        );
    }
}

impl Session {
    pub fn runtime_id(&self) -> &str {
        &self.id
    }
    pub fn acp_session_id(&self) -> &str {
        &self.acp_session
    }
    /// The agent's `initialize` response (capabilities, auth methods, agent info).
    pub fn initialize_response(&self) -> &Value {
        &self.initialize
    }
    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn events(&self, after: u64, wait: Duration) -> (Vec<AgentEvent>, u64) {
        self.events.after(after, wait)
    }
    pub fn stderr_tail(&self) -> String {
        self.stderr.lock().map(|r| r.text()).unwrap_or_default()
    }

    /// Run one turn; blocks until its stop reason. One turn at a time. If the agent exits without
    /// answering (a grandchild may still hold its stdout open), the turn ends a second later.
    pub fn prompt(&self, blocks: Vec<Value>) -> Result<String, String> {
        let blocks = Value::Array(blocks);
        if (self.scan)(&blocks.to_string()) {
            return Err("refused: the prompt carries a credential shape; it was not sent".into());
        }
        if self
            .turn
            .active
            .compare_exchange(false, true, SeqCst, SeqCst)
            .is_err()
        {
            return Err("a turn is already running".into());
        }
        self.turn.cancelling.store(false, SeqCst);
        let (tx, mut rx) = oneshot::channel();
        if self.cmd.send(Cmd::Prompt(blocks, tx)).is_err() {
            self.turn.active.store(false, SeqCst);
            return Err("the agent connection is closed".into());
        }
        let (exited, turn, events) = (self.exited.clone(), self.turn.clone(), self.events.clone());
        self.rt.block_on(async move {
            let mut exited_at: Option<Instant> = None;
            loop {
                tokio::select! {
                    r = &mut rx => return r.unwrap_or_else(|_| Err(EXITED_MID_TURN.into())),
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {
                        if !exited.load(SeqCst) {
                            continue;
                        }
                        if exited_at.get_or_insert_with(Instant::now).elapsed() < Duration::from_secs(1) {
                            continue;
                        }
                        let killed = turn.cancelling.load(SeqCst);
                        let r = if killed { Ok("cancelled".to_string()) } else { Err(EXITED_MID_TURN.to_string()) };
                        end_turn(&events, &turn, &r, killed);
                        return r;
                    }
                }
            }
        })
    }

    /// `authenticate` with one of the agent's own method ids. Core never sends a key (WP14).
    pub fn authenticate(&self, method_id: &str) -> Result<(), String> {
        if (self.scan)(method_id) {
            return Err("refused: authenticate never carries a credential".into());
        }
        let (tx, rx) = oneshot::channel();
        self.cmd
            .send(Cmd::Authenticate(method_id.to_string(), tx))
            .map_err(|_| "the agent connection is closed".to_string())?;
        rx.blocking_recv()
            .unwrap_or_else(|_| Err("the agent connection closed".into()))
    }

    /// `session/cancel`; if the turn has not ended within the grace, the agent's process group is
    /// killed and the turn ends `cancelled`.
    pub fn cancel(&self) -> Result<(), String> {
        self.turn.cancelling.store(true, SeqCst);
        self.cmd
            .send(Cmd::Cancel)
            .map_err(|_| "the agent connection is closed".to_string())?;
        let end = Instant::now() + self.cancel_grace;
        let mut seen = self.events.after(u64::MAX, Duration::ZERO).1;
        while self.turn.active.load(SeqCst) {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                self.kill_now();
                break;
            }
            seen = self
                .events
                .after(seen, left.min(Duration::from_millis(50)))
                .1;
        }
        Ok(())
    }

    /// Stop the agent: SIGTERM to its group, up to 2 s to exit, then SIGKILL.
    pub fn close(self) {
        process::signal_group(self.pid, false);
        let end = Instant::now() + Duration::from_secs(2);
        while !self.exited.load(SeqCst) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        // Drop kills whatever is left of the group.
    }

    fn kill_now(&self) {
        process::signal_group(self.pid, true);
        if let Some(k) = self.kill.lock().ok().and_then(|mut k| k.take()) {
            let _ = k.send(());
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.kill_now();
    }
}
