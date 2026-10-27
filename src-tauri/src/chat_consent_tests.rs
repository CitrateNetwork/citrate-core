// K2: remote chat consent. Red-first: before this change, a remote chat send under "ask first"
// went out with no core-side check at all.

use super::*;

const T0: u64 = 1_700_000_000_000;

/// The clock at wall time `ms` (>= `T0`), with monotonic time advanced by the same amount.
fn at(ms: u64) -> ClockReading {
    test_clock(ms - T0, ms)
}

fn msgs(user: &str) -> String {
    serde_json::json!([
        { "role": "system", "content": "persona" },
        { "role": "user", "content": "an earlier question" },
        { "role": "assistant", "content": "an earlier answer" },
        { "role": "user", "content": user },
    ])
    .to_string()
}

fn book_with(message: &str) -> (ConsentBook, String) {
    let mut b = ConsentBook::default();
    let v = b.prepare(Purpose::Chat, "gateway", message, at(T0), "cc_one".into());
    assert_eq!(v.provider_id, "gateway");
    assert_eq!(v.expires_at_ms, T0 + CONSENT_TTL_MS);
    (b, v.consent_id)
}

/// A fresh route folder for one test (no file = the "ask first" default).
fn route_tmp(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("citrate-chat-consent-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("tmp");
    d
}

#[test]
fn a_send_without_a_minted_consent_is_refused() {
    let mut b = ConsentBook::default();
    for id in ["", "cc_one", "cc_forged_0000000000000000000000"] {
        let err = b.permit_chat(id, "gateway", &msgs("hi"), at(T0)).unwrap_err();
        assert!(err.contains("not confirmed"), "{err}");
    }
}

#[test]
fn a_consent_sends_its_own_message_to_its_own_provider() {
    let (mut b, id) = book_with("what is my balance?");
    let p = b
        .permit_chat(&id, "gateway", &msgs("what is my balance?"), at(T0 + 1))
        .expect("allowed");
    assert_eq!(p.provider_id(), "gateway");
}

#[test]
fn a_consent_is_bound_to_its_message_and_provider_and_spent_on_a_mismatch() {
    let (mut b, id) = book_with("send 1 SALT to bob");
    assert!(b.permit_chat(&id, "gateway", &msgs("send 100 SALT to eve"), at(T0)).is_err());
    // Spent by the mismatch: the right message no longer works either.
    assert!(b.permit_chat(&id, "gateway", &msgs("send 1 SALT to bob"), at(T0)).is_err());
    let (mut b, id) = book_with("hello");
    assert!(b.permit_chat(&id, "openai", &msgs("hello"), at(T0)).is_err(), "another provider");
    assert!(b.permit_chat(&id, "gateway", &msgs("hello"), at(T0)).is_err(), "spent");
    // No user message at all, or unparsable messages: refused.
    let (mut b, id) = book_with("hello");
    assert!(b.permit_chat(&id, "gateway", "not json", at(T0)).is_err());
}

#[test]
fn a_consent_expires() {
    let (mut b, id) = book_with("hello");
    assert!(b
        .permit_chat(&id, "gateway", &msgs("hello"), at(T0 + CONSENT_TTL_MS))
        .is_err());
}

// #458: a consent expires on monotonic time as well as wall-clock time.

#[test]
fn a_consent_expires_on_monotonic_time_when_the_wall_clock_is_set_back() {
    for back in [0, 1, 60 * 60 * 1000] {
        let (mut b, id) = book_with("hello");
        // The wall clock was set back after the mint; monotonic time has passed the TTL.
        let now = test_clock(CONSENT_TTL_MS, T0 - back);
        assert!(
            b.permit_chat(&id, "gateway", &msgs("hello"), now).is_err(),
            "wall clock set back {back} ms must not extend the consent"
        );
        // Just inside the TTL on both clocks it still works.
        let (mut b, id) = book_with("hello");
        let now = test_clock(CONSENT_TTL_MS - 1, T0 - back);
        assert!(b.permit_chat(&id, "gateway", &msgs("hello"), now).is_ok(), "{back}");
    }
    // The same rule for media consents and for the mint-time sweep.
    let mut b = ConsentBook::default();
    b.prepare(Purpose::Media, "gateway", "a tree", at(T0), "cc_img".into());
    let now = test_clock(CONSENT_TTL_MS, T0 - 5_000);
    assert!(b.permit_media("cc_img", "gateway", "a tree", now).is_err(), "media");
    let mut b = ConsentBook::default();
    b.prepare(Purpose::Chat, "gateway", "m", at(T0), "cc_old".into());
    b.prepare(Purpose::Chat, "gateway", "m", test_clock(CONSENT_TTL_MS, T0 - 5_000), "cc_new".into());
    assert_eq!(b.open.len(), 1, "the expired consent is swept at the next mint");
}

#[test]
fn a_consent_expires_on_wall_time_when_monotonic_time_stalls() {
    // Sleep: the monotonic clock may not advance while the wall clock does.
    let (mut b, id) = book_with("hello");
    let now = test_clock(0, T0 + CONSENT_TTL_MS);
    assert!(b.permit_chat(&id, "gateway", &msgs("hello"), now).is_err());
}

#[test]
fn a_consent_covers_one_message_up_to_the_turn_cap() {
    let (mut b, id) = book_with("plan my week");
    // The agent loop's follow-up requests carry assistant and tool messages after the user's.
    let mut convo: Vec<serde_json::Value> = serde_json::from_str(&msgs("plan my week")).unwrap();
    for turn in 0..MAX_SENDS_PER_CONSENT {
        let m = serde_json::Value::Array(convo.clone()).to_string();
        assert!(
            b.permit_chat(&id, "gateway", &m, at(T0 + u64::from(turn))).is_ok(),
            "turn {turn}"
        );
        convo.push(serde_json::json!({ "role": "assistant", "content": null, "tool_calls": [] }));
        convo.push(serde_json::json!({ "role": "tool", "tool_call_id": "c", "content": "r" }));
    }
    let m = serde_json::Value::Array(convo).to_string();
    assert!(b.permit_chat(&id, "gateway", &m, at(T0 + 100)).is_err(), "past the cap");
}

#[test]
fn ids_are_compared_exactly_and_the_book_is_bounded() {
    let (mut b, _) = book_with("hello");
    assert!(b.permit_chat("cc_on", "gateway", &msgs("hello"), at(T0)).is_err());
    assert!(b.permit_chat("cc_onE", "gateway", &msgs("hello"), at(T0)).is_err());
    let mut b = ConsentBook::default();
    for i in 0..(MAX_OPEN + 4) {
        b.prepare(Purpose::Chat, "gateway", "m", at(T0), format!("cc_{i}"));
    }
    assert_eq!(b.open.len(), MAX_OPEN);
    assert!(b.permit_chat("cc_0", "gateway", &msgs("m"), at(T0)).is_err(), "oldest dropped");
    assert!(b
        .permit_chat(&format!("cc_{}", MAX_OPEN + 3), "gateway", &msgs("m"), at(T0))
        .is_ok());
}

#[test]
fn last_user_text_reads_the_member_message() {
    assert_eq!(last_user_text(&msgs("x")).as_deref(), Some("x"));
    assert_eq!(last_user_text("[]"), None);
    assert_eq!(last_user_text("{}"), None);
}

// #459: a question closed without the gateway choice drops its consent.

#[test]
fn a_dropped_consent_sends_nothing() {
    let (mut b, id) = book_with("hello");
    assert!(b.drop_consent(&id), "the open consent is dropped");
    assert!(b.open.is_empty());
    assert!(b.permit_chat(&id, "gateway", &msgs("hello"), at(T0 + 1)).is_err());
    assert!(!b.drop_consent(&id), "dropping twice is a no-op");
    // Only that consent goes: ids are compared exactly, and others stay usable.
    let mut b = ConsentBook::default();
    b.prepare(Purpose::Chat, "gateway", "a", at(T0), "cc_a".into());
    b.prepare(Purpose::Media, "gateway", "b", at(T0), "cc_b".into());
    assert!(!b.drop_consent("cc_"), "a prefix drops nothing");
    assert!(!b.drop_consent(""), "an empty id drops nothing");
    assert!(b.drop_consent("cc_b"));
    assert!(b.permit_media("cc_b", "gateway", "b", at(T0 + 1)).is_err());
    assert!(b.permit_chat("cc_a", "gateway", &msgs("a"), at(T0 + 1)).is_ok());
}

#[test]
fn chat_consent_drop_is_registered_for_the_main_window() {
    let lib = include_str!("lib.rs");
    let acl = include_str!("../permissions/main-window.toml");
    assert!(lib.contains("chat_consent::chat_consent_drop,"), "registered in lib.rs");
    assert!(acl.contains("\"chat_consent_drop\""), "allowed for the main window");
}

// #473: a permit cannot be obtained under Local model only.

#[test]
fn a_permit_is_refused_under_local_model_only() {
    let d = route_tmp("local-only");
    let state = ChatConsentState::default();
    let mint = |purpose, msg: &str, id: &str| {
        state
            .0
            .lock()
            .unwrap()
            .prepare(purpose, "gateway", msg, at(T0), id.into());
    };
    // Ask first (no file): the consents work.
    mint(Purpose::Chat, "hello", "cc_c1");
    assert!(authorize(&state, &d, "cc_c1", "gateway", &msgs("hello"), at(T0 + 1)).is_ok());
    mint(Purpose::Media, "a tree", "cc_m1");
    assert!(authorize_once(&state, &d, "cc_m1", "gateway", "a tree", at(T0 + 1)).is_ok());
    // Local model only, then an unreadable setting: refused with valid consents in hand.
    crate::chat_route::set(&d, crate::chat_route::ChatRoute::LocalOnly, 1).unwrap();
    for case in ["local only", "unreadable"] {
        if case == "unreadable" {
            std::fs::write(d.join(crate::chat_route::ROUTE_FILE), "{").unwrap();
        }
        mint(Purpose::Chat, "hello", "cc_c2");
        let err = authorize(&state, &d, "cc_c2", "gateway", &msgs("hello"), at(T0 + 2))
            .expect_err(case);
        assert!(err.contains("Local only is on"), "{case}: {err}");
        mint(Purpose::Media, "a tree", "cc_m2");
        let err = authorize_once(&state, &d, "cc_m2", "gateway", "a tree", at(T0 + 2))
            .expect_err(case);
        assert!(err.contains("Local only is on"), "{case}: {err}");
    }
    let _ = std::fs::remove_dir_all(d);
}

// ---------------------------------------------------------------------------
// Tripwire: every remote send path takes a permit, and only `authorize` / `authorize_once` hand
// one out, each after the Local model only check.
// ---------------------------------------------------------------------------

/// `src` with every `#[cfg(test)]` item (a block, or one item up to `;`) cut out.
fn without_test_items(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(i) = rest.find("#[cfg(test)]") {
        out.push_str(&rest[..i]);
        let after = &rest[i..];
        let end = match (after.find('{'), after.find(';')) {
            (Some(b), Some(s)) if s < b => s + 1,
            (Some(b), _) => {
                let mut depth = 0usize;
                let mut end = after.len();
                for (j, c) in after[b..].char_indices() {
                    match c {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = b + j + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                end
            }
            (None, Some(s)) => s + 1,
            (None, None) => after.len(),
        };
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// One fn in a source file: its name, whether it is `pub` (any form), its signature, its body.
struct FnItem {
    name: String,
    public: bool,
    sig: String,
    body: String,
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Every fn with a body in `src`, outside test code and comments.
fn fn_items(src: &str) -> Vec<FnItem> {
    let src: String = without_test_items(src)
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("'{'", "' '")
        .replace("'}'", "' '");
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = src[from..].find("fn ") {
        let at = from + i;
        from = at + 3;
        if at > 0 && src[..at].chars().next_back().is_some_and(is_ident) {
            continue;
        }
        let name: String = src[at + 3..].chars().take_while(|c| is_ident(*c)).collect();
        let Some(open) = src[at..].find('{').map(|o| at + o) else {
            continue;
        };
        let sig = &src[at..open];
        if name.is_empty() || sig.contains(';') {
            continue;
        }
        let line_start = src[..at].rfind('\n').map_or(0, |n| n + 1);
        let public = src[line_start..at].trim_start().starts_with("pub");
        let mut depth = 0usize;
        let mut end = src.len();
        for (j, c) in src[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + j;
                        break;
                    }
                }
                _ => {}
            }
        }
        out.push(FnItem {
            name,
            public,
            sig: sig.to_string(),
            body: src[open..end].to_string(),
        });
    }
    out
}

/// Where `body` first calls `name(` (not a longer identifier ending in `name`), if it does.
fn first_call(body: &str, name: &str) -> Option<usize> {
    let pat = format!("{name}(");
    let mut from = 0;
    while let Some(i) = body[from..].find(&pat) {
        let at = from + i;
        if !body[..at].chars().next_back().is_some_and(is_ident) && !body[..at].ends_with("fn ") {
            return Some(at);
        }
        from = at + pat.len();
    }
    None
}

/// `(fn name, takes a permit)` for each fn in `src` that reads a sealed provider config and posts
/// to it (`read_config(` then `post_json(`), outside test code.
fn remote_send_fns(src: &str) -> Vec<(String, bool)> {
    fn_items(src)
        .into_iter()
        .filter(|f| f.body.contains("read_config(") && f.body.contains("post_json("))
        .map(|f| (f.name, f.sig.contains("&RemoteSendPermit")))
        .collect()
}

/// The permit gaps in `files` (`(file name, source)`): a fn that hands out a `RemoteSendPermit`
/// (returns one) and is `pub` must check `remote_chat_allowed(` before it asks for the permit; a
/// private one may be called only by such a checked fn, after its check. Returns the names of the
/// fns that break this, and every fn (in any file) that calls a permit fn.
fn permit_gaps(files: &[(String, String)]) -> (Vec<String>, Vec<String>) {
    let items: Vec<FnItem> = files.iter().flat_map(|(_, s)| fn_items(s)).collect();
    let returns_permit =
        |f: &FnItem| f.sig.split("->").nth(1).is_some_and(|r| r.contains("RemoteSendPermit"));
    let permit_fns: Vec<&FnItem> = items.iter().filter(|f| returns_permit(f)).collect();
    let private: Vec<&str> =
        permit_fns.iter().filter(|f| !f.public).map(|f| f.name.as_str()).collect();
    // The route check, if any, comes before every way this fn could get a permit.
    let checked_before = |f: &FnItem, at: usize| {
        f.body.find("remote_chat_allowed(").is_some_and(|c| c < at)
    };
    let mut gaps = Vec::new();
    let mut callers = Vec::new();
    for f in &items {
        let mut asks: Vec<usize> = private.iter().filter_map(|p| first_call(&f.body, p)).collect();
        if returns_permit(f) {
            asks.extend(first_call(&f.body, "RemoteSendPermit::new"));
        }
        let calls_permit_fn = permit_fns
            .iter()
            .any(|p| p.name != f.name && first_call(&f.body, &p.name).is_some());
        if calls_permit_fn {
            callers.push(f.name.clone());
        }
        let ok = if returns_permit(f) && !f.public {
            // A private permit fn (the book's): it may build the permit; callers are checked below.
            private.iter().all(|p| first_call(&f.body, p).is_none())
        } else if returns_permit(f) {
            !asks.is_empty() && asks.iter().all(|a| checked_before(f, *a))
        } else {
            // Not a permit fn: it must not reach the unchecked ones.
            private.iter().all(|p| first_call(&f.body, p).is_none())
        };
        if !ok {
            gaps.push(f.name.clone());
        }
    }
    (gaps, callers)
}

/// Every production source file of this crate that mentions a permit or the consent book (files
/// named `*test*` are test code).
fn production_sources() -> Vec<(String, String)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    let mut stack = vec![dir];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).expect("src dir").flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            if !name.ends_with(".rs") || name.contains("test") {
                continue;
            }
            let src = std::fs::read_to_string(&p).expect("read");
            if src.contains("RemoteSendPermit") || src.contains("chat_consent::") {
                out.push((name, src));
            }
        }
    }
    out
}

#[test]
fn every_remote_send_takes_a_permit_from_the_consent_book() {
    let ai = include_str!("ai.rs");
    let sends = remote_send_fns(ai);
    let names: Vec<&str> = sends.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"chat") && names.contains(&"chat_tools"), "{names:?}");
    assert!(names.contains(&"post_to_provider"), "{names:?}");
    for (name, takes_permit) in &sends {
        assert!(
            *takes_permit,
            "`{name}` posts to a remote provider without a RemoteSendPermit"
        );
    }
    // Every fn that hands out a permit checks Local model only first, and every production
    // caller (found by scanning, not from a list) gets its permit from such a fn.
    let files = production_sources();
    let (gaps, callers) = permit_gaps(&files);
    assert!(gaps.is_empty(), "permit without the Local model only check: {gaps:?}");
    for known in ["ai_chat_sync", "ai_chat_tools_sync", "remote_image_post", "authorize"] {
        assert!(callers.iter().any(|c| c == known), "the scan sees {known}: {callers:?}");
    }
    // The commands that mint consents refuse under Local model only too.
    let me = include_str!("chat_consent.rs");
    for sig in ["pub async fn chat_consent_prepare(", "pub async fn media_consent_prepare("] {
        let at = me.find(sig).expect(sig);
        let body = &me[at..at + me[at..].find("\n}\n").expect("end")];
        assert!(body.contains("remote_chat_allowed("), "{sig} must check Local model only");
    }
    // Only the consent book constructs a permit.
    let prod = without_test_items(me);
    assert_eq!(prod.matches("RemoteSendPermit {\n            provider_id").count(), 1);
    assert_eq!(prod.matches("RemoteSendPermit::new(").count(), 2, "permit_chat and permit_media");
    for (file, src) in &files {
        if file != "chat_consent.rs" {
            assert!(
                !src.contains("RemoteSendPermit {"),
                "{file} must not construct a RemoteSendPermit"
            );
        }
    }
}

#[test]
fn negative_control_a_remote_send_without_a_permit_is_seen() {
    let src = r#"
impl AiManager {
    fn sneaky(&self, provider_id: &str) -> Result<String> {
        let cfg = self.read_config(provider_id)?;
        self.http.post_json(&cfg.base_url, cfg.api_key.expose(), &json!({}))
    }
    fn chat(&self, permit: &RemoteSendPermit, provider_id: &str) -> Result<String> {
        let cfg = self.read_config(provider_id)?;
        self.http.post_json(&cfg.base_url, cfg.api_key.expose(), &json!({}))
    }
}
"#;
    assert_eq!(
        remote_send_fns(src),
        vec![("sneaky".to_string(), false), ("chat".to_string(), true)]
    );
}

#[test]
fn negative_control_a_permit_without_the_route_check_is_seen() {
    let book = r#"
impl ConsentBook {
    fn permit_chat(&mut self, id: &str) -> Result<RemoteSendPermit, String> {
        Ok(RemoteSendPermit::new(id))
    }
}
// fn commented_out(s: &S) -> Result<RemoteSendPermit, String> { s.permit_chat("x") }
pub(crate) fn authorize(s: &S, d: &Path) -> Result<RemoteSendPermit, String> {
    crate::chat_route::remote_chat_allowed(d)?;
    s.lock().permit_chat("x")
}
pub(crate) fn sneaky(s: &S) -> Result<RemoteSendPermit, String> {
    s.lock().permit_chat("x")
}
pub(crate) fn late(s: &S, d: &Path) -> Result<RemoteSendPermit, String> {
    let p = s.lock().permit_chat("x");
    crate::chat_route::remote_chat_allowed(d)?;
    p
}
pub fn made_up(d: &Path) -> Result<RemoteSendPermit, String> {
    Ok(RemoteSendPermit::new("x"))
}
"#;
    let caller = r#"
fn helper(s: &S) -> u32 {
    let _ = s.lock().permit_chat("x");
    1
}
pub(crate) fn fine(s: &S, d: &Path) -> Result<String, String> {
    let p = crate::chat_consent::authorize(s, d)?;
    Ok(p.provider_id().into())
}
"#;
    let files = vec![("book.rs".to_string(), book.to_string()), ("x.rs".to_string(), caller.to_string())];
    let (gaps, callers) = permit_gaps(&files);
    assert_eq!(gaps, vec!["sneaky", "late", "made_up", "helper"]);
    assert_eq!(callers, vec!["authorize", "sneaky", "late", "helper", "fine"]);
}
