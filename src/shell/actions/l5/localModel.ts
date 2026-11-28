// L0-WP07e: the local model actions (CORE-BC-3: download, verify, select, serve and route),
// moved out of the Store class body into the L5 models and inference lane. Each Store method of the
// same name is a one-line delegate to the function here, so every call site
// (`store.startModelDownload()`) is unchanged. The code is moved as it was: `this` became `store`,
// and the Store's private members are reached through storeInternals(store). The module-level
// helper modelFailureReason moved with it (it had no other user).
//
// Data sources (Rule 7): the Rust model commands through bridge.model (status, download, pause,
// verify, serve; the GGUF file and its .part on disk, checked against the pinned digest), the model
// router's choices (agent/modelRouter.ts) and AppState (`model*`, `routerActive`). Unchanged.

import { activeRouterId, canSelect, resolveActive, type ModelChoice } from "../../../agent/modelRouter";
import { bridge } from "../../../bridge";
import type { ModelStatus } from "../../../bridge/domains";
import { BRIDGE_MODE } from "../../../bridge/mode";
import { LOCAL_MODELS } from "../../localModels";
import type { AppState } from "../../state";
import type { Store } from "../../store";
import { storeInternals } from "../../storeInternals";

/** Why a model download/verify failed. A Tauri command rejects with the Rust error as a plain
 *  string, not an Error; that string is the real cause and is shown as is. */
function modelFailureReason(e: unknown, fallback: string): string {
  if (e instanceof Error && e.message.trim()) return e.message;
  if (typeof e === "string" && e.trim()) return e;
  return fallback;
}

/** Fold the REAL model status from the bridge into AppState. `ready` is EARNED
 * only by a real verify (Rule 1) — the bridge never fabricates it. A failed
 * poll keeps the last honest values. */
export async function refreshModel(store: Store): Promise<void> {
  const internals = storeInternals(store);
  if (internals.modelRefreshing) return; // in-flight guard — skip this 2s tick if the last is still running
  internals.modelRefreshing = true;
  try {
    const st = await bridge.model.status();
    // While a download call is in flight the backend may not have written its `.part` yet
    // (disk check, first request): that reads as notPresent, never as a reason to show the
    // Download button again.
    const state = st.state === "notPresent" && internals.modelDownloadInFlight ? "downloading" : st.state;
    const patch: Partial<AppState> = {
      modelState: state,
      modelError: state === "error" ? store.state.modelError : null,
      modelBytesPerSec: null,
      modelEtaSecs: null,
      modelRetryInSecs: null,
      modelRetryReason: null,
    };
    if (st.state === "downloading" || st.state === "paused") {
      patch.modelDownloadedBytes = st.downloadedBytes;
      patch.modelTotalBytes = st.totalBytes;
    }
    if (st.state === "downloading") {
      patch.modelBytesPerSec = st.bytesPerSec ?? null;
      patch.modelEtaSecs = st.etaSecs ?? null;
      patch.modelRetryInSecs = st.retryInSecs ?? null;
      patch.modelRetryReason = st.lastError ?? null;
    } else if (st.state === "error") {
      patch.modelError = st.msg;
    }
    // An error the download call reported stays on screen until the next start.
    if (store.state.modelState === "error" && !internals.modelDownloadInFlight && (st.state === "notPresent" || st.state === "downloading")) {
      patch.modelState = "error";
      patch.modelError = store.state.modelError;
    }
    store.setState(patch);
    // If the local model is verified-Ready, SPAWN the llama-server sidecar so
    // chat actually uses on-device Gemma. Previously serveStart was only called
    // right after a fresh verify (onboarding), so a normal launch with an
    // already-verified model left the server down → inference fell back to the
    // canned demo provider. Idempotent (serveStart returns AlreadyRunning if up);
    // best-effort (a missing/failed llama-server binary is caught, chat stays on
    // its honest fallback). Re-select the provider once the server is healthy.
    if (BRIDGE_MODE === "tauri" && st.state === "ready") {
      // Spawn + route once healthy (idempotent; polls readiness so it doesn't pick demo while the
      // server is still cold-loading, and doesn't require an app restart to switch to local).
      void store.serveLocalAndRoute();
    }
  } catch {
    /* honest no-op: a failed poll keeps the last real status, never a sim number */
  } finally {
    internals.modelRefreshing = false;
  }
}

/** Read the onboarding model options (the member's choice and the free disk space). */
export async function loadModelOptions(store: Store): Promise<void> {
  try {
    const o = await bridge.model.options();
    store.setState({ modelChoice: o.chosen, modelFreeBytes: o.freeBytes ?? null });
  } catch {
    /* honest no-op: the step shows no disk figure */
  }
}

/** Start (or resume) the STREAMED model download + verify, polling `model.status()` for real
 * progress. `model` picks the default or the smaller pinned model (and becomes the choice).
 * The backend retries a lost connection on its own while the app is open, verifies, and on a
 * checksum mismatch downloads once more from the start, so this resolves at `ready` or rejects
 * with plain words. A second start while one runs attaches to it (the backend waits on it).
 * A pause rejects; the status then reads `paused`, which is shown as paused, not as an error. */
export function startModelDownload(store: Store, model?: "default" | "small"): void {
  const internals = storeInternals(store);
  const gen = ++internals.modelGen;
  const patch: Partial<AppState> = { modelState: "downloading", modelError: null, modelRetryInSecs: null, modelRetryReason: null };
  if (model && model !== store.state.modelChoice) {
    patch.modelChoice = model;
    patch.modelDownloadedBytes = 0;
    patch.modelTotalBytes = LOCAL_MODELS[model].sizeBytes;
    patch.modelBytesPerSec = null;
    patch.modelEtaSecs = null;
  }
  store.setState(patch);
  store.save();
  internals.modelDownloadInFlight = true;
  // Poll the real status every 2s so the progress bar reflects real bytes.
  if (!internals.modelTimer) internals.modelTimer = setInterval(() => void store.refreshModel(), 2000);
  void bridge.model
    .download(model)
    .then(async () => {
      if (gen !== internals.modelGen) return;
      internals.modelDownloadInFlight = false;
      await store.refreshModel();
      // The download call verifies; verify here only if the status does not read ready yet.
      if (store.state.modelState !== "ready") await store.verifyModel();
      else {
        store.toast("Local model verified, SHA-256 matched. Chat can run on this computer.");
        void store.serveLocalAndRoute();
        internals.stopModelPoll();
      }
      void store.loadModelOptions();
    })
    .catch(async (e: unknown) => {
      if (gen !== internals.modelGen) return;
      internals.modelDownloadInFlight = false;
      const reason = modelFailureReason(e, "download failed");
      let st: ModelStatus | null = null;
      try {
        st = await bridge.model.status();
      } catch {
        st = null;
      }
      if (gen !== internals.modelGen) return;
      if (st?.state === "paused") {
        store.setState({ modelState: "paused", modelDownloadedBytes: st.downloadedBytes, modelTotalBytes: st.totalBytes, modelError: null, modelBytesPerSec: null, modelEtaSecs: null, modelRetryInSecs: null, modelRetryReason: null });
        internals.stopModelPoll();
        return;
      }
      if (st?.state === "ready") {
        store.setState({ modelState: "ready", modelError: null });
        internals.stopModelPoll();
        void store.serveLocalAndRoute();
        return;
      }
      store.setState({ modelState: "error", modelError: reason, modelRetryInSecs: null, modelRetryReason: null });
      store.toast("Model download stopped: " + reason + " Chat uses the gateway meanwhile.");
      internals.stopModelPoll();
      void store.loadModelOptions();
      store.save();
    });
}

/** Pause the running download; its progress is kept and Resume continues from the same byte. */
export function pauseModelDownload(store: Store): void {
  void bridge.model
    .pause()
    .then(() => store.refreshModel())
    .catch(() => {
      /* the next poll shows the real state */
    });
}

/** Switch to the smaller pinned model (a slow connection or a full disk). The default model's
 * partial download, if any, is paused and kept. */
export function chooseSmallerModel(store: Store): void {
  const internals = storeInternals(store);
  const wasRunning = internals.modelDownloadInFlight;
  const go = () => store.startModelDownload("small");
  if (wasRunning) {
    void bridge.model.pause().catch(() => undefined).finally(go);
  } else go();
}

/** Go back to the default model (its own partial download resumes). */
export function chooseDefaultModel(store: Store): void {
  const internals = storeInternals(store);
  const wasRunning = internals.modelDownloadInFlight;
  const go = () => store.startModelDownload("default");
  if (wasRunning) {
    void bridge.model.pause().catch(() => undefined).finally(go);
  } else go();
}

/** Stream-verify the downloaded model (SHA-256 == the pinned hash). On a match
 * the model becomes `ready`; on a mismatch the file is quarantined and an
 * honest error surfaces (never a fabricated "verified"). */
export async function verifyModel(store: Store): Promise<void> {
  const internals = storeInternals(store);
  store.setState({ modelState: "verifying" });
  try {
    await bridge.model.verify();
    await store.refreshModel(); // reads the EARNED `ready` from the bridge
    if (store.state.modelState === "ready") {
      store.toast("Local model verified, SHA-256 matched. Chat can run on this computer.");
      // Spawn llama-server AND route chat to it once it's healthy (the cold-load takes tens of
      // seconds; serveLocalAndRoute polls readiness before switching the provider off demo).
      void store.serveLocalAndRoute();
    }
    internals.stopModelPoll();
  } catch (e) {
    const reason = modelFailureReason(e, "verify failed");
    store.setState({ modelState: "error", modelError: reason });
    store.toast("Model verify failed: " + reason + " Chat uses the gateway meanwhile.");
    internals.stopModelPoll();
  }
  store.save();
}

/**
 * Hermes ModelRouter (P0/WP0.4) — persist the member's model selection. `choices` is the
 * live router list the surface built (local + gateway [+ registry]). Fail-closed on a
 * phantom id (INV-Router-3): a selection that is not an enumerated choice is ignored, so
 * the active can never point at a fabricated model. The caller (the chat surface) also
 * switches the SERVED local model for a local pick (modelsSlice.selectModel restarts
 * llama-server); the gateway is the always-ready default, so picking it needs no restart.
 */
/**
 * D-11 — a cloud choice (a model of a provider connected in Settings › Models) is stored in core
 * as the preferred model (`providers_preferred_set`, which checks the provider can take calls),
 * and the provider is rebuilt so the next Hermes turn runs on it through the core model proxy.
 * A local or gateway choice clears a stored cloud model, so the pick in the chat is what answers.
 * Returns core's refusal (nothing changes then), or null.
 */
export async function selectModel(store: Store, id: string, choices: ModelChoice[]): Promise<string | null> {
  if (!canSelect(id, choices)) return null; // phantom id — never persist a non-choice
  const chosen = choices.find((c) => c.id === id);
  if (chosen?.source === "cloud" && chosen.cloud) {
    const err = await store.setHermesCloudModel(chosen.cloud);
    if (err) return err;
    store.setState({ activeModelId: id });
    store.save();
    return null;
  }
  store.setState({ activeModelId: id });
  store.save();
  if (store.state.hermesCloudModel) return store.setHermesCloudModel(null);
  return null;
}

/** The router id shown as selected: the stored cloud model while one is set, else the stored id. */
export function routerActiveId(store: Store): string | null {
  return activeRouterId(store.state.activeModelId, store.state.hermesCloudModel);
}

/**
 * The model the send path actually resolves (INV-Router-2): the active choice iff it is
 * READY, else the always-ready gateway terminal. Pure over the passed `choices` so the
 * chat never routes to a not-ready (still-downloading / not-pulled) model.
 */
export function routerActive(store: Store, choices: ModelChoice[]): ModelChoice {
  return resolveActive(store.routerActiveId(), choices);
}

/** Honestly SKIP the local model: chat routes to the gateway/demo. This is an
 * opt-out, not a failure — no fabricated "ready". */
export function skipModel(store: Store): void {
  const internals = storeInternals(store);
  // A running download keeps going in the background (and its progress keeps showing).
  if (!internals.modelDownloadInFlight) internals.stopModelPoll();
  store.setState({ modelSkipped: true });
  store.toast("Skipped the local model. Chat runs on the gateway; you can download it any time in Settings, Models.");
  store.save();
}

/** Stop the model status poller (called on complete/skip/error). */
export function stopModelPoll(store: Store): void {
  const internals = storeInternals(store);
  if (internals.modelTimer) {
    clearInterval(internals.modelTimer);
    internals.modelTimer = null;
  }
}

/**
 * Spawn the local llama-server for a verified model, then route chat to it ONCE IT IS HEALTHY.
 *
 * WHY THIS EXISTS: `serveStart` only *spawns* llama-server; a cold start mmaps a multi-GB GGUF and
 * builds the compute graph, which takes tens of seconds. The old code called `serveStart` then
 * `rebuildProvider` immediately (server still loading → inferenceState not "ready" → the honest DEMO
 * provider) and then STOPPED polling — so chat stayed on the canned demo agent for the whole session
 * even though the model was serving. Here we spawn, then poll `inferenceState` until it reports
 * "ready" and only THEN rebuild the provider (→ local). Idempotent (a running server → AlreadyRunning;
 * the poll guard prevents overlaps); bounded (~80s cap) so a genuinely dead server falls back honestly.
 */
export async function serveLocalAndRoute(store: Store): Promise<void> {
  const internals = storeInternals(store);
  if (BRIDGE_MODE !== "tauri") return;
  try {
    await bridge.model.serveStart();
  } catch {
    // No local server (binary missing / spawn failed) — leave chat on its honest fallback.
    // SCL-S7.5a: the startup start has settled (failed), so a held message asks now.
    internals.launchLocalStartPending = false;
    await store.rebuildProvider();
    return;
  }
  // SCL-S7.5a: the startup start has settled (spawned; a cold load counts as running).
  internals.launchLocalStartPending = false;
  if (internals.serveRouteTimer) return; // a poll is already waiting for the server to warm up
  let tries = 0;
  const check = async () => {
    tries++;
    let state = "demo";
    try {
      state = await bridge.chat.inferenceState(false);
    } catch {
      /* transient read failure — keep waiting */
    }
    if (state === "ready" || tries >= 40) {
      if (internals.serveRouteTimer) {
        clearInterval(internals.serveRouteTimer);
        internals.serveRouteTimer = null;
      }
      await store.rebuildProvider(); // now routes to local (or stays on fallback at the cap)
    }
  };
  internals.serveRouteTimer = setInterval(() => void check(), 2000);
  void check();
}
