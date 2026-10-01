---
created: 2026-10-01T00:00:00Z
branch: hup/n4-recovery-privacy
author: Larry Klosowski + Claude Opus 5.5
status: active
wp: HUP-S10.5
---

# Offline matrix (HUP-S10.5)

What each feature does when the computer has no network. The canonical list is
[`src/privacy/offline-matrix.json`](../src/privacy/offline-matrix.json). The app shows it
in Settings, Privacy & recovery, and two tests keep it honest:

- `src-tauri/src/privacy_contract_tests.rs::each_feature_degrades_honestly_with_no_network`
  runs every Rust probe named in the JSON against a closed loopback port (the same as no
  network). A feature marked `works` must succeed. A feature marked `unavailable` or
  `degrades` must fail with a plain message within 45 seconds, never pretend to succeed,
  and never echo a key. Mutating any row (for example marking chain reads as `works`)
  fails the test.
- `src/privacy/offlineMatrix.test.ts` checks that this page names every feature id in
  the JSON, so the doc and the data cannot drift apart.

## The matrix

| id | Feature | Offline | Probe |
|---|---|---|---|
| `local-chat` | Hermes chat on your local model | works | `local_chat_is_loopback` |
| `journal` | Journal and encrypted journal export | works | `journal_export_offline` |
| `recovery-kit` | Device key recovery kit | works | `recovery_kit_offline` |
| `delete-local-data` | Delete my local data | works | `local_data_offline` |
| `diagnostics-prepare` | Prepare a diagnostic report | works | `diagnostics_prepare_offline` |
| `diagnostics-send` | Send a diagnostic report | needs the network | `telemetry_send_unreachable` |
| `chain-reads` | Wallet balances, registry reads, deploy checks | needs the network | `chain_rpc_unreachable` |
| `cloud-ai` | Hermes on a cloud AI provider | needs the network | `ai_provider_unreachable` |
| `sign-in` | Sign in and account details | needs the network | `oidc_unreachable` |
| `model-download` | Model downloads | needs the network | `model_download_unreachable` |
| `node-sync` | Node sync with the network | limited | not probed (see below) |
| `messaging` | Group messaging and invites | needs the network | not probed (see below) |

The member-facing sentence for each row (what the screen says or does) lives in the JSON
`behaviour` field, not here.

## What the probes prove, and what they do not

The probes exercise the Rust layer each feature calls: the RPC client, the AI provider
client, the OIDC client, the model transport and the telemetry POST. They prove those
layers fail fast with an error rather than hang or return a made-up value. How each
screen renders that error is covered by the existing honesty tests (for example
`walletVitalsHonesty.test.tsx`, `nodeVitalsHonesty.test.tsx`, `settingsHonesty.test.tsx`).

Two rows are not probed, and the test pins that count at two so a new unprobed row is a
deliberate change:

- **node-sync** needs a running node binary and peers. The node vitals honesty tests
  cover the zero-peer display.
- **messaging** has no transport test seam in core yet. Follow-up: add one in the comms
  bridge and a probe here.

The telemetry send now carries a 20 second bound (it had none), found while writing its
probe.
