// =====================================================================
// citrate-core — fleet wizard state (HUP-S8.2 + S8.3, US-8.1 "Connect my machines")
//
// A pure reducer + selectors; the view (FleetWizard.tsx) is a function of this state and the
// container drives the Rust commands (bridge/tauri/fleet.ts). Steps:
//   intro → machine (local tier probe + role) → discover (OPT-IN mDNS) → pair (link / QR)
//   → connect (Tailscale guidance, only when needed) → done.
// Discovery starts OFF and only a member action turns it on (consent). Nothing here invents a
// device: every row comes from the probe, the local roster, or a real mDNS answer (Rule 1).
// =====================================================================
import type {
  FleetDevice,
  FleetProbe,
  FleetRole,
  JoinResult,
  PairClaim,
  PairOffer,
  SeenDevice,
  TailscaleView,
} from "../bridge/tauri/fleet";
import type { TierId } from "../bridge/domains";

export type Step = "intro" | "machine" | "discover" | "pair" | "connect" | "done";

export const STEPS: Step[] = ["intro", "machine", "discover", "pair", "connect", "done"];

export interface WizardState {
  step: Step;
  busy: boolean;
  error: string | null;
  probe: FleetProbe | null;
  discovery: { enabled: boolean; browsed: boolean; devices: SeenDevice[] };
  offer: PairOffer | null;
  joinLink: string;
  inspect: PairClaim | null;
  roster: FleetDevice[];
  tailscale: TailscaleView | null;
  /** The last join could not reach the other machine at any address. */
  unreachable: boolean;
}

export type WizardAction =
  | { type: "start" }
  | { type: "failed"; error: string }
  | { type: "goto"; step: Step }
  | { type: "probed"; probe: FleetProbe }
  | { type: "roster"; devices: FleetDevice[] }
  | { type: "discoverySet"; enabled: boolean }
  | { type: "browsed"; devices: SeenDevice[] }
  | { type: "offer"; offer: PairOffer }
  | { type: "linkInput"; link: string }
  | { type: "inspected"; claim: PairClaim }
  | { type: "joined"; result: JoinResult }
  | { type: "tailscale"; view: TailscaleView };

export function initialWizard(): WizardState {
  return {
    step: "intro",
    busy: false,
    error: null,
    probe: null,
    discovery: { enabled: false, browsed: false, devices: [] },
    offer: null,
    joinLink: "",
    inspect: null,
    roster: [],
    tailscale: null,
    unreachable: false,
  };
}

function upsert(list: FleetDevice[], d: FleetDevice): FleetDevice[] {
  return [d, ...list.filter((x) => x.id !== d.id)];
}

export function reduce(s: WizardState, a: WizardAction): WizardState {
  switch (a.type) {
    case "start":
      return { ...s, busy: true, error: null };
    case "failed":
      return { ...s, busy: false, error: a.error };
    case "goto":
      return { ...s, step: a.step, error: null };
    case "probed":
      return { ...s, busy: false, probe: a.probe, step: s.step === "intro" ? "machine" : s.step };
    case "roster":
      return { ...s, busy: false, roster: a.devices };
    case "discoverySet":
      return {
        ...s,
        busy: false,
        discovery: a.enabled ? { ...s.discovery, enabled: true } : { enabled: false, browsed: false, devices: [] },
      };
    case "browsed":
      // A late answer after the member turned discovery off is dropped.
      if (!s.discovery.enabled) return { ...s, busy: false };
      return { ...s, busy: false, discovery: { enabled: true, browsed: true, devices: a.devices } };
    case "offer":
      return { ...s, busy: false, offer: a.offer };
    case "linkInput":
      return { ...s, joinLink: a.link, inspect: null, error: null };
    case "inspected":
      return { ...s, busy: false, inspect: a.claim };
    case "joined": {
      const r = a.result;
      if (r.ok && r.device) {
        return { ...s, busy: false, error: null, unreachable: false, roster: upsert(s.roster, r.device), step: "done" };
      }
      const unreachable = r.errorKind === "unreachable";
      return {
        ...s,
        busy: false,
        error: r.message ?? "Pairing failed.",
        unreachable,
        step: unreachable ? "connect" : s.step,
      };
    }
    case "tailscale":
      return { ...s, busy: false, tailscale: a.view };
  }
}

export const canBrowse = (s: WizardState): boolean => s.discovery.enabled;

export interface DeviceRow {
  key: string;
  label: string;
  tier: TierId | null;
  role: FleetRole;
  where: "this machine" | "paired" | "on this network";
  addr: string | null;
}

/** This machine, then paired machines, then machines answering on the local network. */
export function deviceRows(s: WizardState): DeviceRow[] {
  const rows: DeviceRow[] = [];
  if (s.probe) {
    const d = s.probe.device;
    rows.push({ key: `self:${d.deviceId}`, label: d.label, tier: d.tier, role: d.role, where: "this machine", addr: null });
  }
  for (const d of s.roster) {
    // The code is the same on both machines: the member can compare the two screens.
    rows.push({ key: `paired:${d.id}`, label: d.label, tier: d.tier, role: d.role, where: d.code ? `paired · code ${d.code}` : "paired", addr: d.addr });
  }
  for (const d of s.discovery.devices) {
    rows.push({
      key: `seen:${d.advert.id}`,
      label: d.advert.label,
      tier: d.advert.tier,
      role: d.advert.role,
      where: "on this network",
      addr: d.ip,
    });
  }
  return rows;
}

/** S8.3: show the connectivity step when a join could not reach the other machine, or when a
 *  browse found nothing and nothing is paired yet. */
export function needsConnectivityHelp(s: WizardState): boolean {
  if (s.unreachable) return true;
  return s.discovery.browsed && s.discovery.devices.length === 0 && s.roster.length === 0;
}

/** Role names per tier. PENDING OWNER SIGN-OFF (names + mapping are provisional defaults). */
export function roleLabel(role: FleetRole): string {
  switch (role) {
    case "light":
      return "Light: chat and small jobs";
    case "worker":
      return "Worker: mid-size models and background jobs";
    case "heavy":
      return "Heavy: serves the larger models";
    default:
      return "Role unknown";
  }
}

/** "expires in N min" for a pairing link. */
export function expiresIn(expiresAt: number, nowSecs: number): string {
  const left = expiresAt - nowSecs;
  if (left <= 0) return "expired";
  if (left < 60) return "expires in under a minute";
  return `expires in ${Math.ceil(left / 60)} min`;
}
