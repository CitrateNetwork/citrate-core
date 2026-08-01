---
title: "citrate-core — Commissary client QA, 2026-08-01"
created: 2026-08-01
branch: qa/commissary-honesty-2026-08-01
author: Claude (Opus 5, 1M) for SaulBuilds
status: complete — ready for blind internal audit
scope: the Commissary surface + its seam. Second app in the Commissary QA cadence.
follows: core-membership QA (docs/QA_2026-07-30_CORE_MEMBERSHIP.md in that repo)
---

# citrate-core Commissary client — QA 2026-08-01

Second app in the cadence. core-membership was **the gate**; this is **the client** —
the surface a member actually looks at, and where the `setTimeout` download
simulation lived.

**Verdict: the surface was making four claims it could not support.** All four are
fixed and tripwired. The underlying client (WS-G / CM-3) is still unbuilt and is
still blocked on the artifact store — but the surface now says so instead of
pretending otherwise.

`294 → 300` frontend tests, typecheck clean, Rust untouched (Rule 2 count unchanged).

---

## 1. The headline

**The honest seam already existed and the surface was routing around it.**

`commissary_catalog()` in `src-tauri/src/seam.rs` does exactly the right thing: it
returns `unavailable:` and has two tests pinning that it never fabricates a value.
The Rule-1 guarantee at the Rust boundary was intact.

The surface never called it. It imported `CATALOG` from `src/data/seed.ts` — a local
TypeScript array — and rendered it under the header **"catalog · signed manifest v3"**.
So the guarantee was real, and never reached the screen.

This is worth naming because it is a failure mode a review can miss: every individual
piece was honest, and the composition was not.

---

## 2. Defects found and fixed

### C-1 — Fabricated cryptographic verification (Rule 1, high)

`startDownload` was a `setTimeout` chain: `mint → dl → verify → done`. It streamed no
bytes, computed no digest, and wrote no audit row. It then rendered:

> `✓ verified · sha256 match · audit-logged`

Three false claims in one line, two of them about cryptography.

**This is an unfixed variant of an already-remediated bug class.** Storage carried the
identical pattern — a fake progress bar ending in a "sha256 verified" claim — and it
was *removed* there rather than restyled (`storageHonesty.test.tsx` documents it).
Same remedy applied here.

### C-2 — Fail-open tier gating (authorization display, high)

```
const locked = RANK[a.minTier] > RANK[effTier];
```

A tier absent from `RANK` yields `undefined`, and `2 > undefined` is `false`. **An
unrecognised tier rendered every gated card UNLOCKED.**

Reachable, not theoretical: the identity authority mints the tier ladder, `RANK` is a
client-side copy of it, and the two are already known to disagree (federation memory
records that RPs dispute `commercial.kyc`'s rank). `rankOf` now collapses an unknown
tier to `-1` — least access, never most.

### C-3 — Placeholder digests dressed as provenance (Rule 1, medium)

The seed ships elided checksums: `sha256:2f8e17aa…c9c41`. A hash with an ellipsis in
it cannot verify anything. Rendering it in a mono font beside the word "verified"
makes a placeholder look like evidence. Now shown as `checksum pending · no verified
release` until a real release ledger fills them.

### C-4 — A local seed labelled a signed manifest (Rule 1, medium)

Covered in §1. Header now reads `catalog · local seed · not yet the signed manifest`.

---

## 3. On the tests — one of them was vacuous, and that matters

The first version of the sha256 tripwire pinned only the `done` download state. When
the render moved to an `unavailable` branch, the assertion stopped reaching the markup
and **passed vacuously** — it survived a mutant that restored the false claim verbatim.

It now sweeps every download state (`idle`, `mint`, `dl`, `verify`, `done`,
`unavailable`), so it bites regardless of which branch a future client wires up.

Recording this because a green tripwire that cannot fail is worse than no tripwire: it
buys confidence it has not earned. Every assertion in this pass was mutation-checked
for that reason.

**Four mutants, all killed:**

| Mutant | Failures |
|---|---|
| restore `✓ verified · sha256 match · audit-logged` | 1 |
| `rankOf` → fail-open | 2 |
| drop the checksum placeholder guard | 1 |
| restore `signed manifest v3` | 1 |

---

## 4. Findings NOT fixed — for audit and owner decision

### C-5 — `RANK` collapses tiers the server distinguishes (drift, high)

`src/shell/state.ts`:

```
free: 0, public: 0,
pilot: 1, commercial: 1, "commercial.kyc": 1, academic: 1,
enterprise: 2, confidential: 2,
```

core-membership ranks these `public:0, commercial:1, commercial.kyc:2, academic:3,
confidential:4`. **The client flattens `commercial`, `commercial.kyc`, and `academic`
into one rank.** A plain `commercial` member therefore sees `commercial.kyc` artifacts
as unlocked — precisely the verification floor landed in core-membership#30, invisible
on the client.

Not an auth bypass: the server re-checks entitlement and KYC at download time, so the
member is refused on click. It is a UX-and-honesty defect — the surface promises access
the server will deny.

**Not fixed deliberately: `state.ts` is modified by open PR #114** (`feat/m2-bond-status`).
Changing `RANK` underneath an in-flight PR invites a conflict in a file that also
carries grant-status state. This should land immediately after #114 merges, ideally as
part of retiring the client-side copy in favour of the shared entitlement contract
(`CL-C1` / `@citrate/oidc-client`).

### C-6 — Service cards carry no tier at all (modelling gap, medium)

`CATALOG.services` entries are `{ id, name, desc, url }`. There is no tier field, so
the surface **cannot** distinguish a genuinely public link (`explorer.citrate.ai`,
`dashboard.citrate.ai`) from a restricted one (`dataroom`, which is 506(b)-gated).

Deliberately not asserted in tests: blanket-gating would encode something false, and
leaving them open under-states the dataroom. The seed needs a tier per service before
this can be gated honestly. This is a decision, not a bug fix.

### C-7 — The client still does not exist (WS-G / CM-3)

The surface is now honest about being unwired, which is the most that can be true today.
The real client — fetch the signed manifest, verify EdDSA against JWKS, redeem the
single-use URL, stream, verify sha256 — remains unbuilt and remains blocked on
`ARTIFACT_STORE_BASE`. **Nothing in this pass moved that.**

---

## 5. What an auditor should attack first

1. **The `RANK` copy (C-5).** It is a hand-maintained duplicate of a contract owned by
   another service. Duplicated ladders drift; this one already has.
2. **`store.openMicroApp`.** Micro-apps are the capability-bridge surface (WS-F/CM-4,
   spec-only). Whatever it does today is worth reading before the bridge is built on it.
3. **The seed-vs-seam split.** The pattern that produced C-4 — an honest backend seam
   the frontend bypasses — may exist on other surfaces. Worth a sweep rather than a
   spot-check.

---

## 6. Cadence + branch note

PR: `qa/commissary-honesty-2026-08-01` → `main`.

**Deliberately avoided `src/shell/state.ts`, `src/shell/store.ts`, `src/bridge/domains.ts`,
and `src/bridge/sim/index.ts`** — all four are modified by open PR #114. This branch
touches only `src/surfaces/Commissary.tsx` and a new test file, so it should merge
without conflict in either order.

**Commit attribution note:** `CLAUDE.md` specifies `Co-Authored-By: Claude Fable 5`.
These commits are attributed to Claude Opus 5, because that is the model that wrote
them. In a repo whose first rule is that nothing is fabricated, the trailer should not
be either. If the intent is a fixed project trailer rather than real attribution, say so
and I will follow it.

Next in the cadence: **citrate-quorum** (newly pulled into the federation, anchoring
back online, absent from the catalog entirely).
