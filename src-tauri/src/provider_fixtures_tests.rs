// D-11 — shared test fixtures for the provider proxy: an in-memory keychain, a scripted
// OpenAI-compatible vendor transport, and fake keys built at run time (never literals).

use crate::ai::{AiError, AiKeyring};
use crate::model_provider::{InboundResponse, Method, ProviderError, ProviderHttp, WireRequest};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// A fake key with a vendor's prefix, built at run time.
pub(crate) fn fake_key(prefix: &str) -> String {
    format!("{prefix}{}", "Q7".repeat(16))
}

/// A unique scratch directory (removed by [`TmpDir`]'s drop).
pub(crate) struct TmpDir(pub PathBuf);

impl TmpDir {
    pub(crate) fn new(tag: &str) -> TmpDir {
        let d = std::env::temp_dir().join(format!(
            "citrate-prov-{tag}-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&d).unwrap();
        TmpDir(d)
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
pub(crate) struct MemKeyring {
    pub map: Mutex<HashMap<String, String>>,
    pub fail: AtomicBool,
}

impl MemKeyring {
    pub(crate) fn failing() -> MemKeyring {
        let k = MemKeyring::default();
        k.fail.store(true, Ordering::SeqCst);
        k
    }
    pub(crate) fn values(&self) -> Vec<String> {
        self.map.lock().unwrap().values().cloned().collect()
    }
}

impl AiKeyring for MemKeyring {
    fn get(&self, account: &str) -> Result<Option<String>, AiError> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(AiError::KeyringUnavailable);
        }
        Ok(self.map.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, value: &str) -> Result<(), AiError> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(AiError::KeyringUnavailable);
        }
        self.map
            .lock()
            .unwrap()
            .insert(account.to_string(), value.to_string());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), AiError> {
        self.map.lock().unwrap().remove(account);
        Ok(())
    }
}

/// One request the scripted vendor saw.
#[derive(Debug, Clone)]
pub(crate) struct Seen {
    pub method: Method,
    pub url: String,
    pub auth: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Value>,
}

type Hook = Box<dyn Fn() + Send + Sync>;

/// A scripted vendor: `GET …/models` lists `models`; `POST …/chat/completions` (OpenAI-compatible)
/// or `POST …/responses` (OpenAI's native Responses API) answers `reply` (JSON, or SSE when the
/// request streams) with `usage` (if any). `status` overrides the
/// answer's status. `during_call` runs inside the chat call (to break things mid-call).
pub(crate) struct ScriptedVendor {
    pub seen: Mutex<Vec<Seen>>,
    pub models: Vec<String>,
    pub reply: String,
    pub usage: Option<(u64, u64)>,
    pub status: Mutex<Option<u16>>,
    pub during_call: Mutex<Option<Hook>>,
}

impl ScriptedVendor {
    pub(crate) fn new(models: &[&str], reply: &str, usage: Option<(u64, u64)>) -> Arc<Self> {
        Arc::new(ScriptedVendor {
            seen: Mutex::new(Vec::new()),
            models: models.iter().map(|m| m.to_string()).collect(),
            reply: reply.into(),
            usage,
            status: Mutex::new(None),
            during_call: Mutex::new(None),
        })
    }
    pub(crate) fn calls(&self) -> usize {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.url.ends_with("/chat/completions") || s.url.ends_with("/responses"))
            .count()
    }
}

impl ProviderHttp for ScriptedVendor {
    fn send(&self, req: WireRequest<'_>) -> Result<InboundResponse, ProviderError> {
        let body: Option<Value> = req
            .body
            .as_ref()
            .map(|b| serde_json::from_slice(b).unwrap());
        self.seen.lock().unwrap().push(Seen {
            method: req.method,
            url: req.url.to_string(),
            auth: format!("{}: {}", req.auth_header.0, req.auth_header.1.as_str()),
            headers: req.headers.clone(),
            body: body.clone(),
        });
        let status = self.status.lock().unwrap().unwrap_or(200);
        let ok = |ct: &str, s: String| {
            Ok(InboundResponse {
                status,
                content_type: ct.into(),
                body: Box::new(std::io::Cursor::new(s.into_bytes())),
            })
        };
        if req.url.path().ends_with("/models") {
            let data: Vec<Value> = self.models.iter().map(|m| json!({"id": m})).collect();
            return ok("application/json", json!({ "data": data }).to_string());
        }
        if let Some(h) = self.during_call.lock().unwrap().as_ref() {
            h();
        }
        let stream = body
            .as_ref()
            .and_then(|b| b.get("stream"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if req.url.path().ends_with("/responses") {
            return self.responses_answer(stream, status);
        }
        let usage = self
            .usage
            .map(|(p, c)| json!({"prompt_tokens": p, "completion_tokens": c}));
        if stream {
            let mut s = String::new();
            for piece in self.reply.split_inclusive(' ') {
                s.push_str(&format!(
                    "data: {}\n\n",
                    json!({"choices": [{"index": 0, "delta": {"content": piece}}]})
                ));
            }
            s.push_str(&format!(
                "data: {}\n\n",
                json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]})
            ));
            if let Some(u) = usage {
                s.push_str(&format!("data: {}\n\n", json!({"choices": [], "usage": u})));
            }
            s.push_str("data: [DONE]\n\n");
            return ok("text/event-stream", s);
        }
        let mut v = json!({"choices": [{"index": 0, "message": {"role": "assistant",
            "content": self.reply}, "finish_reason": "stop"}]});
        if let Some(u) = usage {
            v["usage"] = u;
        }
        ok("application/json", v.to_string())
    }
}

// ---------------------------------------------------------------------------
// WP-S1-PROV-ADAPT: recorded vendor answers for the native adapters' conformance suites
// ---------------------------------------------------------------------------

impl ScriptedVendor {
    /// The same reply in the Responses API shape (typed SSE events when streamed).
    fn responses_answer(&self, stream: bool, status: u16) -> Result<InboundResponse, ProviderError> {
        let usage = self
            .usage
            .map(|(p, c)| json!({"input_tokens": p, "output_tokens": c}))
            .unwrap_or(Value::Null);
        let item = json!({"type": "message", "role": "assistant", "status": "completed",
            "content": [{"type": "output_text", "text": self.reply, "annotations": []}]});
        let response = json!({"object": "response", "status": "completed", "output": [item],
            "usage": usage, "incomplete_details": null});
        let (ct, text) = if stream {
            let mut s = String::new();
            for piece in self.reply.split_inclusive(' ') {
                s.push_str(&format!(
                    "event: response.output_text.delta\ndata: {}\n\n",
                    json!({"type": "response.output_text.delta", "delta": piece})
                ));
            }
            s.push_str(&format!(
                "event: response.completed\ndata: {}\n\n",
                json!({"type": "response.completed", "response": response})
            ));
            ("text/event-stream", s)
        } else {
            ("application/json", response.to_string())
        };
        Ok(InboundResponse {
            status,
            content_type: ct.into(),
            body: Box::new(std::io::Cursor::new(text.into_bytes())),
        })
    }
}

/// One recorded vendor answer: status, content type, body.
pub(crate) type Answer = (u16, &'static str, String);

/// Answers each send with the next recorded answer, keeps what it was sent, and asserts at the
/// transport (as `PinnedHttp` does) that every URL is on the endpoint's pinned host.
pub(crate) struct Recorded {
    answers: Mutex<Vec<Answer>>,
    pub seen: Mutex<Vec<Seen>>,
}

impl Recorded {
    pub(crate) fn new(answers: Vec<Answer>) -> Recorded {
        Recorded {
            answers: Mutex::new(answers),
            seen: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn one(status: u16, content_type: &'static str, body: &str) -> Recorded {
        Recorded::new(vec![(status, content_type, body.to_string())])
    }

    /// The only request sent (panics unless exactly one was).
    pub(crate) fn sent(&self) -> Seen {
        let s = self.seen.lock().unwrap();
        assert_eq!(s.len(), 1, "exactly one request");
        s[0].clone()
    }

    pub(crate) fn count(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

impl ProviderHttp for Recorded {
    fn send(&self, req: WireRequest<'_>) -> Result<InboundResponse, ProviderError> {
        crate::provider_egress::check_on_endpoint(req.endpoint, &req.url)
            .expect("an adapter request left the pinned host");
        self.seen.lock().unwrap().push(Seen {
            method: req.method,
            url: req.url.to_string(),
            auth: format!("{}: {}", req.auth_header.0, req.auth_header.1.as_str()),
            headers: req.headers.clone(),
            body: req
                .body
                .as_ref()
                .map(|b| serde_json::from_slice(b).expect("request body is JSON")),
        });
        let (status, ct, body) = self.answers.lock().unwrap().remove(0);
        Ok(InboundResponse {
            status,
            content_type: ct.into(),
            body: Box::new(std::io::Cursor::new(body.into_bytes())),
        })
    }
}

/// A recorded fixture file under `src-tauri/tests/fixtures/providers/`.
pub(crate) fn fixture(rel: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/providers")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("fixture {}: {e}", p.display()))
}

pub(crate) fn fixture_json(rel: &str) -> Value {
    serde_json::from_str(&fixture(rel)).unwrap_or_else(|e| panic!("fixture {rel}: {e}"))
}

/// The one conversation every adapter's request golden is built from: a system prompt, a user
/// turn, an assistant turn with text and two parallel tool calls, both tool results, a follow-up
/// user turn, and one tool definition.
pub(crate) fn tool_conversation(model: &str, stream: bool) -> crate::model_provider::ChatRequest {
    use crate::model_provider::{ChatMessage, ChatRequest, Role, ToolCall, ToolDef};
    let msg = |role, content: &str| ChatMessage {
        role,
        content: content.into(),
        tool_calls: vec![],
        tool_call_id: None,
    };
    let result = |id: &str, content: &str| ChatMessage {
        role: Role::Tool,
        content: content.into(),
        tool_calls: vec![],
        tool_call_id: Some(id.into()),
    };
    ChatRequest {
        model: model.into(),
        messages: vec![
            msg(Role::System, "You are Hermes. Be brief."),
            msg(Role::User, "What is the weather in Boston and Paris?"),
            ChatMessage {
                role: Role::Assistant,
                content: "Checking both.".into(),
                tool_calls: vec![
                    ToolCall {
                        id: "call_1".into(),
                        name: "get_weather".into(),
                        arguments: "{\"location\":\"Boston, MA\"}".into(),
                    },
                    ToolCall {
                        id: "call_2".into(),
                        name: "get_weather".into(),
                        arguments: "{\"location\":\"Paris\"}".into(),
                    },
                ],
                tool_call_id: None,
            },
            result("call_1", "{\"temp_c\":12}"),
            result("call_2", "sunny"),
            msg(Role::User, "Thanks. Which is warmer?"),
        ],
        tools: vec![ToolDef {
            name: "get_weather".into(),
            description: "Get the current weather for a location".into(),
            parameters: json!({"type": "object",
                "properties": {"location": {"type": "string"}}, "required": ["location"]}),
        }],
        max_tokens: 512,
        stream,
    }
}

/// Run `f` with a context on `v`'s pinned endpoint and a fake key with `prefix`.
pub(crate) fn with_vendor_ctx<T>(
    v: crate::model_provider::Vendor,
    prefix: &str,
    http: &dyn ProviderHttp,
    f: impl FnOnce(&crate::model_provider::ProviderCtx<'_>) -> T,
) -> T {
    let ep = crate::provider_egress::resolve_vendor(v).unwrap();
    let key = crate::model_provider::ApiKey::from_secret(crate::secret::SecretString::new(
        fake_key(prefix),
    ));
    let ctx = crate::model_provider::ProviderCtx::new(&ep, &key, http);
    f(&ctx)
}

/// Assert `r` is `Ok` (an assertion failure, not a panic in the code under test) and unwrap it.
pub(crate) fn expect_ok<T, E: std::fmt::Debug>(r: Result<T, E>, what: &str) -> T {
    assert!(r.is_ok(), "{what}: expected Ok, got {:?}", r.as_ref().err());
    match r {
        Ok(v) => v,
        Err(_) => unreachable!(),
    }
}
