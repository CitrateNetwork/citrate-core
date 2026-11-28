//! A real ACP agent for the host tests (built from the SDK's own `Agent` role, so the host talks to a
//! true protocol peer over a real child process's stdio). Test-only: it is a `[[bin]]` under
//! `tests/` that no bundle ships. Each prompt's first text block picks what it does:
//!
//! - `updates N`: N `agent_message_chunk` updates numbered 0..N, then `end_turn`.
//! - `permission <kind>`: asks `session/request_permission` for a `<kind>` tool, then answers with
//!   the outcome it got (one chunk) and `end_turn`.
//! - `wait`: waits for `session/cancel`, then answers `cancelled`.
//! - `stubborn <file>`: starts a `sleep 300` grandchild (in this agent's process group), writes the
//!   grandchild's pid to `<file>`, and never answers (ignores `session/cancel`).
//! - `escape <file>`: the same, but the grandchild leaves for its own process group and keeps this
//!   agent's stdout open, so the host never sees EOF.
//! - `exit`: exits the process mid-turn.
//! - `stderr N`: writes N bytes to stderr, then `end_turn`.
//! - `report`: one chunk holding what it saw: the initialize request's client capabilities, the
//!   `session/new` cwd and mcpServers, its argv, and its environment (names and values).

use agent_client_protocol::v1::schema::{
    CancelNotification, InitializeRequest, InitializeResponse, NewSessionRequest,
    NewSessionResponse, PromptRequest, PromptResponse, RequestPermissionRequest,
    RequestPermissionResponse, SessionNotification,
};
use agent_client_protocol::{Agent, ByteStreams, Error};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

#[derive(Default)]
struct Seen {
    init: Value,
    new_session: Value,
}

fn cast<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, Error> {
    serde_json::from_value(v).map_err(Error::into_internal_error)
}

fn chunk(sid: &str, text: &str) -> Result<SessionNotification, Error> {
    cast(
        json!({"sessionId": sid, "update": {"sessionUpdate": "agent_message_chunk",
        "content": {"type": "text", "text": text}}}),
    )
}

fn stop(reason: &str) -> Result<PromptResponse, Error> {
    cast(json!({"stopReason": reason}))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Error> {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let cancel = Arc::new(Notify::new());
    let (s1, s2, s3, c1, c2) = (
        seen.clone(),
        seen.clone(),
        seen.clone(),
        cancel.clone(),
        cancel.clone(),
    );
    Agent
        .v1()
        .name("citrate-acp-test-agent")
        .on_receive_request(
            async move |req: InitializeRequest, responder, _cx| {
                let v = serde_json::to_value(&req).map_err(Error::into_internal_error)?;
                s1.lock().expect("seen").init = v;
                responder.respond(InitializeResponse::new(req.protocol_version))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: NewSessionRequest, responder, _cx| {
                let v = serde_json::to_value(&req).map_err(Error::into_internal_error)?;
                s2.lock().expect("seen").new_session = v;
                responder.respond(cast::<NewSessionResponse>(json!({"sessionId": "test-s1"}))?)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            async move |_n: CancelNotification, _cx| {
                c1.notify_waiters();
                c1.notify_one();
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |req: PromptRequest, responder, cx| {
                let sid = req.session_id.to_string();
                let first = serde_json::to_value(&req.prompt)
                    .map_err(Error::into_internal_error)?
                    .pointer("/0/text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let (seen, cancel) = (s3.clone(), c2.clone());
                let cx2 = cx.clone();
                cx.spawn(async move {
                    let mut words = first.split_whitespace();
                    let verb = words.next().unwrap_or("");
                    let arg = words.next().unwrap_or("").to_string();
                    match verb {
                        "updates" => {
                            let n: usize = arg.parse().unwrap_or(0);
                            for i in 0..n {
                                cx2.send_notification(chunk(&sid, &i.to_string())?)?;
                            }
                            responder.respond(stop("end_turn")?)
                        }
                        "permission" => {
                            let ask: RequestPermissionRequest = cast(json!({
                                "sessionId": sid,
                                "toolCall": {"toolCallId": "t1", "title": format!("{arg} a file"), "kind": arg},
                                "options": [
                                    {"optionId": "allow-always", "name": "Always", "kind": "allow_always"},
                                    {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                                    {"optionId": "reject", "name": "Reject", "kind": "reject_once"}
                                ]
                            }))?;
                            let got: RequestPermissionResponse =
                                cx2.send_request(ask).block_task().await?;
                            let out = serde_json::to_string(&got.outcome)
                                .map_err(Error::into_internal_error)?;
                            cx2.send_notification(chunk(&sid, &out)?)?;
                            responder.respond(stop("end_turn")?)
                        }
                        "wait" => {
                            cancel.notified().await;
                            responder.respond(stop("cancelled")?)
                        }
                        #[cfg(unix)]
                        "stubborn" | "escape" => {
                            use std::os::unix::process::CommandExt as _;
                            let mut cmd = std::process::Command::new("sleep");
                            cmd.arg("300");
                            if verb == "escape" {
                                cmd.process_group(0);
                            }
                            let child = cmd.spawn().map_err(Error::into_internal_error)?;
                            std::fs::write(&arg, child.id().to_string())
                                .map_err(Error::into_internal_error)?;
                            std::future::pending::<()>().await;
                            responder.respond(stop("end_turn")?)
                        }
                        "exit" => std::process::exit(3),
                        "stderr" => {
                            let n: usize = arg.parse().unwrap_or(0);
                            use std::io::Write as _;
                            let mut e = std::io::stderr();
                            e.write_all(&vec![b'e'; n]).map_err(Error::into_internal_error)?;
                            responder.respond(stop("end_turn")?)
                        }
                        "report" => {
                            let s = seen.lock().expect("seen");
                            let env: serde_json::Map<String, Value> =
                                std::env::vars().map(|(k, v)| (k, Value::String(v))).collect();
                            let report = json!({
                                "clientCapabilities": s.init["clientCapabilities"],
                                "cwd": s.new_session["cwd"],
                                "mcpServers": s.new_session["mcpServers"],
                                "argv": std::env::args().collect::<Vec<_>>(),
                                "env": env,
                            });
                            drop(s);
                            cx2.send_notification(chunk(&sid, &report.to_string())?)?;
                            responder.respond(stop("end_turn")?)
                        }
                        _ => responder.respond(stop("refusal")?),
                    }
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_to(ByteStreams::new(
            tokio::io::stdout().compat_write(),
            tokio::io::stdin().compat(),
        ))
        .await
}
