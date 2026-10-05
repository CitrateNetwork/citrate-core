//! HUP-S4.3 — a READ-ONLY stdio bridge from Hermes's MCP host to the local mem-mcp daemon.
//!
//! The memory daemon (`mem-mcp`, supervised by [`crate::memory`]) speaks newline-delimited MCP
//! JSON-RPC over a local socket. The Hermes sidecar's MCP host (citrate-agent-runtime
//! `agent-mcp-host`, HUP-S4.1) speaks to servers over stdio or HTTP only, so core hands it this
//! bridge as a stdio server: the allowlist file core writes ([`crate::hermes_mcp`]) names the
//! citrate-core executable itself with [`BRIDGE_FLAG`] and the daemon's socket path, and
//! `main.rs` runs [`maybe_run_from_args`] before any app start-up.
//!
//! What the bridge enforces (core is the trust boundary, not the daemon):
//! - Only [`READ_ONLY_MEMORY_TOOLS`] are listed, each annotated `readOnlyHint: true`, and a
//!   `tools/call` for any other tool (the writes: assert, propose/confirm edge, merge diff, or a
//!   tool a newer daemon adds) is answered locally with an error and never reaches the daemon.
//!   Memory writes stay on core's gated `memory_assert` path.
//! - Only `initialize`, `ping`, `tools/list`, `tools/call`, and `notifications/*` pass. Anything
//!   else is answered `-32601` (requests) or dropped (notifications). Batches are refused.
//!
//! - HUP-S3.4 (fan-out 7): given the learned-memory ledger (an optional third argument), every
//!   tool answer leaves out the hits of learned memories with an unresolved contradiction
//!   ([`crate::hermes_learn::RecallHide`]), so Hermes never recalls a claim the member has not
//!   settled. The ledger is read for each answer, so a resolution shows on the next call.
//!
//! Keyless (Rule 3): the bridge holds no key and never touches the keyring; the daemon's own
//! session grant applies. It is a byte relay plus a filter.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// The argv flag that turns the citrate-core executable into the bridge. Distinctive so it can
/// never collide with a Tauri, webview, or OS-supplied argument.
pub const BRIDGE_FLAG: &str = "--citrate-mem-mcp-stdio";

/// The mem-mcp tools Hermes may call through MCP: every tool that only reads the graph.
pub const READ_ONLY_MEMORY_TOOLS: &[&str] = &[
    "memory.recall",
    "memory.search",
    "memory.neighbors",
    "memory.as_of",
    "memory.verify",
    "memory.critique",
    "memory.analogy",
];

/// Request methods relayed to the daemon.
const RELAYED_METHODS: &[&str] = &["initialize", "ping", "tools/list", "tools/call"];

/// How long the bridge waits for the daemon socket and, after input ends, for answers.
pub const DEFAULT_WAIT: Duration = Duration::from_secs(5);

/// Poll interval for the socket reader (lets it notice shutdown).
const READ_POLL: Duration = Duration::from_millis(100);

/// What to do with one inbound (host → daemon) line.
#[derive(Debug, Clone, PartialEq)]
pub enum Gate {
    /// Relay the line to the daemon. `list_id` is the request id when this is `tools/list` (its
    /// answer is filtered); `has_id` is false for notifications (no answer expected).
    Forward {
        list_id: Option<String>,
        has_id: bool,
        /// The line to send: the gate's own serialization of the parsed request.
        line: String,
    },
    /// Answer locally with this JSON-RPC response; never relay.
    Reply(Value),
    /// Ignore the line.
    Drop,
}

fn err_reply(id: Value, code: i64, message: &str) -> Gate {
    Gate::Reply(json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}))
}

/// The stable map key for a JSON-RPC id.
fn id_key(id: &Value) -> String {
    id.to_string()
}

/// Decide what happens to one host → daemon line. Pure.
pub fn gate_request(line: &str) -> Gate {
    if line.trim().is_empty() {
        return Gate::Drop;
    }
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return err_reply(Value::Null, -32700, "parse error"),
    };
    if v.is_array() {
        return err_reply(Value::Null, -32600, "batches are not supported");
    }
    let Some(method) = v.get("method").and_then(Value::as_str) else {
        // A response or junk: the daemon sends no requests, so nothing is waiting for it.
        return Gate::Drop;
    };
    let id = v.get("id").filter(|i| !i.is_null()).cloned();
    let Some(id) = id else {
        return if method.starts_with("notifications/") {
            Gate::Forward {
                list_id: None,
                has_id: false,
                line: v.to_string(),
            }
        } else {
            Gate::Drop
        };
    };
    if !RELAYED_METHODS.contains(&method) {
        return err_reply(id, -32601, "method not offered through the memory bridge");
    }
    if method == "tools/call" {
        let name = v
            .get("params")
            .and_then(|p| p.get("name"))
            .and_then(Value::as_str);
        match name {
            Some(n) if READ_ONLY_MEMORY_TOOLS.contains(&n) => {}
            Some(_) => {
                return err_reply(
                    id,
                    -32602,
                    "this memory tool is not offered to Hermes; memory writes go through the app's approval",
                )
            }
            None => return err_reply(id, -32602, "missing tool name"),
        }
    }
    Gate::Forward {
        list_id: (method == "tools/list").then(|| id_key(&id)),
        has_id: true,
        line: v.to_string(),
    }
}

/// Keep only the read tools in a `tools/list` result and mark each read-only. Pure.
pub fn filter_tools_list(result: &mut Value) {
    let Some(tools) = result.get_mut("tools").and_then(Value::as_array_mut) else {
        return;
    };
    tools.retain(|t| {
        t.get("name")
            .and_then(Value::as_str)
            .is_some_and(|n| READ_ONLY_MEMORY_TOOLS.contains(&n))
    });
    for t in tools.iter_mut() {
        if let Some(obj) = t.as_object_mut() {
            obj.insert(
                "annotations".into(),
                json!({
                    "readOnlyHint": true,
                    "destructiveHint": false,
                    "idempotentHint": true,
                    "openWorldHint": false
                }),
            );
        }
    }
}

/// Rewrite one daemon → host line. A response to a tracked `tools/list` id is filtered; every
/// other line passes unchanged. Returns the line and the response id key, if any.
pub fn rewrite_response(line: &str, list_ids: &mut HashSet<String>) -> (String, Option<String>) {
    let Ok(mut v) = serde_json::from_str::<Value>(line) else {
        return (line.to_string(), None);
    };
    let key = v.get("id").filter(|i| !i.is_null()).map(id_key);
    if let Some(k) = &key {
        if list_ids.remove(k) {
            if let Some(result) = v.get_mut("result") {
                filter_tools_list(result);
            }
            return (v.to_string(), key);
        }
    }
    (line.to_string(), key)
}

/// Leave unresolved learned memories out of a tool answer's text (`result.content[].text`). Any
/// other line passes unchanged. Pure, given the hide set.
pub fn hide_unresolved_in_response(line: &str, hide: &crate::hermes_learn::RecallHide) -> String {
    let Ok(mut v) = serde_json::from_str::<Value>(line) else {
        return line.to_string();
    };
    let Some(content) = v
        .get_mut("result")
        .and_then(|r| r.get_mut("content"))
        .and_then(Value::as_array_mut)
    else {
        return line.to_string();
    };
    let mut changed = false;
    for item in content.iter_mut() {
        if let Some(text) = item.get("text").and_then(Value::as_str) {
            let kept = hide.filter_text(text);
            if kept != text {
                if let Some(obj) = item.as_object_mut() {
                    obj.insert("text".into(), Value::String(kept));
                    changed = true;
                }
            }
        }
    }
    if changed {
        v.to_string()
    } else {
        line.to_string()
    }
}

fn connect_with_retry(socket: &Path, wait: Duration) -> Result<crate::ipc_name::IpcStream, String> {
    let deadline = Instant::now() + wait;
    loop {
        match crate::ipc_name::connect(&socket.to_string_lossy()) {
            Ok(s) => return Ok(s),
            Err(e) if Instant::now() >= deadline => {
                return Err(format!(
                    "the memory daemon is not reachable at {}: {e}",
                    socket.display()
                ))
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

fn write_line(out: &Mutex<Box<dyn Write + Send>>, line: &str) {
    let mut w = out.lock().unwrap_or_else(|e| e.into_inner());
    let _ = writeln!(w, "{line}");
    let _ = w.flush();
}

enum Ended {
    Input,
    Socket,
}

/// Relay `input` (host → daemon) and the daemon's answers (→ `output`) until input ends (then
/// wait up to `wait` for outstanding answers) or the daemon closes the connection.
pub fn run_bridge<R: BufRead + Send + 'static>(
    socket: &Path,
    input: R,
    output: Box<dyn Write + Send>,
    wait: Duration,
) -> Result<(), String> {
    run_bridge_with(socket, input, output, wait, None)
}

/// [`run_bridge`], leaving unresolved learned memories out of every answer when `ledger` is set.
pub fn run_bridge_with<R: BufRead + Send + 'static>(
    socket: &Path,
    input: R,
    output: Box<dyn Write + Send>,
    wait: Duration,
    ledger: Option<std::path::PathBuf>,
) -> Result<(), String> {
    use interprocess::local_socket::traits::Stream as _;
    let stream = connect_with_retry(socket, wait)?;
    stream
        .set_recv_timeout(Some(READ_POLL))
        .map_err(|e| format!("memory daemon socket: {e}"))?;
    stream
        .set_send_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| format!("memory daemon socket: {e}"))?;
    let mut to_daemon = interprocess::TryClone::try_clone(&stream)
        .map_err(|e| format!("memory daemon socket: {e}"))?;

    let output = Arc::new(Mutex::new(output));
    let list_ids = Arc::new(Mutex::new(HashSet::<String>::new()));
    let outstanding = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<Ended>();

    // Daemon → host.
    let reader = {
        let (output, list_ids, outstanding, stop, tx) = (
            Arc::clone(&output),
            Arc::clone(&list_ids),
            Arc::clone(&outstanding),
            Arc::clone(&stop),
            tx.clone(),
        );
        std::thread::spawn(move || {
            let mut r = BufReader::new(stream);
            let mut buf: Vec<u8> = Vec::new();
            loop {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                // read_until keeps partial bytes across a poll timeout, so a line split by the
                // timeout is completed on the next pass.
                match r.read_until(b'\n', &mut buf) {
                    Ok(0) => break,
                    Ok(_) if buf.last() != Some(&b'\n') => break,
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&buf).trim_end().to_string();
                        buf.clear();
                        if line.is_empty() {
                            continue;
                        }
                        let (out_line, key) = {
                            let mut ids = list_ids.lock().unwrap_or_else(|e| e.into_inner());
                            rewrite_response(&line, &mut ids)
                        };
                        let out_line = match &ledger {
                            Some(p) if out_line.contains("\"content\"") => {
                                hide_unresolved_in_response(
                                    &out_line,
                                    &crate::hermes_learn::RecallHide::load(p),
                                )
                            }
                            _ => out_line,
                        };
                        write_line(&output, &out_line);
                        if key.is_some() {
                            // Saturating decrement (compare_exchange loop: works on every
                            // supported toolchain without the renamed update helper).
                            let mut n = outstanding.load(Ordering::SeqCst);
                            while n > 0 {
                                match outstanding.compare_exchange(
                                    n,
                                    n - 1,
                                    Ordering::SeqCst,
                                    Ordering::SeqCst,
                                ) {
                                    Ok(_) => break,
                                    Err(current) => n = current,
                                }
                            }
                        }
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(_) => break,
                }
            }
            let _ = tx.send(Ended::Socket);
        })
    };

    // Host → daemon. Detached: a real stdin may block forever; the process exits around it.
    {
        let (output, list_ids, outstanding, tx) = (
            Arc::clone(&output),
            Arc::clone(&list_ids),
            Arc::clone(&outstanding),
            tx,
        );
        std::thread::spawn(move || {
            for line in input.lines() {
                let Ok(line) = line else { break };
                match gate_request(&line) {
                    Gate::Drop => {}
                    Gate::Reply(v) => write_line(&output, &v.to_string()),
                    Gate::Forward {
                        list_id,
                        has_id,
                        line: fwd,
                    } => {
                        if let Some(k) = list_id {
                            list_ids.lock().unwrap_or_else(|e| e.into_inner()).insert(k);
                        }
                        if has_id {
                            outstanding.fetch_add(1, Ordering::SeqCst);
                        }
                        if writeln!(to_daemon, "{fwd}").is_err() || to_daemon.flush().is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = tx.send(Ended::Input);
        });
    }

    if let Ok(Ended::Input) = rx.recv() {
        let deadline = Instant::now() + wait;
        while outstanding.load(Ordering::SeqCst) > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    stop.store(true, Ordering::SeqCst);
    let _ = reader.join();
    Ok(())
}

/// The learned-memory ledger the bridge filters with: the optional argument after the socket.
pub fn parse_bridge_ledger(args: &[String]) -> Option<std::path::PathBuf> {
    match args.first() {
        Some(flag) if flag == BRIDGE_FLAG => args
            .get(2)
            .filter(|p| !p.is_empty())
            .map(std::path::PathBuf::from),
        _ => None,
    }
}

/// Parse the bridge's argv (after the program name). `None` = not a bridge invocation.
pub fn parse_bridge_args(args: &[String]) -> Option<Result<String, String>> {
    match args.first() {
        Some(flag) if flag == BRIDGE_FLAG => Some(
            args.get(1)
                .cloned()
                .ok_or_else(|| format!("{BRIDGE_FLAG} needs the memory socket path")),
        ),
        _ => None,
    }
}

/// Called first thing in `main`: if this process was started as the bridge, run it and return the
/// exit code; otherwise `None` and the app starts normally.
pub fn maybe_run_from_args() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let socket = match parse_bridge_args(&args)? {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return Some(2);
        }
    };
    let stdin = BufReader::new(std::io::stdin());
    match run_bridge_with(
        Path::new(&socket),
        stdin,
        Box::new(std::io::stdout()),
        DEFAULT_WAIT,
        parse_bridge_ledger(&args),
    ) {
        Ok(()) => Some(0),
        Err(e) => {
            eprintln!("{e}");
            Some(1)
        }
    }
}

#[cfg(test)]
mod tests {
    include!("mem_mcp_bridge_tests.rs");
}
