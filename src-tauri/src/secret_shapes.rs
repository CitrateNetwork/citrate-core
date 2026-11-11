//! #613 review (P1): the secret shapes a sidecar log line or a diagnostics bundle must not carry,
//! found **anywhere** in a line, not only at the start of a whitespace-separated word:
//!
//! | shape | found as | replaced by |
//! |---|---|---|
//! | auth | the value after `Bearer` or `Basic` (any case), 8+ characters | `<redacted>` |
//! | jwt | `eyJ….….…` anywhere, or three base64url segments of 16+ characters | `<redacted>` |
//! | key/value | the value after a key ending in password, passwd, pwd, pass, secret, token, credential(s), api key, private key, secret access key, access key id, mnemonic, seed, seed phrase, recovery phrase or authorization, in `k=v`, `k: v`, `k => v`, `"k":"v"`, `k='v'` and `--k v` forms, any case; for the phrase keys and authorization an unquoted value runs to the end of the line | `<redacted>` |
//! | hex64 | 64+ hex digits, bare or after `0x` (private-key shaped) | `<hex64 xxxxxx>`, 6 hex of its SHA-256, so equal values still line up |
//! | email | `local@domain.tld` | `<redacted>` |
//! | token | GitHub (`ghp_` ... `github_pat_`), Slack (`xox?-`) and `sk-` tokens, AWS key ids (`AKIA`/`ASIA`), `nsec1` keys, WIF private keys | `<redacted>` |
//! | cookie | the whole value after `Cookie` or `Set-Cookie` (`:` or `=`, quoted or not) to the end of the line or the closing quote | `<redacted>` |
//!
//! [`redact`] replaces every hit; [`first_shape`] names the first one left (the bundle's final
//! check). Both are pure. What they do not catch: free prose (a prompt, a message), and a seed
//! phrase or key printed with no key name in front of it. That is why the content-bearing
//! sidecars keep only lifecycle and error lines (`sidecar_logs::LogPolicy`).
use sha2::Digest as _;

pub const REDACTED: &str = "<redacted>";

/// Key-name endings whose value is a secret (lowercase).
const SECRET_KEYS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "api-key",
    "apikey",
    "private_key",
    "private-key",
    "privatekey",
    "mnemonic",
    "seed",
    "seed_phrase",
    "seed-phrase",
    "seedphrase",
    "recovery_phrase",
    "recovery-phrase",
    "recoveryphrase",
    "authorization",
    "pwd",
    "pass",
    "credentials",
    "credential",
    "secret_access_key",
    "access_key_id",
    "access_key",
];

/// Keys whose value runs to the end of the line when it is not quoted: a phrase of words (seed,
/// mnemonic) or an `Authorization` value of any scheme.
const LINE_KEYS: &[&str] = &[
    "mnemonic",
    "seed",
    "seed_phrase",
    "seed-phrase",
    "seedphrase",
    "recovery_phrase",
    "recovery-phrase",
    "recoveryphrase",
    "authorization",
];

/// Token prefixes: (prefix, the fewest characters after it, whether `_` and `-` count).
const TOKEN_PREFIXES: &[(&str, usize, bool)] = &[
    ("ghp_", 20, false),
    ("gho_", 20, false),
    ("ghs_", 20, false),
    ("ghu_", 20, false),
    ("ghr_", 20, false),
    ("github_pat_", 20, true),
    ("xoxa-", 10, true),
    ("xoxb-", 10, true),
    ("xoxp-", 10, true),
    ("xoxr-", 10, true),
    ("xoxs-", 10, true),
    ("sk-", 16, true),
];

const BASE58: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const BECH32: &[u8] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Redact,
    Hex,
}

struct Hit {
    start: usize,
    end: usize,
    kind: Kind,
    shape: &'static str,
}

fn b64url(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-' || c == b'_'
}

fn run(b: &[u8], from: usize, f: impl Fn(u8) -> bool) -> usize {
    let mut j = from;
    while j < b.len() && f(b[j]) {
        j += 1;
    }
    j
}

/// The value after `Bearer ` / `Basic `.
fn auth_hits(b: &[u8], out: &mut Vec<Hit>) {
    for scheme in [&b"bearer"[..], &b"basic"[..]] {
        let n = scheme.len();
        let mut i = 0;
        while i + n < b.len() {
            let at_word = i == 0 || !b[i - 1].is_ascii_alphanumeric();
            if at_word
                && b[i..i + n].eq_ignore_ascii_case(scheme)
                && matches!(b[i + n], b' ' | b'\t' | b':')
            {
                // `Bearer x`, `Bearer: x` and `Bearer:x`.
                let after = if b[i + n] == b':' { i + n + 1 } else { i + n };
                let v = run(b, after, |c| c == b' ' || c == b'\t');
                let e = run(b, v, |c| {
                    !c.is_ascii_whitespace() && !b"\"',;<>".contains(&c)
                });
                if e - v >= 8 {
                    out.push(Hit {
                        start: v,
                        end: e,
                        kind: Kind::Redact,
                        shape: "auth value",
                    });
                }
                i = e.max(i + 1);
                continue;
            }
            i += 1;
        }
    }
}

/// JWTs and three-segment base64url tokens.
fn jwt_hits(b: &[u8], out: &mut Vec<Hit>) {
    let mut i = 0;
    while i < b.len() {
        if !b64url(b[i]) || (i > 0 && (b64url(b[i - 1]) || b[i - 1] == b'.')) {
            i += 1;
            continue;
        }
        let jwt = b[i..].starts_with(b"eyJ");
        let min = if jwt { 1 } else { 16 };
        let s1 = run(b, i, b64url);
        let mut ok = s1 - i >= if jwt { 4 } else { 16 } && s1 < b.len() && b[s1] == b'.';
        let mut end = s1;
        if ok {
            let s2 = run(b, s1 + 1, b64url);
            ok = s2 - (s1 + 1) >= min && s2 < b.len() && b[s2] == b'.';
            if ok {
                let s3 = run(b, s2 + 1, b64url);
                ok = s3 - (s2 + 1) >= if jwt { 0 } else { 16 };
                end = s3;
            }
        }
        if ok {
            out.push(Hit {
                start: i,
                end,
                kind: Kind::Redact,
                shape: "JWT",
            });
            i = end;
        } else {
            i = s1.max(i + 1);
        }
    }
}

/// Whether the identifier `id` (original case) ends in a secret key name at a word boundary.
fn secret_key(id: &[u8]) -> bool {
    let low = id.to_ascii_lowercase();
    SECRET_KEYS.iter().any(|k| {
        let k = k.as_bytes();
        if !low.ends_with(k) {
            return false;
        }
        let at = low.len() - k.len();
        at == 0 || matches!(id[at - 1], b'_' | b'-' | b'.') || id[at].is_ascii_uppercase()
    })
}

/// Whether the key's unquoted value runs to the end of the line.
fn line_key(id: &[u8]) -> bool {
    let low = id.to_ascii_lowercase();
    LINE_KEYS.iter().any(|k| low.ends_with(k.as_bytes()))
}

/// Known token prefixes (GitHub, Slack, `sk-` keys), AWS key ids (`AKIA`/`ASIA` + 16), `nsec1`
/// keys and WIF private keys (base58: 51 characters from `5`, or 52 from `K`/`L`).
fn prefix_hits(b: &[u8], out: &mut Vec<Hit>) {
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'-';
    let mut i = 0;
    while i < b.len() {
        if !word(b[i]) || (i > 0 && word(b[i - 1])) {
            i += 1;
            continue;
        }
        let e = run(b, i, word);
        let tok = &b[i..e];
        let mut hit = TOKEN_PREFIXES.iter().any(|(p, min, under)| {
            let p = p.as_bytes();
            tok.len() >= p.len() + min
                && tok.starts_with(p)
                && tok[p.len()..]
                    .iter()
                    .all(|c| c.is_ascii_alphanumeric() || (*under && (*c == b'_' || *c == b'-')))
        });
        hit |= tok.len() == 20
            && (tok.starts_with(b"AKIA") || tok.starts_with(b"ASIA"))
            && tok[4..]
                .iter()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
        hit |= tok.len() >= 55
            && tok.starts_with(b"nsec1")
            && tok[5..].iter().all(|c| BECH32.contains(c));
        let b58 = tok.iter().all(|c| BASE58.contains(c));
        hit |= b58
            && ((tok.len() == 51 && tok[0] == b'5')
                || (tok.len() == 52 && (tok[0] == b'K' || tok[0] == b'L')));
        if hit {
            out.push(Hit {
                start: i,
                end: e,
                kind: Kind::Redact,
                shape: "token",
            });
        }
        i = e;
    }
}

/// Values after a secret key name: `k=v`, `k: v`, `"k":"v"`, `k='v'`.
fn kv_hits(b: &[u8], out: &mut Vec<Hit>) {
    let id = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b'.';
    let mut i = 0;
    while i < b.len() {
        if !id(b[i]) || (i > 0 && id(b[i - 1])) {
            i += 1;
            continue;
        }
        let e = run(b, i, id);
        if !secret_key(&b[i..e]) {
            i = e;
            continue;
        }
        let to_line = line_key(&b[i..e]);
        let flag = b[i..e].starts_with(b"--");
        let mut j = e;
        if j < b.len() && (b[j] == b'"' || b[j] == b'\'') {
            j += 1;
        }
        let spaced = run(b, j, |c| c == b' ' || c == b'\t');
        if spaced < b.len() && (b[spaced] == b'=' || b[spaced] == b':') {
            // `k=v`, `k: v`, `k => v`.
            j = spaced + 1;
            if b[spaced] == b'=' && j < b.len() && b[j] == b'>' {
                j += 1;
            }
            j = run(b, j, |c| c == b' ' || c == b'\t');
        } else if flag && spaced > j && spaced < b.len() {
            // A CLI flag: `--password x`, `--mnemonic w1 w2 ...`.
            j = spaced;
        } else {
            i = e;
            continue;
        }
        let (vs, ve) = if j < b.len() && (b[j] == b'"' || b[j] == b'\'') {
            let q = b[j];
            let close = run(b, j + 1, |c| c != q);
            (j + 1, close)
        } else if to_line {
            let mut end = run(b, j, |c| c != b'\n' && c != b'\r');
            while end > j && (b[end - 1] == b' ' || b[end - 1] == b'\t') {
                end -= 1;
            }
            (j, end)
        } else {
            (
                j,
                run(b, j, |c| {
                    !c.is_ascii_whitespace() && !b",;&}])\"'".contains(&c)
                }),
            )
        };
        if ve > vs && &b[vs..ve] != REDACTED.as_bytes() && !b[vs..ve].starts_with(b"<hex64 ") {
            out.push(Hit {
                start: vs,
                end: ve,
                kind: Kind::Redact,
                shape: "secret value",
            });
        }
        i = ve.max(e);
    }
}

/// 64+ hex digits, bare or after `0x`.
fn hex_hits(b: &[u8], out: &mut Vec<Hit>) {
    let mut i = 0;
    while i < b.len() {
        if i > 0 && b[i - 1].is_ascii_alphanumeric() {
            i += 1;
            continue;
        }
        let prefixed = b[i] == b'0' && i + 1 < b.len() && (b[i + 1] == b'x' || b[i + 1] == b'X');
        let digits = if prefixed { i + 2 } else { i };
        let e = run(b, digits, |c| c.is_ascii_hexdigit());
        if e - digits >= 64 && (e == b.len() || !b[e].is_ascii_alphanumeric()) {
            out.push(Hit {
                start: i,
                end: e,
                kind: Kind::Hex,
                shape: "64-hex value",
            });
            i = e;
            continue;
        }
        i = e.max(i + 1);
    }
}

/// `local@domain.tld`.
fn email_hits(b: &[u8], out: &mut Vec<Hit>) {
    let local = |c: u8| c.is_ascii_alphanumeric() || b"._%+-".contains(&c);
    let dom = |c: u8| c.is_ascii_alphanumeric() || c == b'-' || c == b'.';
    for (at, _) in b.iter().enumerate().filter(|(_, c)| **c == b'@') {
        let mut s = at;
        while s > 0 && local(b[s - 1]) {
            s -= 1;
        }
        let e = run(b, at + 1, dom);
        let d = &b[at + 1..e];
        let d = d.strip_suffix(b".").unwrap_or(d);
        let tld_ok = d
            .iter()
            .rposition(|c| *c == b'.')
            .is_some_and(|p| p > 0 && d.len() - p > 2);
        if s < at && tld_ok {
            out.push(Hit {
                start: s,
                end: at + 1 + d.len(),
                kind: Kind::Redact,
                shape: "email",
            });
        }
    }
}

/// The whole value of a `Cookie` / `Set-Cookie` header (every cookie in it can be a session).
fn cookie_hits(b: &[u8], out: &mut Vec<Hit>) {
    let id = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'-';
    let mut i = 0;
    while i < b.len() {
        if !id(b[i]) || (i > 0 && id(b[i - 1])) {
            i += 1;
            continue;
        }
        let e = run(b, i, id);
        let name = b[i..e].to_ascii_lowercase();
        if name != b"cookie" && name != b"set-cookie" {
            i = e;
            continue;
        }
        let mut j = e;
        if j < b.len() && (b[j] == b'"' || b[j] == b'\'') {
            j += 1;
        }
        j = run(b, j, |c| c == b' ' || c == b'\t');
        if j >= b.len() || (b[j] != b':' && b[j] != b'=') {
            i = e;
            continue;
        }
        j = run(b, j + 1, |c| c == b' ' || c == b'\t');
        let (vs, ve) = if j < b.len() && (b[j] == b'"' || b[j] == b'\'') {
            let q = b[j];
            (j + 1, run(b, j + 1, |c| c != q))
        } else {
            (j, run(b, j, |c| c != b'\n' && c != b'\r'))
        };
        if ve > vs && &b[vs..ve] != REDACTED.as_bytes() {
            out.push(Hit {
                start: vs,
                end: ve,
                kind: Kind::Redact,
                shape: "cookie value",
            });
        }
        i = ve.max(e);
    }
}

fn hits(s: &str) -> Vec<Hit> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    cookie_hits(b, &mut out);
    auth_hits(b, &mut out);
    prefix_hits(b, &mut out);
    jwt_hits(b, &mut out);
    kv_hits(b, &mut out);
    hex_hits(b, &mut out);
    email_hits(b, &mut out);
    out.sort_by_key(|h| (h.start, std::cmp::Reverse(h.end)));
    out
}

fn hex_tag(v: &str) -> String {
    let v = v
        .trim_start_matches("0x")
        .trim_start_matches("0X")
        .to_ascii_lowercase();
    let d = sha2::Sha256::digest(v.as_bytes());
    format!("<hex64 {}>", &hex::encode(d)[..6])
}

/// Replace every secret shape in `s`. Idempotent. Every hit starts and ends on an ASCII byte, so
/// non-ASCII text around it is kept whole.
pub fn redact(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut at = 0;
    for h in hits(s) {
        if h.start < at {
            // Overlaps a hit already replaced: extend over it.
            if h.end > at {
                at = h.end;
            }
            continue;
        }
        out.push_str(&s[at..h.start]);
        match h.kind {
            Kind::Redact => out.push_str(REDACTED),
            Kind::Hex => out.push_str(&hex_tag(&s[h.start..h.end])),
        }
        at = h.end;
    }
    out.push_str(&s[at..]);
    out
}

/// The first secret shape left in `s`, if any (the bundle's final check).
pub fn first_shape(s: &str) -> Option<&'static str> {
    hits(s).first().map(|h| h.shape)
}

/// The fixture corpus: one line per shape, as a sidecar might print it, and the secret part that
/// must not survive. Every value is assembled at run time from parts, so no scanner reads the
/// source as holding a credential; none of them is a real key, token or password.
#[cfg(test)]
pub(crate) fn test_corpus() -> Vec<(&'static str, String, String)> {
    let opaque = ["opaqueSession", "Token1234567890"].concat();
    let jwt = [
        "eyJhbGciOiJSUzI1NiJ9",
        ".",
        "eyJzdWIiOiJ4In0",
        ".",
        "c2lnbmF0dXJlc2lnbmF0dXJl",
    ]
    .concat();
    let jwt2 = [
        "eyJhbGciOiJIUzI1NiJ9",
        ".",
        "eyJzdWIiOiJ4In0",
        ".",
        "abcdefghijklmnop",
    ]
    .concat();
    let jwt_plain = [
        "QUJDREVGR0hJSktMTU5PUA",
        ".",
        "UVJTVFVWV1hZWjAxMjM0NQ",
        ".",
        "c2lnbmF0dXJlLXNpZ25hdHVyZQ",
    ]
    .concat();
    let pw = ["hunter2", "hunter2"].concat();
    let api = ["3f9a8b7c6d5e", "4f3a2b1c0d9e8f7a6b5c"].concat();
    let basic = ["dXNlcjpwYX", "NzLXdvcmQ="].concat();
    let hex64 = "5e".repeat(32);
    let aws_secret = ["wJalrXUtnFEMI", "K7MDENGbPxRfiCY", "SYNTHETIC0"].concat();
    let akia = ["AKIA", "SYNTHETIC", "0000000"].concat();
    let ghp = ["ghp", "_", "SyntheticSyntheticSynthetic0123456789"].concat();
    let gpat = [
        "github",
        "_pat_",
        "11SYNTHETIC0_SyntheticSyntheticSynthetic",
    ]
    .concat();
    let xoxb = ["xox", "b-", "123456789012-1234567890123-SyntheticToken"].concat();
    let sk = ["sk", "-", "proj-SyntheticSyntheticSynthetic01"].concat();
    let b58 = "abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ123456789";
    let wif = ["5", &b58[..50]].concat();
    let wifk = ["K", &b58[1..52]].concat();
    let nsec = [
        "nsec",
        "1",
        &"qpzry9x8gf2tvdw0s3jn54khce6mua7l".repeat(2)[..58],
    ]
    .concat();
    let words = [
        "abandon ability able about above absent ",
        "absorb abstract absurd abuse access accident",
    ]
    .concat();
    vec![
        (
            "bearer header",
            format!("Authorization: Bearer {opaque}"),
            opaque.clone(),
        ),
        (
            "bearer lowercase",
            format!("auth=bearer {opaque} ok"),
            opaque.clone(),
        ),
        (
            "basic header",
            format!("Authorization: Basic {basic}"),
            basic.clone(),
        ),
        (
            "jwt in json",
            format!("{{\"id_token\":\"{jwt}\"}}"),
            jwt.clone(),
        ),
        (
            "jwt in a query",
            format!("GET /mcp?token={jwt2}&x=1"),
            jwt2.clone(),
        ),
        ("jwt after a word", format!("session={jwt}"), jwt.clone()),
        (
            "jwt-shaped, no eyJ",
            format!("t {jwt_plain}"),
            jwt_plain.clone(),
        ),
        ("password=", format!("password={pw} next"), pw.clone()),
        ("api_key:", format!("api_key: {api}"), api.clone()),
        (
            "api-key quoted",
            format!("\"x-api-key\": \"{api}\""),
            api.clone(),
        ),
        ("apiKey camel", format!("apiKey={api}"), api.clone()),
        (
            "secret single quotes",
            format!("client_secret='{pw}'"),
            pw.clone(),
        ),
        ("passwd", format!("PASSWD = {pw}"), pw.clone()),
        ("private_key", format!("private_key: {pw}"), pw.clone()),
        (
            "access_token",
            format!("access_token={opaque}"),
            opaque.clone(),
        ),
        (
            "mnemonic quoted",
            format!("mnemonic=\"{words}\""),
            words.clone(),
        ),
        ("seed", format!("seed: {pw}"), pw.clone()),
        ("bare 64-hex", format!("pk {hex64} loaded"), hex64.clone()),
        ("0x 64-hex", format!("key=0x{hex64}"), hex64.clone()),
        (
            "64-hex in json",
            format!("{{\"k\":\"{hex64}\"}}"),
            hex64.clone(),
        ),
        (
            "email",
            "contact member@example.com now".to_string(),
            "member@example.com".to_string(),
        ),
        // Round 3 (#613 review P1, lead).
        (
            "mnemonic unquoted, all words",
            format!("mnemonic: {words}"),
            words.clone(),
        ),
        ("seed= unquoted", format!("seed={words}"), words.clone()),
        (
            "seed_phrase =>",
            format!("seed_phrase => {words}"),
            words.clone(),
        ),
        ("seedPhrase:", format!("seedPhrase: {words}"), words.clone()),
        (
            "recovery_phrase quoted",
            format!("recovery_phrase=\"{words}\""),
            words.clone(),
        ),
        (
            "--mnemonic flag",
            format!("citrate wallet import --mnemonic {words}"),
            words.clone(),
        ),
        (
            "AWS_SECRET_ACCESS_KEY",
            format!("AWS_SECRET_ACCESS_KEY={aws_secret}"),
            aws_secret.clone(),
        ),
        (
            "secret_access_key:",
            format!("secret_access_key: {aws_secret}"),
            aws_secret.clone(),
        ),
        (
            "access_key_id",
            format!("access_key_id = {akia}"),
            akia.clone(),
        ),
        (
            "AKIA key id bare",
            format!("using {akia} for uploads"),
            akia.clone(),
        ),
        ("password =>", format!("password => {pw}"), pw.clone()),
        (
            "--password flag",
            format!("login --password {pw} --user m"),
            pw.clone(),
        ),
        ("pwd=", format!("pwd={pw} next"), pw.clone()),
        ("pass=", format!("pass={pw}"), pw.clone()),
        ("credentials=", format!("credentials={api}"), api.clone()),
        (
            "Authorization Token",
            format!("Authorization: Token {opaque}"),
            opaque.clone(),
        ),
        (
            "Authorization custom scheme",
            format!("authorization: Custom realm {opaque}"),
            opaque.clone(),
        ),
        (
            "Bearer: no space",
            format!("got Bearer:{opaque}"),
            opaque.clone(),
        ),
        ("ghp_ token", format!("token {ghp}"), ghp.clone()),
        ("github_pat_ token", format!("using {gpat}"), gpat.clone()),
        ("xoxb- token", format!("slack {xoxb}"), xoxb.clone()),
        ("sk- token", format!("model key {sk}"), sk.clone()),
        ("WIF private key", format!("wif {wif}"), wif.clone()),
        ("WIF compressed", format!("wif {wifk}"), wifk.clone()),
        ("nsec1 key", format!("nostr {nsec}"), nsec.clone()),
        (
            "cookie header",
            format!("Cookie: theme=dark; sid={opaque}"),
            opaque.clone(),
        ),
        (
            "set-cookie header",
            format!("Set-Cookie: session={opaque}; Path=/; HttpOnly"),
            opaque.clone(),
        ),
        (
            "set-cookie in json",
            format!("{{\"set-cookie\":\"sid={opaque}\"}}"),
            opaque.clone(),
        ),
        // Round 4 (#613 review P1, BerryManifold): a seed phrase by its value, whatever the key.
        (
            "seed phrase: (space)",
            format!("seed phrase: {words}"),
            words.clone(),
        ),
        (
            "recovery words =",
            format!("recovery words = {words}"),
            words.clone(),
        ),
        (
            "wallet words:",
            format!("wallet words: {words}"),
            words.clone(),
        ),
        (
            "backup_phrase quoted",
            format!("backup_phrase=\"{words}\""),
            words.clone(),
        ),
        ("passphrase=", format!("passphrase={pw}"), pw.clone()),
        (
            "bracketed word list",
            format!(
                "words: [{}]",
                words
                    .split(' ')
                    .map(|w| format!("\"{w}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            "absurd".to_string(),
        ),
        ("bare 12-word line", words.clone(), words.clone()),
        (
            "24-word phrase, upper case",
            format!("restore {} {}", words.to_uppercase(), words.to_uppercase()),
            words.to_uppercase(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    include!("secret_shapes_tests.rs");
}
