---
created: 2026-09-11
branch: (unstarted)
author: (assign at start)
status: backlog
sprint: sprint-telemetry-optin
tier: T1
---

# Sprint — Opt-In Diagnostics (private, local, anonymous)

A **local-first, consent-gated diagnostic reporter** — NOT analytics. The whole point:
make it easy for a member to send us a crash/fix report, while never spying or monitoring.
The invariant that makes it not-spying: **exactly one network egress — a user-clicked
"Send report" — and nothing else ever leaves the machine.** No beacon, no heartbeat, no
usage pings. Ships in **v0.2.5** (new code → new version; this is NOT a re-notarize of 0.2.4).

## Locked design (owner, 2026-09-11)
- **Off by default.** Reuse the existing `telemetry: boolean` state (already `false`,
  persisted) as the opt-in gate.
- **Local capture, always local.** ErrorBoundary (#3) + a Rust `panic::set_hook` write
  crashes to a bounded local diagnostics file in app-data; reuse the `node_logs` ring for a
  redacted recent-log tail. On-device regardless of sending.
- **Manual + on-crash prompt.** Default is a Settings "Send a diagnostic report" action;
  when the toggle is on, a crash also *prompts* ("Something broke — send a report?"). NEVER
  silent auto-send.
- **Anonymous.** No persistent device/user id; each report carries an EPHEMERAL random id,
  never stored or linked. Scrub before send: `$HOME → ~`, drop all `0x…` addresses / emails
  / OIDC subs / tokens.
- **Transparent (Rule 1).** The exact JSON is shown for review before sending; cancelable.
- **Standalone ingest.** A dumb, append-only `POST /report` (size/shape-capped, rate-limited,
  **no IP logging, no correlation**), kept OFF the money path for privacy isolation.

## Work packages
- **WP-T.1 — Consent spec (TLA+ first).** Author `spec/telemetry/ConsentGate.tla` (+ `.cfg`),
  TLC-green: **INV-Consent-1** no diagnostic bundle egresses without an explicit consent
  event for THAT bundle; **INV-Consent-2** toggle off ⇒ zero egress; **INV-Consent-3** the
  bundle sent is exactly the reviewed bundle (no post-review mutation).
- **WP-T.2 — Local capture.** Rust `panic::set_hook` → local diagnostics file; frontend
  error ring fed by ErrorBoundary; a `diagnostics_bundle()` command that assembles
  {app version, OS, crash stack, redacted log tail} — no network.
- **WP-T.3 — Scrub.** Pure `scrubBundle()` (HOME→~, strip 0x/emails/subs/tokens) + ephemeral
  id. Unit-tested against fixtures with planted secrets (must all be removed).
- **WP-T.4 — Review + send UI.** Settings "Send a diagnostic report" + on-crash prompt (gated
  on the toggle); shows the exact JSON; sends ONLY on explicit click via a single pinned
  HTTPS POST. No other egress.
- **WP-T.5 — Ingest endpoint.** Minimal standalone `POST /report`: size/shape cap, rate limit,
  append-only store, **no IP logging / no correlation**. Own tiny service, off the money path.
- **WP-T.6 — Ship v0.2.5.** Release ceremony (bump → bundle-lite build → notarize → GH release
  → DGX mirror → parity gate) + the re-mirror prompt.

## Acceptance (BDD — expand at start)
- Given the toggle is OFF, when the app runs (incl. a crash), then NOTHING is sent (verify no
  egress). Given a crash with the toggle ON, then a prompt appears; sending is one click after
  review. Given any bundle, then it contains no HOME path, address, email, sub, or token.

## Definition of done
ConsentGate TLC-green · code+tests local-green (no ratchet drop) · scrub tests prove all
planted secrets removed · EVIDENCE.md · RETRO.md · journal entry · essay "telemetry without
surveillance" (docs/essays) · @rule8 sign-off (privacy/T1) before the endpoint goes live.
