---
created: 2026-10-01T00:00:00Z
branch: hup/n3-hf-token
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S0
wp: HUP-S0.3b
issue: CitrateNetwork/citrate-federation#291
---

# HUP-S0.3b: Hugging Face token for gated model downloads

Closes the open item on the `g1-downloads` row of [EVIDENCE.md](EVIDENCE.md)
("S0.3b HF token for gated repos"). T1 code: needs a security reviewer before merge.

## What changed

| File | Change |
|---|---|
| `src-tauri/src/hf_auth.rs` (new) | `HfToken` (zeroizing, redacted `Debug`, refuses any byte outside visible ASCII), `AuthScope` (exact origins: `https://huggingface.co:443`, `https://hf.co:443`, no userinfo), `header_for`, and `fetch`: a GET that follows redirects by hand (max 10), recomputing `Authorization` per hop from that hop's origin. `401`/`403` from an in-scope origin is `FetchError::Gated`, with the member-facing text pointing at Settings › Connections. `connected_token(app)` reads the token from the vault. |
| `src-tauri/src/connections.rs` | `sealed_access_token(service, vault)`: crate-private reader of the sealed connection record. `None` when not connected, vault locked (no forced unlock), unreadable, or past the provider's stated expiry. No `#[tauri::command]` returns it (I-2 barrier unchanged). |
| `src-tauri/src/model.rs` | `UreqModelTransport` goes through `hf_auth::fetch` for `total_size`, `get_from`, `get_range`; builder `with_hf_token`. New `ModelError::Gated { token_sent }` is final in the download loop (no backoff retries against a `401`). The `206` resume gate is unchanged. |
| `src-tauri/src/model_catalog.rs` | `model_catalog_download` attaches the connected HF token only when the descriptor's source is Hugging Face. |
| `src/shell/slices/models.test.ts` | Pins that the gated message reaches the Models error banner verbatim. |

Data source (Rule 7): the token is the `connection-hf` custody vault slot written by the existing
Settings › Connections OAuth flow (`connections.rs`, scope `read-repos`). No new storage.

## Behavior for members

- Hugging Face not connected: identical to before for public repos (no `Authorization` sent
  anywhere). A gated or private repo now fails with: "This model needs a Hugging Face token with
  access. Add it in Settings › Connections (connect Hugging Face), and accept the model's terms on
  its Hugging Face page if it asks." instead of a raw `http status: 401`.
- Hugging Face connected: the token rides only the `huggingface.co` / `hf.co` hops. The resolve
  redirect to the CDN (`cdn-lfs*.hf.co`, `*.xethub.hf.co`, `us.aws.cdn.hf.co`, …) never carries it.
- The default Gemma download (`citrate.ai/download/model` vanity redirect) does not read the token.

## Evidence

| Gate | Result |
|---|---|
| Red first | `hf_auth` tests with a host-only scope check: 4 failed (cross-origin hop on another port carried the token; CDN `403` misreported as gated). `connections` tests: compile-red before `sealed_access_token` existed. `transport_*` tests: compile-red before `with_hf_token`/`ModelError::Gated`. |
| `cargo test --lib` (src-tauri) | 533 passed / 5 ignored (base `525b9ca`) → **557 passed / 5 ignored** (+24: 22 `hf_auth`, 2 `connections`) |
| Wire proof | `cross_origin_redirect_strips_authorization`, `transport_segment_strips_authorization_on_the_cdn_hop_and_keeps_the_206_gate`, `transport_total_size_follows_the_redirect_without_leaking_the_token`: two real loopback HTTP servers on different origins; the raw request head received by the redirect target contains no `Authorization` line and no token bytes, while `Range` survives. |
| Mutation check | 7 mutants, 7 killed: drop the expiry check; drop the in-scope condition on gated; attach the token regardless of origin; accept any redirect scheme; loosen the hop bound; loosen the token byte filter; drop the `Gated` mapping in the transport. |
| clippy 1.98.1 `-p citrate-core --all-targets -D warnings` | clean |
| `main_thread_tripwire` | 3/3 pass (no new commands; the download stays on `spawn_blocking`) |
| `npx tsc --noEmit` | clean |
| `npx vitest run` | 797 passed / 3 skipped (+1) |
| `cargo test --workspace` | 769 passed / 6 ignored (src-tauri 557 + kit 212), 0 failed |

## Not done (honest)

- Search (`model_catalog_search`) and id re-resolution (`resolve_by_id`) still run without the
  token. On Hugging Face a gated repo lists its files without a token, so the download is the step that needed it;
  private repos (which hide even the listing) are not reachable from the catalog yet.
- No token refresh: an expired OAuth token is treated as absent (never sent), so the member sees
  the gated message and reconnects. A refresh-token flow is a separate WP.
- No paste-a-token path; the token comes only from the existing Connections OAuth flow (which
  needs `HF_CLIENT_ID` / `HF_CLIENT_SECRET` configured for the app).
- Live proof against a real gated repo on huggingface.co is a manual QA step (needs a member
  account that has accepted a gated model's terms).

## Journal

The first instinct was to lean on ureq: its default already strips `Authorization` on every
redirect. That default is the reason the token could not simply be attached: Hugging Face
sometimes redirects within its own origin (renamed repos, resolve caches), and a blanket strip
loses the token exactly where a gated repo needs it. Following redirects by hand made the rule
explicit and testable per hop: the header is a function of the hop's origin, never of the
request that started the chain. Matching on the full origin (scheme, host, port) rather than the
host alone is also what made the loopback test honest: two servers on one IP are different
origins, and the red run showed the host-only check leaking across them.
