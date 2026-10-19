// GROW-S1 task 10 — tests for the v2 invite-link client (`join.rs`).
//
// TEST-SIDE STUB. `StubJoinService` below is a pure test fixture: a loopback HTTP server on a real
// `std::net::TcpListener` that implements the citrate-landing join contract (reserve → attach,
// uniform 404, 409, revoke, mine, Ed25519-signed resolve, JWKS with current + next kids). It
// exists only under `#[cfg(test)]` and is never reachable from a production path (Rule 1). The
// client under test talks to it through the production `UreqClient`, so these are real HTTP
// round-trips, not a mocked transport.

use super::*;
use crate::oidc::UreqClient;
use jsonwebtoken::{EncodingKey, Header};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

// --- fixed test keys (RFC 8032 §7.1 test vectors 1-3) -----------------------------------------

const SEED_CURRENT: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
const PUB_CURRENT: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
const SEED_NEXT: &str = "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb";
const PUB_NEXT: &str = "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c";
const SEED_ROGUE: &str = "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7";
const PUB_ROGUE: &str = "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025";

const BEARER: &str = "test-access-token-7f3c9a";
const FAR_EXP: u64 = 4_102_444_800; // 2100-01-01T00:00:00Z
const FAR_EXP_ISO: &str = "2100-01-01T00:00:00.000Z";
const PAST_EXP: u64 = 978_307_200; // 2001-01-01T00:00:00Z
const PAST_EXP_ISO: &str = "2001-01-01T00:00:00.000Z";

fn hexb(s: &str) -> Vec<u8> {
    hex::decode(s).expect("test hex")
}

/// PKCS#8 v1 wrapping of an Ed25519 seed (RFC 8410).
fn pkcs8(seed_hex: &str) -> Vec<u8> {
    let mut der = hexb("302e020100300506032b657004220420");
    der.extend(hexb(seed_hex));
    der
}

fn x_of(pub_hex: &str) -> String {
    URL_SAFE_NO_PAD.encode(hexb(pub_hex))
}

fn jwk(kid: &str, pub_hex: &str) -> serde_json::Value {
    serde_json::json!({ "kty": "OKP", "crv": "Ed25519", "x": x_of(pub_hex), "kid": kid, "use": "sig", "alg": "EdDSA" })
}

fn served_jwks() -> serde_json::Value {
    // A compromised / swapped JWKS that ALSO carries a rogue kid: the client must ignore it.
    serde_json::json!({ "keys": [jwk("join-1", PUB_CURRENT), jwk("join-2", PUB_NEXT), jwk("rogue", PUB_ROGUE)] })
}

fn served_jwks_typed() -> Jwks {
    serde_json::from_value(served_jwks()).expect("jwks")
}

fn sample_invite() -> InvitePayload {
    InvitePayload {
        g: "grp_0a1b2c3d4e5f".into(),
        t: "Zt9QxV2mK7pL0aRr4sYuWbNcEdFgHiJkLmNoPqRsTuv".into(),
        k: "02a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90".into(),
        n: hex::encode("Garden club"),
    }
}

const CODE: &str = "Ab3dE7gh";

struct FixedBearer(&'static str);
impl BearerSource for FixedBearer {
    fn bearer(&self) -> std::result::Result<Zeroizing<String>, AuthError> {
        Ok(Zeroizing::new(self.0.to_string()))
    }
}
struct NoSession;
impl BearerSource for NoSession {
    fn bearer(&self) -> std::result::Result<Zeroizing<String>, AuthError> {
        Err(AuthError::NotSignedIn)
    }
}

// --- signed resolve builder (shared by the unit tests and the stub) ----------------------------

#[derive(Clone)]
struct ResolveSpec {
    code: String,
    kind: JoinKind,
    cluster_name: Option<String>,
    goal: Option<String>,
    inviter: Option<String>,
    inviter_verified: bool,
    exp: u64,
    exp_iso: String,
    env: Option<String>,
    kid: String,
    seed: &'static str,
}

impl ResolveSpec {
    fn direct(code: &str, env: &str) -> Self {
        ResolveSpec {
            code: code.into(),
            kind: JoinKind::Direct,
            cluster_name: Some("Garden club".into()),
            goal: Some("storage".into()),
            inviter: Some("0x1234…abcd".into()),
            inviter_verified: true,
            exp: FAR_EXP,
            exp_iso: FAR_EXP_ISO.into(),
            env: Some(env.into()),
            kid: "join-1".into(),
            seed: SEED_CURRENT,
        }
    }
    fn referral(code: &str) -> Self {
        ResolveSpec {
            kind: JoinKind::Referral,
            env: None,
            ..ResolveSpec::direct(code, "")
        }
    }

    /// (claims, body-without-sig) as citrate-landing `signResolve` + the GET route build them.
    fn parts(&self) -> (serde_json::Value, serde_json::Value) {
        let mut claims = serde_json::json!({
            "v": 2, "status": "valid", "code": self.code, "kind": self.kind.as_str(),
            "clusterName": self.cluster_name, "goal": self.goal, "inviter": self.inviter,
            "inviterVerified": self.inviter_verified,
            "iss": JOIN_ISSUER, "sub": self.code, "iat": 1_760_000_000u64, "exp": self.exp,
        });
        let mut body = serde_json::json!({
            "ok": true, "v": 2, "status": "valid", "code": self.code, "kind": self.kind.as_str(),
            "clusterName": self.cluster_name, "goal": self.goal, "inviter": self.inviter,
            "inviterVerified": self.inviter_verified, "exp": self.exp_iso,
        });
        if let Some(env) = &self.env {
            let h = envelope_hash(env).expect("hash");
            claims["envVersion"] = 1.into();
            claims["envHash"] = h.clone().into();
            body["env"] = env.clone().into();
            body["envVersion"] = 1.into();
            body["envHash"] = h.into();
        }
        (claims, body)
    }

    fn sign(&self, claims: &serde_json::Value) -> String {
        let mut h = Header::new(Algorithm::EdDSA);
        h.kid = Some(self.kid.clone());
        h.typ = Some("JWT".into());
        jsonwebtoken::encode(&h, claims, &EncodingKey::from_ed_der(&pkcs8(self.seed)))
            .expect("sign")
    }

    fn body_json(&self) -> serde_json::Value {
        let (claims, mut body) = self.parts();
        body["sig"] = self.sign(&claims).into();
        body
    }

    fn body(&self) -> ResolveBody {
        serde_json::from_value(self.body_json()).expect("resolve body")
    }
}

fn sealed_env(code: &str) -> (LinkKey, String) {
    let key = LinkKey::from_bytes([7u8; 16]);
    let env = seal(&key, JoinKind::Direct, code, &sample_invite()).expect("seal");
    (key, env)
}

// --- envelope ----------------------------------------------------------------------------------

#[test]
fn envelope_round_trips() {
    let (key, env) = sealed_env(CODE);
    let opened = open(&key, JoinKind::Direct, CODE, &env).expect("open");
    assert_eq!(opened, sample_invite());
    // nonce || ct || tag, and the plaintext is not visible in the envelope.
    let raw = URL_SAFE_NO_PAD.decode(&env).expect("b64");
    let pt_len = serde_json::to_vec(&sample_invite()).expect("json").len();
    assert_eq!(raw.len(), 24 + pt_len + 16);
    assert!(!env.contains(&sample_invite().t));
}

#[test]
fn envelope_nonce_is_fresh_per_seal() {
    let key = LinkKey::from_bytes([7u8; 16]);
    let a = seal(&key, JoinKind::Direct, CODE, &sample_invite()).expect("seal");
    let b = seal(&key, JoinKind::Direct, CODE, &sample_invite()).expect("seal");
    assert_ne!(a, b);
    assert_ne!(a[..32], b[..32]);
}

#[test]
fn envelope_aad_binds_kind() {
    let (key, env) = sealed_env(CODE);
    assert_eq!(
        open(&key, JoinKind::Referral, CODE, &env),
        Err(JoinError::Envelope)
    );
}

#[test]
fn envelope_aad_binds_code() {
    let (key, env) = sealed_env(CODE);
    assert_eq!(
        open(&key, JoinKind::Direct, "Ab3dE7gj", &env),
        Err(JoinError::Envelope)
    );
}

#[test]
fn envelope_rejects_wrong_key_and_tampering() {
    let (_key, env) = sealed_env(CODE);
    let other = LinkKey::from_bytes([8u8; 16]);
    assert_eq!(
        open(&other, JoinKind::Direct, CODE, &env),
        Err(JoinError::Envelope)
    );
    let key = LinkKey::from_bytes([7u8; 16]);
    let mut raw = URL_SAFE_NO_PAD.decode(&env).expect("b64");
    let last = raw.len() - 1;
    raw[last] ^= 1;
    let tampered = URL_SAFE_NO_PAD.encode(&raw);
    assert_eq!(
        open(&key, JoinKind::Direct, CODE, &tampered),
        Err(JoinError::Envelope)
    );
    assert_eq!(
        open(&key, JoinKind::Direct, CODE, "short"),
        Err(JoinError::Envelope)
    );
    assert_eq!(
        open(&key, JoinKind::Direct, CODE, &format!("{env}=")),
        Err(JoinError::Envelope)
    );
}

#[test]
fn aad_string_is_pinned() {
    assert_eq!(
        aad_bytes(JoinKind::Direct, CODE, 1).expect("aad"),
        b"citrate-join-v1|direct|Ab3dE7gh".to_vec()
    );
    assert_eq!(
        aad_bytes(JoinKind::Referral, CODE, 1).expect("aad"),
        b"citrate-join-v1|referral|Ab3dE7gh".to_vec()
    );
    assert_eq!(
        aad_bytes(JoinKind::Direct, "bad", 1),
        Err(JoinError::Invalid)
    );
    assert_eq!(
        aad_bytes(JoinKind::Direct, CODE, 0),
        Err(JoinError::Invalid)
    );
}

/// The shared AAD vectors from citrate-landing (PR #123 head f704313; provenance in the fixture):
/// the Rust builder must produce exactly the bytes the service-side `joinAadBytes` does.
#[test]
fn aad_matches_the_shared_landing_vectors() {
    let fx: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/join-aad-vectors.json")).expect("fixture");
    assert!(fx["_provenance"]
        .as_str()
        .expect("provenance")
        .contains("f704313"));
    let vectors = fx["vectors"].as_array().expect("vectors");
    assert!(vectors.len() >= 5);
    for v in vectors {
        let kind: JoinKind = serde_json::from_value(v["kind"].clone()).expect("kind");
        let code = v["code"].as_str().expect("code");
        let ver = u32::try_from(v["envVersion"].as_u64().expect("version")).expect("u32");
        let got = aad_bytes(kind, code, ver).expect("aad");
        assert_eq!(
            hex::encode(&got),
            v["aadHex"].as_str().expect("hex"),
            "{code}"
        );
        assert_eq!(
            got,
            v["aadUtf8"].as_str().expect("utf8").as_bytes(),
            "{code}"
        );
    }
}

#[test]
fn hkdf_key_derivation_matches_known_answer() {
    // Independent computation (RFC 5869 with an empty salt, i.e. 32 zero bytes, info
    // "citrate-join-envelope-v1", K = 00 01 .. 0f), so the web side can check its own implementation.
    let mut b = [0u8; 16];
    for (i, x) in b.iter_mut().enumerate() {
        *x = i as u8;
    }
    let okm = derive_aead_key(&LinkKey::from_bytes(b)).expect("hkdf");
    assert_eq!(
        hex::encode(&okm[..]),
        "be763053d8bcb5a9e70a3ef3d38e8cac1808bac2dc7eef32c24358f867c2365d"
    );
}

#[test]
fn hkdf_info_is_its_own_domain_label() {
    assert_eq!(ENVELOPE_HKDF_INFO, "citrate-join-envelope-v1");
    // Distinct from the AAD: not equal to, and not a prefix of, any AAD string.
    let aad = String::from_utf8(aad_bytes(JoinKind::Direct, CODE, ENVELOPE_VERSION).expect("aad"))
        .expect("utf8");
    assert!(!aad.starts_with(ENVELOPE_HKDF_INFO));
    assert!(!ENVELOPE_HKDF_INFO.starts_with("citrate-join-v1"));
    // The derivation really uses it: the old shared label gives a different key.
    let k = LinkKey::from_bytes([3u8; 16]);
    let ours = derive_aead_key(&k).expect("hkdf");
    let mut old = [0u8; 32];
    Hkdf::<Sha256>::new(None, &[3u8; 16])
        .expand(b"citrate-join-v1", &mut old)
        .expect("hkdf");
    assert_ne!(&ours[..], &old[..]);
}

#[test]
fn env_hash_matches_landing_envelope_hash() {
    // landing src/lib/join.ts:198 (f704313): base64url(SHA-256(Buffer.from(env, "base64url"))).
    // Vector computed independently (python hashlib + urlsafe_b64encode, padding stripped) over the
    // 40 bytes 00..27.
    assert_eq!(
        envelope_hash("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8gISIjJCUmJw").expect("hash"),
        "X6pO7DYRVWgSwtdLQ3yMSa3T-RDxAGPYAUQffXXNXjs"
    );
}

#[test]
fn link_key_fragment_is_22_canonical_chars() {
    let k = LinkKey::generate();
    let f = k.to_fragment();
    assert_eq!(f.len(), 22);
    let back = LinkKey::from_fragment(&f).expect("parse");
    assert_eq!(back.to_fragment(), f);
    assert!(LinkKey::from_fragment(&f[..21]).is_err());
    assert!(LinkKey::from_fragment(&format!("{}A", f.as_str())).is_err());
    assert!(LinkKey::from_fragment("AAAAAAAAAAAAAAAAAAAAA=").is_err());
    // Non-canonical: last char carries non-zero trailing bits.
    assert!(LinkKey::from_fragment("AAAAAAAAAAAAAAAAAAAAAB").is_err());
    assert!(LinkKey::from_fragment("AAAAAAAAAAAAAAAAAAAAAA").is_ok());
}

#[test]
fn share_url_shape_and_parse_back() {
    let k = LinkKey::from_bytes([9u8; 16]);
    let url = share_url(CODE, Some(&k)).expect("url");
    assert_eq!(
        url.as_str(),
        format!(
            "https://citrate.ai/join/{CODE}#{}",
            k.to_fragment().as_str()
        )
    );
    let (c, back) = parse_link(&url).expect("parse");
    assert_eq!(c, CODE);
    assert_eq!(back.expect("key").to_fragment(), k.to_fragment());
    assert_eq!(
        share_url(CODE, None).expect("url").as_str(),
        "https://citrate.ai/join/Ab3dE7gh"
    );
    assert_eq!(share_url("bad", None), Err(JoinError::Invalid));
}

#[test]
fn parse_link_accepts_the_three_schemes_and_a_bare_code() {
    let f = LinkKey::from_bytes([9u8; 16]).to_fragment();
    for l in [
        format!("https://citrate.ai/join/{CODE}#{}", f.as_str()),
        format!("https://www.citrate.ai/join/{CODE}#{}", f.as_str()),
        format!("citrate://join/{CODE}#{}", f.as_str()),
        format!("{CODE}#{}", f.as_str()),
        format!("  https://citrate.ai/join/{CODE}/?x=1#{}  ", f.as_str()),
    ] {
        let (c, k) = parse_link(&l).expect("parse");
        assert_eq!(c, CODE);
        assert!(k.is_some());
    }
    let (_, k) = parse_link(&format!("https://citrate.ai/join/{CODE}")).expect("parse");
    assert!(k.is_none());
    let (_, k) = parse_link(&format!("https://citrate.ai/join/{CODE}#")).expect("parse");
    assert!(k.is_none());
    for bad in [
        format!("https://evil.example/join/{CODE}"),
        format!("http://citrate.ai/join/{CODE}"),
        format!("https://citrate.ai:8443/join/{CODE}"),
        format!("https://citrate.ai/other/{CODE}"),
        "https://citrate.ai/join/Ab3dE7g0".to_string(), // '0' not in the alphabet
        format!("https://citrate.ai/join/{CODE}#tooShort"),
        format!("citrate://invite/{CODE}"),
    ] {
        assert_eq!(
            parse_link(&bad).map(|v| v.0),
            Err(JoinError::Invalid),
            "{bad}"
        );
    }
}

#[test]
fn invite_link_parse_and_rebuild() {
    let p = sample_invite();
    let link = p.to_invite_link();
    assert_eq!(InvitePayload::from_invite_link(&link).expect("parse"), p);
    let mut no_name = sample_invite();
    no_name.n = String::new();
    assert!(!no_name.to_invite_link().contains("&n="));
    assert!(InvitePayload::from_invite_link("citrate://invite?g=a&t=b").is_err());
    assert!(InvitePayload::from_invite_link("citrate://invite?g=a&t=b&k=c&g=d").is_err());
    assert!(InvitePayload::from_invite_link("citrate://invite?g=a b&t=b&k=c").is_err());
    assert!(InvitePayload::from_invite_link("https://x/?g=a&t=b&k=c").is_err());
}

// --- resolve signature ---------------------------------------------------------------------------

#[test]
fn jwt_good_signature_verifies_under_current_and_next_kid() {
    let (_k, env) = sealed_env(CODE);
    let v = verify_resolve(
        &ResolveSpec::direct(CODE, &env).body(),
        CODE,
        &served_jwks_typed(),
        JOIN_PINNED_KIDS,
    )
    .expect("verify");
    assert_eq!(v.kind, JoinKind::Direct);
    assert_eq!(v.cluster_name.as_deref(), Some("Garden club"));
    assert_eq!(v.exp, FAR_EXP);
    let next = ResolveSpec {
        kid: "join-2".into(),
        seed: SEED_NEXT,
        ..ResolveSpec::referral(CODE)
    };
    assert!(verify_resolve(&next.body(), CODE, &served_jwks_typed(), JOIN_PINNED_KIDS).is_ok());
}

#[test]
fn jwt_wrong_kid_fails() {
    // Pinned kid in the header, but signed by a different key than the JWKS publishes under it.
    let spec = ResolveSpec {
        kid: "join-1".into(),
        seed: SEED_NEXT,
        ..ResolveSpec::referral(CODE)
    };
    assert_eq!(
        verify_resolve(&spec.body(), CODE, &served_jwks_typed(), JOIN_PINNED_KIDS),
        Err(JoinError::Signature)
    );
    // A pinned kid that the fetched JWKS doesn't carry.
    let jwks: Jwks =
        serde_json::from_value(serde_json::json!({ "keys": [jwk("join-2", PUB_NEXT)] }))
            .expect("jwks");
    assert_eq!(
        verify_resolve(
            &ResolveSpec::referral(CODE).body(),
            CODE,
            &jwks,
            JOIN_PINNED_KIDS
        ),
        Err(JoinError::Signature)
    );
}

#[test]
fn jwt_unpinned_kid_fails_even_when_the_jwks_serves_it() {
    let spec = ResolveSpec {
        kid: "rogue".into(),
        seed: SEED_ROGUE,
        ..ResolveSpec::referral(CODE)
    };
    assert_eq!(
        verify_resolve(&spec.body(), CODE, &served_jwks_typed(), JOIN_PINNED_KIDS),
        Err(JoinError::Signature)
    );
}

#[test]
fn jwt_tampered_field_fails() {
    let (_k, env) = sealed_env(CODE);
    let spec = ResolveSpec::direct(CODE, &env);
    for (field, value) in [
        ("clusterName", serde_json::json!("Free airdrop")),
        ("goal", serde_json::json!("training")),
        ("inviter", serde_json::json!("0xdead…beef")),
        ("inviterVerified", serde_json::json!(false)),
        ("exp", serde_json::json!("2100-01-02T00:00:00.000Z")),
        ("kind", serde_json::json!("referral")),
    ] {
        let mut b = spec.body_json();
        b[field] = value;
        let body: ResolveBody = serde_json::from_value(b).expect("body");
        assert_eq!(
            verify_resolve(&body, CODE, &served_jwks_typed(), JOIN_PINNED_KIDS),
            Err(JoinError::Signature),
            "{field}"
        );
    }
    // A changed signed payload byte breaks the signature itself.
    let mut b = spec.body_json();
    let sig = b["sig"].as_str().expect("sig").to_string();
    let mut parts: Vec<String> = sig.split('.').map(String::from).collect();
    let mut claims: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&parts[1]).expect("b64")).expect("json");
    claims["clusterName"] = "Free airdrop".into();
    parts[1] = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("json"));
    b["sig"] = parts.join(".").into();
    b["clusterName"] = "Free airdrop".into();
    let body: ResolveBody = serde_json::from_value(b).expect("body");
    assert_eq!(
        verify_resolve(&body, CODE, &served_jwks_typed(), JOIN_PINNED_KIDS),
        Err(JoinError::Signature)
    );
    // Asking for a different code than the one signed.
    assert_eq!(
        verify_resolve(
            &spec.body(),
            "Ab3dE7gj",
            &served_jwks_typed(),
            JOIN_PINNED_KIDS
        ),
        Err(JoinError::Signature)
    );
}

#[test]
fn jwt_env_hash_mismatch_fails() {
    let (_k, env) = sealed_env(CODE);
    let (_k2, other_env) = sealed_env(CODE);
    // Envelope swapped in the body after signing (envHash still the signed one).
    let mut b = ResolveSpec::direct(CODE, &env).body_json();
    b["env"] = other_env.clone().into();
    let body: ResolveBody = serde_json::from_value(b).expect("body");
    assert_eq!(
        verify_resolve(&body, CODE, &served_jwks_typed(), JOIN_PINNED_KIDS),
        Err(JoinError::Signature)
    );
    // envHash rewritten in the body to match the swapped envelope: no longer equals the claim.
    let mut b = ResolveSpec::direct(CODE, &env).body_json();
    b["env"] = other_env.clone().into();
    b["envHash"] = envelope_hash(&other_env).expect("hash").into();
    let body: ResolveBody = serde_json::from_value(b).expect("body");
    assert_eq!(
        verify_resolve(&body, CODE, &served_jwks_typed(), JOIN_PINNED_KIDS),
        Err(JoinError::Signature)
    );
    // A direct body with its envelope stripped.
    let mut b = ResolveSpec::direct(CODE, &env).body_json();
    b.as_object_mut().expect("obj").remove("env");
    let body: ResolveBody = serde_json::from_value(b).expect("body");
    assert_eq!(
        verify_resolve(&body, CODE, &served_jwks_typed(), JOIN_PINNED_KIDS),
        Err(JoinError::Signature)
    );
}

#[test]
fn jwt_referral_carrying_an_envelope_fails() {
    let (_k, env) = sealed_env(CODE);
    let mut b = ResolveSpec::referral(CODE).body_json();
    b["env"] = env.into();
    let body: ResolveBody = serde_json::from_value(b).expect("body");
    assert_eq!(
        verify_resolve(&body, CODE, &served_jwks_typed(), JOIN_PINNED_KIDS),
        Err(JoinError::Signature)
    );
}

#[test]
fn jwt_expired_wrong_issuer_and_wrong_alg_fail() {
    let expired = ResolveSpec {
        exp: PAST_EXP,
        exp_iso: PAST_EXP_ISO.into(),
        ..ResolveSpec::referral(CODE)
    };
    assert_eq!(
        verify_resolve(
            &expired.body(),
            CODE,
            &served_jwks_typed(),
            JOIN_PINNED_KIDS
        ),
        Err(JoinError::Signature)
    );
    let spec = ResolveSpec::referral(CODE);
    let (mut claims, mut body) = spec.parts();
    claims["iss"] = "https://evil.example/join".into();
    body["sig"] = spec.sign(&claims).into();
    let b: ResolveBody = serde_json::from_value(body).expect("body");
    assert_eq!(
        verify_resolve(&b, CODE, &served_jwks_typed(), JOIN_PINNED_KIDS),
        Err(JoinError::Signature)
    );
    // HS256 with the public x as the HMAC secret (alg confusion).
    let (claims, mut body) = spec.parts();
    let mut h = Header::new(Algorithm::HS256);
    h.kid = Some("join-1".into());
    let x = x_of(PUB_CURRENT);
    body["sig"] = jsonwebtoken::encode(&h, &claims, &EncodingKey::from_secret(x.as_bytes()))
        .expect("hs")
        .into();
    let b: ResolveBody = serde_json::from_value(body).expect("body");
    assert_eq!(
        verify_resolve(&b, CODE, &served_jwks_typed(), JOIN_PINNED_KIDS),
        Err(JoinError::Signature)
    );
}

#[test]
fn iso_expiry_parses_like_javascript_dates() {
    assert_eq!(
        parse_iso_utc_secs("2100-01-01T00:00:00.000Z"),
        Some(FAR_EXP)
    );
    assert_eq!(parse_iso_utc_secs("2001-01-01T00:00:00Z"), Some(PAST_EXP));
    assert_eq!(
        parse_iso_utc_secs("2026-10-18T12:34:56.999Z"),
        Some(1_792_326_896)
    );
    assert_eq!(parse_iso_utc_secs("2026-10-18T12:34:56+00:00"), None);
    assert_eq!(parse_iso_utc_secs("2026-13-18T12:34:56Z"), None);
    assert_eq!(parse_iso_utc_secs("garbage"), None);
}

// --- status mapping ------------------------------------------------------------------------------

#[test]
fn http_status_maps_to_join_errors() {
    assert_eq!(
        map_http_error(AuthError::Rejected(404)),
        JoinError::NotFound
    );
    assert_eq!(
        map_http_error(AuthError::Rejected(409)),
        JoinError::Conflict
    );
    assert_eq!(
        map_http_error(AuthError::Unauthorized),
        JoinError::Unauthorized
    );
    assert_eq!(
        map_http_error(AuthError::Rejected(422)),
        JoinError::NameNotAllowed
    );
    assert_eq!(
        map_http_error(AuthError::Rejected(429)),
        JoinError::RateLimited
    );
    assert_eq!(
        map_http_error(AuthError::Rejected(503)),
        JoinError::Unavailable
    );
    assert_eq!(map_http_error(AuthError::Rejected(400)), JoinError::Invalid);
    assert_eq!(map_http_error(AuthError::Rejected(413)), JoinError::Invalid);
    assert_eq!(
        map_http_error(AuthError::Rejected(500)),
        JoinError::Rejected(500)
    );
    assert_eq!(map_http_error(AuthError::Network), JoinError::Network);
    assert_eq!(
        map_http_error(AuthError::NotSignedIn),
        JoinError::NotSignedIn
    );
}

// --- redaction -----------------------------------------------------------------------------------

#[test]
fn log_lines_redact_code_tails_and_carry_no_secrets() {
    let key = LinkKey::from_bytes([9u8; 16]);
    let frag = key.to_fragment();
    let line = log_line("resolve", Some(CODE), Some(&JoinError::NotFound));
    assert!(line.contains("code=Ab******"), "{line}");
    assert!(!line.contains(CODE));
    assert!(!line.contains("3dE7gh"));
    assert_eq!(redact_code("not a code at all"), "<code>");
    let link = format!("https://citrate.ai/join/{CODE}#{}", frag.as_str());
    assert_eq!(redact_link(&link), "https://citrate.ai");
    assert_eq!(
        redact_link(&format!("citrate://join/{CODE}#{}", frag.as_str())),
        "citrate://join"
    );
    assert_eq!(redact_link("not a url"), "<link>");
    // No error Display carries anything caller-supplied.
    for e in [
        JoinError::NotSignedIn,
        JoinError::Unauthorized,
        JoinError::Unavailable,
        JoinError::NameNotAllowed,
        JoinError::RateLimited,
        JoinError::NotFound,
        JoinError::Conflict,
        JoinError::Invalid,
        JoinError::Signature,
        JoinError::Envelope,
        JoinError::BadResponse,
        JoinError::Network,
        JoinError::Rejected(418),
    ] {
        let s = log_line("x", None, Some(&e));
        assert!(!s.contains(CODE) && !s.contains(frag.as_str()) && !s.contains(BEARER));
    }
}

#[test]
fn debug_renderings_are_redacted() {
    let key = LinkKey::from_bytes([9u8; 16]);
    let frag = key.to_fragment();
    assert_eq!(format!("{key:?}"), "LinkKey(<redacted>)");
    let p = sample_invite();
    let d = format!("{p:?}");
    assert!(!d.contains(&p.t) && !d.contains(&p.g) && !d.contains(&p.k));
    let mv = MintView {
        code: CODE.into(),
        kind: JoinKind::Direct,
        status: "valid".into(),
        exp: FAR_EXP_ISO.into(),
        share_url: Some(share_url(CODE, Some(&key)).expect("url").to_string()),
        attach_error: None,
    };
    let d = format!("{mv:?}");
    assert!(!d.contains(frag.as_str()) && !d.contains(CODE), "{d}");
}

#[test]
fn every_log_goes_through_log_line() {
    // Tripwire: the module has exactly one stderr sink, inside `emit`, which only prints
    // `log_line(..)`. A new eprintln!/println! with interpolated state would bypass redaction.
    let src = include_str!("join.rs");
    let prod = src
        .split("#[cfg(test)]\nmod tests")
        .next()
        .expect("prod part");
    assert_eq!(prod.matches("eprintln!").count(), 1);
    assert!(prod.contains("eprintln!(\"{}\", log_line(event, code, err));"));
    assert_eq!(prod.matches("println!").count(), 1); // the same eprintln!
    assert!(!prod.contains("dbg!"));
}

// --- the test-side stub service ------------------------------------------------------------------

struct Row {
    kind: JoinKind,
    status: &'static str,
    cluster_name: String,
    goal: String,
    env: Option<String>,
    resolves: u32,
    max_resolves: u32,
    revoked: bool,
    minter: String,
}

#[derive(Default)]
struct StubState {
    rows: HashMap<String, Row>,
    order: Vec<String>,
    /// Every request the stub saw, verbatim (request line + headers + body).
    requests: Vec<String>,
    seq: usize,
    fail_next_put: Option<u16>,
    /// Answer the next attach with an envHash that is not the hash of what was sent.
    lie_about_env_hash: bool,
    /// Answer every JWKS fetch with this status (an outage / misconfiguration).
    fail_jwks: Option<u16>,
    /// Serve a JWKS that carries only an unpinned kid.
    jwks_unpinned_only: bool,
    /// Answer every request (except the redirect target) with a 307 to `<base>/followed`.
    redirect_everything: bool,
}

struct StubJoinService {
    base: String,
    state: Arc<Mutex<StubState>>,
}

fn respond(stream: &mut TcpStream, status: u16, body: &serde_json::Value) {
    let text = body.to_string();
    let location = match body["location"].as_str() {
        Some(l) if (300..400).contains(&status) => format!("Location: {l}\r\n"),
        _ => String::new(),
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nCache-Control: no-store\r\n{location}Content-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    );
    let _ = stream.flush();
}

fn not_found() -> serde_json::Value {
    serde_json::json!({ "ok": false, "message": "not found" })
}

fn read_request(
    stream: &mut TcpStream,
) -> Option<(String, String, Option<String>, String, String)> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut head = String::new();
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let request_line = line.trim_end().to_string();
    head.push_str(&line);
    let mut len = 0usize;
    let mut chunked = false;
    let mut auth = None;
    loop {
        line.clear();
        reader.read_line(&mut line).ok()?;
        head.push_str(&line);
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim().to_string());
            if k == "content-length" {
                len = v.parse().unwrap_or(0);
            } else if k == "transfer-encoding" && v.eq_ignore_ascii_case("chunked") {
                chunked = true;
            } else if k == "authorization" {
                auth = Some(v);
            }
        }
    }
    let mut body = Vec::new();
    if chunked {
        loop {
            line.clear();
            reader.read_line(&mut line).ok()?;
            let n = usize::from_str_radix(line.trim(), 16).ok()?;
            let mut chunk = vec![0u8; n + 2];
            reader.read_exact(&mut chunk).ok()?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
    } else {
        body.resize(len, 0);
        reader.read_exact(&mut body).ok()?;
    }
    let body = String::from_utf8(body).ok()?;
    let mut parts = request_line.split(' ');
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    Some((method, path, auth, body, head))
}

impl StubJoinService {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base = format!("http://{}/api/join", listener.local_addr().expect("addr"));
        let state = Arc::new(Mutex::new(StubState::default()));
        let st = state.clone();
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                if let Some((method, path, auth, body, head)) = read_request(&mut stream) {
                    let mut s = st.lock().unwrap_or_else(|e| e.into_inner());
                    s.requests.push(format!("{head}{body}"));
                    let (status, out) = if s.redirect_everything && path != "/api/join/followed" {
                        (307, serde_json::json!({ "location": format!("http://{addr}/api/join/followed") }))
                    } else {
                        handle(&mut s, &method, &path, auth.as_deref(), &body)
                    };
                    drop(s);
                    respond(&mut stream, status, &out);
                }
            }
        });
        StubJoinService { base, state }
    }

    fn client(&self) -> JoinClient {
        // The production join HTTP client (no redirects), as `build_join_state` uses.
        JoinClient::new(JoinConfig::for_tests(&self.base), Box::new(JoinHttp))
    }

    fn state(&self) -> std::sync::MutexGuard<'_, StubState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn minter_of(auth: Option<&str>) -> Option<String> {
    match auth {
        Some(h) if h == format!("Bearer {BEARER}") => Some("minter-a".into()),
        Some("Bearer other-member-token") => Some("minter-b".into()),
        _ => None,
    }
}

fn handle(
    s: &mut StubState,
    method: &str,
    path: &str,
    auth: Option<&str>,
    body: &str,
) -> (u16, serde_json::Value) {
    let unauthorized = (
        401,
        serde_json::json!({ "ok": false, "message": "unauthorized" }),
    );
    let rest = path.strip_prefix("/api/join").unwrap_or("");
    match (method, rest) {
        ("POST", "") => {
            let Some(minter) = minter_of(auth) else {
                return unauthorized;
            };
            let Ok(req) = serde_json::from_str::<serde_json::Value>(body) else {
                return (
                    400,
                    serde_json::json!({ "ok": false, "message": "invalid json" }),
                );
            };
            let name = req["clusterName"].as_str().unwrap_or("").to_string();
            if name.to_ascii_lowercase().contains("citrate") {
                return (
                    422,
                    serde_json::json!({ "ok": false, "message": "cluster name not allowed" }),
                );
            }
            let kind = if req["kind"] == "direct" {
                JoinKind::Direct
            } else {
                JoinKind::Referral
            };
            let alpha: Vec<char> = CODE_ALPHABET.chars().collect();
            let code = format!("Ab3dE7g{}", alpha[s.seq % alpha.len()]);
            s.seq += 1;
            let status = if kind == JoinKind::Direct {
                "pending"
            } else {
                "valid"
            };
            s.rows.insert(
                code.clone(),
                Row {
                    kind,
                    status,
                    cluster_name: name,
                    goal: req["goal"].as_str().unwrap_or("general").to_string(),
                    env: None,
                    resolves: 0,
                    // landing MINT_CAPS: direct 1..=50 (default 50), referral up to 5,000.
                    max_resolves: req["maxResolves"].as_u64().map(|n| n as u32).unwrap_or(
                        if kind == JoinKind::Direct { 50 } else { 5000 },
                    ),
                    revoked: false,
                    minter,
                },
            );
            s.order.insert(0, code.clone());
            let mut out = serde_json::json!({
                "ok": true, "code": code, "kind": kind.as_str(), "status": status,
                "exp": FAR_EXP_ISO, "url": format!("https://www.citrate.ai/join/{code}"),
            });
            if kind == JoinKind::Direct {
                out["aad"] = serde_json::json!({ "v": 1, "kind": "direct", "code": code });
            }
            (201, out)
        }
        ("GET", "/jwks") => match (s.fail_jwks, s.jwks_unpinned_only) {
            (Some(st), _) => (st, serde_json::json!({ "ok": false, "message": "not configured" })),
            (None, true) => (200, serde_json::json!({ "keys": [jwk("rogue", PUB_ROGUE)] })),
            (None, false) => (200, served_jwks()),
        },
        ("GET", "/mine") => {
            let Some(minter) = minter_of(auth) else {
                return unauthorized;
            };
            let codes: Vec<serde_json::Value> = s
                .order
                .iter()
                .filter_map(|c| s.rows.get(c).map(|r| (c, r)))
                .filter(|(_, r)| r.minter == minter)
                .map(|(c, r)| {
                    let status = if r.revoked {
                        "revoked"
                    } else if r.resolves >= r.max_resolves {
                        "exhausted"
                    } else {
                        r.status
                    };
                    serde_json::json!({ "code": c, "kind": r.kind.as_str(), "status": status, "exp": FAR_EXP_ISO, "resolveCount": r.resolves })
                })
                .collect();
            (200, serde_json::json!({ "ok": true, "codes": codes }))
        }
        (_, seg) if seg.len() == 1 + CODE_LEN => {
            let code = seg[1..].to_string();
            match method {
                "GET" => {
                    let Some(r) = s.rows.get_mut(&code) else {
                        return (404, not_found());
                    };
                    if r.revoked || r.status != "valid" || r.resolves >= r.max_resolves {
                        return (404, not_found());
                    }
                    r.resolves += 1;
                    let spec = ResolveSpec {
                        code: code.clone(),
                        kind: r.kind,
                        cluster_name: Some(r.cluster_name.clone()),
                        goal: Some(r.goal.clone()),
                        inviter: Some("0x1234…abcd".into()),
                        inviter_verified: true,
                        exp: FAR_EXP,
                        exp_iso: FAR_EXP_ISO.into(),
                        env: r.env.clone(),
                        kid: "join-1".into(),
                        seed: SEED_CURRENT,
                    };
                    (200, spec.body_json())
                }
                "PUT" => {
                    let Some(minter) = minter_of(auth) else {
                        return unauthorized;
                    };
                    if let Some(st) = s.fail_next_put.take() {
                        return (
                            st,
                            serde_json::json!({ "ok": false, "message": "unavailable" }),
                        );
                    }
                    let Ok(req) = serde_json::from_str::<serde_json::Value>(body) else {
                        return (
                            400,
                            serde_json::json!({ "ok": false, "message": "invalid json" }),
                        );
                    };
                    let env = req["env"].as_str().unwrap_or("").to_string();
                    let Some(r) = s.rows.get_mut(&code) else {
                        return (404, not_found());
                    };
                    if r.minter != minter
                        || r.kind != JoinKind::Direct
                        || r.revoked
                        || req["envVersion"] != 1
                    {
                        return (404, not_found());
                    }
                    match (r.status, r.env.as_deref()) {
                        ("pending", _) => {
                            r.env = Some(env.clone());
                            r.status = "valid";
                        }
                        ("valid", Some(existing)) if existing == env => {}
                        _ => {
                            return (
                                409,
                                serde_json::json!({ "ok": false, "message": "envelope already attached" }),
                            )
                        }
                    }
                    let mut h = envelope_hash(&env).unwrap_or_default();
                    if std::mem::take(&mut s.lie_about_env_hash) {
                        h = envelope_hash(&sealed_env(&code).1).unwrap_or_default();
                    }
                    (
                        200,
                        serde_json::json!({ "ok": true, "code": code, "kind": "direct", "status": "valid", "exp": FAR_EXP_ISO, "envVersion": 1, "envHash": h }),
                    )
                }
                "DELETE" => {
                    let Some(minter) = minter_of(auth) else {
                        return unauthorized;
                    };
                    let Some(r) = s.rows.get_mut(&code) else {
                        return (404, not_found());
                    };
                    if r.minter != minter {
                        return (404, not_found());
                    }
                    r.revoked = true;
                    (
                        200,
                        serde_json::json!({ "ok": true, "code": code, "status": "revoked" }),
                    )
                }
                _ => (405, serde_json::json!({ "ok": false })),
            }
        }
        _ => (404, not_found()),
    }
}

fn direct_req() -> MintRequest {
    MintRequest {
        kind: JoinKind::Direct,
        cluster_name: "Garden club".into(),
        goal: "storage".into(),
        ttl_seconds: Some(7 * 86_400),
        max_resolves: None,
    }
}

fn referral_req() -> MintRequest {
    MintRequest {
        kind: JoinKind::Referral,
        ttl_seconds: None,
        ..direct_req()
    }
}

// --- end to end ----------------------------------------------------------------------------------

#[test]
fn e2e_mint_attach_resolve_decrypt_equals_the_original_invite() {
    let stub = StubJoinService::start();
    let inviter = stub.client();
    let invite = sample_invite();
    let minted = inviter
        .mint(&FixedBearer(BEARER), &direct_req(), Some(&invite))
        .expect("mint");
    assert_eq!(minted.status, "valid");
    assert_eq!(minted.attach_error, None);
    let url = minted.share_url.clone().expect("share url");
    assert!(url.starts_with(&format!("https://citrate.ai/join/{}#", minted.code)));
    let frag = url.split_once('#').expect("fragment").1.to_string();
    assert_eq!(frag.len(), 22);

    // A different device (fresh client, no shared state) opens the link.
    let invitee = stub.client();
    let view = invitee.resolve(&url).expect("resolve");
    assert_eq!(view.code, minted.code);
    assert_eq!(view.kind, JoinKind::Direct);
    assert!(view.actionable && !view.key_missing);
    assert_eq!(view.cluster_name.as_deref(), Some("Garden club"));
    let opened = invitee.take_opened(&minted.code).expect("opened invite");
    assert_eq!(opened, invite);
    assert_eq!(opened.to_invite_link(), invite.to_invite_link());
    assert!(
        invitee.take_opened(&minted.code).is_none(),
        "held once, handed over once"
    );

    // Server-blind: K and the relay token never reached the service; resolve carried no bearer.
    let s = stub.state();
    let mut saw_put = false;
    for r in &s.requests {
        assert!(!r.contains(&frag), "K reached the service");
        assert!(!r.contains(&invite.t), "relay token reached the service");
        assert!(!r.contains(&invite.g), "group id reached the service");
        if r.starts_with("GET /api/join/Ab3dE7g") {
            assert!(
                !r.to_ascii_lowercase().contains("authorization:"),
                "resolve sent a bearer"
            );
        }
        if r.starts_with("POST") || r.starts_with("PUT") {
            assert!(r.contains(&format!("Bearer {BEARER}")));
        }
        saw_put |= r.starts_with("PUT ");
    }
    assert!(saw_put);
}

#[test]
fn e2e_resolve_limit_and_unknown_revoked_exhausted_are_one_not_found() {
    let stub = StubJoinService::start();
    let c = stub.client();
    // Resolve is not redeem: a direct code allows 1..=50 resolves (default 50); the relay redeem
    // is the single-use step. Mint one with a limit of 1 to reach "exhausted".
    let one = MintRequest {
        max_resolves: Some(1),
        ..direct_req()
    };
    let m = c
        .mint(&FixedBearer(BEARER), &one, Some(&sample_invite()))
        .expect("mint");
    let url = m.share_url.expect("url");
    assert!(c.resolve(&url).is_ok());
    let exhausted = c.resolve(&url);
    let unknown = c.resolve("https://citrate.ai/join/Zz9zZz9z");
    let m2 = c
        .mint(&FixedBearer(BEARER), &referral_req(), None)
        .expect("mint referral");
    c.revoke(&FixedBearer(BEARER), &m2.code).expect("revoke");
    let revoked = c.resolve(&m2.share_url.expect("url"));
    assert_eq!(exhausted, Err(JoinError::NotFound));
    assert_eq!(unknown, Err(JoinError::NotFound));
    assert_eq!(revoked, Err(JoinError::NotFound));
    // Revoke of someone else's code is the same 404.
    assert_eq!(
        c.revoke(&FixedBearer("other-member-token"), &m.code),
        Err(JoinError::NotFound)
    );
}

#[test]
fn e2e_referral_is_display_only() {
    let stub = StubJoinService::start();
    let c = stub.client();
    let m = c
        .mint(&FixedBearer(BEARER), &referral_req(), None)
        .expect("mint");
    let url = m.share_url.clone().expect("url");
    assert!(!url.contains('#'));
    let v = c.resolve(&url).expect("resolve");
    assert_eq!(v.kind, JoinKind::Referral);
    assert!(!v.actionable && !v.key_missing);
    assert!(c.take_opened(&m.code).is_none());
}

#[test]
fn e2e_direct_without_key_is_display_only_and_flagged() {
    let stub = StubJoinService::start();
    let c = stub.client();
    let m = c
        .mint(&FixedBearer(BEARER), &direct_req(), Some(&sample_invite()))
        .expect("mint");
    let v = c
        .resolve(&format!("https://citrate.ai/join/{}", m.code))
        .expect("resolve");
    assert!(v.key_missing && !v.actionable);
    // With a wrong key, the envelope doesn't open.
    let m = c
        .mint(&FixedBearer(BEARER), &direct_req(), Some(&sample_invite()))
        .expect("mint");
    let wrong = LinkKey::from_bytes([1u8; 16]).to_fragment();
    assert_eq!(
        c.resolve(&format!(
            "https://citrate.ai/join/{}#{}",
            m.code,
            wrong.as_str()
        )),
        Err(JoinError::Envelope)
    );
}

#[test]
fn e2e_attach_retry_after_outage_is_idempotent_and_a_different_body_is_409() {
    let stub = StubJoinService::start();
    let c = stub.client();
    stub.state().fail_next_put = Some(503);
    let m = c
        .mint(&FixedBearer(BEARER), &direct_req(), Some(&sample_invite()))
        .expect("mint");
    assert_eq!(m.status, "pending");
    assert_eq!(m.share_url, None);
    assert_eq!(
        m.attach_error.as_deref(),
        Some(JoinError::Unavailable.to_string().as_str())
    );
    // Pending codes resolve as the uniform 404.
    assert_eq!(c.resolve(&m.code), Err(JoinError::NotFound));
    let retried = c.attach(&FixedBearer(BEARER), &m.code).expect("attach");
    assert_eq!(retried.status, "valid");
    assert!(retried.share_url.is_some());
    // Nothing left to attach locally.
    assert_eq!(
        c.attach(&FixedBearer(BEARER), &m.code),
        Err(JoinError::NotFound)
    );

    // A different envelope for an attached code is a 409 → Conflict.
    let (_k, other_env) = sealed_env(&m.code);
    let http = UreqClient;
    let err = http
        .put_json(
            &format!("{}/{}", stub.base, m.code),
            Some(BEARER),
            &serde_json::json!({ "env": other_env, "envVersion": 1 }).to_string(),
        )
        .map_err(map_http_error);
    assert_eq!(err, Err(JoinError::Conflict));
    // The original link still opens.
    let v = c
        .resolve(&retried.share_url.expect("url"))
        .expect("resolve");
    assert!(v.actionable);
}

#[test]
fn e2e_auth_and_policy_errors_surface_honestly() {
    let stub = StubJoinService::start();
    let c = stub.client();
    assert_eq!(
        c.mint(&NoSession, &referral_req(), None),
        Err(JoinError::NotSignedIn)
    );
    assert_eq!(
        c.mint(&FixedBearer("expired"), &referral_req(), None),
        Err(JoinError::Unauthorized)
    );
    let reserved = MintRequest {
        cluster_name: "Citrate Official".into(),
        ..referral_req()
    };
    let e = c.mint(&FixedBearer(BEARER), &reserved, None);
    assert_eq!(e, Err(JoinError::NameNotAllowed));
    // The message tells the inviter what to change (task 11 shows it as-is).
    assert!(JoinError::NameNotAllowed
        .to_string()
        .contains("Latin, Greek, Cyrillic or Cherokee"));
    // Shape errors never reach the network.
    let before = stub.state().requests.len();
    assert_eq!(
        c.mint(&FixedBearer(BEARER), &direct_req(), None),
        Err(JoinError::Invalid)
    );
    assert_eq!(
        c.mint(
            &FixedBearer(BEARER),
            &referral_req(),
            Some(&sample_invite())
        ),
        Err(JoinError::Invalid)
    );
    let bad_goal = MintRequest {
        goal: "airdrop".into(),
        ..referral_req()
    };
    assert_eq!(
        c.mint(&FixedBearer(BEARER), &bad_goal, None),
        Err(JoinError::Invalid)
    );
    assert_eq!(
        c.revoke(&FixedBearer(BEARER), "../../x"),
        Err(JoinError::Invalid)
    );
    assert_eq!(stub.state().requests.len(), before);
}

#[test]
fn e2e_list_shows_the_callers_codes_only() {
    let stub = StubJoinService::start();
    let c = stub.client();
    let a = c
        .mint(&FixedBearer(BEARER), &referral_req(), None)
        .expect("mint");
    let b = c
        .mint(&FixedBearer(BEARER), &direct_req(), Some(&sample_invite()))
        .expect("mint");
    let _other = c
        .mint(&FixedBearer("other-member-token"), &referral_req(), None)
        .expect("mint");
    c.revoke(&FixedBearer(BEARER), &a.code).expect("revoke");
    let mine = c.list(&FixedBearer(BEARER)).expect("list");
    assert_eq!(mine.len(), 2);
    assert_eq!(mine[0].code, b.code);
    assert_eq!(mine[0].status, "valid");
    assert_eq!(mine[1].code, a.code);
    assert_eq!(mine[1].status, "revoked");
    assert_eq!(c.list(&NoSession), Err(JoinError::NotSignedIn));
}

#[test]
fn e2e_attach_reply_with_a_different_env_hash_is_not_trusted() {
    let stub = StubJoinService::start();
    let c = stub.client();
    stub.state().lie_about_env_hash = true;
    let m = c
        .mint(&FixedBearer(BEARER), &direct_req(), Some(&sample_invite()))
        .expect("mint");
    assert_eq!(m.status, "pending");
    assert_eq!(m.share_url, None);
    assert_eq!(
        m.attach_error.as_deref(),
        Some(JoinError::BadResponse.to_string().as_str())
    );
}

fn resolves_of(stub: &StubJoinService, code: &str) -> u32 {
    stub.state().rows.get(code).map(|r| r.resolves).unwrap_or(u32::MAX)
}

#[test]
fn e2e_direct_link_resolves_more_than_once_within_its_limit() {
    let stub = StubJoinService::start();
    let c = stub.client();
    let m = c
        .mint(&FixedBearer(BEARER), &direct_req(), Some(&sample_invite()))
        .expect("mint");
    let url = m.share_url.expect("url");
    assert!(c.resolve(&url).is_ok());
    assert!(c.resolve(&url).is_ok(), "default direct limit is 50, not 1");
    assert_eq!(resolves_of(&stub, &m.code), 2);
}

#[test]
fn e2e_jwks_failure_does_not_spend_a_resolve() {
    let stub = StubJoinService::start();
    let c = stub.client();
    let one = MintRequest {
        max_resolves: Some(1),
        ..direct_req()
    };
    let m = c
        .mint(&FixedBearer(BEARER), &one, Some(&sample_invite()))
        .expect("mint");
    let url = m.share_url.expect("url");
    stub.state().fail_jwks = Some(503);
    assert_eq!(c.resolve(&url), Err(JoinError::Unavailable));
    assert_eq!(resolves_of(&stub, &m.code), 0, "a JWKS outage spent a resolve");
    stub.state().fail_jwks = None;
    stub.state().jwks_unpinned_only = true;
    assert_eq!(c.resolve(&url), Err(JoinError::Signature));
    assert_eq!(resolves_of(&stub, &m.code), 0, "an unusable JWKS spent a resolve");
    stub.state().jwks_unpinned_only = false;
    // The single allowed resolve is still there.
    assert!(c.resolve(&url).expect("resolve").actionable);
    assert_eq!(resolves_of(&stub, &m.code), 1);
}

#[test]
fn opened_invite_survives_a_failed_redeem_and_is_handed_over_once_on_success() {
    let stub = StubJoinService::start();
    let c = stub.client();
    let m = c
        .mint(&FixedBearer(BEARER), &direct_req(), Some(&sample_invite()))
        .expect("mint");
    let view = c.resolve(&m.share_url.expect("url")).expect("resolve");
    assert!(view.actionable);
    // A transient failure (relay down): the invite stays held for a retry.
    let mut seen = Vec::new();
    let first = c.redeem_opened(&m.code, |p| {
        seen.push(p.to_invite_link().to_string());
        Err("comms: relay unreachable".to_string())
    });
    assert_eq!(first, Err("comms: relay unreachable".to_string()));
    let second = c.redeem_opened(&m.code, |p| {
        seen.push(p.to_invite_link().to_string());
        Ok(())
    });
    assert_eq!(second, Ok(()));
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0], sample_invite().to_invite_link().as_str());
    assert_eq!(seen[0], seen[1]);
    // Success consumed it.
    let third = c.redeem_opened(&m.code, |_| Ok(()));
    assert_eq!(third, Err(JoinError::NotFound.to_string()));
    assert!(c.take_opened(&m.code).is_none());
}

#[test]
fn e2e_join_http_never_follows_a_redirect() {
    let stub = StubJoinService::start();
    let c = stub.client();
    stub.state().redirect_everything = true;
    assert_eq!(
        c.mint(&FixedBearer(BEARER), &referral_req(), None),
        Err(JoinError::Rejected(307))
    );
    assert_eq!(c.resolve("https://citrate.ai/join/Ab3dE7gh"), Err(JoinError::Rejected(307)));
    assert_eq!(c.list(&FixedBearer(BEARER)), Err(JoinError::Rejected(307)));
    let s = stub.state();
    assert!(!s.requests.is_empty());
    assert!(
        s.requests.iter().all(|r| !r.contains("/api/join/followed")),
        "a redirect was followed"
    );
}
