// =====================================================================
// citrate-core — wires the pop-out host to the real app (HUP-S5.4 + S7.6)
//
// Desktop app only. The monitor's inputs are the live store, the turn activity slice, the tier
// report and the model router (the same sources the chat header uses); the context window comes
// from Rust (`popout_monitor_facts`, the local llama-server's --ctx-size); Stop is
// `store.stopAgentTurn`. In the web preview there are no windows to open, and it says so.
// =====================================================================
import { invoke } from "../bridge/tauri/invoke";
import { BRIDGE_MODE } from "../bridge/mode";
import { store } from "../shell/store";
import { turnActivity } from "../shell/slices/turnActivity";
import { refreshTier, tierSlice } from "../shell/slices/tier";
import { modelsSlice } from "../shell/slices/models";
import { choicesFromSources, registryModelsToChoiceInput } from "../agent/modelRouterSources";
import { resolveActive } from "../agent/modelRouter";
import { createPopoutHost, type PopoutHost } from "./host";
import { tauriTransport } from "./bridge";
import type { PopoutKind } from "./kinds";
import { daemonsSection } from "./monitorSnapshot";
import { daemonsSlice } from "../daemons/slice";
import { setDaemonPaused, stopDaemonRun } from "../daemons/appRunner";

let hostPromise: Promise<PopoutHost> | null = null;

/** Start the main-window host once (desktop app only); null in the web preview. */
export function startPopoutHost(): Promise<PopoutHost> | null {
  if (BRIDGE_MODE !== "tauri") return null;
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
        const offs = [store.subscribe(fn), turnActivity.subscribe(fn), tierSlice.subscribe(fn), modelsSlice.subscribe(fn), daemonsSlice.subscribe(fn)];
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
      now: () => Date.now(),
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
    await (await host).open(kind);
    // The monitor shows the tier; probe it once if nothing has yet (a local check, no network).
    if (kind === "monitor" && !tierSlice.get().loaded) void refreshTier();
  } catch (e) {
    store.toast("Could not open the pop-out: " + (e instanceof Error ? e.message : String(e)));
  }
}
