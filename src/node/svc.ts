// =====================================================================
// V60 SVC-2 — the background service: webview types and the IO seam.
//
// The service and its settings live in Rust (src-tauri/src/svc*.rs). Data sources (Rule 7):
// `svc_status` reads svc/settings.json, the unit file, and the service's own status over its
// socket; `svc_power_status` reads `pmset -g batt` (macOS) or /sys/class/power_supply (Linux).
// =====================================================================

export interface PowerWindow {
  from: number;
  to: number;
}

export interface PowerPolicy {
  trainOnBattery: boolean;
  pauseBelowPct: number;
  nightly: PowerWindow | null;
}

export interface SvcNodeStatus {
  state: string;
  height: number;
  peers: number;
  miningArmed?: boolean;
  miningHold?: string;
}

export interface SvcServiceStatus {
  node: SvcNodeStatus;
  ipfsRunning: boolean;
  nodeHold: string | null;
  drained: boolean;
  coinbase: string | null;
}

export interface SvcView {
  supported: boolean;
  enabled: boolean | null;
  unitInstalled: boolean;
  attached: boolean;
  inProcessReason: string | null;
  service: SvcServiceStatus | null;
  runsLoggedOut: boolean | null;
  notes: string[];
  optOutCopy: string;
  power: PowerPolicy;
}

export interface PowerView {
  state: { onAc: boolean | null; batteryPct: number | null; hasBattery: boolean };
  training: { allowed: boolean; reason: string };
  source: string;
}

/** The IO seam: the runtime and the timeout-wrapped invoke. */
export interface SvcIo {
  mode: "tauri" | "sim";
  invoke<T>(cmd: string, args: Record<string, unknown>): Promise<T>;
}

export const SVC_DESKTOP_ONLY = "The background service runs with the desktop app. This web preview has no node to keep running.";

/** The production IO. */
export async function desktopSvcIo(): Promise<SvcIo> {
  const { BRIDGE_MODE } = await import("../bridge/mode");
  return {
    mode: BRIDGE_MODE,
    invoke: async <T,>(cmd: string, args: Record<string, unknown>) => {
      const { invoke } = await import("../bridge/tauri/invoke");
      return invoke<T>(cmd, args);
    },
  };
}

/** The one-line state for the panel header. */
export function svcLine(v: SvcView): string {
  if (!v.supported) return "Not available on this platform yet: the node runs while the app is open.";
  if (v.enabled === false) return "Off · the node runs only while the app is open";
  if (v.enabled === null) return "Not set up yet";
  if (!v.attached) return "On · the service is not answering yet; the node runs in the app";
  const s = v.service;
  if (!s) return "On · running in the background";
  if (s.drained) return "On · paused for an update";
  if (s.nodeHold) return "On · waiting to start the node: " + s.nodeHold;
  if (s.node.state === "stopped") return "On · node stopped";
  return "Node running in background" + (s.node.miningArmed ? " · producing" : " · " + s.node.state);
}

/** Hours for the nightly window select. */
export const HOURS: readonly number[] = Array.from({ length: 24 }, (_, i) => i);

export function hh(h: number): string {
  return String(h).padStart(2, "0") + ":00";
}
