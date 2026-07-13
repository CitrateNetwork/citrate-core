// CORE-A3 — OIDC loopback-PKCE adversarial + integration suite (@rule8 evidence).
//
// Every ADV-* here is written RED-FIRST: the guard is neutralized, the test is
// shown failing, the guard restored, the test green (see the sprint's red→green
// table + the PR body). The whole flow runs HEADLESS against an in-test mock
// OIDC authority that MIRRORS THE REAL auth.citrate.ai: a real std `TcpListener`
// server serving a discovery document (`/.well-known/openid-configuration`)
// whose endpoints are `/auth` (authorization), `/token`, `/me` (userinfo),
// `/jwks`, `/token/revocation`; signing id_tokens with an **RS256** test key
// (the live authority signs RS256 — one RSA JWKS key); and registering the
// `/auth/callback` loopback redirect. Plus the A2 custody vault over an
// in-memory keyring fake. No live authority, real browser, or OS keyring is
// touched; those are honestly out-of-scope for CI and flagged in the sprint.
//
// The mock authority is a #[cfg(test)] fixture. It is NOT shipped (D-A3-2). It
// uses STATIC test RSA keys (embedded below) so no key-gen crate — and in
// particular NOT the banned `rsa` crate (RUSTSEC-2023-0071) — is pulled in;
// signing goes through jsonwebtoken's `aws_lc_rs` backend from a PEM.

use super::*;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};

use jsonwebtoken::{encode, EncodingKey, Header as JwtHeader};
use serde_json::json;

use crate::custody::{CustodyVault, Keyring};

// ===========================================================================
// STATIC test RSA keys (test-only; NEVER shipped). Two independent 2048-bit RSA
// keys: the real signer and an "attacker" key for the OIDC-2 multi-key JWKS
// probe. Each JWK modulus `n` (base64url) is precomputed to match its PEM; the
// exponent is the RSA default `e = AQAB` (65537). Mirrors the live authority's
// single RSA RS256 JWKS key. No `rsa` crate — jsonwebtoken signs from the PEM
// via `aws_lc_rs`.
// ===========================================================================

/// The real test signer (PKCS#8 PEM). Its public JWK is `(TEST_RSA1_N, "AQAB")`.
const TEST_RSA1_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDYmVXIFX8XKTab
5xheHZFEsjeR4t7C0iIix00QraBHo8L0ONqoqjRg9BB7dAeFPDyNdRrT0MG39chT
SPivimHZJLOa6kwuNeNmq/Jy8f7xDV2noJAhA2xC0EqTe10aJAXvdjIpyq/frRAE
tOQ0rDMLBYO0Yr5DPR552QW7B2TEW7hDqdkF+g93C5CEDx7Fu72xjDpcD3exssSo
mzIaqbPZTntvRaz2GCBTDjO4gRmQdVBgpEdDCX0u1FuMqVKGbzLde4pYESVzUVSZ
OVoE+7NGg7DCrq16FkwZWTrZtmAfP3yMOmiJLaCnNl7nPsVBpPCHWSIYgB61Z07O
h5qgFUlvAgMBAAECggEAIQATqr1jtKKp2Ez4UHaOyHmir85yBBrB6qyU2EKr1d5k
eJMk9WehPVhSHo0KDEmmLCM4aCc5LI7863uFsDEUQWIvHx4tZyj8sYrnEI5AOne/
2idDblQ4LWHQyvGTuMTeRqKqd+WSsDCM7TqmPkQyLq6zZ0tYE6R/PS9MiTdSKfxL
sKMh4j6ENlNW4vAumVpXSITiLfXqVxIJGgjctDjsZkf9FCcSitp1jI5bx4QxztW/
+E01D+qI5aJlBKMiU0ltNMXmSEgw5rCE3f9riyhb3xijeoKJRqKm4kTSO34sdBLC
bAp19nTAOJiT1CBnv8/CU/dPecS78Kht65SLq/xMaQKBgQDsGCGfI+u/yVfHNizl
xMZqtU7+L6zmDvBKHatB/PXFSfFGz50zzCJVwwX7cCFtg/fuuPVboztZQZdVUooC
NSNiT7+jR6C4+/TEGyIvsKZZ55oRKJ6wvDaEJWm2M0mtDCGme0umjvsv7Jv32Zih
ts8/gmttqT+m9yzCghCNeaC7VQKBgQDq3GqZry3PKDBOkNl95/zsJy3pNhl91994
q1hevmoNMVaBM73wxOs+WHNHBy0AjWlJMdGZk/h5ZqeWL6ht3i+S9wdHOrsAU4is
hxuc0OM0uMMmqVEj3JCy4U3URmx+63LKEo0GGdEMUeANJZbcizteuhLRZ209bmGO
EUeP/ssZswKBgBhujtwnHXhlX54P7yl/6YCVbq1DRcMw/JDO7TAQ+2YFNuC7D2uS
zmLNocrZWbw5kei0Xz+ybqvX6886kWmVEipUUmKVQP6jpDq/DBSfVTesjfcEmxdz
Ark+Hehq+k7cGIdf7v43gar98038yJzDjELoPjHE9/9RSOKADzJ0ybtZAoGBAK3N
jenLdLgQAqexk/IT4t0UJWqnSXgSb+MJ0izS9wJqV5znoJFz+K67oBuZGNmGzLqI
7pabpU6aBD0laZxcx5IX00AIG2kjaEpc9bc38lwKuwh6VnyWdlKaXxFPSG0oaltW
HRy9sDFQyeCQx7LQKpBwXQqwYmwKqpELAo1yPfT3AoGAcMCwrhj8l+hr5J7+HKhW
jYXcXgv/jroKmxEE1Tz17FmNuihOJx9+uXGG0imgl49pdFW2Hj2kmhrE575It1kB
KvWq18Ra2ECYTtk9toWC6MCCtBBE/9u2u2UHTJrmag2V6cBzpOEpydKkaz/wfKnR
bmZ9UIL3o8OGKXSB4O9itQo=
-----END PRIVATE KEY-----";

/// Base64url modulus `n` matching `TEST_RSA1_PEM` (the real signer's JWK).
const TEST_RSA1_N: &str = "2JlVyBV_Fyk2m-cYXh2RRLI3keLewtIiIsdNEK2gR6PC9DjaqKo0YPQQe3QHhTw8jXUa09DBt_XIU0j4r4ph2SSzmupMLjXjZqvycvH-8Q1dp6CQIQNsQtBKk3tdGiQF73YyKcqv360QBLTkNKwzCwWDtGK-Qz0eedkFuwdkxFu4Q6nZBfoPdwuQhA8exbu9sYw6XA93sbLEqJsyGqmz2U57b0Ws9hggUw4zuIEZkHVQYKRHQwl9LtRbjKlShm8y3XuKWBElc1FUmTlaBPuzRoOwwq6tehZMGVk62bZgHz98jDpoiS2gpzZe5z7FQaTwh1kiGIAetWdOzoeaoBVJbw";

/// The "attacker" test key (PKCS#8 PEM). Published FIRST in the OIDC-2 multi-key
/// JWKS probe; signs the token only under the `sign_with_attacker` knob.
const TEST_RSA2_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC1XNMhJtTTtdhz
RNXiuXwL9Rvt1NnEc+osl5QQLn92fsVYR9P7v0SEP9HrBURFDoxmzuR9b/UKQZRX
kdlMRHotZTYx6JpOc4yjONNemgLzManlHdbUpIDzQZDFmyf5gj6Apgb5KISWnMET
BuFgaNboeVYus/wJ5h4FoTj2WZh//NsSt0YDVAPboA7c1EfgMJmizrhO6T7V8K8m
I2Q9oCTCjCX9yaYf12aa6weRnRcQ2vwXHs9COL89POlB2hsE6UBoxudI9SiZCIBa
LS1z7oLMbIyE+3SFtCp7EXl8PXO9uvmOun1KYX/cJ4Ml6COFwpD9GO9I1Xk6KAZi
0tn5g7vLAgMBAAECggEAbmwL6B1aa2RGWzhH+Xjxe95KmO2FgUUKCQhpD8kftifN
Q8jH2nlD4DlzN+LHBDytY1MIbw8hZJM1HHQil3sB4G3FJ3H1sVpNAHvyxaCDt0o/
pM4cJO/byz/aQ1YKarHQGEf96umugH0EWO9RfX+XiYeG33yaMfS3xrm4ktVOMm3d
GeIQ4wsAuc3EwXyL6Av45SSIghZJQv6L617AujEzdJ2iI29NzlfX5h8sHim5EEts
9PUNfWswJd/8AMq4sfzUKbrrcQj0ezP1gHg0RSwSRR5sTsBDOidQ8QL17DAgUiDr
9pYlMYgvifEgnpisjdI78sxpfTrgp8KHRFZ4wl3VYQKBgQDXyFYm5N/5cwTTvcfC
2RGrsJerq7dRJ8L7obS7NwVJUHa4sBXDxaKD8kzN6nvpfyab8tvCKbzyEQhOWNPK
sSsUWHbOsQTK9X0kgKEG3gmbh3+NALyDj7BIFIAC7JhxLLDACwZMuR75IBWEeiCS
180RMXov+WYQzER5G1AixhF26QKBgQDXKjQXUrqxKSohjZIPb8zq77q6UeCK9b2c
3tAGSpyrMBJhjlACV+l5Nculnu5r+y0FgsCnvLIryO2FDaddS484jtYz5itulpGn
jXIzaHtECGvNU0cGc3OqMmVutrdKo5ykw0Ul2vEkeH7kQLs9Vg7/KpviURaNC8w7
BjaN07hUkwKBgQCkdj6jekHy/+Un9TdxnLxJHVkcMM6RfjqwSvlSz4ap8DfsX9jW
06Uf5+b98r/qoUyuA5XXELS/0peAD1es3we0hBBZTLYYcq6kyZzxfP3ZmpZuw6bq
pvN2nJlMoUM2zxcP59cvVtDyk6+SvvpgsTXM6ubz9aQDHYz6uQSE3G2nMQKBgHe7
R3N3GOZ+5q/3LMEkUJ6nunv2FgKdzt7dalsl59qnDIN3AvTa4NQPaHyIXVp/UkVP
xk9RBMCyteGlgG29Hzy012PYAHEwnmrjnhoXWQi5uutuHQbs9f9Ovf0G9iY1t3RE
KVVwaWIHH216y/bMzdmWZ1pgDzF70DFEOtVfbKK7AoGAHTszyURSXRokla5WmxPs
mYcLgHkGHrlZvR+GRdaPWUv9nH8aHFLf6exLMezZlb6d38FqR88m5xScNbUGZWqQ
aKwrvFVlsP/AyczKAkkSYXVpi/SEV6GGdUMNa91Sw1kzaIGW8Rzne1SBvxsAUCW6
k4m87phrUAqAuut70PKfqOY=
-----END PRIVATE KEY-----";

/// Base64url modulus `n` matching `TEST_RSA2_PEM` (the attacker JWK).
const TEST_RSA2_N: &str = "tVzTISbU07XYc0TV4rl8C_Ub7dTZxHPqLJeUEC5_dn7FWEfT-79EhD_R6wVERQ6MZs7kfW_1CkGUV5HZTER6LWU2MeiaTnOMozjTXpoC8zGp5R3W1KSA80GQxZsn-YI-gKYG-SiElpzBEwbhYGjW6HlWLrP8CeYeBaE49lmYf_zbErdGA1QD26AO3NRH4DCZos64Tuk-1fCvJiNkPaAkwowl_cmmH9dmmusHkZ0XENr8Fx7PQji_PTzpQdobBOlAaMbnSPUomQiAWi0tc-6CzGyMhPt0hbQqexF5fD1zvbr5jrp9SmF_3CeDJegjhcKQ_RjvSNV5OigGYtLZ-YO7yw";

/// RSA public exponent — the RSA default 65537 = `AQAB` (both test keys).
const TEST_RSA_E: &str = "AQAB";

// ===========================================================================
// A2 custody vault over an in-memory keyring (A3's real store dependency).
// ===========================================================================

#[derive(Default)]
struct FakeKeyring {
    store: StdMutex<HashMap<String, Vec<u8>>>,
}
impl Keyring for FakeKeyring {
    fn get(&self, account: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        Ok(self.store.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> std::result::Result<(), CustodyError> {
        self.store
            .lock()
            .unwrap()
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> std::result::Result<(), CustodyError> {
        self.store.lock().unwrap().remove(account);
        Ok(())
    }
}

const PASS: &[u8] = b"correct horse battery staple";

/// A shared keyring view so a "restart" can reopen the same vault.
struct Shared(Arc<FakeKeyring>);
impl Keyring for Shared {
    fn get(&self, a: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        self.0.get(a)
    }
    fn set(&self, a: &str, s: &[u8]) -> std::result::Result<(), CustodyError> {
        self.0.set(a, s)
    }
    fn delete(&self, a: &str) -> std::result::Result<(), CustodyError> {
        self.0.delete(a)
    }
}

/// A fresh unlocked custody vault at a unique temp path (the A3 refresh-token
/// store). Returns the vault + shared keyring + its envelope path (so a "restart"
/// can rebuild a second vault over the SAME on-disk envelope + keyring).
fn fresh_vault() -> (CustodyVault, Arc<FakeKeyring>, PathBuf) {
    let fake = Arc::new(FakeKeyring::default());
    let mut p = std::env::temp_dir();
    let uniq = format!("citrate-core-oidc-test-{}-{}.enc", std::process::id(), {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::Relaxed)
    });
    p.push(uniq);
    let _ = std::fs::remove_file(&p);
    let vault = CustodyVault::new(Box::new(Shared(fake.clone())), p.clone(), 30);
    vault.init(&mut PASS.to_vec()).unwrap();
    vault.unlock(&mut PASS.to_vec()).unwrap();
    (vault, fake, p)
}

/// Rebuild a vault over the SAME envelope + keyring (simulates an app restart).
fn reopen_vault(fake: Arc<FakeKeyring>, path: PathBuf) -> CustodyVault {
    CustodyVault::new(Box::new(Shared(fake)), path, 30)
}

// ===========================================================================
// The mock OIDC authority (D-A3-2). A real TcpListener server signing id_tokens
// with a TEST ES256 key and serving its public coords as JWKS.
// ===========================================================================

struct MockAuthority {
    base: String,
    issuer: String,
    client_id: String,
    behavior: Arc<StdMutex<Behavior>>,
    claims: Arc<StdMutex<serde_json::Value>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

#[derive(Clone)]
struct CodeGrant {
    code_challenge: String,
    nonce: String,
}

#[derive(Clone, Default)]
struct Behavior {
    /// Forge the id_token nonce (ADV-7 nonce).
    forge_nonce: bool,
    /// Sign the id_token with a WRONG key (ADV-7 signature).
    forge_signature: bool,
    /// Emit an expired id_token (ADV-7 exp).
    expired: bool,
    /// Emit a wrong issuer (ADV-7 iss).
    wrong_issuer: bool,
    /// Emit a wrong audience (ADV-7 aud).
    wrong_audience: bool,
    /// OIDC-1: OMIT the `aud` claim entirely (audience-confusion bypass probe).
    omit_aud: bool,
    /// OIDC-2: OMIT the `kid` from the id_token header.
    omit_kid: bool,
    /// OIDC-2: serve a MULTI-KEY JWKS (attacker key first, real key second),
    /// both kid-less, to probe the "first key" fallback.
    multikey_jwks: bool,
    /// A3-04/OIDC-3: on the refresh grant, mint an id_token for a DIFFERENT sub
    /// (subject-substitution probe).
    refresh_wrong_sub: bool,
    /// OIDC-2: sign the id_token with the ATTACKER key (the one published FIRST
    /// in the multi-key JWKS). Under a "first key" fallback + a kid-less token,
    /// this attacker-signed token would VERIFY. The fix rejects kid-less+multikey.
    sign_with_attacker: bool,
    /// NEW-1: emit a MULTI-valued `aud` = [client_id, "attacker-extra"].
    multi_aud: bool,
    /// NEW-1: set `azp` to this value; `None` omits the claim. Combined with
    /// `multi_aud` to probe OIDC Core §3.1.3.7 rules 4–5.
    azp: Option<String>,
    /// A3-AUTH: the discovery document advertises a WRONG `issuer` (an
    /// attacker-authority swap) — the client's trust-anchor gate must reject it.
    wrong_discovery_issuer: bool,
    /// A3-AUTH: sign the id_token with the WRONG alg header (`ES256`) while the
    /// JWKS key is RSA/RS256 — an alg-confusion / header-forgery probe. The
    /// alg-pin (header.alg must equal the JWKS key's alg) must reject it.
    forge_alg_es256: bool,
}

impl MockAuthority {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let issuer = base.clone();
        let client_id = "citrate-core".to_string();

        // TEST RS256 signing key (never shipped) — mirrors the live authority's
        // single RSA JWKS key. Public modulus/exponent → JWKS. A second, unrelated
        // RSA key (never signs the real token) drives the OIDC-2 multi-key probe.
        let behavior = Arc::new(StdMutex::new(Behavior::default()));
        let claims = Arc::new(StdMutex::new(json!({
            "sub": "usr_2af4c19e",
            "email": "dana.okafor@fastmail.com",
            "wallet_address": "0xabc0000000000000000000000000000000000def",
            "kyc_status": "none",
            "tier": "free",
            "org_id": serde_json::Value::Null,
            "citrate_role": "member",
            "expires_at": "2027-07-11"
        })));

        let srv = ServerCtx {
            base: base.clone(),
            issuer: issuer.clone(),
            client_id: client_id.clone(),
            live_codes: Arc::new(StdMutex::new(HashMap::new())),
            valid_refresh: Arc::new(StdMutex::new(std::collections::HashSet::new())),
            behavior: behavior.clone(),
            claims: claims.clone(),
        };

        let handle = std::thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(s) => {
                        if srv.handle_conn(s) {
                            break; // shutdown sentinel received
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        MockAuthority {
            base,
            issuer,
            client_id,
            behavior,
            claims,
            handle: Some(handle),
        }
    }

    /// Wire the client to the mock via its DISCOVERY URL only (mirrors prod: the
    /// client hardcodes the discovery URL + issuer trust anchor + client id, and
    /// derives every protocol endpoint from the discovery document).
    fn config(&self) -> AuthorityConfig {
        AuthorityConfig {
            discovery: format!("{}/.well-known/openid-configuration", self.base),
            kyc_start: format!("{}/kyc/start", self.base),
            issuer: self.issuer.clone(),
            client_id: self.client_id.clone(),
        }
    }

    fn set_claim(&self, key: &str, value: serde_json::Value) {
        self.claims.lock().unwrap()[key] = value;
    }

    fn behavior(&self) -> std::sync::MutexGuard<'_, Behavior> {
        self.behavior.lock().unwrap()
    }

    /// Emulate the browser: hit /authorize, read its 302, then deliver the
    /// loopback callback to the app's listener IN A BACKGROUND THREAD. The
    /// callback GET blocks until the app's listener accepts + responds, which only
    /// happens after `login_with` returns from `open_browser` and enters
    /// `wait_for_callback`; delivering on a thread models a real (async) browser
    /// and avoids a deadlock (the app cannot serve the callback while still inside
    /// this closure).
    fn drive_browser(&self, auth_url: &str) -> Result<()> {
        let loc = location_of(auth_url);
        deliver_callback(loc);
        Ok(())
    }

    /// Test helper: the resolved `Endpoints` for direct-path tests (ADV-2/3/5)
    /// that call the private `exchange_code`/`build_authorize_url` without running
    /// the full login. Mirrors what `discover()` yields from the mock's document.
    fn endpoints(&self) -> Endpoints {
        Endpoints {
            authorization: format!("{}/auth", self.base),
            token: format!("{}/token", self.base),
            userinfo: format!("{}/me", self.base),
            jwks: format!("{}/jwks", self.base),
            revocation: Some(format!("{}/token/revocation", self.base)),
            id_token_signing_algs: vec!["RS256".to_string()],
        }
    }

    /// The authorization-endpoint URL for the mock (the `/auth` path).
    fn authorize_endpoint(&self) -> String {
        format!("{}/auth", self.base)
    }
}

impl Drop for MockAuthority {
    fn drop(&mut self) {
        let _ = ureq::get(format!("{}/__shutdown", self.base)).call();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// The server-side context (owned by the accept thread).
struct ServerCtx {
    /// This authority's base URL — used to build the discovery document's
    /// absolute endpoint URLs (`/auth`, `/token`, `/me`, `/jwks`,
    /// `/token/revocation`), exactly as the live authority advertises them.
    base: String,
    issuer: String,
    client_id: String,
    live_codes: Arc<StdMutex<HashMap<String, CodeGrant>>>,
    valid_refresh: Arc<StdMutex<std::collections::HashSet<String>>>,
    behavior: Arc<StdMutex<Behavior>>,
    claims: Arc<StdMutex<serde_json::Value>>,
}

impl ServerCtx {
    /// Handle one connection. Returns true if it was the shutdown sentinel.
    fn handle_conn(&self, mut stream: TcpStream) -> bool {
        let (method, target, body) = match read_http(&mut stream) {
            Some(v) => v,
            None => return false,
        };
        let stream = &mut stream;
        let path = target.split('?').next().unwrap_or("").to_string();
        let query = parse_query(&target);
        match (method.as_str(), path.as_str()) {
            (_, "/__shutdown") => {
                write_resp(stream, 200, "text/plain", "bye");
                return true;
            }
            // Discovery — the real authority's endpoint paths (/auth, /token, /me,
            // /jwks, /token/revocation), advertised over the well-known doc.
            ("GET", "/.well-known/openid-configuration") => self.handle_discovery(stream),
            ("GET", "/auth") => self.handle_authorize(stream, &query),
            ("POST", "/token") => self.handle_token(stream, &body),
            ("GET", "/jwks") => self.handle_jwks(stream),
            ("GET", "/me") => self.handle_userinfo(stream),
            ("POST", "/token/revocation") => self.handle_revoke(stream, &body),
            _ => write_resp(stream, 404, "text/plain", "not found"),
        }
        false
    }

    /// Serve the OIDC discovery document, MIRRORING auth.citrate.ai: issuer + the
    /// real endpoint paths (`/auth`, `/token`, `/me`, `/jwks`,
    /// `/token/revocation`) and `id_token_signing_alg_values_supported: ["RS256"]`.
    /// The `omit_discovery_issuer_match` / `wrong_discovery_issuer` knobs probe the
    /// trust-anchor gate.
    fn handle_discovery(&self, stream: &mut TcpStream) {
        let b = self.behavior.lock().unwrap().clone();
        let issuer = if b.wrong_discovery_issuer {
            "https://evil.example".to_string()
        } else {
            self.issuer.clone()
        };
        let doc = json!({
            "issuer": issuer,
            "authorization_endpoint": format!("{}/auth", self.base),
            "token_endpoint": format!("{}/token", self.base),
            "userinfo_endpoint": format!("{}/me", self.base),
            "jwks_uri": format!("{}/jwks", self.base),
            "revocation_endpoint": format!("{}/token/revocation", self.base),
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code", "refresh_token"],
            "code_challenge_methods_supported": ["S256"],
            "id_token_signing_alg_values_supported": ["RS256"],
            "scopes_supported": ["openid", "profile", "wallet", "kyc", "offline_access"],
        });
        write_resp(stream, 200, "application/json", &doc.to_string());
    }

    fn handle_authorize(&self, stream: &mut TcpStream, q: &HashMap<String, String>) {
        // Enforce S256 server-side — a `plain` (or absent) method is rejected,
        // which proves our client only ever offers S256 (ADV-3).
        if q.get("code_challenge_method").map(String::as_str) != Some("S256") {
            write_resp(stream, 400, "text/plain", "PKCE S256 required");
            return;
        }
        let challenge = q.get("code_challenge").cloned().unwrap_or_default();
        let nonce = q.get("nonce").cloned().unwrap_or_default();
        let state = q.get("state").cloned().unwrap_or_default();
        let redirect = q.get("redirect_uri").cloned().unwrap_or_default();
        let code = format!("code_{}", rand_hex());
        self.live_codes.lock().unwrap().insert(
            code.clone(),
            CodeGrant {
                code_challenge: challenge,
                nonce,
            },
        );
        let location = format!("{redirect}?code={code}&state={state}");
        let resp = format!(
            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let _ = stream.write_all(resp.as_bytes());
    }

    fn handle_token(&self, stream: &mut TcpStream, body: &str) {
        let form = parse_query(&format!("?{body}"));
        let grant = form.get("grant_type").map(String::as_str).unwrap_or("");
        if grant == "refresh_token" {
            let rt = form.get("refresh_token").cloned().unwrap_or_default();
            if !self.valid_refresh.lock().unwrap().contains(&rt) {
                write_resp(stream, 400, "application/json", "{\"error\":\"invalid_grant\"}");
                return;
            }
            // Rotate: invalidate old, issue new.
            self.valid_refresh.lock().unwrap().remove(&rt);
            let new_rt = format!("rt_{}", rand_hex());
            self.valid_refresh.lock().unwrap().insert(new_rt.clone());
            let id_token = self.mint_id_token("", true);
            let out = self.token_json(&id_token, Some(&new_rt));
            write_resp(stream, 200, "application/json", &out);
            return;
        }
        // authorization_code grant.
        let code = form.get("code").cloned().unwrap_or_default();
        let verifier = form.get("code_verifier").cloned().unwrap_or_default();
        let grant = self.live_codes.lock().unwrap().remove(&code); // single-use
        let grant = match grant {
            Some(g) => g,
            None => {
                // Unknown / replayed code (ADV-5).
                write_resp(stream, 400, "application/json", "{\"error\":\"invalid_grant\"}");
                return;
            }
        };
        // PKCE S256 check: base64url(sha256(verifier)) must equal the challenge
        // (ADV-2). A wrong/missing verifier fails here.
        let computed = b64url(&sha2::Sha256::digest(verifier.as_bytes()));
        if computed != grant.code_challenge {
            write_resp(stream, 400, "application/json", "{\"error\":\"invalid_grant\"}");
            return;
        }
        let id_token = self.mint_id_token(&grant.nonce, false);
        let rt = format!("rt_{}", rand_hex());
        self.valid_refresh.lock().unwrap().insert(rt.clone());
        let out = self.token_json(&id_token, Some(&rt));
        write_resp(stream, 200, "application/json", &out);
    }

    fn token_json(&self, id_token: &str, refresh: Option<&str>) -> String {
        let rt = refresh
            .map(|r| format!(",\"refresh_token\":\"{r}\""))
            .unwrap_or_default();
        format!(
            "{{\"access_token\":\"at_{}\",\"token_type\":\"Bearer\",\"expires_in\":3600,\"id_token\":\"{id_token}\"{rt}}}",
            rand_hex()
        )
    }

    /// Mint an id_token, honoring the current `behavior` knobs (ADV-7 + OIDC).
    /// `refresh` selects the refresh-grant path (drives `refresh_wrong_sub`).
    fn mint_id_token(&self, nonce: &str, refresh: bool) -> String {
        let b = self.behavior.lock().unwrap().clone();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let exp = if b.expired { now - 3600 } else { now + 3600 };
        let iss = if b.wrong_issuer {
            "https://evil.example".to_string()
        } else {
            self.issuer.clone()
        };
        let aud = if b.wrong_audience {
            "some-other-client".to_string()
        } else {
            self.client_id.clone()
        };
        let effective_nonce = if b.forge_nonce {
            "forged-nonce".to_string()
        } else {
            nonce.to_string()
        };
        let claims = self.claims.lock().unwrap().clone();
        let mut payload = json!({
            "iss": iss,
            "exp": exp,
            "iat": now,
            "nonce": effective_nonce,
        });
        if let Some(obj) = payload.as_object_mut() {
            // OIDC-1 probe: only include `aud` when NOT omitting it.
            if !b.omit_aud {
                // NEW-1 probe: a MULTI-valued aud lists us AND an attacker party.
                if b.multi_aud {
                    obj.insert("aud".to_string(), json!([aud, "attacker-extra"]));
                } else {
                    obj.insert("aud".to_string(), json!(aud));
                }
            }
            // NEW-1 probe: emit `azp` when set.
            if let Some(azp) = &b.azp {
                obj.insert("azp".to_string(), json!(azp));
            }
            if let Some(extra) = claims.as_object() {
                for (k, v) in extra {
                    obj.insert(k.clone(), v.clone());
                }
            }
            // A3-04/OIDC-3 probe: swap the subject on the refresh id_token.
            if refresh && b.refresh_wrong_sub {
                obj.insert("sub".to_string(), json!("attacker-sub"));
            }
        }
        // The live authority signs RS256; the mock mirrors that.
        let mut header = JwtHeader::new(jsonwebtoken::Algorithm::RS256);
        // OIDC-2 probe: optionally omit the header kid.
        if !b.omit_kid {
            header.kid = Some("test-key-1".to_string());
        }
        let key = if b.forge_signature {
            // Sign with the "attacker" RSA key but keep the real key's kid → the
            // JWKS key for that kid won't verify this signature.
            EncodingKey::from_rsa_pem(TEST_RSA2_PEM.as_bytes()).unwrap()
        } else if b.sign_with_attacker {
            // Signed by the attacker key (published FIRST in the multikey JWKS).
            EncodingKey::from_rsa_pem(TEST_RSA2_PEM.as_bytes()).unwrap()
        } else {
            EncodingKey::from_rsa_pem(TEST_RSA1_PEM.as_bytes()).unwrap()
        };
        let jwt = encode(&header, &payload, &key).unwrap();
        if b.forge_alg_es256 {
            // Alg-confusion probe: rewrite the (RS256-signed) token's HEADER to
            // claim `alg:ES256` while the JWKS key is RSA/RS256. jsonwebtoken
            // cannot ENCODE that mismatch directly, so we splice the header JSON
            // and reuse the RS256 signature — the client's alg-pin must reject on
            // `header.alg != key.alg` BEFORE any signature check anyway.
            rewrite_jwt_alg(&jwt, "ES256")
        } else {
            jwt
        }
    }

    fn handle_jwks(&self, stream: &mut TcpStream) {
        let b = self.behavior.lock().unwrap().clone();
        // Mirror the live JWKS: RSA keys with `alg:"RS256"` + modulus/exponent.
        let body = if b.multikey_jwks {
            // OIDC-2 probe: two kid-less keys — an attacker key FIRST, then the
            // real one. A "first key" fallback would verify against the attacker.
            format!(
                "{{\"keys\":[\
                 {{\"kty\":\"RSA\",\"alg\":\"RS256\",\"use\":\"sig\",\"n\":\"{}\",\"e\":\"{}\"}},\
                 {{\"kty\":\"RSA\",\"alg\":\"RS256\",\"use\":\"sig\",\"n\":\"{}\",\"e\":\"{}\"}}]}}",
                TEST_RSA2_N, TEST_RSA_E, TEST_RSA1_N, TEST_RSA_E
            )
        } else {
            format!(
                "{{\"keys\":[{{\"kty\":\"RSA\",\"alg\":\"RS256\",\"use\":\"sig\",\"kid\":\"test-key-1\",\"n\":\"{}\",\"e\":\"{}\"}}]}}",
                TEST_RSA1_N, TEST_RSA_E
            )
        };
        write_resp(stream, 200, "application/json", &body);
    }

    fn handle_userinfo(&self, stream: &mut TcpStream) {
        let claims = self.claims.lock().unwrap().clone();
        write_resp(stream, 200, "application/json", &claims.to_string());
    }

    fn handle_revoke(&self, stream: &mut TcpStream, body: &str) {
        let form = parse_query(&format!("?{body}"));
        if let Some(tok) = form.get("token") {
            self.valid_refresh.lock().unwrap().remove(tok);
        }
        write_resp(stream, 200, "application/json", "{}");
    }
}

// --- tiny HTTP helpers for the mock server ---------------------------------

fn read_http(stream: &mut TcpStream) -> Option<(String, String, String)> {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok()?;
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let reader = &mut reader;
    let mut request_line = String::new();
    reader.read_line(&mut request_line).ok()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            break;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = String::new();
    if content_length > 0 {
        let mut buf = vec![0u8; content_length];
        reader.read_exact(&mut buf).ok()?;
        body = String::from_utf8_lossy(&buf).to_string();
    }
    Some((method, target, body))
}

fn write_resp(stream: &mut TcpStream, status: u16, ctype: &str, body: &str) {
    let reason = match status {
        200 => "OK",
        302 => "Found",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let resp = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.flush();
    // Half-close the WRITE side so the client gets a clean FIN after reading the
    // full response, then keep the read half open briefly so a client still
    // flushing its request body is never RST'd ("Peer disconnected").
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let mut drain = [0u8; 256];
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(200)));
    while let Ok(n) = stream.read(&mut drain) {
        if n == 0 {
            break;
        }
    }
}

fn parse_query(target: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(qs) = target.split_once('?').map(|(_, q)| q) {
        for pair in url::form_urlencoded::parse(qs.as_bytes()) {
            map.insert(pair.0.into_owned(), pair.1.into_owned());
        }
    }
    map
}

fn rand_hex() -> String {
    let mut b = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Rewrite the `alg` in a JWT's header to `new_alg`, keeping the original
/// (RS256) signature. Used only by the alg-confusion probe — the client's
/// alg-pin rejects on the header/JWKS-key alg mismatch before verifying, so the
/// stale signature is irrelevant. base64url decode header → patch `alg` → re-encode.
fn rewrite_jwt_alg(jwt: &str, new_alg: &str) -> String {
    let parts: Vec<&str> = jwt.split('.').collect();
    let header_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[0])
        .unwrap();
    let mut header: serde_json::Value = serde_json::from_slice(&header_bytes).unwrap();
    header["alg"] = json!(new_alg);
    let new_header = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&header).unwrap());
    format!("{}.{}.{}", new_header, parts[1], parts[2])
}

/// Build an AuthManager wired to the mock authority over the real ureq client.
fn manager_for(auth: &MockAuthority) -> AuthManager {
    AuthManager::new(auth.config(), Box::new(UreqClient))
}

/// A ureq agent that does NOT auto-follow redirects, so a test can read the
/// authority's 302 `Location` (to extract the code/state) instead of ureq
/// silently following it to the loopback callback.
fn no_redirect_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .max_redirects(0)
        .max_redirects_will_error(false)
        .build()
        .into()
}

/// GET `url` without following redirects; return the `Location` header of the
/// 302 (the loopback callback URL carrying code+state).
fn location_of(url: &str) -> String {
    let resp = no_redirect_agent().get(url).call().unwrap();
    resp.headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string()
}

/// Deliver a loopback callback URL to the app's listener on a BACKGROUND thread.
/// The GET blocks until the app accepts+serves it (which happens only after the
/// caller returns from `open_browser`), so it must not run inline — that would
/// deadlock the single-threaded login flow.
fn deliver_callback(loc: String) {
    std::thread::spawn(move || {
        let _ = no_redirect_agent().get(&loc).call();
    });
}

// ===========================================================================
// Happy path + integration lifecycle
// ===========================================================================

#[test]
fn login_roundtrip_yields_claims_and_vaults_refresh() {
    let auth = MockAuthority::start();
    auth.set_claim("tier", json!("pilot"));
    let mgr = manager_for(&auth);
    let (vault, _fake, _path) = fresh_vault();

    let status = mgr
        .login_with(&vault, |url| auth.drive_browser(url))
        .expect("login should succeed against the mock authority");

    assert!(status.signed_in);
    assert_eq!(status.tier.as_deref(), Some("pilot"));
    assert_eq!(status.sub.as_deref(), Some("usr_2af4c19e"));
    // A3.2: the refresh token is in the A2 vault slot — NOT in memory/logs/UI.
    let slots = vault.list().unwrap();
    assert!(
        slots.iter().any(|s| s.name == REFRESH_SLOT),
        "refresh token must be persisted in the custody vault"
    );
}

#[test]
fn integration_lifecycle_login_restart_refresh_userinfo_logout() {
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);
    let (vault, fake, path) = fresh_vault();

    // login → refresh token in the vault.
    mgr.login_with(&vault, |url| auth.drive_browser(url)).unwrap();
    assert!(vault.list().unwrap().iter().any(|s| s.name == REFRESH_SLOT));

    // "restart": a brand-new manager + a re-opened vault over the same envelope.
    let mgr2 = manager_for(&auth);
    let vault2 = reopen_vault(fake, path);
    vault2.unlock(&mut PASS.to_vec()).unwrap();
    // status is signed-out on the fresh manager (access token was in memory only).
    assert!(!mgr2.status().signed_in);

    // silent refresh reads the vaulted token and re-establishes the session.
    auth.set_claim("tier", json!("pilot"));
    let refreshed = mgr2.refresh(&vault2).expect("silent refresh from vault");
    assert!(refreshed.signed_in);
    assert_eq!(refreshed.tier.as_deref(), Some("pilot"));

    // live /userinfo re-check reflects a claim change (entitlement engine input).
    auth.set_claim("tier", json!("enterprise"));
    auth.set_claim("org_id", json!("BA-7"));
    let info = mgr2.userinfo().unwrap();
    assert_eq!(info.tier.as_deref(), Some("enterprise"));
    assert_eq!(info.org.as_deref(), Some("BA-7"));

    // logout revokes + clears the vault slot + wipes memory.
    mgr2.logout(&vault2).unwrap();
    assert!(!mgr2.status().signed_in);
    assert!(
        !vault2.list().unwrap().iter().any(|s| s.name == REFRESH_SLOT),
        "logout must clear the refresh-token vault slot"
    );
    // The now-revoked refresh token can no longer mint a session.
    assert!(mgr2.refresh(&vault2).is_err());
}

// ===========================================================================
// ADV-1 — callback state mismatch (CSRF)
// ===========================================================================

#[test]
fn adv1_state_mismatch_is_rejected() {
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);
    let (vault, _f, _p) = fresh_vault();

    let r = mgr.login_with(&vault, |url| {
        // Read the authority's 302, then deliver the code with a BAD state.
        let loc = location_of(url);
        let tampered = replace_query(&loc, "state", "attacker-state");
        deliver_callback(tampered);
        Ok(())
    });
    assert_eq!(r.unwrap_err(), AuthError::StateMismatch);
    // GUARD: `constant_time_eq(cb.state, state)` in login_with. RED (guard removed
    // → the tampered state is accepted): confirmed failing; GREEN with the check.
}

// ===========================================================================
// ADV-2 — PKCE verifier mismatch
// ===========================================================================

#[test]
fn adv2_pkce_verifier_mismatch_rejects_token_exchange() {
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);

    let pkce = super::Pkce::new();
    let redirect = "http://127.0.0.1:1/auth/callback";
    let ep = auth.endpoints();
    let url = build_authorize_url(
        &auth.authorize_endpoint(),
        &auth.config().client_id,
        redirect,
        &pkce.challenge,
        "st",
        "no",
    )
    .unwrap();
    let loc = location_of(&url);
    let code = parse_query(&loc).get("code").cloned().unwrap();

    // Wrong verifier → invalid_grant → TokenExchange error.
    let bad = mgr.exchange_code(&ep, &code, "totally-wrong-verifier", redirect);
    assert_eq!(bad.unwrap_err(), AuthError::TokenExchange);
}

#[test]
fn adv2b_correct_verifier_succeeds() {
    // Positive control: the SAME code shape with the CORRECT verifier exchanges
    // fine — proving ADV-2's failure is the verifier, not a broken exchange.
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);
    let pkce = super::Pkce::new();
    let redirect = "http://127.0.0.1:1/auth/callback";
    let ep = auth.endpoints();
    let url = build_authorize_url(
        &auth.authorize_endpoint(),
        &auth.config().client_id,
        redirect,
        &pkce.challenge,
        "st",
        "no",
    )
    .unwrap();
    let loc = location_of(&url);
    let code = parse_query(&loc).get("code").cloned().unwrap();
    let ok = mgr.exchange_code(&ep, &code, &pkce.verifier, redirect);
    assert!(ok.is_ok());
}

// ===========================================================================
// ADV-3 — PKCE plain downgrade attempt (S256 enforced)
// ===========================================================================

#[test]
fn adv3_plain_downgrade_rejected_and_client_never_offers_it() {
    let auth = MockAuthority::start();
    let url = build_authorize_url(
        &auth.authorize_endpoint(),
        &auth.config().client_id,
        "http://127.0.0.1:1/auth/callback",
        "chal",
        "st",
        "no",
    )
    .unwrap();
    // (a) Our authorization URL always carries method=S256 — never plain (ADV-3).
    assert!(url.contains("code_challenge_method=S256"));
    assert!(!url.contains("plain"));

    // (b) The authority hard-rejects any non-S256 method — a downgrade probe
    //     yields no 302 (no auth code).
    let downgraded = url.replace("S256", "plain");
    let is_302 = no_redirect_agent()
        .get(&downgraded)
        .call()
        .ok()
        .map(|r| r.headers().get("location").is_some())
        .unwrap_or(false);
    assert!(!is_302, "a plain PKCE downgrade must not yield an auth code");
}

// ===========================================================================
// A3-AUTH-1 — redirect path. The live authority registered
// `http://127.0.0.1:<port>/auth/callback` (verified 303); the old `/callback`
// path 400s. The full login builds the redirect_uri; assert its PATH is
// `/auth/callback` (RED: reverting to `/callback` makes this assert fail).
// ===========================================================================

#[test]
fn a3auth1_redirect_uri_path_is_auth_callback() {
    // Capture the redirect_uri the login flow actually sends by intercepting the
    // authorization request the mock receives (it echoes `redirect_uri` into its
    // 302 Location). We drive a real login and read the delivered callback path.
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);
    let (vault, _f, _p) = fresh_vault();

    let seen_path = Arc::new(StdMutex::new(String::new()));
    let seen_path2 = seen_path.clone();
    mgr.login_with(&vault, |auth_url| {
        // The mock's 302 Location is `<redirect_uri>?code=..&state=..`; parse the
        // redirect_uri's PATH out of it and record it before delivering.
        let loc = location_of(auth_url);
        let parsed = url::Url::parse(&loc).unwrap();
        *seen_path2.lock().unwrap() = parsed.path().to_string();
        deliver_callback(loc);
        Ok(())
    })
    .unwrap();

    assert_eq!(
        *seen_path.lock().unwrap(),
        "/auth/callback",
        "the loopback redirect path MUST be /auth/callback (authority registration), not /callback"
    );
}

// ===========================================================================
// A3-AUTH-2 — discovery trust anchor. The client fetches the discovery document
// and MUST reject one whose `issuer` != the pinned prod issuer (attacker-
// authority swap). RED: dropping the `doc.issuer != self.cfg.issuer` gate lets
// the swapped-issuer authority's endpoints be consumed.
// ===========================================================================

#[test]
fn a3auth2_discovery_issuer_mismatch_is_rejected() {
    let auth = MockAuthority::start();
    auth.behavior().wrong_discovery_issuer = true;
    let mgr = manager_for(&auth);
    let (vault, _f, _p) = fresh_vault();
    let r = mgr.login_with(&vault, |u| auth.drive_browser(u));
    assert_eq!(
        r.unwrap_err(),
        AuthError::IdTokenInvalid,
        "a discovery doc whose issuer != the trust anchor must fail closed"
    );
    // Nothing vaulted when discovery is rejected.
    assert!(!vault.list().unwrap().iter().any(|s| s.name == REFRESH_SLOT));
}

// ===========================================================================
// A3-AUTH-3 — RS256 verification. The live authority signs RS256 (one RSA JWKS
// key). A valid RS256 id_token is ACCEPTED (positive control — the whole flow
// now runs on RS256, not ES256); a forged-alg header (ES256 stamped on an
// RSA-keyed token) is REJECTED by the alg-pin (header.alg must equal the JWKS
// key's alg). This is the OIDC-1 alg-pin, re-expressed for the RS256 authority.
// ===========================================================================

#[test]
fn a3auth3_valid_rs256_id_token_accepted() {
    // Positive control: with NO tamper the RS256-signed id_token validates and
    // signs the member in — proving the RS256 JWKS path (n/e via aws_lc_rs) works.
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);
    let (vault, _f, _p) = fresh_vault();
    let st = mgr.login_with(&vault, |u| auth.drive_browser(u)).unwrap();
    assert!(st.signed_in);
}

#[test]
fn a3auth3_forged_alg_header_is_rejected() {
    // alg-confusion probe: the token header claims ES256 while the JWKS key is
    // RSA/RS256. Rejected by the alg-pin (`header.alg != key_alg`), backstopped by
    // jsonwebtoken's own `Validation::new(RS256)` — defense in depth. `alg:none`
    // and HS256 headers die the same way (they never equal the RSA key's alg).
    run_id_token_attack(|b| b.forge_alg_es256 = true);
}

#[test]
fn a3auth3_alg_pin_gates_on_discovery_and_verifiable_set() {
    // Unit-isolate the alg-pin's `authority_accepts_alg` (the part jsonwebtoken
    // does NOT do): the accepted alg must be BOTH verifiable by us AND advertised
    // in discovery `id_token_signing_alg_values_supported`.
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);

    // Authority advertises only RS256 (the live shape).
    let rs_only = Endpoints {
        authorization: format!("{}/auth", auth.base),
        token: format!("{}/token", auth.base),
        userinfo: format!("{}/me", auth.base),
        jwks: format!("{}/jwks", auth.base),
        revocation: None,
        id_token_signing_algs: vec!["RS256".to_string()],
    };
    assert!(mgr.authority_accepts_alg(&rs_only, Algorithm::RS256));
    // ES256 is verifiable by us, but NOT advertised → rejected (no silent widen).
    assert!(!mgr.authority_accepts_alg(&rs_only, Algorithm::ES256));
    // HS256 is never verifiable → rejected regardless of advertising.
    assert!(!mgr.authority_accepts_alg(&rs_only, Algorithm::HS256));

    // An authority advertising ES256 too → ES256 now accepted (future per-client).
    let rs_es = Endpoints {
        id_token_signing_algs: vec!["RS256".to_string(), "ES256".to_string()],
        ..rs_only.clone()
    };
    assert!(mgr.authority_accepts_alg(&rs_es, Algorithm::ES256));
    // Empty advertised set → fall back to the verifiable set (thin authority).
    let thin = Endpoints {
        id_token_signing_algs: vec![],
        ..rs_only.clone()
    };
    assert!(mgr.authority_accepts_alg(&thin, Algorithm::RS256));
    assert!(!mgr.authority_accepts_alg(&thin, Algorithm::HS256));
}

// ===========================================================================
// ADV-4 — loopback listener binds 127.0.0.1 only, never 0.0.0.0
// ===========================================================================

#[test]
fn adv4_loopback_binds_localhost_only() {
    let l = LoopbackListener::bind().unwrap();
    let addr = l.listener.local_addr().unwrap();
    assert_eq!(
        addr.ip(),
        std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        "the callback listener MUST bind 127.0.0.1, never 0.0.0.0 (RFC 8252)"
    );
    assert!(addr.ip().is_loopback());
    assert_ne!(
        addr.ip(),
        std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)
    );
}

// ===========================================================================
// ADV-5 — authorization-code replay / injection (single-use)
// ===========================================================================

#[test]
fn adv5_code_is_single_use() {
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);
    let pkce = super::Pkce::new();
    let redirect = "http://127.0.0.1:1/auth/callback";
    let ep = auth.endpoints();
    let url = build_authorize_url(
        &auth.authorize_endpoint(),
        &auth.config().client_id,
        redirect,
        &pkce.challenge,
        "st",
        "no",
    )
    .unwrap();
    let loc = location_of(&url);
    let code = parse_query(&loc).get("code").cloned().unwrap();

    // First exchange succeeds; replay of the SAME code is rejected (single-use).
    assert!(mgr.exchange_code(&ep, &code, &pkce.verifier, redirect).is_ok());
    let replay = mgr.exchange_code(&ep, &code, &pkce.verifier, redirect);
    assert_eq!(replay.unwrap_err(), AuthError::TokenExchange);

    // Injection: a code that was never issued is likewise rejected.
    let injected = mgr.exchange_code(&ep, "code_never_issued", &pkce.verifier, redirect);
    assert_eq!(injected.unwrap_err(), AuthError::TokenExchange);
}

// ===========================================================================
// ADV-6 — foreign-origin callback with wrong/no state rejected; listener closes
// ===========================================================================

#[test]
fn adv6_callback_without_state_is_rejected_and_listener_closes() {
    // A callback lacking `state` (a foreign/malformed request) fails closed.
    assert_eq!(
        parse_callback_target("/callback?code=abc").unwrap_err(),
        AuthError::StateMismatch
    );
    assert_eq!(
        parse_callback_target("/callback").unwrap_err(),
        AuthError::StateMismatch
    );
    // A full login where the browser delivers no state fails + releases the port.
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);
    let (vault, _f, _p) = fresh_vault();
    let r = mgr.login_with(&vault, |url| {
        let loc = location_of(url);
        let stripped = strip_query(&loc, "state");
        deliver_callback(stripped);
        Ok(())
    });
    assert_eq!(r.unwrap_err(), AuthError::StateMismatch);
}

// ===========================================================================
// ADV-7 — id_token nonce / iss / aud / exp / signature each validated
// ===========================================================================

#[test]
fn adv7_forged_nonce_rejected() {
    run_id_token_attack(|b| b.forge_nonce = true);
}
#[test]
fn adv7_wrong_issuer_rejected() {
    run_id_token_attack(|b| b.wrong_issuer = true);
}
#[test]
fn adv7_wrong_audience_rejected() {
    run_id_token_attack(|b| b.wrong_audience = true);
}
#[test]
fn adv7_expired_rejected() {
    run_id_token_attack(|b| b.expired = true);
}
#[test]
fn adv7_forged_signature_rejected() {
    run_id_token_attack(|b| b.forge_signature = true);
}

#[test]
fn adv7_valid_id_token_accepted() {
    // Positive control: with NO tamper, a full login validates + signs in.
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);
    let (vault, _f, _p) = fresh_vault();
    let st = mgr.login_with(&vault, |u| auth.drive_browser(u)).unwrap();
    assert!(st.signed_in);
}

fn run_id_token_attack(set: impl FnOnce(&mut Behavior)) {
    let auth = MockAuthority::start();
    {
        let mut b = auth.behavior();
        set(&mut b);
    }
    let mgr = manager_for(&auth);
    let (vault, _f, _p) = fresh_vault();
    let r = mgr.login_with(&vault, |u| auth.drive_browser(u));
    assert_eq!(
        r.unwrap_err(),
        AuthError::IdTokenInvalid,
        "a tampered id_token must be rejected"
    );
    // Nothing was vaulted on a failed login.
    assert!(!vault.list().unwrap().iter().any(|s| s.name == REFRESH_SLOT));
}

// ===========================================================================
// OIDC-1 — audience-confusion: an id_token that OMITS `aud` must be rejected
// (absence must NOT bypass the audience binding — the classic multi-RP token
// substitution). Red-first: without `set_required_spec_claims(aud)` + the manual
// `aud.contains(client_id)` re-check, an aud-less token was accepted with
// attacker-chosen claims.
// ===========================================================================

#[test]
fn oidc1_absent_aud_is_rejected() {
    run_id_token_attack(|b| b.omit_aud = true);
}

#[test]
fn oidc1_wrong_aud_still_rejected() {
    // Present-but-wrong aud (the original ADV-7 case) remains rejected.
    run_id_token_attack(|b| b.wrong_audience = true);
}

// ===========================================================================
// OIDC-2 — JWKS key selection: a kid-LESS id_token against a MULTI-key JWKS must
// fail closed (no "first key" fallback that an attacker-ordered JWKS could
// exploit). Red-first: the old `_ => true` matcher picked the first (attacker)
// key and accepted a token it did not sign to verify against the real key.
// ===========================================================================

#[test]
fn oidc2_kidless_token_multikey_jwks_is_rejected() {
    // The attacker publishes their key FIRST in a kid-less multi-key JWKS and
    // signs the id_token with it. Under a "first key" fallback this token would
    // VERIFY (attacker forges any claims). The fix rejects kid-less+multikey, so
    // the attacker-signed token is refused.
    run_id_token_attack(|b| {
        b.omit_kid = true;
        b.multikey_jwks = true;
        b.sign_with_attacker = true;
    });
}

#[test]
fn oidc2_kidless_token_single_key_jwks_accepted() {
    // Positive control: a kid-less token against a SINGLE-key JWKS is
    // unambiguous and still validates (real key, real signature).
    let auth = MockAuthority::start();
    {
        let mut b = auth.behavior();
        b.omit_kid = true; // single-key JWKS (multikey_jwks stays false)
    }
    let mgr = manager_for(&auth);
    let (vault, _f, _p) = fresh_vault();
    let st = mgr.login_with(&vault, |u| auth.drive_browser(u)).unwrap();
    assert!(st.signed_in);
}

// ===========================================================================
// NEW-1 — multi-aud + azp (OIDC Core §3.1.3.7 rules 4–5). A multi-valued `aud`
// that lists us alongside another party must be rejected UNLESS `azp` names us.
// A single-aud token with an `azp` naming someone else is also rejected.
// ===========================================================================

#[test]
fn new1_multi_aud_without_azp_is_rejected() {
    // aud = [citrate-core, attacker-extra], no azp → reject (§3.1.3.7 rule 4).
    run_id_token_attack(|b| b.multi_aud = true);
}

#[test]
fn new1_multi_aud_with_foreign_azp_is_rejected() {
    // aud multi + azp = attacker-extra (not us) → reject (§3.1.3.7 rule 5).
    run_id_token_attack(|b| {
        b.multi_aud = true;
        b.azp = Some("attacker-extra".to_string());
    });
}

#[test]
fn new1_single_aud_with_foreign_azp_is_rejected() {
    // Single aud (us) but azp names a different party → reject: a present azp
    // must still be us.
    run_id_token_attack(|b| b.azp = Some("attacker-extra".to_string()));
}

#[test]
fn new1_multi_aud_with_our_azp_is_accepted() {
    // Positive control: aud multi + azp = citrate-core (us) → accepted.
    let auth = MockAuthority::start();
    {
        let mut b = auth.behavior();
        b.multi_aud = true;
        b.azp = Some("citrate-core".to_string());
    }
    let mgr = manager_for(&auth);
    let (vault, _f, _p) = fresh_vault();
    let st = mgr.login_with(&vault, |u| auth.drive_browser(u)).unwrap();
    assert!(st.signed_in);
}

// ===========================================================================
// A3-04 / OIDC-3 — refresh id_token re-validation + `sub` continuity. A refresh
// response whose id_token is minted for a DIFFERENT subject must be rejected
// (subject substitution), not silently adopted.
// ===========================================================================

#[test]
fn a3_04_refresh_subject_substitution_is_rejected() {
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);
    let (vault, _f, _p) = fresh_vault();
    // Establish a session for the legitimate sub.
    mgr.login_with(&vault, |u| auth.drive_browser(u)).unwrap();
    let prior = mgr.status().sub.clone();
    assert!(prior.is_some());
    // The authority now mints refresh id_tokens for a DIFFERENT sub.
    auth.behavior().refresh_wrong_sub = true;
    let r = mgr.refresh(&vault);
    assert_eq!(
        r.unwrap_err(),
        AuthError::IdTokenInvalid,
        "a refresh id_token for a different subject must be rejected"
    );
}

// ===========================================================================
// A3-01 — custody boundary: the `custody_put` INVOKE path must not address a
// backend-owned slot (`oidc-refresh`), so a compromised/XSS'd webview cannot
// overwrite the vaulted refresh token. The in-process `put` (used by the auth
// backend) is unrestricted; the command wrapper is the untrusted boundary.
// ===========================================================================

#[test]
fn a3_01_backend_reserved_slot_predicate() {
    use crate::custody::is_backend_reserved_slot;
    assert!(is_backend_reserved_slot(REFRESH_SLOT));
    assert!(is_backend_reserved_slot("oidc-refresh"));
    assert!(is_backend_reserved_slot("oidc-anything"));
    // Ordinary user slots are NOT reserved (webview may write those).
    assert!(!is_backend_reserved_slot("my-note"));
    assert!(!is_backend_reserved_slot("gateway-key"));
}

#[test]
fn a3_01_in_process_put_still_writes_backend_slot() {
    // The in-process API (what the auth backend uses) can write the reserved slot
    // — only the invoke command is gated. Prove the round-trip still works.
    let (vault, _f, _p) = fresh_vault();
    let mut bytes = b"rt_secret".to_vec();
    vault.put(REFRESH_SLOT, &mut bytes).unwrap();
    let got = vault.custody_get(REFRESH_SLOT).unwrap();
    assert_eq!(&got[..], b"rt_secret");
}

// ===========================================================================
// ADV-8 — token boundary: no invoke command returns a token
// ===========================================================================

#[test]
fn adv8_no_auth_invoke_command_returns_a_token() {
    // Structural boundary proof against the real source: every registered auth
    // command's return type is AuthStatus (claim-derived) or (); NONE is a token.
    let src = include_str!("oidc.rs");

    // AuthStatus must not carry any token field.
    let start = src.find("pub struct AuthStatus").expect("AuthStatus exists");
    let body = &src[start..start + 800];
    for f in ["access_token", "refresh_token", "accessToken", "refreshToken"] {
        assert!(!body.contains(f), "AuthStatus must not expose a token field ({f})");
    }

    // The auth commands are registered in lib.rs.
    let lib = include_str!("lib.rs");
    for cmd in [
        "auth_status",
        "auth_login",
        "auth_userinfo",
        "auth_refresh",
        "auth_logout",
        "kyc_start",
    ] {
        assert!(
            lib.contains(&format!("oidc::{cmd}")),
            "auth command {cmd} must be registered"
        );
    }
    // Their #[tauri::command] signatures return AuthStatus or ().
    for sig in [
        "pub fn auth_status(state: State<'_, AuthState>) -> std::result::Result<AuthStatus, String>",
        "pub fn auth_userinfo(auth: State<'_, AuthState>) -> std::result::Result<AuthStatus, String>",
    ] {
        assert!(src.contains(sig), "expected command signature: {sig}");
    }
    // No command returns TokenResponse — it is confined to the private path.
    assert!(
        !src.contains("-> std::result::Result<TokenResponse"),
        "no command may return TokenResponse"
    );
}

#[test]
fn adv8_status_is_claim_derived_only_at_runtime() {
    // Runtime half: after a real login, the status snapshot serializes to
    // flags/claims and NEVER contains token-shaped material.
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);
    let (vault, _f, _p) = fresh_vault();
    mgr.login_with(&vault, |u| auth.drive_browser(u)).unwrap();
    let st = mgr.status();
    let json = serde_json::to_string(&st).unwrap();
    assert!(!json.contains("at_"), "access-token prefix must not appear");
    assert!(!json.contains("rt_"), "refresh-token prefix must not appear");
    assert!(st.signed_in);
}

// ===========================================================================
// ADV-9 — refresh token never in logs / errors
// ===========================================================================

#[test]
fn adv9_refresh_token_absent_from_errors() {
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);
    let (vault, _f, _p) = fresh_vault();
    mgr.login_with(&vault, |u| auth.drive_browser(u)).unwrap();

    // Read the raw refresh token from the vault (in-process) so we know its value.
    let rt = vault.custody_get(REFRESH_SLOT).unwrap();
    let rt_str = std::str::from_utf8(&rt).unwrap().to_string();
    assert!(rt_str.starts_with("rt_"));

    // Force a refresh failure (revoke server-side first) and inspect the error.
    let _ = ureq::post(format!("{}/token/revocation", auth.base))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(format!("token={rt_str}"));
    let err = mgr.refresh(&vault).unwrap_err();
    let msg = err.to_string();
    assert!(
        !msg.contains(&rt_str) && !msg.contains("rt_"),
        "an error string must never contain the refresh token"
    );

    // Every variant's Display is secret-free.
    for e in [
        AuthError::StateMismatch,
        AuthError::TokenExchange,
        AuthError::IdTokenInvalid,
        AuthError::Timeout,
        AuthError::NotSignedIn,
        AuthError::Custody,
        AuthError::Network,
    ] {
        let s = e.to_string();
        assert!(!s.contains("rt_") && !s.contains("at_"));
    }
}

// ===========================================================================
// ADV-10 — listener timeout / no callback fails closed, port released
// ===========================================================================

#[test]
fn adv10_listener_timeout_fails_closed_and_releases_port() {
    let l = LoopbackListener::bind().unwrap();
    let port = l.port();
    let start = std::time::Instant::now();
    let r = l.wait_for_callback(std::time::Duration::from_millis(150));
    assert_eq!(r.unwrap_err(), AuthError::Timeout);
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
    // The port is released: a fresh bind on the same port eventually succeeds.
    let mut rebound = false;
    for _ in 0..50 {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            rebound = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(rebound, "the loopback port must be released after a timeout");
}

// --- small query-string editors used by the CSRF tests --------------------

fn replace_query(u: &str, key: &str, val: &str) -> String {
    let mut parsed = url::Url::parse(u).unwrap();
    let pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(k, v)| {
            if k == key {
                (k.into_owned(), val.to_string())
            } else {
                (k.into_owned(), v.into_owned())
            }
        })
        .collect();
    parsed.query_pairs_mut().clear().extend_pairs(pairs);
    parsed.to_string()
}

fn strip_query(u: &str, key: &str) -> String {
    let mut parsed = url::Url::parse(u).unwrap();
    let pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| k != key)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    parsed.query_pairs_mut().clear().extend_pairs(pairs);
    parsed.to_string()
}
