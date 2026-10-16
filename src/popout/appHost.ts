// =====================================================================
// citrate-core — wires the pop-out host to the real app (HUP-S5.4 + S7.6)
//
// Desktop app only. The monitor's inputs are the live store, the turn activity slice, the tier
// report and the model router (the same sources the chat header uses); the context window comes
// from Rust (`popout_monitor_facts`, the local llama-server's --ctx-size); Stop is
// `store.stopAgentTurn`; the worker processes come from Rust (`hermes_workers`, HUP-S1.9). In the
// web preview there are no windows to open, and it says so.
// HUP-S5.1: the Browser pop-out reads Hermes's browser through the `hermes_browser_*` commands;
// its Stop runs `hermes_browser_stop`, and a failure is shown to the member.
// =====================================================================
import { invoke } from "../bridge/tauri/invoke";
import { BRIDGE_MODE } from "../bridge/mode";
import { store } from "../shell/store";
import { turnActivity } from "../shell/slices/turnActivity";
import { refreshTier, tierSlice } from "../shell/slices/tier";
import { modelsSlice } from "../shell/slices/models";
import { agentUndo, refreshUndoPanel, undoChange, undoSession } from "../shell/slices/agentUndo";
import { bridge } from "../bridge";
import { choicesFromSources, registryModelsToChoiceInput } from "../agent/modelRouterSources";
import { resolveActive } from "../agent/modelRouter";
import { createPopoutHost, type PopoutHost } from "./host";
import { tauriTransport } from "./bridge";
import { focusContractReader, startContractHost } from "./contractHost";
import { focusDiff, startDiffHost } from "./diffHost";
import { tauriContractInbox } from "./contractChannel";
import { tauriBrowserApi } from "./browserApi";
import type { PopoutKind } from "./kinds";
import { daemonsSection } from "./monitorSnapshot";
import { daemonsSlice } from "../daemons/slice";
import { setDaemonPaused, stopDaemonRun } from "../daemons/appRunner";
import type { DecideMetering, WorkerRow } from "./monitorSnapshot";

let hostPromise: Promise<PopoutHost> | null = null;

/** Start the main-window host once (desktop app only); null in the web preview. */
export function startPopoutHost(): Promise<PopoutHost> | null {
  if (BRIDGE_MODE !== "tauri") return null;
  // HUP-S6.7: the Contract reader's requests are answered here too.
  startContractHost(tauriTransport, tauriContractInbox);
  // HUP-S5.4: and the Code and diff pop-out's.
  startDiffHost(tauriTransport);
  hostPromise ??= (async () =>
    createPopoutHost({
      transport: await tauriTransport(),
      openWindow: (kind) => invoke<string>("popout_open", { kind }).then(() => undefined),
      inputs: () => {
        const s = store.state;
        const models = modelsSlice.get();
        const active = resolveActive(s.activeModelId, choicesFromSources(models.local, registryModelsToChoiceInput(models.registry)));
        return {
          activity: turnActivity.get(),
          providerKind: store.activeProviderKind(),
          providerLabel: s.chatProviderLabel,
          modelLabel: active.label,
          modelId: active.id,
          tier: tierSlice.get().report?.effective ?? null,
          // HUP-S10.3: scheduled daemons (Rust daemons_list + the runner's state).
          daemons: (() => {
            const d = daemonsSlice.get();
            return daemonsSection(d.view, { blocked: d.runner.blockedReason, error: d.error ?? d.runner.error });
          })(),
        };
      },
      subscribe: (fn) => {
        const offs = [store.subscribe(fn), turnActivity.subscribe(fn), tierSlice.subscribe(fn), modelsSlice.subscribe(fn), agentUndo.subscribe(fn), daemonsSlice.subscribe(fn)];
        return () => offs.forEach((off) => off());
      },
      stop: () => store.stopAgentTurn(),
      pauseDaemon: (id, paused) => {
        void setDaemonPaused(id, paused).then((err) => {
          if (err) store.toast("Could not change the daemon: " + err);
        });
      },
      stopDaemon: () => stopDaemonRun(),
      contextWindow: async () => (await invoke<{ localCtxTokens: number }>("popout_monitor_facts")).localCtxTokens,
      browser: {
        status: () => tauriBrowserApi.status(),
        frame: (after) => tauriBrowserApi.frame(after),
        stop: () =>
          tauriBrowserApi.stop().catch((e) => {
            store.toast("Could not stop the browser: " + (e instanceof Error ? e.message : String(e)));
          }),
      },
      // HUP-S1.9: the sidecar's worker processes (Rust → the sidecar's GET /workers).
      workers: () => invoke<WorkerRow[]>("hermes_workers"),
      // HUP-S5.3: the decide() slot's per-backend metering (Rust → the sidecar's GET /decide/stats).
      decide: () => invoke<DecideMetering | null>("hermes_decide_stats"),
      now: () => Date.now(),
      // HUP-S2.9: the agent session's recent file changes, refreshed from the sidecar when the monitor
      // opens; an undo the monitor asks for runs here, in the main window.
      undo: {
        panel: () => agentUndo.get().panel,
        refresh: () => void refreshUndoPanel(bridge.agentHarness),
        request: (session, seq) =>
          void (seq === null ? undoSession(bridge.agentHarness, session) : undoChange(bridge.agentHarness, session, seq)),
      },
    }))();
  return hostPromise;
}

/** Open (or focus) a pop-out. Failures are shown to the member, never swallowed. */
export async function openPopout(kind: PopoutKind): Promise<void> {
  const host = startPopoutHost();
  if (!host) {
    store.toast("Pop-out windows need the desktop app.");
    return;
  }
  try {
    // HUP-S10.1: the Media player's data comes from its own host; start it before the window asks.
    if (kind === "media") await import("./mediaHost").then(({ startMediaHost }) => startMediaHost());
    await (await host).open(kind);
    // The monitor shows the tier; probe it once if nothing has yet (a local check, no network).
    if (kind === "monitor" && !tierSlice.get().loaded) void refreshTier();
  } catch (e) {
    store.toast("Could not open the pop-out: " + (e instanceof Error ? e.message : String(e)));
  }
}

/** HUP-S6.7 — open the Contract reader, optionally on an address (40204 unless a fork URL). */
export async function openContractReader(address?: string, target = "citrate"): Promise<void> {
  if (address) await focusContractReader(address, target);
  await openPopout("contract");
}

/** HUP-S5.4 — open the Code and diff pop-out on an agent file change (or a session's latest). */
export async function openDiff(session: string, seq: number | null = null): Promise<void> {
  if (startPopoutHost()) await focusDiff(session, seq);
  await openPopout("diff");
}
