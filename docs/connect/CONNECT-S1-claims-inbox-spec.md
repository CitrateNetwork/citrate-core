---
created: 2026-08-31
branch: feat/connect-s1-claims-inbox
author: Claude (Opus 4.8), directed by @SaulBuilds
status: spec (Stage-1)
planset: connect-realign
sprint: CONNECT-S1
repo: citrate-comms (relay + member-daemon) + citrate-core (client)
companions:
  - docs/CONNECT_REALIGN_PLANSET.md
---

# CONNECT-S1 — server-blind claims-inbox (kill the DM-back)

## Outcome (one sentence)

When an invitee opens an invite link, their signed claim is **delivered to the owner through the
server-blind relay** — sealed to a key carried in the link — and lands in the owner's **Requests**
inbox to approve in one click; **no copy-paste of the claim, no DM round-trip.**

## The mechanism (why it stays server-blind)

Today the return path is a manual clipboard/DM because there is **no relay channel a non-member can
write that the owner can read**. S1 adds exactly that, minimally:

1. `group_invite_create` mints an **ephemeral keypair**, embeds the **public** key in the link
   (`citrate://invite?g=&t=&k=`), and persists the **private** key in the owner's local `PendingInvite`.
2. The invitee opens the link → their client **seals `{group, token, address}` to `k`** → submits
   `SubmitClaim(token_hash, ciphertext)` to the relay (via the member-daemon → WS). `token_hash =
   hash(token)` so the relay keys the inbox without seeing the token.
3. The relay stores the **ciphertext opaquely** under `token_hash` (a new server-blind CF) and returns
   it on `PollClaims(token_hash)` — **it never decrypts** (same posture as message envelopes, unlike
   the public key-package directory).
4. The owner **polls**, decrypts with the ephemeral private key → the claim → a **Requests** inbox row.
5. Owner **Approve** → the *existing, unchanged* path: `group_invite_verify_consume(token)` (one-time
   gate) → `groups_add_member(address)` → the daemon consumes the invitee's already-published,
   wallet-signed key package (`take_key_package`) → MLS add. Ceremony-gated / relay-RBAC'd as today.

## Data sources / surfaces (Rule 7)

| Piece | Source |
|---|---|
| Ephemeral invite key | new: minted in `invites.rs group_invite_create`, private key in `PendingInvite` (`app_data/invites/pending.json`) |
| Claim ciphertext transport | new relay frames `SubmitClaim`/`PollClaims`/`Claims` (`comms-wire/frames.rs`), handlers on `DeliveryService` (`comms-relay/lib.rs`), CF `CF_CLAIMS` keyed `token_hash(32)` (`comms-core/store.rs`) |
| Daemon IPC | new ops mirroring the frames in `comms-member-daemon/ipc.rs` + `Relay` trait impls (`relay.rs`) |
| Client commands | new `group_invite_poll_claims` (owner, decrypts) + claim-submit on redeem (`invites.rs`/`comms.rs`) |
| Address proof | UNCHANGED — the invitee's wallet-signed `KeyPackagePublication` the relay already verified; MLS add fails closed (`NoKeyPackage`) on a wrong address |

## User stories

1. As an **invitee**, opening an invite link **sends my request to the owner automatically** — I never copy a claim and DM it back.
2. As an **owner**, incoming requests appear in a **Requests inbox I approve in one click** — I never paste a claim.
3. As **either party**, the **relay never sees my identity or the group** — the claim is sealed to the invite's key end-to-end.
4. As an **owner**, a **forged or replayed** claim fails: the token is one-time, and the address must match a real wallet-signed key package.
5. As an **invitee whose relay is unreachable**, I get an honest "couldn't send request" — never a fake "sent."

## Acceptance criteria (source + verifying test)

- **AC1** — `group_invite_create` embeds an ephemeral pubkey in the link (`k=`) and persists the private key in `PendingInvite`. *Test:* `invites` test — link contains `k=`, pending record carries the key.
- **AC2** — Redeeming seals `{group,token,address}` to the link pubkey and submits `SubmitClaim(token_hash, ciphertext)`; **nothing is written to the clipboard**. *Test:* client test — redeem yields a submit call, not a clipboard copy.
- **AC3** — The relay round-trips the ciphertext under `token_hash` **without decrypting** (server-blind). *Test:* `comms-relay` test — submit+poll returns the same ciphertext; a ciphertext-only assertion (mirroring the envelope test) proves the relay never reads plaintext.
- **AC4** — The owner poll decrypts to the claim → Requests inbox; Approve runs `verify_consume` then `add_member(address)`. *Test:* client test — poll → decrypt → claim; approve consumes the token and calls add_member.
- **AC5** — A **replayed** (already-consumed) token or a **wrong-group** claim is rejected. *Test:* `verify_consume` single-use; group-mismatch rejected.
- **AC6** — `SubmitClaim` requires a **SIWE session but not group membership** (pre-membership); the members-only group-submit path is unchanged. *Test:* relay test — an authenticated non-member can SubmitClaim; `non_member_cannot_submit` still holds for envelopes.
- **AC7** — Relay/daemon failures surface **honestly** ("couldn't send/receive request"), never a fabricated success. *Test:* transport error → honest error, no fake claim.

## Out of scope (S1)

- The **people-picker one-click add** (S2) — S1 is the connect/claim plumbing.
- **Outbound delivery of the invite link** — still shared however the owner chooses; S1 automates only the *return* path.
- **X/Discord repositioning** (S3), the **role navigator** (S4), the **wallet↔comms seam** (S5).

## Simulation boundary + ship reality (explicit)

- In **sim / no-relay** mode, submit/poll are **honest no-ops (Unavailable)** — never a fabricated delivered claim.
- **The code is "done" when relay + client build and their tests are green.** LIVE end-to-end additionally
  requires two **ops** steps, flagged here and not part of the code's definition-of-done: **(1) the
  citrate-comms relay redeployed** with the new frames + CF, and **(2) the `comms-member-daemon` rebuilt
  and re-bundled** into citrate-core. Until both land, the client honestly reports the claims-inbox as
  unavailable rather than pretending — the manual link/claim path (S0-era) remains as the fallback.

## Security invariants (preserved / added)

- **Server-blind:** the claim is sealed to the invite's ephemeral pubkey; the relay stores/returns opaque
  bytes and never decrypts (like envelopes). `token_hash` (not the token) keys the inbox.
- **One-time bearer:** the token stays unguessable + single-use (`verify_consume` unchanged).
- **Address is proven, not asserted:** MLS `add_member` consumes the invitee's wallet-signed key package;
  a forged address has no matching package and fails closed.
- **Approve is the authz gate:** the owner's one-click approve is a ceremony-signed / relay-RBAC'd add,
  exactly as today. `SubmitClaim` needs SIWE, never membership; nothing is auto-added.
