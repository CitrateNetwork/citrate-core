// =====================================================================
// citrate-core — pop-out host, the main-window side (HUP-S5.4 + S7.6)
//
// Opens pop-outs through the guarded Rust command, answers a ready Activity monitor with snapshots
// of real state as it changes, and runs the existing Stop path when the monitor asks. A pop-out
// closing does nothing here: closing a window never stops or changes any work. HUP-S1.9: while the
// monitor is open the host also polls the agent sidecar's worker processes and republishes when
// their state changes (a worker crash and restart is not a store change).
// Kept free of the store so it is testable on its own; `appHost.ts` wires the real dependencies.
// =====================================================================
import { createMainEnd, type BridgeTransport, type MainEnd } from "./bridge";
import { isPopoutKind, type PopoutKind } from "./kinds";
import { buildMonitorSnapshot, type MonitorInputs, type WorkerRow } from "./monitorSnapshot";

export interface PopoutHostDeps {
  transport: BridgeTransport;
  /** Ask Rust to open (or focus) the pop-out window of this kind. */
  openWindow(kind: PopoutKind): Promise<void>;
  /** The live inputs for a monitor snapshot (everything except the context window and the clock). */
  inputs(): Omit<MonitorInputs, "localCtxTokens" | "now" | "workers">;
  /** Subscribe to changes of anything `inputs()` reads; returns an unsubscribe. */
  subscribe(fn: () => void): () => void;
  /** The existing stop path (store.stopAgentTurn). */
  stop(): void;
  /** The local server's context window, from Rust. */
  contextWindow(): Promise<number | null>;
  now(): number;
  /** HUP-S1.9: the agent sidecar's worker processes, from Rust (hermes_workers). A worker crash
   *  is not a store change, so the host polls this while the monitor is open. */
  workers?(): Promise<WorkerRow[]>;
  /** How often to poll `workers` while the monitor is open (default 5000 ms). */
  workersPollMs?: number;
  /** Coalesce bursts of changes into one snapshot per this many ms. */
  throttleMs?: number;
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
  let workers: WorkerRow[] | null = null; // null = not read (or the read failed)
  let workersKey = "";
  let poll: ReturnType<typeof setInterval> | null = null;

  const readWorkers = async (): Promise<boolean> => {
    if (!deps.workers) return false;
    let next: WorkerRow[] | null;
    try {
      const v = await deps.workers();
      next = Array.isArray(v) ? v : null;
    } catch {
      next = null; // honest: unknown, shown as such
    }
    const key = JSON.stringify(next);
    if (key === workersKey) return false;
    workersKey = key;
    workers = next;
    return true;
  };

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
    await end.sendSnapshot(buildMonitorSnapshot({ ...deps.inputs(), localCtxTokens, workers, now: deps.now() })).catch(() => undefined);
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
      void (async () => {
        await readWorkers();
        await publish();
      })();
      if (deps.workers && poll === null) {
        poll = setInterval(() => {
          if (disposed) return;
          void readWorkers().then((changed) => {
            if (changed) schedule();
          });
        }, deps.workersPollMs ?? 5000);
      }
    },
    onStop: () => deps.stop(),
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
      if (poll !== null) clearInterval(poll);
      unsubscribe();
      end?.close();
    },
  };
}
