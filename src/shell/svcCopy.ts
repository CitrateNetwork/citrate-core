// =====================================================================
// V60 SVC-2 — the background service's copy (planset fed
// .agentile/planset/2026-11-07-v060/02 §9, 03 §3.5, F-B1).
//
// The opt-out sentence is exact and owner-set (2026-11-07). The Rust side has the same string
// (src-tauri/src/svc_settings.rs OPT_OUT_COPY); a Rust test reads this file and fails on drift.
// =====================================================================

export const SVC_OPT_OUT_COPY = "If the app is closed and the service is off, your node won't produce blocks.";

export const SVC_STEP_TITLE = "Keep your node running";

export const SVC_ON_COPY =
  "Your node keeps syncing and, once it is allowed to, producing blocks while the app is closed.";

/** What runs in the background today (the service runs these two; nothing else yet). */
export const SVC_WHAT_RUNS: readonly string[] = [
  "Your node: it syncs the chain and produces blocks once the network allows it. Block production is the only thing the service signs.",
  "IPFS: it serves the files you pinned.",
  "Nothing else runs in the background in this release, and the service never opens your wallet.",
];

export const SVC_POWER_SUMMARY =
  "Power rules: training runs only on AC power by default. If you allow battery, it pauses below the threshold you set (30 % by default). An optional nightly window limits it to those hours.";
