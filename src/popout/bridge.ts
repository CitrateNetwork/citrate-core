// =====================================================================
// citrate-core — pop-out message bridge (HUP-S5.4)
//
// The typed channel between the main window and its pop-outs. Every message carries a version and
// is validated on receipt; anything malformed is dropped whole. The pop-outs hold no app commands
// at all (their capability grants only these events), so this channel is the only thing a pop-out
// can do: announce it is ready, and ask the main window to stop the running turn (the Activity
// monitor) or to stop Hermes's browser (the Browser, HUP-S5.1). Decisions on browser actions and
// origin consent are never taken over this channel: they stay in the main window.
//
// Transport: Tauri events addressed to one window label (`emitTo`), heard by that window only
// (`getCurrentWebviewWindow().listen`). Tests use an in-memory transport.
// =====================================================================
import { isPopoutKind, popoutLabel, type PopoutKind } from "./kinds";
import { isMonitorSnapshot, type MonitorSnapshot } from "./monitorSnapshot";
import { parseBrowserView, type BrowserView } from "./browserView";

/** The one event name the channel uses (both directions). */
export const POPOUT_EVENT = "citrate-popout";
export const BRIDGE_VERSION = 1 as const;
export const MAIN_LABEL = "main";

export type ToMain = { v: 1; type: "popout.ready"; kind: PopoutKind } | { v: 1; type: "monitor.stop" } | { v: 1; type: "browser.stop" };
export type ToPopout = { v: 1; type: "monitor.snapshot"; snapshot: MonitorSnapshot } | { v: 1; type: "browser.view"; view: BrowserView };

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

export function parseToMain(raw: unknown): ToMain | null {
  if (!isObj(raw) || raw.v !== BRIDGE_VERSION) return null;
  if (raw.type === "monitor.stop") return { v: 1, type: "monitor.stop" };
  if (raw.type === "browser.stop") return { v: 1, type: "browser.stop" };
  if (raw.type === "popout.ready" && isPopoutKind(raw.kind)) return { v: 1, type: "popout.ready", kind: raw.kind };
  return null;
}

export function parseToPopout(raw: unknown): ToPopout | null {
  if (!isObj(raw) || raw.v !== BRIDGE_VERSION) return null;
  if (raw.type === "monitor.snapshot" && isMonitorSnapshot(raw.snapshot)) {
    return { v: 1, type: "monitor.snapshot", snapshot: raw.snapshot };
  }
  if (raw.type === "browser.view") {
    const view = parseBrowserView(raw.view);
    return view ? { v: 1, type: "browser.view", view } : null;
  }
  return null;
}

/** Sends to a window by label; listens for messages addressed to this window. */
export interface BridgeTransport {
  send(targetLabel: string, payload: unknown): Promise<void>;
  listen(handler: (payload: unknown) => void): Promise<() => void>;
}

/** The Tauri transport (desktop app only). Loaded lazily so tests and the web preview never touch it. */
export async function tauriTransport(): Promise<BridgeTransport> {
  const { emitTo } = await import("@tauri-apps/api/event");
  const { getCurrentWebviewWindow } = await import("@tauri-apps/api/webviewWindow");
  const self = getCurrentWebviewWindow();
  return {
    send: (target, payload) => emitTo(target, POPOUT_EVENT, payload),
    listen: (handler) => self.listen<unknown>(POPOUT_EVENT, (e) => handler(e.payload)),
  };
}

export interface MainEnd {
  sendSnapshot(snapshot: MonitorSnapshot): Promise<void>;
  sendBrowserView(view: BrowserView): Promise<void>;
  close(): void;
}

export async function createMainEnd(
  t: BridgeTransport,
  handlers: { onReady: (kind: PopoutKind) => void; onStop: () => void; onBrowserStop?: () => void },
): Promise<MainEnd> {
  let open = true;
  const unlisten = await t.listen((raw) => {
    if (!open) return;
    const msg = parseToMain(raw);
    if (!msg) return;
    if (msg.type === "popout.ready") handlers.onReady(msg.kind);
    else if (msg.type === "browser.stop") handlers.onBrowserStop?.();
    else handlers.onStop();
  });
  return {
    async sendSnapshot(snapshot) {
      if (!open) return;
      const msg: ToPopout = { v: 1, type: "monitor.snapshot", snapshot };
      await t.send(popoutLabel("monitor"), msg);
    },
    async sendBrowserView(view) {
      if (!open) return;
      const msg: ToPopout = { v: 1, type: "browser.view", view };
      await t.send(popoutLabel("browser"), msg);
    },
    close() {
      open = false;
      unlisten();
    },
  };
}

export interface PopoutEnd {
  ready(): Promise<void>;
  stop(): Promise<void>;
  /** HUP-S5.1: ask the main window to stop Hermes's browser. */
  stopBrowser(): Promise<void>;
  close(): void;
}

export async function createPopoutEnd(
  t: BridgeTransport,
  kind: PopoutKind,
  onSnapshot: (s: MonitorSnapshot) => void,
  onBrowserView?: (v: BrowserView) => void,
): Promise<PopoutEnd> {
  let open = true;
  const unlisten = await t.listen((raw) => {
    if (!open) return;
    const msg = parseToPopout(raw);
    if (!msg) return;
    if (msg.type === "monitor.snapshot") onSnapshot(msg.snapshot);
    else onBrowserView?.(msg.view);
  });
  const send = (msg: ToMain) => (open ? t.send(MAIN_LABEL, msg) : Promise.resolve());
  return {
    ready: () => send({ v: 1, type: "popout.ready", kind }),
    stop: () => send({ v: 1, type: "monitor.stop" }),
    stopBrowser: () => send({ v: 1, type: "browser.stop" }),
    close() {
      open = false;
      unlisten();
    },
  };
}
