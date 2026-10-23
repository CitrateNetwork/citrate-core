# created: 2026-10-23 | branch: test/join-e2e-gate | author: Larry Klosowski + Claude Opus 5.5 (DGX lane)
# status: active. Join-E2E harness only (scripts/ci/join-e2e.sh). Run with the system python3 -I
# (needs python3-nacl and python3-cryptography, both Ubuntu 24.04 packages).
"""Invite crypto for the Join-E2E gate, written from the contract (citrate-core kit/src/join.rs,
citrate-landing src/lib/join.ts), not from either implementation.

  keygen <dir>                 a THROWAWAY Ed25519 resolve-signing key: <dir>/signing.jwk (private,
                               0600, for the local landing's JOIN_SIGNING_JWK) and <dir>/pin.txt
                               ("<kid> <x>", the pin a test build of the app carries)
  seal <code> <dir>            read the relay invite {g,t,k[,n]} as JSON on stdin; draw a link key K;
                               write <dir>/env.txt (the envelope) and <dir>/key.txt (K, 0600)
  verify <resolve.json> <pin.txt> <key.txt>
                               check a GET /api/join/<code> answer the way the app must: the JWT's
                               kid and key equal the pin, EdDSA signature, issuer, subject = code,
                               not expired, every response field equal to its signed claim, envHash =
                               SHA-256(env), and the envelope opens under K with the bound AAD.
                               Prints a one-line summary (no K, no token, no key material).
  selfcheck                    the known answers citrate-core kit/src/join_tests.rs pins (HKDF over
                               K = 00..0f, envHash over bytes 00..27), so this file and the app
                               agree before the gate relies on either

Envelope v1: K = 16 random bytes (22 base64url chars); AEAD key = HKDF-SHA256(ikm=K, salt=none,
info="citrate-join-envelope-v1"); XChaCha20-Poly1305, 24-byte random nonce, AAD =
"citrate-join-v1|<kind>|<code>"; env = base64url(nonce || ciphertext || tag), unpadded.
"""

import base64
import hashlib
import hmac
import json
import os
import sys
import time

from nacl.bindings import (
    crypto_aead_xchacha20poly1305_ietf_decrypt,
    crypto_aead_xchacha20poly1305_ietf_encrypt,
)
from nacl.exceptions import BadSignatureError, CryptoError
from nacl.signing import SigningKey, VerifyKey

ISSUER = "https://citrate.ai/join"
KID = "join-1"
HKDF_INFO = b"citrate-join-envelope-v1"
ENV_VERSION = 1
# Claims the service signs that the app checks against the response body (landing ResolvedInvite).
SIGNED_FIELDS = ("v", "status", "kind", "code", "clusterName", "goal", "inviter", "inviterVerified",
                 "envVersion", "envHash")


def b64u(b: bytes) -> str:
    return base64.urlsafe_b64encode(b).rstrip(b"=").decode()


def unb64u(s: str) -> bytes:
    return base64.urlsafe_b64decode(s + "=" * (-len(s) % 4))


def hkdf_sha256(ikm: bytes, info: bytes, length: int = 32) -> bytes:
    prk = hmac.new(b"\x00" * 32, ikm, hashlib.sha256).digest()  # salt = none -> HashLen zeros
    out, t, i = b"", b"", 1
    while len(out) < length:
        t = hmac.new(prk, t + info + bytes([i]), hashlib.sha256).digest()
        out += t
        i += 1
    return out[:length]


def aad(kind: str, code: str) -> bytes:
    return f"citrate-join-v{ENV_VERSION}|{kind}|{code}".encode()


def write_private(path: str, text: str) -> None:
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w") as f:
        f.write(text)


def cmd_keygen(d: str) -> int:
    sk = SigningKey.generate()
    x = b64u(bytes(sk.verify_key))
    jwk = {"kty": "OKP", "crv": "Ed25519", "kid": KID, "x": x, "d": b64u(bytes(sk))}
    write_private(os.path.join(d, "signing.jwk"), json.dumps(jwk))
    with open(os.path.join(d, "pin.txt"), "w") as f:
        f.write(f"{KID} {x}\n")
    return 0


def cmd_seal(code: str, d: str) -> int:
    invite = json.load(sys.stdin)
    plain = {k: invite[k] for k in ("g", "t", "k")}
    if invite.get("n"):
        plain["n"] = invite["n"]
    key = os.urandom(16)
    nonce = os.urandom(24)
    ct = crypto_aead_xchacha20poly1305_ietf_encrypt(
        json.dumps(plain, separators=(",", ":")).encode(), aad("direct", code), nonce,
        hkdf_sha256(key, HKDF_INFO))
    with open(os.path.join(d, "env.txt"), "w") as f:
        f.write(b64u(nonce + ct))
    write_private(os.path.join(d, "key.txt"), b64u(key))
    return 0


class Reject(Exception):
    pass


def cmd_verify(resolve_path: str, pin_path: str, key_path: str) -> int:
    body = json.load(open(resolve_path))
    pin_kid, pin_x = open(pin_path).read().split()
    key = unb64u(open(key_path).read().strip())
    try:
        if body.get("ok") is not True:
            raise Reject(f"resolve not ok: {body.get('message', '?')}")
        tok = body.get("sig")
        if not isinstance(tok, str) or tok.count(".") != 2:
            raise Reject("no compact JWT in sig")
        h64, p64, s64 = tok.split(".")
        header = json.loads(unb64u(h64))
        if header.get("alg") != "EdDSA":
            raise Reject("alg is not EdDSA")
        if header.get("kid") != pin_kid:
            raise Reject("kid is not the pinned kid")
        try:
            VerifyKey(unb64u(pin_x)).verify(f"{h64}.{p64}".encode(), unb64u(s64))
        except BadSignatureError:
            raise Reject("signature does not verify under the pinned key")
        claims = json.loads(unb64u(p64))
        if claims.get("iss") != ISSUER:
            raise Reject("issuer mismatch")
        if claims.get("sub") != body.get("code"):
            raise Reject("subject is not the code")
        if not isinstance(claims.get("exp"), int) or claims["exp"] <= time.time():
            raise Reject("signed expiry missing or past")
        for f in SIGNED_FIELDS:
            if f in body or f in claims:
                if body.get(f) != claims.get(f):
                    raise Reject(f"response field {f} differs from its signed claim")
        if body.get("kind") != "direct":
            raise Reject("not a direct invite")
        env = body.get("env")
        if not isinstance(env, str):
            raise Reject("direct invite without an envelope")
        raw = unb64u(env)
        if b64u(hashlib.sha256(raw).digest()) != claims.get("envHash"):
            raise Reject("envHash does not match the envelope")
        try:
            plain = crypto_aead_xchacha20poly1305_ietf_decrypt(
                raw[24:], aad("direct", body["code"]), raw[:24], hkdf_sha256(key, HKDF_INFO))
        except CryptoError:
            raise Reject("envelope does not open under the link key")
        inv = json.loads(plain)
        if not all(isinstance(inv.get(k), str) and inv[k] for k in ("g", "t", "k")):
            raise Reject("opened envelope is not a relay invite")
    except Reject as e:
        print(f"REJECT {e}")
        return 1
    print(f"ok kid={pin_kid} kind=direct group={inv['g'][:12]}... exp_in={claims['exp'] - int(time.time())}s")
    return 0


def cmd_selfcheck() -> int:
    okm = hkdf_sha256(bytes(range(16)), HKDF_INFO).hex()
    eh = b64u(hashlib.sha256(bytes(range(40))).digest())
    ok = (okm == "be763053d8bcb5a9e70a3ef3d38e8cac1808bac2dc7eef32c24358f867c2365d"
          and eh == "X6pO7DYRVWgSwtdLQ3yMSa3T-RDxAGPYAUQffXXNXjs"
          and aad("direct", "aBcDeFgH").hex()
          == "636974726174652d6a6f696e2d76317c6469726563747c6142634465466748")
    print("selfcheck ok (hkdf, envHash, aad known answers)" if ok else "selfcheck FAILED")
    return 0 if ok else 1


def main(argv: list) -> int:
    if argv == ["selfcheck"]:
        return cmd_selfcheck()
    if len(argv) >= 2 and argv[0] == "keygen":
        return cmd_keygen(argv[1])
    if len(argv) >= 3 and argv[0] == "seal":
        return cmd_seal(argv[1], argv[2])
    if len(argv) >= 4 and argv[0] == "verify":
        return cmd_verify(argv[1], argv[2], argv[3])
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
