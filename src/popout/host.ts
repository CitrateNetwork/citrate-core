// =====================================================================
// citrate-core — pop-out host, the main-window side (HUP-S5.4 + S7.6)
//
// Opens pop-outs through the guarded Rust command, answers a ready Activity monitor with snapshots
// of real state as it changes, and runs the existing Stop path when the monitor asks. A pop-out
// closing does nothing here: closing a window never stops or changes any work.
// Kept free of the store so it is testable on its own; `appHost.ts` wires the real dependencies.
// =====================================================================
import { createMainEnd, type BridgeTransport, type MainEnd } from "./bridge";
import { isPopoutKind, type PopoutKind } from "./kinds";
import { buildMonitorSnapshot, type MonitorInputs } from "./monitorSnapshot";
import type { UndoPanel } from "./undoPanel";

export interface PopoutHostDeps {
  transport: BridgeTransport;
  /** Ask Rust to open (or focus) the pop-out window of this kind. */
  openWindow(kind: PopoutKind): Promise<void>;
  /** The live inputs for a monitor snapshot (everything except the context window and the clock). */
  inputs(): Omit<MonitorInputs, "localCtxTokens" | "now">;
  /** Subscribe to changes of anything `inputs()` reads; returns an unsubscribe. */
  subscribe(fn: () => void): () => void;
  /** The existing stop path (store.stopAgentTurn). */
  stop(): void;
  /** The local server's context window, from Rust. */
  contextWindow(): Promise<number | null>;
  now(): number;
  /** Coalesce bursts of changes into one snapshot per this many ms. */
  throttleMs?: number;
  /** HUP-S2.9: the undo panel (its changes must reach `subscribe`), its refresh from the sidecar,
   *  and the main window's undo path. Absent: the monitor shows no undo panel. */
  undo?: {
    panel(): UndoPanel;
    refresh(): void;
    request(session: string, seq: number | null): void;
  };
}

export interface PopoutHost {
  open(kind: PopoutKind): Promise<void>;
  dispose(): void;
}

export async function createPopoutHost(deps: PopoutHostDeps): Promise<PopoutHost> {
  let monitorOpen = false;
  let disposed = false;
  let ctx: number | null | undefined; // undefined = not read yet
  let timer: ReturnType<typeof setTimeout> | null = null;
  const throttle = deps.throttleMs ?? 150;

  const readCtx = async (): Promise<number | null> => {
    if (ctx !== undefined) return ctx;
    try {
      const v = await deps.contextWindow();
      ctx = typeof v === "number" && Number.isFinite(v) && v > 0 ? v : null;
    } catch {
      ctx = null; // honest: unknown, shown as such
    }
    return ctx;
  };

  let end: MainEnd | null = null;
  const publish = async () => {
    if (disposed || !monitorOpen || !end) return;
    const localCtxTokens = await readCtx();
    if (disposed) return;
    await end.sendSnapshot(buildMonitorSnapshot({ ...deps.inputs(), localCtxTokens, now: deps.now() })).catch(() => undefined);
    if (deps.undo && !disposed) await end.sendUndoPanel(deps.undo.panel()).catch(() => undefined);
  };
  const schedule = () => {
    if (disposed || !monitorOpen || timer !== null) return;
    timer = setTimeout(() => {
      timer = null;
      void publish();
    }, throttle);
  };

  end = await createMainEnd(deps.transport, {
    onReady: (kind) => {
      if (kind !== "monitor") return;
      monitorOpen = true;
      deps.undo?.refresh();
      void publish();
    },
    onStop: () => deps.stop(),
    onUndo: (session, seq) => deps.undo?.request(session, seq),
  });
  const unsubscribe = deps.subscribe(schedule);

  return {
    async open(kind) {
      if (!isPopoutKind(kind)) throw new Error(`${String(kind)} is not a pop-out`);
      await deps.openWindow(kind);
    },
    dispose() {
      disposed = true;
      if (timer !== null) clearTimeout(timer);
      unsubscribe();
      end?.close();
    },
  };
}
