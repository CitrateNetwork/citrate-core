// =====================================================================
// citrate-core: the frozen v1 eval datasets (A50, HUP-S1.7 / HUP-S1.10)
//
// toolcall-v1 (57 tasks) and injection-v1 (23 cases) stay at the exact bytes every v1 scorecard
// was produced on (commit 8e28ed3). datasetHashes.test.ts pins them, and scripts/eval-check.mjs
// (the no-model `eval-check` pull-request job) refuses a datasets.sha256 manifest that moves
// either pin. Add items as a fragment under toolcall-v2.d/ or injection-v2.d/ instead.
// No imports, so the app's DOM-only tsconfig and plain Node both load it.
// =====================================================================

export const FROZEN_V1_SHA256: Readonly<Record<string, string>> = Object.freeze({
  "toolcall-v1.json": "0bb4205ef14cd1b1701cf7b2e0392e15c858a8bf9b020ab8c2bc49ebbdbb0bdf",
  "injection-v1.json": "9c2148ade8e50d106c4c8b14c94522d43fed1931390c5dfb45f43d9f9ff8f1e2",
});
