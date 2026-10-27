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
    /// Writes to this one account fail (everything else works).
    pub fail_set_account: Mutex<Option<String>>,
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
        if self.fail_set_account.lock().unwrap().as_deref() == Some(account) {
            return Err(AiError::KeyringUnavailable);
        }
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

/// An OpenAI-compatible vendor: `GET …/models` lists `models`; `POST …/chat/completions` answers
/// `reply` (JSON, or SSE when the request streams) with `usage` (if any). `status` overrides the
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
            .filter(|s| s.url.ends_with("/chat/completions"))
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
