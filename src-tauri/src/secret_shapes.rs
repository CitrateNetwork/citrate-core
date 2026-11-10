//! #613 review (P1): the secret shapes a sidecar log line or a diagnostics bundle must not carry.
//! (Red: tests only.)

/// The fixture corpus: one line per shape, as a sidecar might print it, and the secret part that
/// must not survive. Every value is assembled at run time from parts, so no scanner reads the
/// source as holding a credential; none of them is a real key, token or password.
#[cfg(test)]
pub(crate) fn test_corpus() -> Vec<(&'static str, String, String)> {
    let opaque = ["opaqueSession", "Token1234567890"].concat();
    let jwt = ["eyJhbGciOiJSUzI1NiJ9", ".", "eyJzdWIiOiJ4In0", ".", "c2lnbmF0dXJlc2lnbmF0dXJl"].concat();
    let jwt2 = ["eyJhbGciOiJIUzI1NiJ9", ".", "eyJzdWIiOiJ4In0", ".", "abcdefghijklmnop"].concat();
    let jwt_plain = ["QUJDREVGR0hJSktMTU5PUA", ".", "UVJTVFVWV1hZWjAxMjM0NQ", ".", "c2lnbmF0dXJlLXNpZ25hdHVyZQ"].concat();
    let pw = ["hunter2", "hunter2"].concat();
    let api = ["3f9a8b7c6d5e", "4f3a2b1c0d9e8f7a6b5c"].concat();
    let basic = ["dXNlcjpwYX", "NzLXdvcmQ="].concat();
    let hex64 = "5e".repeat(32);
    let words = ["abandon ability able about above absent ", "absorb abstract absurd abuse access accident"].concat();
    vec![
        ("bearer header", format!("Authorization: Bearer {opaque}"), opaque.clone()),
        ("bearer lowercase", format!("auth=bearer {opaque} ok"), opaque.clone()),
        ("basic header", format!("Authorization: Basic {basic}"), basic.clone()),
        ("jwt in json", format!("{{\"id_token\":\"{jwt}\"}}"), jwt.clone()),
        ("jwt in a query", format!("GET /mcp?token={jwt2}&x=1"), jwt2.clone()),
        ("jwt after a word", format!("session={jwt}"), jwt.clone()),
        ("jwt-shaped, no eyJ", format!("t {jwt_plain}"), jwt_plain.clone()),
        ("password=", format!("password={pw} next"), pw.clone()),
        ("api_key:", format!("api_key: {api}"), api.clone()),
        ("api-key quoted", format!("\"x-api-key\": \"{api}\""), api.clone()),
        ("apiKey camel", format!("apiKey={api}"), api.clone()),
        ("secret single quotes", format!("client_secret='{pw}'"), pw.clone()),
        ("passwd", format!("PASSWD = {pw}"), pw.clone()),
        ("private_key", format!("private_key: {pw}"), pw.clone()),
        ("access_token", format!("access_token={opaque}"), opaque.clone()),
        ("mnemonic quoted", format!("mnemonic=\"{words}\""), words.clone()),
        ("seed", format!("seed: {pw}"), pw.clone()),
        ("bare 64-hex", format!("pk {hex64} loaded"), hex64.clone()),
        ("0x 64-hex", format!("key=0x{hex64}"), hex64.clone()),
        ("64-hex in json", format!("{{\"k\":\"{hex64}\"}}"), hex64.clone()),
        ("email", "contact member@example.com now".to_string(), "member@example.com".to_string()),
    ]
}

#[cfg(test)]
mod tests {
    include!("secret_shapes_tests.rs");
}
