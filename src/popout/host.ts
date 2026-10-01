// =====================================================================
// citrate-core — pop-out host, the main-window side (HUP-S5.4 + S7.6)
//
// Opens pop-outs through the guarded Rust command, answers a ready Activity monitor with snapshots
// of real state as it changes, and runs the existing Stop path when the monitor asks. A pop-out
// closing does nothing here: closing a window never stops or changes any work. HUP-S1.9: while the
// monitor is open the host also polls the agent sidecar's worker processes and republishes when
// their state changes (a worker crash and restart is not a store change).
// HUP-S5.1: while a Browser pop-out is alive (it re-announces itself every few seconds), the host
// polls Hermes's browser through Rust (status and the latest screencast frame) and sends the pop-out
// checked views; the pop-out's Stop runs the browser's stop. When the announcements stop, so does
// the polling.
// Kept free of the store so it is testable on its own; `appHost.ts` wires the real dependencies.
// =====================================================================
import { createMainEnd, type BridgeTransport, type MainEnd } from "./bridge";
import { isPopoutKind, type PopoutKind } from "./kinds";
import { buildMonitorSnapshot, type MonitorInputs, type WorkerRow } from "./monitorSnapshot";
import { BROWSER_OFF, parseBrowserFrame, parseBrowserStatus, type BrowserFrame, type BrowserState } from "./browserView";

/** HUP-S5.1: Hermes's browser, through the Rust commands. */
export interface BrowserDeps {
  /** `hermes_browser_status` */
  status(): Promise<unknown>;
  /** `hermes_browser_frame` — the view when newer than `after`, else null. */
  frame(after: number): Promise<unknown>;
  /** `hermes_browser_stop` */
  stop(): Promise<void>;
}

/** A Browser pop-out that has not announced itself for this long is treated as closed. */
export const BROWSER_HEARTBEAT_TIMEOUT_MS = 10_000;
/** How often the browser status is re-read while a Browser pop-out is open. */
export const BROWSER_STATUS_EVERY_MS = 1_000;

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
  /** HUP-S5.1: Hermes's browser. Absent (web preview, tests), the Browser pop-out shows "off". */
  browser?: BrowserDeps;
  /** How often the screencast is polled while a Browser pop-out is open. */
  browserPollMs?: number;
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

  // --- HUP-S5.1 the Browser pop-out ---------------------------------------------------------
  const browserPoll = deps.browserPollMs ?? 250;
  let browserSeen = Number.NEGATIVE_INFINITY;
  let browserTimer: ReturnType<typeof setTimeout> | null = null;
  let browserRunning = false;
  let browserState: BrowserState = BROWSER_OFF;
  let browserFrame: BrowserFrame | null = null;
  let statusReadAt = Number.NEGATIVE_INFINITY;

  const sendBrowser = async () => {
    if (disposed || !end) return;
    await end.sendBrowserView({ state: browserState, frame: browserFrame }).catch(() => undefined);
  };
  const browserTick = async () => {
    browserTimer = null;
    const b = deps.browser;
    if (disposed || !b || deps.now() - browserSeen > BROWSER_HEARTBEAT_TIMEOUT_MS) {
      browserRunning = false;
      return;
    }
    let changed = false;
    if (deps.now() - statusReadAt >= BROWSER_STATUS_EVERY_MS) {
      statusReadAt = deps.now();
      let next: BrowserState;
      try {
        next = parseBrowserStatus(await b.status());
      } catch {
        next = BROWSER_OFF; // honest: unknown is shown as off, never as working
      }
      // A different mode is a different browser: its frames start over.
      if (next.mode !== browserState.mode) browserFrame = null;
      browserState = next;
      changed = true;
    }
    try {
      const f = parseBrowserFrame(await b.frame(browserFrame?.version ?? 0));
      if (f && (!browserFrame || f.version > browserFrame.version)) {
        browserFrame = f;
        changed = true;
      }
    } catch {
      // A failed frame read keeps the last frame; the status says what is wrong.
    }
    if (changed) await sendBrowser();
    if (disposed) {
      browserRunning = false;
      return;
    }
    browserTimer = setTimeout(() => void browserTick(), browserPoll);
  };
  const browserReady = () => {
    browserSeen = deps.now();
    if (!deps.browser) {
      void sendBrowser();
      return;
    }
    if (browserRunning) return;
    browserRunning = true;
    statusReadAt = Number.NEGATIVE_INFINITY;
    void browserTick();
  };

  end = await createMainEnd(deps.transport, {
    onReady: (kind) => {
      if (kind === "browser") {
        browserReady();
        return;
      }
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
    onBrowserStop: () => {
      void deps.browser?.stop().catch(() => undefined);
    },
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
      if (browserTimer !== null) clearTimeout(browserTimer);
      if (poll !== null) clearInterval(poll);
      unsubscribe();
      end?.close();
    },
  };
}
