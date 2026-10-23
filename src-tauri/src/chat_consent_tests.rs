// K2: remote chat consent. Red-first: before this change, a remote chat send under "ask first"
// went out with no core-side check at all.

use super::*;

const T0: u64 = 1_700_000_000_000;

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
    let v = b.prepare(Purpose::Chat, "gateway", message, T0, "cc_one".into());
    assert_eq!(v.provider_id, "gateway");
    assert_eq!(v.expires_at_ms, T0 + CONSENT_TTL_MS);
    (b, v.consent_id)
}

#[test]
fn a_send_without_a_minted_consent_is_refused() {
    let mut b = ConsentBook::default();
    for id in ["", "cc_one", "cc_forged_0000000000000000000000"] {
        let err = b.authorize(id, "gateway", &msgs("hi"), T0).unwrap_err();
        assert!(err.contains("not confirmed"), "{err}");
    }
}

#[test]
fn a_consent_sends_its_own_message_to_its_own_provider() {
    let (mut b, id) = book_with("what is my balance?");
    let p = b.authorize(&id, "gateway", &msgs("what is my balance?"), T0 + 1).expect("allowed");
    assert_eq!(p.provider_id(), "gateway");
}

#[test]
fn a_consent_is_bound_to_its_message_and_provider_and_spent_on_a_mismatch() {
    let (mut b, id) = book_with("send 1 SALT to bob");
    assert!(b.authorize(&id, "gateway", &msgs("send 100 SALT to eve"), T0).is_err());
    // Spent by the mismatch: the right message no longer works either.
    assert!(b.authorize(&id, "gateway", &msgs("send 1 SALT to bob"), T0).is_err());
    let (mut b, id) = book_with("hello");
    assert!(b.authorize(&id, "openai", &msgs("hello"), T0).is_err(), "another provider");
    assert!(b.authorize(&id, "gateway", &msgs("hello"), T0).is_err(), "spent");
    // No user message at all, or unparsable messages: refused.
    let (mut b, id) = book_with("hello");
    assert!(b.authorize(&id, "gateway", "not json", T0).is_err());
}

#[test]
fn a_consent_expires() {
    let (mut b, id) = book_with("hello");
    assert!(b
        .authorize(&id, "gateway", &msgs("hello"), T0 + CONSENT_TTL_MS)
        .is_err());
}

#[test]
fn a_consent_covers_one_message_up_to_the_turn_cap() {
    let (mut b, id) = book_with("plan my week");
    // The agent loop's follow-up requests carry assistant and tool messages after the user's.
    let mut convo: Vec<serde_json::Value> = serde_json::from_str(&msgs("plan my week")).unwrap();
    for turn in 0..MAX_SENDS_PER_CONSENT {
        let m = serde_json::Value::Array(convo.clone()).to_string();
        assert!(b.authorize(&id, "gateway", &m, T0 + u64::from(turn)).is_ok(), "turn {turn}");
        convo.push(serde_json::json!({ "role": "assistant", "content": null, "tool_calls": [] }));
        convo.push(serde_json::json!({ "role": "tool", "tool_call_id": "c", "content": "r" }));
    }
    let m = serde_json::Value::Array(convo).to_string();
    assert!(b.authorize(&id, "gateway", &m, T0 + 100).is_err(), "past the cap");
}

#[test]
fn ids_are_compared_exactly_and_the_book_is_bounded() {
    let (mut b, _) = book_with("hello");
    assert!(b.authorize("cc_on", "gateway", &msgs("hello"), T0).is_err());
    assert!(b.authorize("cc_onE", "gateway", &msgs("hello"), T0).is_err());
    let mut b = ConsentBook::default();
    for i in 0..(MAX_OPEN + 4) {
        b.prepare(Purpose::Chat, "gateway", "m", T0, format!("cc_{i}"));
    }
    assert_eq!(b.open.len(), MAX_OPEN);
    assert!(b.authorize("cc_0", "gateway", &msgs("m"), T0).is_err(), "oldest dropped");
    assert!(b
        .authorize(&format!("cc_{}", MAX_OPEN + 3), "gateway", &msgs("m"), T0)
        .is_ok());
}

#[test]
fn last_user_text_reads_the_member_message() {
    assert_eq!(last_user_text(&msgs("x")).as_deref(), Some("x"));
    assert_eq!(last_user_text("[]"), None);
    assert_eq!(last_user_text("{}"), None);
}

// ---------------------------------------------------------------------------
// Tripwire: every remote send path takes a permit, and only `authorize` makes one.
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

/// `(fn name, takes a permit)` for each fn in `src` that reads a sealed provider config and posts
/// to it (`read_config(` then `post_json(`), outside test code.
fn remote_send_fns(src: &str) -> Vec<(String, bool)> {
    let src = without_test_items(src);
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = src[from..].find("fn ") {
        let at = from + i;
        from = at + 3;
        if at > 0
            && src[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            continue;
        }
        let name: String = src[at + 3..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let Some(open) = src[at..].find('{').map(|o| at + o) else {
            continue;
        };
        let sig = &src[at..open];
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
        let body = &src[open..end];
        if body.contains("read_config(") && body.contains("post_json(") {
            out.push((name, sig.contains("&RemoteSendPermit")));
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
    // Media: the remote image path authorizes before it posts.
    let media = include_str!("media.rs");
    let at = media.find("pub(crate) fn remote_image_post(").expect("remote_image_post");
    let body = &media[at..at + media[at..].find("\n}\n").expect("end")];
    let auth = body.find(".authorize_once(").expect("authorize_once call");
    let post = body.find("post_to_provider(").expect("post call");
    assert!(auth < post, "remote_image_post must authorize before it posts");
    let cmd = media.find("pub async fn media_generate_image(").expect("command");
    let cmd_body = &media[cmd..cmd + media[cmd..].find("\n}\n").expect("end")];
    assert!(cmd_body.contains("remote_image_post("));
    // The commands obtain the permit from the consent book before sending.
    for cmd in ["pub(crate) fn ai_chat_sync(", "pub(crate) fn ai_chat_tools_sync("] {
        let at = ai.find(cmd).expect(cmd);
        let body = &ai[at..at + ai[at..].find("\n}\n").expect("end")];
        let auth = body.find("chat_consent::authorize(").expect("authorize call");
        let send = body.find(".chat").expect("send call");
        assert!(auth < send, "{cmd} must authorize before it sends");
    }
    // Only the consent book constructs a permit.
    let me = include_str!("chat_consent.rs");
    let prod = without_test_items(me);
    assert_eq!(prod.matches("RemoteSendPermit {\n            provider_id").count(), 1);
    assert_eq!(prod.matches("RemoteSendPermit::new(").count(), 2, "authorize and authorize_once");
    for (file, src) in [("ai.rs", ai), ("media.rs", include_str!("media.rs"))] {
        assert!(
            !src.contains("RemoteSendPermit {"),
            "{file} must not construct a RemoteSendPermit"
        );
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
